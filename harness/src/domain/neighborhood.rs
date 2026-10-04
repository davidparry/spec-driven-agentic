//! Which source files one implementation attempt needs to see.
//!
//! A kata fits in a prompt whole; a project of any size does not. Sending
//! every source file made a 1.8 MB request that the model refused outright,
//! and cutting the list at an arbitrary byte count is just as likely to drop
//! the one file that explains the failure.
//!
//! So the files are treated as a graph. Each file declares symbols and
//! mentions the symbols of others; those mentions are the edges. Starting
//! from the files the change actually touches, the walk moves outward a
//! neighbor at a time, strongest tie first, and stops when the budget is
//! spent. Whatever is left over is still *named* — path and declared
//! symbols — so the model knows the rest of the project exists and what
//! lives in it, without being handed all of it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

use regex::Regex;

use super::language::Language;

/// A symbol shorter than this carries no signal: `i`, `ok`, `id`.
const MIN_SYMBOL_LEN: usize = 3;

/// How many declared symbols to name per file in the map. Enough to say
/// what the file is for, short enough that the map stays a map.
const MAPPED_SYMBOLS: usize = 12;

/// How many distinct symbols the evidence has to name before a file
/// counts as the answer.
///
/// One is an accident. A pending step definition echoes its own Gherkin
/// back in a `todo!`, so a single ordinary word of that text colliding
/// with a declared name is enough to crown a file the requirement has
/// nothing to do with. Two independent names is the cheapest threshold
/// that a coincidence rarely clears.
const MIN_EVIDENCE_SYMBOLS: usize = 2;

/// The files chosen for one prompt, and a map of the ones left out.
#[derive(Debug)]
pub struct Selection<'a> {
    /// Sent whole, nearest first — the seeds, then outward.
    pub included: Vec<&'a (String, String)>,
    /// Named but not sent, in path order.
    pub mapped: Vec<MappedFile<'a>>,
}

/// One line of the map: a file the model can ask about but has not read.
#[derive(Debug)]
pub struct MappedFile<'a> {
    pub path: &'a str,
    pub symbols: Vec<String>,
}

/// Walk out from `seeds` until `budget` bytes of source are chosen.
///
/// Seeds are always included — an attempt cannot write a file it has not
/// been shown — even when one of them alone overruns the budget.
pub fn select<'a>(
    language: Language,
    files: &'a [(String, String)],
    seeds: &[&str],
    budget: usize,
) -> Selection<'a> {
    let Graph { declared, ties } = graph(language, files);
    let seeded: HashSet<&str> = seeds.iter().copied().collect();

    let mut included = Vec::new();
    let mut mapped = Vec::new();
    let mut remaining = budget;
    for index in walk(files, seeds, &ties) {
        let file = &files[index];
        if seeded.contains(file.0.as_str()) || file.1.len() <= remaining {
            remaining = remaining.saturating_sub(file.1.len());
            included.push(file);
        } else {
            let mut symbols = declared[index].clone();
            symbols.truncate(MAPPED_SYMBOLS);
            mapped.push(MappedFile {
                path: &file.0,
                symbols,
            });
        }
    }
    mapped.sort_by_key(|file| file.path);
    Selection { included, mapped }
}

/// Where a requirement's production code belongs, inferred instead of
/// declared.
///
/// A path recorded per requirement would be one more thing to keep true,
/// and it cannot be right for a project the harness has never seen. The
/// graph already knows the answer: among the files that count as
/// production, the one the failing tests lean on hardest is the one the
/// behavior is missing from. A scenario calling an MCP tool reaches the
/// server through its step definitions, so the server is what comes back
/// — not whichever file happened to sort first under `src/`.
///
/// `None` when no production file is connected to the seeds at all, which
/// leaves the caller's own convention in charge.
pub fn nearest_production<'a>(
    language: Language,
    files: &'a [(String, String)],
    seeds: &[&str],
    is_production: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let Graph { ties, .. } = graph(language, files);
    let index: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, (path, _))| (path.as_str(), i))
        .collect();
    let seeded: Vec<usize> = seeds
        .iter()
        .filter_map(|seed| index.get(seed))
        .copied()
        .collect();
    files
        .iter()
        .enumerate()
        .filter(|(i, (path, _))| is_production(path) && !seeded.contains(i))
        .map(|(i, (path, _))| {
            let weight: usize = seeded.iter().filter_map(|seed| ties[*seed].get(&i)).sum();
            (weight, path.as_str())
        })
        .filter(|(weight, _)| *weight > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)))
        .map(|(_, path)| path)
}

/// Which production file a piece of evidence is talking about.
///
/// Seeding by *file* is too blunt when one glue file binds the whole
/// suite: its strongest tie is just the biggest thing it touches. The
/// evidence here is narrower — the bodies of the step definitions this
/// requirement's own scenarios run through — so the symbols in it name
/// the behavior under test rather than the project at large.
///
/// `None` when the evidence names too little to be sure, or names two
/// files equally well, which leaves the decision to the caller.
pub fn nearest_to_evidence<'a>(
    language: Language,
    files: &'a [(String, String)],
    evidence: &str,
    is_production: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let declared: Vec<Vec<String>> = files
        .iter()
        .map(|(_, content)| declared_symbols(language, content))
        .collect();
    let owners = owners_of(&declared);
    // How many *different* things of a file the evidence names, not how
    // often. A file mentioned forty times through one common helper is
    // incidental; one named through three distinct types is what the
    // behavior is actually built on.
    let mut named: Vec<HashSet<&str>> = vec![HashSet::new(); files.len()];
    for mention in IDENTIFIER.find_iter(evidence) {
        if let Some(&owner) = owners.get(mention.as_str()) {
            named[owner].insert(mention.as_str());
        }
    }
    let mut ranked: Vec<(usize, &str)> = files
        .iter()
        .enumerate()
        .filter(|(_, (path, _))| is_production(path))
        .map(|(i, (path, _))| (named[i].len(), path.as_str()))
        .filter(|(count, _)| *count >= MIN_EVIDENCE_SYMBOLS)
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let (best, path) = *ranked.first()?;
    // A tie is not an answer. Two files the evidence names equally well
    // means the evidence does not say which of them the work belongs
    // in, and guessing writes the implementation into the wrong module.
    let runner_up = ranked.get(1).map_or(0, |(count, _)| *count);
    (best > runner_up).then_some(path)
}

/// Who declares what, and who leans on whom.
struct Graph {
    declared: Vec<Vec<String>>,
    ties: Vec<HashMap<usize, usize>>,
}

fn graph(language: Language, files: &[(String, String)]) -> Graph {
    let declared: Vec<Vec<String>> = files
        .iter()
        .map(|(_, content)| declared_symbols(language, content))
        .collect();
    let ties = ties_between(files, &owners_of(&declared));
    Graph { declared, ties }
}

/// Visit order: the seeds, then their neighbors strongest tie first, then
/// the neighbors of those. Files no edge reaches come last, in path order,
/// so a project with no detectable structure still degrades to a list.
fn walk(files: &[(String, String)], seeds: &[&str], ties: &[HashMap<usize, usize>]) -> Vec<usize> {
    let index: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, (path, _))| (path.as_str(), i))
        .collect();
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    let mut queue = VecDeque::new();
    for seed in seeds {
        if let Some(&i) = index.get(seed)
            && seen.insert(i)
        {
            order.push(i);
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let mut neighbors: Vec<(usize, usize)> = ties[i]
            .iter()
            .map(|(&j, &weight)| (j, weight))
            .filter(|(j, _)| !seen.contains(j))
            .collect();
        neighbors.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| files[a.0].0.cmp(&files[b.0].0)));
        for (j, _) in neighbors {
            if seen.insert(j) {
                order.push(j);
                queue.push_back(j);
            }
        }
    }
    let mut unreached: Vec<usize> = (0..files.len()).filter(|i| !seen.contains(i)).collect();
    unreached.sort_by(|a, b| files[*a].0.cmp(&files[*b].0));
    order.extend(unreached);
    order
}

/// How many of each other file's symbols a file mentions. The count is
/// symmetric: a definition explains its caller and a caller explains what
/// the definition is for, and the walk wants both.
fn ties_between(
    files: &[(String, String)],
    owners: &HashMap<&str, usize>,
) -> Vec<HashMap<usize, usize>> {
    let mut ties: Vec<HashMap<usize, usize>> = vec![HashMap::new(); files.len()];
    for (i, (_, content)) in files.iter().enumerate() {
        for name in IDENTIFIER.find_iter(content) {
            let Some(&owner) = owners.get(name.as_str()) else {
                continue;
            };
            if owner == i {
                continue;
            }
            *ties[i].entry(owner).or_default() += 1;
            *ties[owner].entry(i).or_default() += 1;
        }
    }
    ties
}

/// Which file declares each symbol — only where the answer is
/// unambiguous. A name two files both declare cannot attribute a mention
/// to either of them, and guessing the first silently credits whichever
/// file happened to sort earliest.
fn owners_of(declared: &[Vec<String>]) -> HashMap<&str, usize> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for (i, symbols) in declared.iter().enumerate() {
        for symbol in symbols {
            let entry = counts.entry(symbol.as_str()).or_insert((i, 0));
            entry.1 += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, (_, owners))| *owners == 1)
        .map(|(symbol, (only, _))| (symbol, only))
        .collect()
}

/// The named things a file introduces: the types, functions and constants
/// another file would have to mention in order to use it.
pub fn declared_symbols(language: Language, source: &str) -> Vec<String> {
    let regex = match language {
        Language::Java => &JAVA_DECL,
        Language::JavaScript | Language::TypeScript => &JS_DECL,
        Language::DotNet => &CSHARP_DECL,
        Language::Rust => &RUST_DECL,
    };
    let mut seen = HashSet::new();
    regex
        .captures_iter(source)
        .filter_map(|captures| captures.get(1))
        .map(|name| name.as_str())
        .filter(|name| name.len() >= MIN_SYMBOL_LEN)
        .filter(|name| seen.insert(*name))
        .map(str::to_string)
        .collect()
}

static IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]{2,}").expect("valid regex"));
static RUST_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        // `mod x;` is deliberately absent. It declares no behavior, and
        // the name reappears in every `use crate::a::x::Thing` path that
        // merely passes through, which ties the whole crate to whichever
        // `mod.rs` lists the most modules.
        r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?(?:fn|struct|enum|trait|type|const|static|union|macro_rules!)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("valid regex")
});
static JAVA_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*(?:(?:public|protected|private|static|final|abstract|sealed|non-sealed)\s+)*(?:class|interface|enum|record|@interface)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("valid regex")
});
static CSHARP_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*(?:(?:public|protected|internal|private|static|sealed|abstract|partial|readonly)\s+)*(?:class|interface|enum|struct|record|delegate)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("valid regex")
});
static JS_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*(?:export\s+)?(?:default\s+)?(?:declare\s+)?(?:async\s+)?(?:function\*?|class|const|let|var|interface|type|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
    )
    .expect("valid regex")
});

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, content: &str) -> (String, String) {
        (path.to_string(), content.to_string())
    }

    #[test]
    fn rust_declarations_are_the_names_another_file_would_use() {
        let source = "pub struct Coverage {}\n\
                      pub fn covers_all(x: u8) {}\n\
                      const LIMIT: u8 = 3;\n\
                      fn helper() {}\n\
                      mod tests {}\n";
        assert_eq!(
            declared_symbols(Language::Rust, source),
            vec!["Coverage", "covers_all", "LIMIT", "helper"],
            "a module declaration names no behavior"
        );
    }

    #[test]
    fn each_language_names_its_own_declarations() {
        assert_eq!(
            declared_symbols(Language::Java, "public final class StringCalculator {}"),
            vec!["StringCalculator"]
        );
        assert_eq!(
            declared_symbols(Language::DotNet, "  internal sealed class Calculator {}"),
            vec!["Calculator"]
        );
        assert_eq!(
            declared_symbols(
                Language::TypeScript,
                "export async function addNumbers() {}"
            ),
            vec!["addNumbers"]
        );
    }

    #[test]
    fn a_neighbor_is_pulled_in_before_an_unrelated_file() {
        // `lib.rs` mentions Coverage, so coverage.rs is one hop away.
        // noise.rs shares nothing and is only reachable as a leftover.
        let files = vec![
            file("src/lib.rs", "pub fn report() { Coverage::new(); }"),
            file("src/noise.rs", "pub fn unrelated_thing() {}"),
            file("src/coverage.rs", "pub struct Coverage {}"),
        ];
        let chosen = select(Language::Rust, &files, &["src/lib.rs"], usize::MAX);
        let paths: Vec<&str> = chosen.included.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            vec!["src/lib.rs", "src/coverage.rs", "src/noise.rs"],
            "seed first, then its neighbor, then the unreachable file"
        );
    }

    #[test]
    fn the_strongest_tie_is_walked_first() {
        let files = vec![
            file(
                "src/lib.rs",
                "fn go() { Coverage::new(); Coverage::all(); Helper::one(); }",
            ),
            file("src/helper.rs", "pub struct Helper {}"),
            file("src/coverage.rs", "pub struct Coverage {}"),
        ];
        let chosen = select(Language::Rust, &files, &["src/lib.rs"], usize::MAX);
        let paths: Vec<&str> = chosen.included.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            vec!["src/lib.rs", "src/coverage.rs", "src/helper.rs"]
        );
    }

    #[test]
    fn what_does_not_fit_is_mapped_instead_of_dropped() {
        let bulk = "// filler\n".repeat(1_000);
        let files = vec![
            file("src/lib.rs", "pub fn report() {}"),
            file("src/huge.rs", &format!("pub struct Huge {{}}\n{bulk}")),
        ];
        let chosen = select(Language::Rust, &files, &["src/lib.rs"], 100);
        assert_eq!(chosen.included.len(), 1);
        assert_eq!(chosen.mapped.len(), 1);
        assert_eq!(chosen.mapped[0].path, "src/huge.rs");
        assert_eq!(chosen.mapped[0].symbols, vec!["Huge"]);
    }

    #[test]
    fn a_seed_is_sent_even_when_it_alone_overruns_the_budget() {
        let files = vec![file("src/lib.rs", &"x".repeat(5_000))];
        let chosen = select(Language::Rust, &files, &["src/lib.rs"], 10);
        assert_eq!(chosen.included.len(), 1);
        assert!(chosen.mapped.is_empty());
    }

    #[test]
    fn a_name_half_the_project_declares_draws_no_edge() {
        // Every Rust file has `mod tests`; if that counted as a tie the
        // walk would reach the whole crate in one hop.
        let declared: Vec<Vec<String>> = (0..6)
            .map(|_| declared_symbols(Language::Rust, "fn shared_helper() {}"))
            .collect();
        assert!(!owners_of(&declared).contains_key("shared_helper"));
    }

    #[test]
    fn the_production_target_is_what_the_failing_tests_lean_on() {
        // The bug this closes: the target was the first file under the
        // production root — `src/lib.rs` — so an attempt to add an MCP
        // tool rewrote the crate root instead of the server.
        let files = vec![
            file("src/lib.rs", "pub mod mcp;\npub mod adapters;"),
            file("src/mcp.rs", "pub struct WorkflowServer {}"),
            file("src/adapters/unrelated.rs", "pub struct Unrelated {}"),
            file(
                "tests/cucumber.rs",
                "fn step() { WorkflowServer::new(root); WorkflowServer::tools(); }",
            ),
        ];
        assert_eq!(
            nearest_production(Language::Rust, &files, &["tests/cucumber.rs"], |path| {
                path.starts_with("src/")
            }),
            Some("src/mcp.rs")
        );
    }

    #[test]
    fn narrow_evidence_outvotes_the_biggest_module_the_glue_file_touches() {
        // Seeding with the whole glue file picked whatever it exercised
        // most — here `generation.rs`. The bodies of the steps this one
        // requirement runs through name the server instead.
        let files = vec![
            file("src/lib.rs", "pub fn report() {}"),
            file(
                "src/mcp.rs",
                "pub struct WorkflowServer {}\npub struct ToolRequest {}",
            ),
            file(
                "src/domain/generation.rs",
                "pub fn scenario_prompt() {} pub fn unit_test_prompt() {}",
            ),
        ];
        let evidence =
            "let broker = WorkflowServer::new(root); broker.call(ToolRequest::new(name));";
        assert_eq!(
            nearest_to_evidence(Language::Rust, &files, evidence, |path| path
                .starts_with("src/")),
            Some("src/mcp.rs")
        );
    }

    /// A pending step definition echoes its Gherkin back in a `todo!`,
    /// and one ordinary word of that text can collide with a declared
    /// name. One hit is a coincidence, not an answer.
    #[test]
    fn a_single_name_is_a_coincidence_rather_than_a_target() {
        let files = vec![
            file("src/domain/model.rs", "pub struct Requirement {}"),
            file("src/mcp.rs", "pub struct WorkflowServer {}"),
        ];
        let evidence = "todo!(\"implement step: coverage is requested for a Requirement\");";
        assert_eq!(
            nearest_to_evidence(Language::Rust, &files, evidence, |path| path
                .starts_with("src/")),
            None
        );
    }

    /// Two files the evidence names equally well means the evidence
    /// does not say which; answering anyway is a coin flip.
    #[test]
    fn evidence_that_names_two_files_equally_names_neither() {
        let files = vec![
            file("src/one.rs", "pub struct Alpha {}\npub struct Beta {}"),
            file("src/two.rs", "pub struct Gamma {}\npub struct Delta {}"),
        ];
        let evidence = "Alpha::new(); Beta::new(); Gamma::new(); Delta::new();";
        assert_eq!(
            nearest_to_evidence(Language::Rust, &files, evidence, |path| path
                .starts_with("src/")),
            None
        );
    }

    #[test]
    fn breadth_of_coupling_beats_one_name_repeated() {
        // A step that loops over a helper mentions it dozens of times
        // without the file being what the behavior is built on. Three
        // different types of one file say far more than forty calls to
        // a single function of another.
        let files = vec![
            file("src/busy.rs", "pub fn push_one() {}"),
            file(
                "src/mcp.rs",
                "pub struct WorkflowServer {}\npub struct ToolRouter {}\npub fn advertise() {}",
            ),
        ];
        let evidence = format!(
            "{} WorkflowServer ToolRouter advertise",
            "push_one(); ".repeat(40)
        );
        assert_eq!(
            nearest_to_evidence(Language::Rust, &files, &evidence, |path| path
                .starts_with("src/")),
            Some("src/mcp.rs")
        );
    }

    #[test]
    fn evidence_that_names_nothing_leaves_the_target_to_convention() {
        let files = vec![file("src/lib.rs", "pub fn add() {}")];
        assert_eq!(
            nearest_to_evidence(Language::Rust, &files, "assert!(true);", |path| path
                .starts_with("src/")),
            None
        );
    }

    #[test]
    fn a_requirement_connected_to_nothing_leaves_the_target_to_convention() {
        let files = vec![
            file("src/lib.rs", "pub fn add() {}"),
            file("tests/lonely_test.rs", "#[test] fn t() { assert!(true); }"),
        ];
        assert_eq!(
            nearest_production(Language::Rust, &files, &["tests/lonely_test.rs"], |path| {
                path.starts_with("src/")
            }),
            None
        );
    }

    #[test]
    fn a_project_with_no_seed_still_returns_every_file() {
        let files = vec![
            file("src/a.rs", "pub fn a() {}"),
            file("src/b.rs", "pub fn b() {}"),
        ];
        let chosen = select(Language::Rust, &files, &["src/gone.rs"], usize::MAX);
        assert_eq!(chosen.included.len(), 2);
    }
}

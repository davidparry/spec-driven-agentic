//! `spec refactor`: what the model may rewrite, what it may only read,
//! and what it is shown of the code around the thing it is cleaning up.
//!
//! The one invariant this module exists to hold is that **a test is never
//! writable**. A refactor is only a refactor if the thing judging it did
//! not move, so the suite has to be the same suite afterwards. The model
//! is told that, but being told is not a guarantee - so the split is
//! computed here from the project layout, the reply is checked against
//! it, and a reply naming a test path is rejected whole rather than
//! filtered down to its acceptable parts.

use serde::Serialize;

use crate::domain::generation::{FileUpdate, brief_failure, decode_json, strip_code_fences};
use crate::domain::language::Language;
use crate::domain::memory::ProjectStructure;
use crate::domain::model::Requirement;
use crate::domain::prompts::{RenderedPrompt, render};

/// How many earlier rounds of one refactor loop the prompt recounts.
/// Each carries only the first line of each failure, so a loop that runs
/// its whole budget cannot grow the prompt past what a local model will
/// read.
const PROMPT_HISTORY_ROUNDS: usize = 3;

/// One rolled-back round of a refactor loop: what it rewrote and what
/// the suite said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefactorRound {
    pub targets: Vec<String>,
    pub failures: Vec<String>,
}

/// The files of a project sorted into what a refactor may rewrite and
/// what it may only read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefactorScope {
    /// Production sources, the only paths a reply may name.
    pub writable: Vec<(String, String)>,
    /// Tests, step definitions, features and manifests: the behaviour
    /// being preserved, and the context needed to preserve it.
    pub readonly: Vec<(String, String)>,
}

impl RefactorScope {
    pub fn writable_paths(&self) -> Vec<String> {
        self.writable.iter().map(|(path, _)| path.clone()).collect()
    }
}

/// Whether `path` belongs to the test suite, and so may never be
/// rewritten.
///
/// The layout's test root decides it when the project has one. The name
/// check behind it is not redundant: a project whose structure was never
/// recorded has no test root at all, and answering "not a test" for
/// every path in that project is the one wrong answer this function can
/// give.
pub fn is_test_path(path: &str, structure: &ProjectStructure) -> bool {
    if let Some(root) = structure.tests.as_deref()
        && under(path, root)
    {
        return true;
    }
    if let Some(root) = structure.features.as_deref()
        && under(path, root)
    {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with(".feature")
        || name.ends_with("Test.java")
        || name.ends_with("Tests.java")
        || name.ends_with("IT.java")
        || name.ends_with("Steps.java")
        || name.ends_with("Steps.cs")
        || name.ends_with("Tests.cs")
        || name.ends_with("_test.rs")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.js")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.js")
        || name.ends_with(".steps.ts")
        || name.ends_with(".steps.js")
}

fn under(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    !root.is_empty() && (path == root || path.starts_with(&format!("{root}/")))
}

/// Sort a project's sources into writable production code and read-only
/// behaviour, keeping only the production files that the refactor target
/// actually connects to.
///
/// `focus` is the production file the loop is cleaning up. The files
/// kept alongside it are the ones that name it or that it names - a
/// deterministic reference walk rather than the whole project, so a
/// large module does not bury the one class being refactored in a prompt
/// the model then reads half of.
pub fn scope(
    files: &[(String, String)],
    structure: &ProjectStructure,
    focus: &str,
    manifests: &[(String, String)],
) -> RefactorScope {
    let mut scope = RefactorScope::default();
    let focus_symbol = symbol_of(focus);
    for (path, content) in files {
        if is_test_path(path, structure) {
            scope.readonly.push((path.clone(), content.clone()));
        } else if path == focus || references(content, &focus_symbol) {
            scope.writable.push((path.clone(), content.clone()));
        } else if focus_symbol
            .as_deref()
            .is_some_and(|_| references_any(files, focus, path))
        {
            // Named by the file under refactor: read-only context, since
            // preserving its callers is the point, not editing them.
            scope.readonly.push((path.clone(), content.clone()));
        }
    }
    scope
        .readonly
        .extend(manifests.iter().map(|(p, c)| (p.clone(), c.clone())));
    scope
}

/// The type name a source path declares, which is how a reference to it
/// is spotted in another file without parsing the language.
fn symbol_of(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name);
    (!stem.is_empty()).then(|| stem.to_string())
}

fn references(content: &str, symbol: &Option<String>) -> bool {
    symbol
        .as_deref()
        .is_some_and(|symbol| mentions(content, symbol))
}

/// Whether the file under refactor names `candidate`'s type.
fn references_any(files: &[(String, String)], focus: &str, candidate: &str) -> bool {
    let Some(focus_content) = files
        .iter()
        .find(|(path, _)| path == focus)
        .map(|(_, content)| content)
    else {
        return false;
    };
    symbol_of(candidate).is_some_and(|symbol| mentions(focus_content, &symbol))
}

/// A whole-word match, so `Calculator` is not found inside
/// `StringCalculator` and pulled in as a false reference.
fn mentions(content: &str, symbol: &str) -> bool {
    content.match_indices(symbol).any(|(at, _)| {
        let before = content[..at].chars().next_back();
        let after = content[at + symbol.len()..].chars().next();
        let boundary = |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        boundary(before) && boundary(after)
    })
}

/// The manifest file names whose declared dependencies bound what a
/// refactor may reach for.
pub fn manifest_names(language: Language) -> &'static [&'static str] {
    match language {
        Language::Java => &["pom.xml", "build.gradle", "build.gradle.kts"],
        Language::JavaScript | Language::TypeScript => &["package.json"],
        Language::DotNet => &[],
        Language::Rust => &["Cargo.toml"],
    }
}

/// The dependency coordinates a project declares, read straight out of
/// its manifests.
///
/// Deliberately line-based rather than a parse: the list is prompt
/// context, so a coordinate missed costs a sentence of accuracy, while a
/// build-file parser would cost a dependency and a class of crashes on
/// manifests this harness does not own.
pub fn declared_libraries(language: Language, manifests: &[(String, String)]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for (path, content) in manifests {
        let name = path.rsplit('/').next().unwrap_or(path);
        match language {
            Language::Java if name == "pom.xml" => found.extend(maven_dependencies(content)),
            Language::Java => found.extend(gradle_dependencies(content)),
            Language::JavaScript | Language::TypeScript => {
                found.extend(package_json_dependencies(content));
            }
            Language::Rust => found.extend(cargo_dependencies(content)),
            Language::DotNet => {}
        }
    }
    found.sort();
    found.dedup();
    found
}

/// `groupId:artifactId` per `<dependency>` block in a POM.
fn maven_dependencies(pom: &str) -> Vec<String> {
    let mut found = Vec::new();
    for block in pom.split("<dependency>").skip(1) {
        let block = block.split("</dependency>").next().unwrap_or(block);
        let group = tag(block, "groupId");
        let artifact = tag(block, "artifactId");
        if let (Some(group), Some(artifact)) = (group, artifact) {
            found.push(format!("{group}:{artifact}"));
        }
    }
    found
}

fn tag(block: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(block[start..end].trim().to_string())
}

/// The quoted coordinate on a Gradle configuration line.
fn gradle_dependencies(script: &str) -> Vec<String> {
    const CONFIGURATIONS: [&str; 6] = [
        "implementation",
        "testImplementation",
        "api",
        "compileOnly",
        "runtimeOnly",
        "testRuntimeOnly",
    ];
    script
        .lines()
        .map(str::trim)
        .filter(|line| CONFIGURATIONS.iter().any(|c| line.starts_with(c)))
        .filter_map(quoted)
        .collect()
}

fn quoted(line: &str) -> Option<String> {
    let (open, rest) = line
        .find(['"', '\''])
        .map(|at| (line.as_bytes()[at] as char, &line[at + 1..]))?;
    let end = rest.find(open)?;
    Some(rest[..end].to_string())
}

/// `name@version` for every entry of `dependencies` and
/// `devDependencies`.
fn package_json_dependencies(manifest: &str) -> Vec<String> {
    let Some(parsed) = serde_json::from_str::<serde_json::Value>(manifest).ok() else {
        return Vec::new();
    };
    ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|block| parsed.get(block)?.as_object())
        .flat_map(|entries| {
            entries.iter().map(|(name, version)| {
                let version = version.as_str().unwrap_or_default();
                format!("{name}@{version}")
            })
        })
        .collect()
}

/// The crate names under `[dependencies]` and `[dev-dependencies]`.
fn cargo_dependencies(manifest: &str) -> Vec<String> {
    let Ok(table) = manifest.parse::<toml::Table>() else {
        return Vec::new();
    };
    ["dependencies", "dev-dependencies"]
        .iter()
        .filter_map(|block| table.get(*block)?.as_table())
        .flat_map(|entries| entries.keys().cloned())
        .collect()
}

#[derive(Serialize)]
struct FileContext {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct RoundContext {
    targets: String,
    failures: Vec<String>,
}

/// The instructions for one refactor round: the goal, the behaviour that
/// must survive it, the code that may change, the code that may not, and
/// every earlier round of this loop that broke the suite. Rendered from
/// the `[refactor]` templates.
pub fn refactor_prompt(
    language: Language,
    goal: Option<&str>,
    requirement: Option<&Requirement>,
    scope: &RefactorScope,
    libraries: &[String],
    tests: u32,
    history: &[RefactorRound],
) -> RenderedPrompt {
    let context = |files: &[(String, String)]| -> Vec<FileContext> {
        files
            .iter()
            .map(|(path, content)| FileContext {
                path: path.clone(),
                content: content.clone(),
            })
            .collect()
    };
    let omitted = history.len().saturating_sub(PROMPT_HISTORY_ROUNDS);
    let history_context: Vec<RoundContext> = history[omitted..]
        .iter()
        .map(|round| RoundContext {
            targets: round.targets.join(", "),
            failures: round.failures.iter().map(|f| brief_failure(f)).collect(),
        })
        .collect();
    render(
        "refactor",
        minijinja::context! {
            language => language.display(),
            practices => crate::domain::generation::best_practices(language),
            goal,
            id => requirement.map(|r| r.id.clone()),
            title => requirement.map(|r| r.title.clone()),
            story => requirement.map(|r| r.story.clone()),
            criteria => requirement.map(|r| r.acceptance_criteria.clone()),
            writable => scope.writable_paths(),
            files => context(&scope.writable),
            readonly => context(&scope.readonly),
            libraries,
            tests,
            attempt => history.len() + 1,
            history => history_context,
        },
    )
}

/// The model's rewrite of the writable files, or a reason the whole
/// reply was thrown away.
///
/// An empty array is a valid answer meaning "this code needs no change",
/// so it parses to an empty list rather than an error - the caller
/// decides what to do with a refactor that declined to happen.
pub fn parse_refactor_updates(
    reply: &str,
    writable: &[String],
    structure: &ProjectStructure,
) -> Result<Vec<FileUpdate>, String> {
    let body = strip_code_fences(reply);
    let Some(updates) = decode_json::<Vec<FileUpdate>>(&body, '[') else {
        return Err(
            "the reply was not a JSON array of {path, content} file updates (an empty array is \
             the right answer when nothing needs changing)"
                .into(),
        );
    };
    // A test in the reply is the one failure that is not worth salvaging.
    // Dropping it and keeping the rest would apply a refactor the model
    // believed came with a test change, against tests it never saw
    // surviving - so the whole reply goes back.
    if let Some(update) = updates
        .iter()
        .find(|update| is_test_path(&update.path, structure))
    {
        return Err(format!(
            "the reply rewrites {}, which is a test - tests are read-only in a refactor, so the \
             whole reply was rejected. Refactor only: {}",
            update.path,
            writable.join(", ")
        ));
    }
    if let Some(update) = updates
        .iter()
        .find(|update| !writable.contains(&update.path))
    {
        return Err(format!(
            "the reply rewrites {}, which is not one of the files it may change. Refactor only: {}",
            update.path,
            writable.join(", ")
        ));
    }
    if let Some(update) = updates
        .iter()
        .find(|update| update.content.trim().is_empty())
    {
        return Err(format!(
            "the reply left {} empty, which would delete the file rather than refactor it",
            update.path
        ));
    }
    let mut paths: Vec<&str> = updates.iter().map(|u| u.path.as_str()).collect();
    paths.sort_unstable();
    let duplicates = paths.windows(2).find(|pair| pair[0] == pair[1]);
    if let Some(pair) = duplicates {
        return Err(format!(
            "the reply holds {} twice, so which version to keep is undecidable",
            pair[0]
        ));
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure() -> ProjectStructure {
        ProjectStructure {
            production: Some("kata/src/main/java".into()),
            tests: Some("kata/src/test/java".into()),
            features: Some("kata/src/test/resources/features".into()),
            ..Default::default()
        }
    }

    const PRODUCTION: &str = "kata/src/main/java/Calc.java";
    const UNIT_TEST: &str = "kata/src/test/java/CalcTest.java";
    const FEATURE: &str = "kata/src/test/resources/features/calc.feature";

    #[test]
    fn the_test_roots_and_the_naming_conventions_both_mark_a_test() {
        let structure = structure();
        assert!(is_test_path(UNIT_TEST, &structure));
        assert!(is_test_path(FEATURE, &structure));
        assert!(!is_test_path(PRODUCTION, &structure));
        // A project whose layout was never recorded still has tests.
        let blank = ProjectStructure::default();
        assert!(is_test_path("src/test/java/CalcTest.java", &blank));
        assert!(is_test_path("steps/Checkout.steps.ts", &blank));
        assert!(is_test_path("src/domain/thing_test.rs", &blank));
        assert!(!is_test_path("src/main/java/Calc.java", &blank));
    }

    /// `kata/src/main/java/Calculator.java` must not be dragged in as a
    /// reference to `Calc`.
    #[test]
    fn a_reference_is_a_whole_word_not_a_prefix() {
        assert!(mentions("int x = Calc.add(\"1\");", "Calc"));
        assert!(!mentions("new StringCalculator();", "Calculator"));
        assert!(!mentions("Calculator c;", "Calc"));
    }

    #[test]
    fn scope_keeps_tests_read_only_and_connected_production_writable() {
        let files = vec![
            (PRODUCTION.to_string(), "class Calc { int add() {} }".into()),
            (
                "kata/src/main/java/Parser.java".to_string(),
                "class Parser { Calc c; }".into(),
            ),
            (
                "kata/src/main/java/Unrelated.java".to_string(),
                "class Unrelated {}".into(),
            ),
            (UNIT_TEST.to_string(), "class CalcTest {}".into()),
        ];
        let manifests = vec![("pom.xml".to_string(), "<project/>".to_string())];
        let scope = scope(&files, &structure(), PRODUCTION, &manifests);
        assert_eq!(
            scope.writable_paths(),
            vec![PRODUCTION, "kata/src/main/java/Parser.java"],
            "Parser names Calc, so refactoring Calc may have to touch it"
        );
        let readonly: Vec<&String> = scope.readonly.iter().map(|(p, _)| p).collect();
        assert!(readonly.contains(&&UNIT_TEST.to_string()));
        assert!(readonly.contains(&&"pom.xml".to_string()));
        assert!(
            !readonly.contains(&&"kata/src/main/java/Unrelated.java".to_string()),
            "an unconnected file is not context: {readonly:?}"
        );
    }

    #[test]
    fn a_reply_naming_a_test_is_rejected_whole() {
        let writable = vec![PRODUCTION.to_string()];
        let reply = format!(
            r#"[{{"path": "{PRODUCTION}", "content": "class Calc {{}}"}},
                {{"path": "{UNIT_TEST}", "content": "class CalcTest {{}}"}}]"#
        );
        let error = parse_refactor_updates(&reply, &writable, &structure()).unwrap_err();
        assert!(error.contains(UNIT_TEST), "error: {error}");
        assert!(error.contains("tests are read-only"), "error: {error}");
    }

    #[test]
    fn a_reply_naming_an_unlisted_path_is_rejected() {
        let writable = vec![PRODUCTION.to_string()];
        let reply = r#"[{"path": "kata/pom.xml", "content": "<project/>"}]"#;
        let error = parse_refactor_updates(reply, &writable, &structure()).unwrap_err();
        assert!(error.contains("not one of the files"), "error: {error}");
    }

    #[test]
    fn an_empty_array_is_a_valid_answer_meaning_no_change() {
        let updates =
            parse_refactor_updates("[]", &[PRODUCTION.to_string()], &structure()).unwrap();
        assert!(updates.is_empty());
    }

    #[test]
    fn prose_or_an_object_is_not_a_usable_reply() {
        let writable = vec![PRODUCTION.to_string()];
        assert!(parse_refactor_updates("Sure! Here you go:", &writable, &structure()).is_err());
        let error =
            parse_refactor_updates("I left the code alone.", &writable, &structure()).unwrap_err();
        assert!(
            error.contains("an empty array is the right answer"),
            "{error}"
        );
    }

    #[test]
    fn emptying_a_file_or_listing_it_twice_is_rejected() {
        let writable = vec![PRODUCTION.to_string()];
        let emptied = format!(r#"[{{"path": "{PRODUCTION}", "content": "  "}}]"#);
        let error = parse_refactor_updates(&emptied, &writable, &structure()).unwrap_err();
        assert!(error.contains("would delete the file"), "error: {error}");
        let twice = format!(
            r#"[{{"path": "{PRODUCTION}", "content": "a"}}, {{"path": "{PRODUCTION}", "content": "b"}}]"#
        );
        let error = parse_refactor_updates(&twice, &writable, &structure()).unwrap_err();
        assert!(error.contains("twice"), "error: {error}");
    }

    #[test]
    fn maven_and_gradle_coordinates_are_read_from_the_manifest() {
        let pom = r#"<project>
            <dependencies>
              <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>5.10.2</version></dependency>
              <dependency><groupId>io.cucumber</groupId><artifactId>cucumber-java</artifactId></dependency>
            </dependencies></project>"#;
        assert_eq!(
            declared_libraries(Language::Java, &[("pom.xml".into(), pom.into())]),
            vec![
                "io.cucumber:cucumber-java",
                "org.junit.jupiter:junit-jupiter"
            ]
        );
        let gradle = "dependencies {\n  testImplementation 'io.cucumber:cucumber-java:7.0.0'\n  implementation \"com.google.guava:guava:33.0\"\n  // comment\n}";
        assert_eq!(
            declared_libraries(Language::Java, &[("build.gradle".into(), gradle.into())]),
            vec![
                "com.google.guava:guava:33.0",
                "io.cucumber:cucumber-java:7.0.0"
            ]
        );
    }

    #[test]
    fn node_and_cargo_manifests_are_read_too() {
        let package =
            r#"{"dependencies": {"axios": "^1.0.0"}, "devDependencies": {"vitest": "^2.0.0"}}"#;
        assert_eq!(
            declared_libraries(
                Language::TypeScript,
                &[("package.json".into(), package.into())]
            ),
            vec!["axios@^1.0.0", "vitest@^2.0.0"]
        );
        let cargo = "[dependencies]\nserde = \"1\"\n\n[dev-dependencies]\ntempfile = \"3\"\n";
        assert_eq!(
            declared_libraries(Language::Rust, &[("Cargo.toml".into(), cargo.into())]),
            vec!["serde", "tempfile"]
        );
    }

    #[test]
    fn an_unparseable_manifest_yields_no_libraries_rather_than_failing() {
        assert!(
            declared_libraries(
                Language::Rust,
                &[("Cargo.toml".into(), "not = = toml".into())]
            )
            .is_empty()
        );
        assert!(
            declared_libraries(Language::JavaScript, &[("package.json".into(), "{".into())])
                .is_empty()
        );
    }

    fn requirement() -> Requirement {
        Requirement {
            id: "REQ-003".into(),
            title: "Comma separated numbers are summed".into(),
            status: "implemented".into(),
            story: "As a user, I want comma sums so that totals arrive.".into(),
            acceptance_criteria: vec![
                "Given \"1,2\", when add is called, then the result is 3".into(),
            ],
            feature_file: None,
        }
    }

    #[test]
    fn the_prompt_carries_the_goal_the_behaviour_and_the_read_only_mandate() {
        let scope = RefactorScope {
            writable: vec![(PRODUCTION.to_string(), "class Calc {}".into())],
            readonly: vec![(UNIT_TEST.to_string(), "class CalcTest {}".into())],
        };
        let prompt = refactor_prompt(
            Language::Java,
            Some("extract comma delimiter constant"),
            Some(&requirement()),
            &scope,
            &["io.cucumber:cucumber-java".to_string()],
            9,
            &[],
        );
        assert!(prompt.system.contains("You MUST NOT modify a test"));
        assert!(prompt.system.contains(PRODUCTION), "{}", prompt.system);
        assert!(prompt.user.contains("extract comma delimiter constant"));
        assert!(prompt.user.contains("REQ-003"));
        assert!(prompt.user.contains("green at 9 test(s)"));
        assert!(prompt.user.contains("io.cucumber:cucumber-java"));
        assert!(
            prompt
                .user
                .contains("--- kata/src/test/java/CalcTest.java (read-only) ---")
        );
        assert!(
            !prompt.user.contains("This is round"),
            "a first round has no history"
        );
    }

    #[test]
    fn a_goalless_prompt_asks_the_model_to_judge_and_recounts_rolled_back_rounds() {
        let scope = RefactorScope {
            writable: vec![(PRODUCTION.to_string(), "class Calc {}".into())],
            readonly: Vec::new(),
        };
        let history = vec![RefactorRound {
            targets: vec![PRODUCTION.to_string()],
            failures: vec!["CalcTest.adds: expected 3 but was 0\n\tat Calc.java:9".into()],
        }];
        let prompt = refactor_prompt(Language::Java, None, None, &scope, &[], 9, &history);
        assert!(prompt.user.contains("No specific refactoring was named"));
        assert!(prompt.user.contains("This is round 2"));
        assert!(prompt.user.contains("CalcTest.adds: expected 3 but was 0"));
        assert!(
            !prompt.user.contains("at Calc.java:9"),
            "only the first line of a failure reaches the prompt"
        );
        assert!(!prompt.user.contains("REQ-"), "no requirement was resolved");
    }
}

//! Where a project keeps its production code, tests, features, and step
//! definitions.
//!
//! One resolver answers this for the whole harness: the paths generation
//! writes to, the directory the test runner is pointed at, and the layout
//! the model is briefed with all come from here. They used to be decided
//! in three unrelated places, which is how `steps generate` came to write
//! into a directory no build file owned.
//!
//! Resolution is pure — it reads a listing of the project tree, never the
//! filesystem — so every layout it has to handle is a unit test.

use crate::domain::generation::{decode_json, strip_code_fences};
use crate::domain::language::Language;
use crate::domain::memory::{ProjectStructure, capped_outline};
use crate::domain::paths::confine;
use crate::domain::prompts::{RenderedPrompt, render};
use crate::domain::steps::source_extension;

/// Everything the resolver needs. Paths are project-root-relative with
/// directories ending in `/`, exactly as
/// [`crate::ports::ProjectInventory::list_tree`] reports them.
pub struct LayoutInput<'a> {
    pub language: Language,
    pub build_tool: Option<&'a str>,
    pub tree: &'a [String],
    /// The feature files the spec's requirements name. The spec is the
    /// source of truth, so when a tree holds several buildable modules
    /// its own references decide which one the harness works in.
    pub spec_features: &'a [String],
}

/// The resolved layout plus the module roots discovery could not choose
/// between. A non-empty `candidates` is the only case that warrants
/// asking a model — every other layout is decided here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLayout {
    pub structure: ProjectStructure,
    pub candidates: Vec<String>,
}

impl ResolvedLayout {
    /// True when discovery needs help choosing the module root.
    pub fn is_ambiguous(&self) -> bool {
        !self.candidates.is_empty()
    }
}

/// Directory names, relative to the module root, that each ecosystem
/// keeps things in. Listed most conventional first: the resolver takes
/// the first one the tree actually has, and falls back to the first
/// entry as the path to create in a greenfield project.
struct Conventions {
    production: &'static [&'static str],
    tests: &'static [&'static str],
    features: &'static [&'static str],
    steps_dir: &'static [&'static str],
    steps_file: &'static str,
}

/// Keyed on `(language, build_tool)` rather than language alone so a new
/// ecosystem is one more arm instead of a rewrite — Python would add its
/// `pyproject.toml` module marker and a `tests/` convention here.
fn conventions(language: Language, _build_tool: Option<&str>) -> Conventions {
    match language {
        Language::Java => Conventions {
            production: &["src/main/java"],
            tests: &["src/test/java"],
            features: &["src/test/resources/features", "features"],
            steps_dir: &["src/test/java"],
            steps_file: "GeneratedSteps.java",
        },
        Language::JavaScript => Conventions {
            production: &["src", "lib"],
            tests: &["tests", "test"],
            features: &["features"],
            steps_dir: &["features/step_definitions"],
            steps_file: "generated.steps.js",
        },
        Language::TypeScript => Conventions {
            production: &["src", "lib"],
            tests: &["tests", "test"],
            features: &["features"],
            steps_dir: &["features/step_definitions"],
            steps_file: "generated.steps.ts",
        },
        Language::DotNet => Conventions {
            production: &["src", ""],
            tests: &["Tests", "tests", ""],
            features: &["Features", "features"],
            steps_dir: &["StepDefinitions"],
            steps_file: "GeneratedSteps.cs",
        },
        Language::Rust => Conventions {
            production: &["src"],
            tests: &["tests"],
            features: &["tests/features", "features"],
            steps_dir: &["tests/steps"],
            steps_file: "generated.rs",
        },
    }
}

/// Resolve the layout from the project tree.
pub fn resolve_layout(input: &LayoutInput<'_>) -> ResolvedLayout {
    let modules = module_dirs(input);
    let (module_root, candidates) = choose_module(input, &modules);
    resolve_in(input, module_root, candidates)
}

/// Resolve with the module root already settled — the answer to the
/// ambiguity [`resolve_layout`] reported. The module decides every other
/// path, so the whole layout is re-derived rather than patched.
pub fn resolve_in_module(input: &LayoutInput<'_>, module_root: &str) -> ResolvedLayout {
    resolve_in(input, module_root.to_string(), Vec::new())
}

fn resolve_in(
    input: &LayoutInput<'_>,
    module_root: String,
    candidates: Vec<String>,
) -> ResolvedLayout {
    let conventions = conventions(input.language, input.build_tool);
    let extension = format!(".{}", source_extension(input.language));

    let production = under_module(input.tree, &module_root, conventions.production);
    let tests = under_module(input.tree, &module_root, conventions.tests);
    let features = under_module(input.tree, &module_root, conventions.features);
    let steps_dir = under_module(input.tree, &module_root, conventions.steps_dir);

    // An existing step-definition file wins over the conventional one:
    // generated steps join it rather than landing in a second file whose
    // patterns Cucumber would report as duplicates.
    let step_definitions = input
        .tree
        .iter()
        .filter(|path| is_within(path, &module_root))
        .find(|path| looks_like_steps_file(input.language, path, &extension))
        .cloned()
        .unwrap_or_else(|| join(&steps_dir, conventions.steps_file));

    let package = package_of(input.language, &step_definitions, &tests);

    ResolvedLayout {
        structure: ProjectStructure {
            module_root: at_root(&module_root),
            production: at_root(&production),
            tests: at_root(&tests),
            features: at_root(&features),
            step_definitions: at_root(&step_definitions),
            package,
            spec: spec_file(input.tree),
            outline: Vec::new(),
        },
        candidates,
    }
}

/// The strict instructions for choosing between module roots discovery
/// could not decide between, rendered from the `[layout]` templates.
/// Asked only on that branch: every other layout is resolved here.
pub fn layout_prompt(input: &LayoutInput<'_>, candidates: &[String]) -> RenderedPrompt {
    render(
        "layout",
        minijinja::context! {
            language => input.language.display(),
            build_tool => input.build_tool.unwrap_or_default(),
            candidates,
            spec_features => input.spec_features,
            outline => capped_outline(input.tree),
        },
    )
}

/// Parse the model's choice of module root, with a reason when the reply
/// cannot be used so the retry can tell the model what was wrong.
///
/// The answer has to be one of `candidates` verbatim: discovery already
/// found every buildable module, so the model is choosing between them,
/// never naming a directory the build does not own.
pub fn parse_layout_checked(reply: &str, candidates: &[String]) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Reply {
        #[serde(default, alias = "moduleRoot", alias = "module", alias = "root")]
        module_root: String,
    }

    let body = strip_code_fences(reply);
    let Some(Reply { module_root }) = decode_json::<Reply>(&body, '{') else {
        return Err("the reply was not a JSON object of the form {\"moduleRoot\": \"...\"}".into());
    };
    if module_root.trim().is_empty() {
        return Err("the JSON object held no moduleRoot".into());
    }
    let confined = confine(&module_root)
        .map_err(|jail| format!("moduleRoot {module_root:?} is not usable - {jail}"))?;
    if candidates.contains(&confined) {
        return Ok(confined);
    }
    Err(format!(
        "moduleRoot {confined:?} is not one of the candidates - answer with exactly one of: {}",
        candidates.join(", ")
    ))
}

/// `None` for a path that resolves to the project root itself, which is
/// how the layout says "here" without inventing a `.` or an empty string
/// that would then be joined into a staging path.
fn at_root(path: &str) -> Option<String> {
    (!path.is_empty()).then(|| path.to_string())
}

/// Where a generated test file goes: the layout's test root, plus the
/// package directories the existing code declares, so a new test lands
/// beside the tests it belongs with rather than at a conventional path
/// that may be in no module at all.
pub fn in_test_root(structure: &ProjectStructure, file_name: &str) -> String {
    in_root(
        structure.tests.as_deref(),
        structure.package.as_deref(),
        file_name,
    )
}

/// [`in_test_root`] for production code.
pub fn in_production_root(structure: &ProjectStructure, file_name: &str) -> String {
    in_root(
        structure.production.as_deref(),
        structure.package.as_deref(),
        file_name,
    )
}

fn in_root(root: Option<&str>, package: Option<&str>, file_name: &str) -> String {
    let dir = join(root.unwrap_or_default(), &package_dirs(package));
    join(&dir, file_name)
}

/// A dotted package as directories (`com.example` -> `com/example`), or
/// empty for the ecosystems that do not mirror namespaces onto paths.
fn package_dirs(package: Option<&str>) -> String {
    package.map(|p| p.replace('.', "/")).unwrap_or_default()
}

/// Whether `path` sits inside the module the layout resolved to. Used to
/// keep discovery and the test runner looking at the same subtree: a
/// source file outside the module is not compiled by the build, so
/// counting it would let a gate go green over a bar that cannot pass.
pub fn within_module(structure: &ProjectStructure, path: &str) -> bool {
    is_within(path, structure.module_root.as_deref().unwrap_or_default())
}

/// Every directory holding a build manifest for this ecosystem, `""`
/// meaning the project root.
fn module_dirs(input: &LayoutInput<'_>) -> Vec<String> {
    let mut dirs: Vec<String> = input
        .tree
        .iter()
        .filter(|path| !path.ends_with('/'))
        .filter(|path| is_manifest(input.language, input.build_tool, file_name(path)))
        .map(|path| dir_of(path).to_string())
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn is_manifest(language: Language, build_tool: Option<&str>, name: &str) -> bool {
    let gradle = name == "build.gradle" || name == "build.gradle.kts";
    match (language, build_tool) {
        (Language::Java, Some("Maven")) => name == "pom.xml",
        (Language::Java, Some("Gradle")) => gradle,
        (Language::Java, _) => name == "pom.xml" || gradle,
        (Language::JavaScript | Language::TypeScript, _) => name == "package.json",
        (Language::DotNet, _) => name.ends_with(".csproj"),
        (Language::Rust, _) => name == "Cargo.toml",
    }
}

/// Which module the harness works in, and the candidates left over when
/// the tree cannot say. An aggregator that owns no sources of its own is
/// never the answer, which is what keeps a repository root from winning
/// over the module its build file only lists.
fn choose_module(input: &LayoutInput<'_>, modules: &[String]) -> (String, Vec<String>) {
    if modules.is_empty() {
        return (String::new(), Vec::new());
    }
    if let [only] = modules {
        return (only.clone(), Vec::new());
    }
    // The spec's own feature references are the most direct evidence
    // there is: the requirements say which module they are about.
    let by_spec = owners_of(modules, input.spec_features.iter().map(String::as_str));
    if let [only] = by_spec.as_slice() {
        return ((*only).clone(), Vec::new());
    }
    let extension = format!(".{}", source_extension(input.language));
    let with_sources = owners_of(
        modules,
        input
            .tree
            .iter()
            .filter(|path| path.ends_with(&extension))
            .map(String::as_str),
    );
    if let [only] = with_sources.as_slice() {
        return ((*only).clone(), Vec::new());
    }
    // Several buildable modules: the one holding the Gherkin is the one
    // a BDD workflow runs in.
    let narrowed: Vec<String> = if with_sources.is_empty() {
        modules.to_vec()
    } else {
        with_sources.iter().map(|dir| (*dir).clone()).collect()
    };
    let with_features = owners_of(
        &narrowed,
        input
            .tree
            .iter()
            .filter(|path| path.ends_with(".feature"))
            .map(String::as_str),
    );
    if let [only] = with_features.as_slice() {
        return ((*only).clone(), Vec::new());
    }
    let mut candidates = if with_features.is_empty() {
        narrowed
    } else {
        with_features.iter().map(|dir| (*dir).clone()).collect()
    };
    candidates.sort_by_key(|dir| (depth(dir), dir.clone()));
    if let [only] = candidates.as_slice() {
        return (only.clone(), Vec::new());
    }
    // Provisional answer so every other field still resolves; the
    // candidate list is what sends this to the model.
    (candidates[0].clone(), candidates)
}

/// The modules that own at least one of `paths`: a path belongs to the
/// deepest module directory that contains it, so a nested module's files
/// are never credited to the aggregator above it.
fn owners_of<'a, I>(modules: &'a [String], paths: I) -> Vec<&'a String>
where
    I: Iterator<Item = &'a str>,
{
    let mut owners: Vec<&String> = Vec::new();
    for path in paths {
        if let Some(owner) = modules
            .iter()
            .filter(|dir| is_within(path, dir))
            .max_by_key(|dir| dir.len())
            && !owners.contains(&owner)
        {
            owners.push(owner);
        }
    }
    owners.sort();
    owners
}

/// The first convention present in the tree under `module`, else the
/// first convention as the path a greenfield project should create.
fn under_module(tree: &[String], module: &str, candidates: &'static [&'static str]) -> String {
    let joined: Vec<String> = candidates
        .iter()
        .map(|candidate| join(module, candidate))
        .collect();
    joined
        .iter()
        .find(|candidate| dir_present(tree, candidate))
        .cloned()
        .unwrap_or_else(|| joined[0].clone())
}

fn dir_present(tree: &[String], dir: &str) -> bool {
    if dir.is_empty() {
        return true;
    }
    let slash = format!("{dir}/");
    tree.iter()
        .any(|path| path == dir || path == &slash || path.starts_with(&slash))
}

fn looks_like_steps_file(language: Language, path: &str, extension: &str) -> bool {
    if !path.ends_with(extension) {
        return false;
    }
    let name = file_name(path);
    if name.contains("RunCucumber") {
        return false;
    }
    match language {
        Language::Java | Language::DotNet => name.contains("Steps"),
        Language::JavaScript | Language::TypeScript => {
            name.contains(".steps.") || path.contains("step_definitions/")
        }
        Language::Rust => path.contains("steps/") || name.contains("steps"),
    }
}

/// The package or namespace the existing tests sit in, so generated code
/// declares the same one instead of a conventional guess.
fn package_of(language: Language, step_definitions: &str, tests: &str) -> Option<String> {
    if !matches!(language, Language::Java | Language::DotNet) {
        return None;
    }
    let prefix = format!("{tests}/");
    let relative = step_definitions.strip_prefix(&prefix)?;
    let dir = dir_of(relative);
    (!dir.is_empty()).then(|| dir.replace('/', "."))
}

fn spec_file(tree: &[String]) -> Option<String> {
    let conventional = "requirements/requirements.json";
    if tree.iter().any(|path| path == conventional) {
        return Some(conventional.to_string());
    }
    tree.iter()
        .find(|path| path.ends_with("requirements.json"))
        .cloned()
}

fn join(dir: &str, rest: &str) -> String {
    match (dir.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (false, true) => dir.to_string(),
        (false, false) => format!("{dir}/{rest}"),
    }
}

/// Whether `path` sits inside directory `dir` (`""` being the root).
fn is_within(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.starts_with(&format!("{dir}/"))
}

fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(cut) => &path[..cut],
        None => "",
    }
}

fn file_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(cut) => &path[cut + 1..],
        None => path,
    }
}

fn depth(dir: &str) -> usize {
    if dir.is_empty() {
        0
    } else {
        dir.matches('/').count() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    fn resolve(language: Language, paths: &[&str]) -> ResolvedLayout {
        resolve_layout(&LayoutInput {
            language,
            build_tool: None,
            tree: &tree(paths),
            spec_features: &[],
        })
    }

    fn resolve_with_spec(language: Language, paths: &[&str], features: &[&str]) -> ResolvedLayout {
        resolve_layout(&LayoutInput {
            language,
            build_tool: None,
            tree: &tree(paths),
            spec_features: &tree(features),
        })
    }

    const WORKSHOP: &[&str] = &[
        "pom.xml",
        "requirements/requirements.json",
        "kata/pom.xml",
        "kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java",
        "kata/src/test/java/com/davidparry/workshop/kata/RunCucumberTest.java",
        "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java",
        "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java",
        "kata/src/test/resources/features/string_calculator.feature",
        "smoke-test/pom.xml",
        "smoke-test/src/main/java/com/davidparry/workshop/smoke/AgentWorkflow.java",
        "smoke-test/src/test/java/com/davidparry/workshop/smoke/ToolSweepSteps.java",
        "smoke-test/src/test/resources/features/tool_sweep.feature",
    ];

    // The layout that shipped the bug: an aggregator root plus two
    // buildable modules that both hold Gherkin. Only the spec's own
    // feature references separate them.
    #[test]
    fn the_spec_reference_picks_the_module_in_a_multi_module_tree() {
        let layout = resolve_with_spec(
            Language::Java,
            WORKSHOP,
            &["kata/src/test/resources/features/string_calculator.feature"],
        );
        assert!(
            !layout.is_ambiguous(),
            "candidates: {:?}",
            layout.candidates
        );
        let structure = layout.structure;
        assert_eq!(structure.module_root.as_deref(), Some("kata"));
        assert_eq!(
            structure.step_definitions.as_deref(),
            Some("kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java")
        );
        assert_eq!(structure.tests.as_deref(), Some("kata/src/test/java"));
        assert_eq!(structure.production.as_deref(), Some("kata/src/main/java"));
        assert_eq!(
            structure.features.as_deref(),
            Some("kata/src/test/resources/features")
        );
        assert_eq!(
            structure.package.as_deref(),
            Some("com.davidparry.workshop.kata")
        );
    }

    #[test]
    fn two_gherkin_modules_with_no_spec_reference_are_ambiguous() {
        let layout = resolve(Language::Java, WORKSHOP);
        assert!(layout.is_ambiguous());
        assert_eq!(layout.candidates, vec!["kata", "smoke-test"]);
        // Still answers, so the rest of the report is usable while the
        // developer settles the module root.
        assert_eq!(layout.structure.module_root.as_deref(), Some("kata"));
    }

    #[test]
    fn a_settled_module_root_re_derives_every_other_path_around_it() {
        // The answer to the ambiguity above: the whole layout moves to
        // the chosen module rather than the module root alone.
        let paths = tree(WORKSHOP);
        let layout = resolve_in_module(
            &LayoutInput {
                language: Language::Java,
                build_tool: Some("Maven"),
                tree: &paths,
                spec_features: &[],
            },
            "smoke-test",
        );
        assert!(!layout.is_ambiguous());
        let structure = layout.structure;
        assert_eq!(structure.module_root.as_deref(), Some("smoke-test"));
        assert_eq!(
            structure.step_definitions.as_deref(),
            Some("smoke-test/src/test/java/com/davidparry/workshop/smoke/ToolSweepSteps.java")
        );
        assert_eq!(
            structure.features.as_deref(),
            Some("smoke-test/src/test/resources/features")
        );
        assert_eq!(
            structure.package.as_deref(),
            Some("com.davidparry.workshop.smoke")
        );
    }

    #[test]
    fn the_layout_prompt_carries_the_candidates_the_stack_and_the_tree() {
        let paths = tree(WORKSHOP);
        let prompt = layout_prompt(
            &LayoutInput {
                language: Language::Java,
                build_tool: Some("Maven"),
                tree: &paths,
                spec_features: &tree(&["kata/src/test/resources/features/x.feature"]),
            },
            &tree(&["kata", "smoke-test"]),
        );
        assert_eq!(prompt.section, "layout");
        assert!(prompt.user.contains("Language: Java, build Maven"));
        assert!(prompt.user.contains("- kata\n- smoke-test"));
        assert!(
            prompt
                .user
                .contains("- kata/src/test/resources/features/x.feature")
        );
        assert!(prompt.user.contains("- kata/pom.xml"));
        assert!(prompt.system.contains("ONLY a JSON object"));
        assert!(prompt.system.contains("Never invent a path"));
    }

    #[test]
    fn a_candidate_module_root_parses_with_or_without_fences() {
        let candidates = tree(&["kata", "smoke-test"]);
        assert_eq!(
            parse_layout_checked(r#"{"moduleRoot": "kata"}"#, &candidates).unwrap(),
            "kata"
        );
        assert_eq!(
            parse_layout_checked("```json\n{\"module\": \"smoke-test\"}\n```", &candidates)
                .unwrap(),
            "smoke-test"
        );
        // Windows separators and a trailing slash still name the module.
        assert_eq!(
            parse_layout_checked(r#"{"moduleRoot": "./kata/"}"#, &candidates).unwrap(),
            "kata"
        );
        // Seen live on other sections: a valid object, then commentary.
        assert_eq!(
            parse_layout_checked(
                "{\"moduleRoot\": \"kata\"}\n\nNote: kata holds the calculator.",
                &candidates
            )
            .unwrap(),
            "kata"
        );
    }

    #[test]
    fn a_module_root_outside_the_candidates_is_refused_with_the_list() {
        let candidates = tree(&["kata", "smoke-test"]);
        // Discovery already found every buildable module, so anything
        // else would point generation at a directory no build owns -
        // the bug this whole resolver exists to close.
        let reason = parse_layout_checked(r#"{"moduleRoot": "kata/src/test/java"}"#, &candidates)
            .unwrap_err();
        assert!(reason.contains("kata, smoke-test"), "{reason}");
        assert!(
            parse_layout_checked(r#"{"moduleRoot": "/etc"}"#, &candidates)
                .unwrap_err()
                .contains("absolute paths are not allowed")
        );
        assert!(
            parse_layout_checked(r#"{"moduleRoot": "../elsewhere"}"#, &candidates)
                .unwrap_err()
                .contains("could reach outside")
        );
    }

    #[test]
    fn prose_or_an_empty_choice_is_refused_with_the_shape() {
        let candidates = tree(&["kata", "smoke-test"]);
        assert!(
            parse_layout_checked("The kata module, I think.", &candidates)
                .unwrap_err()
                .contains("moduleRoot")
        );
        assert!(
            parse_layout_checked(r#"{"moduleRoot": "  "}"#, &candidates)
                .unwrap_err()
                .contains("no moduleRoot")
        );
        assert!(parse_layout_checked(r#"{"other": "kata"}"#, &candidates).is_err());
    }

    #[test]
    fn an_aggregator_never_wins_over_the_module_holding_the_sources() {
        let layout = resolve(
            Language::Java,
            &[
                "pom.xml",
                "kata/pom.xml",
                "kata/src/main/java/com/example/Calc.java",
                "kata/src/test/java/com/example/CalcSteps.java",
            ],
        );
        assert!(!layout.is_ambiguous());
        assert_eq!(layout.structure.module_root.as_deref(), Some("kata"));
    }

    #[test]
    fn an_extracted_flat_kata_keeps_the_root_as_its_module() {
        let layout = resolve(
            Language::Java,
            &[
                "pom.xml",
                "src/main/java/com/example/StringCalculator.java",
                "src/test/java/com/example/StringCalculatorTest.java",
                "src/test/java/com/example/StringCalculatorSteps.java",
            ],
        );
        assert!(!layout.is_ambiguous());
        assert_eq!(layout.structure.module_root, None);
        assert_eq!(
            layout.structure.step_definitions.as_deref(),
            Some("src/test/java/com/example/StringCalculatorSteps.java")
        );
        assert_eq!(layout.structure.package.as_deref(), Some("com.example"));
    }

    // infer_structure used to return (None, None) for .NET, so the model
    // was briefed with no layout at all and generation fell back to a
    // conventional path that ignored the project.
    #[test]
    fn dotnet_resolves_the_layout_that_used_to_come_back_empty() {
        let layout = resolve(
            Language::DotNet,
            &[
                "Calc/Calc.csproj",
                "Calc/StringCalculator.cs",
                "Calc/Tests/CalculatorSteps.cs",
                "Calc/Features/calc.feature",
            ],
        );
        assert!(!layout.is_ambiguous());
        assert_eq!(layout.structure.module_root.as_deref(), Some("Calc"));
        assert_eq!(layout.structure.tests.as_deref(), Some("Calc/Tests"));
        assert_eq!(
            layout.structure.step_definitions.as_deref(),
            Some("Calc/Tests/CalculatorSteps.cs")
        );
        assert_eq!(layout.structure.features.as_deref(), Some("Calc/Features"));
    }

    // A greenfield tree has to keep resolving to the conventional paths
    // the scaffold writes, or `spec init` and generation disagree.
    #[test]
    fn an_empty_tree_falls_back_to_each_ecosystem_convention() {
        let expected = [
            (
                Language::Java,
                "src/test/java/GeneratedSteps.java",
                Some("src/main/java"),
            ),
            (
                Language::JavaScript,
                "features/step_definitions/generated.steps.js",
                Some("src"),
            ),
            (
                Language::TypeScript,
                "features/step_definitions/generated.steps.ts",
                Some("src"),
            ),
            // .NET keeps its sources beside the project file, so the
            // production root is the project root itself.
            (Language::DotNet, "StepDefinitions/GeneratedSteps.cs", None),
            (Language::Rust, "tests/steps/generated.rs", Some("src")),
        ];
        for (language, steps, production) in expected {
            let layout = resolve(language, &[]);
            assert_eq!(
                layout.structure.step_definitions.as_deref(),
                Some(steps),
                "{language:?}"
            );
            assert_eq!(
                layout.structure.production.as_deref(),
                production,
                "{language:?}"
            );
            assert!(!layout.is_ambiguous(), "{language:?}");
        }
    }

    #[test]
    fn an_existing_step_file_wins_over_the_conventional_one() {
        let layout = resolve(
            Language::TypeScript,
            &[
                "package.json",
                "tsconfig.json",
                "src/calculator.ts",
                "features/calc.feature",
                "features/step_definitions/calculator.steps.ts",
            ],
        );
        assert_eq!(
            layout.structure.step_definitions.as_deref(),
            Some("features/step_definitions/calculator.steps.ts")
        );
        assert_eq!(layout.structure.package, None);
    }

    #[test]
    fn the_cucumber_runner_is_not_mistaken_for_step_definitions() {
        let layout = resolve(
            Language::Java,
            &[
                "pom.xml",
                "src/test/java/com/example/RunCucumberTest.java",
                "src/main/java/com/example/Calc.java",
            ],
        );
        assert_eq!(
            layout.structure.step_definitions.as_deref(),
            Some("src/test/java/GeneratedSteps.java")
        );
    }

    #[test]
    fn gradle_markers_resolve_a_java_module() {
        let layout = resolve_layout(&LayoutInput {
            language: Language::Java,
            build_tool: Some("Gradle"),
            tree: &tree(&[
                "build.gradle.kts",
                "app/build.gradle.kts",
                "app/src/main/java/com/example/Calc.java",
                "app/src/test/java/com/example/CalcSteps.java",
            ]),
            spec_features: &[],
        });
        assert_eq!(layout.structure.module_root.as_deref(), Some("app"));
        assert_eq!(layout.structure.tests.as_deref(), Some("app/src/test/java"));
    }

    #[test]
    fn a_tree_with_no_build_file_resolves_at_the_root() {
        let layout = resolve(Language::Rust, &["src/lib.rs", "tests/steps/calc.rs"]);
        assert_eq!(layout.structure.module_root, None);
        assert_eq!(
            layout.structure.step_definitions.as_deref(),
            Some("tests/steps/calc.rs")
        );
    }

    #[test]
    fn the_spec_path_is_found_wherever_it_sits() {
        assert_eq!(
            resolve(Language::Java, &["requirements/requirements.json"])
                .structure
                .spec
                .as_deref(),
            Some("requirements/requirements.json")
        );
        assert_eq!(
            resolve(Language::Java, &["spec/requirements.json"])
                .structure
                .spec
                .as_deref(),
            Some("spec/requirements.json")
        );
        assert_eq!(resolve(Language::Java, &[]).structure.spec, None);
    }
}

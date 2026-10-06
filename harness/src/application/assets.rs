//! The requirement/asset queries shared by step generation, the
//! implement preflight, and `spec status`: looking a requirement up by
//! id, finding undefined steps, and surveying the assets an
//! implementation rests on.

use crate::application::spec_service::ServiceError;
use crate::domain::feature::FeatureDoc;
use crate::domain::generation::{
    ImplementAsset, implementation_file_name, implementation_target_path, is_pending_step_body,
    steps_target_path, unit_test_file_name, unit_test_target_path,
};
use crate::domain::language::Language;
use crate::domain::layout::{in_production_root, in_test_root};
use crate::domain::memory::ProjectStructure;
use crate::domain::model::{Requirement, Spec, SpecCatalog};
use crate::domain::neighborhood;
use crate::domain::steps::{
    MissingStep, extract_definitions, extract_patterns, find_missing, pattern_matches,
    source_extension, split_step,
};
use crate::ports::{FeatureCatalog, SourceFiles, SpecRepository};

/// The requirement with `req_id`, or the refusal naming the recovery
/// command.
pub(crate) fn find_requirement<'a>(
    spec: &'a Spec,
    req_id: &str,
) -> Result<&'a Requirement, ServiceError> {
    spec.requirements
        .iter()
        .find(|r| r.id == req_id)
        .ok_or_else(|| {
            ServiceError(format!(
                "No requirement with id {req_id}. Call spec list to see valid ids."
            ))
        })
}

/// Every feature step with no matching definition in the sources.
pub(crate) fn find_missing_steps(
    features: &impl FeatureCatalog,
    sources: &impl SourceFiles,
    language: Language,
) -> Result<Vec<MissingStep>, ServiceError> {
    let docs: Vec<FeatureDoc> = features
        .list()?
        .iter()
        .map(|summary| features.read(&summary.path))
        .collect::<Result<_, _>>()?;
    let patterns: Vec<String> = sources
        .sources(source_extension(language))?
        .iter()
        .flat_map(|file| extract_patterns(language, &file.content))
        .collect();
    Ok(find_missing(&docs, &patterns))
}

/// The first feature file carrying `tag` on the feature or any scenario
/// in it.
pub(crate) fn feature_tagged(
    features: &impl FeatureCatalog,
    tag: &str,
) -> Result<Option<String>, ServiceError> {
    for summary in features.list()? {
        let doc = features.read(&summary.path)?;
        if doc.all_tags().iter().any(|t| t == tag) {
            return Ok(Some(doc.path));
        }
    }
    Ok(None)
}

/// Survey the assets a requirement's implementation rests on - the
/// tagged scenario, the step definitions, the unit test, the
/// production file - and the finding (naming the command to run)
/// for each one that is missing. Shared by the implement preflight
/// and `spec status`.
pub(crate) fn asset_survey(
    features: &impl FeatureCatalog,
    sources: &impl SourceFiles,
    language: Language,
    requirement: &Requirement,
    project: &str,
    layout: &ProjectStructure,
    explicit: Option<&str>,
) -> Result<(Vec<ImplementAsset>, Vec<String>), ServiceError> {
    let req_id = requirement.id.as_str();
    let mut assets = Vec::new();
    let mut findings = Vec::new();
    let tag = format!("@{req_id}");
    let tagged_feature = feature_tagged(features, &tag)?;
    if tagged_feature.is_none() {
        findings.push(format!(
            "No scenario is tagged {tag} - add one with spec scenario add."
        ));
    }
    assets.push(ImplementAsset {
        role: format!("scenario tagged {tag}"),
        path: tagged_feature.clone().unwrap_or_else(|| {
            requirement
                .feature_file
                .clone()
                .unwrap_or_else(|| "features/*.feature".into())
        }),
        present: tagged_feature.is_some(),
    });

    let source_files = sources.sources(source_extension(language))?;

    let missing_steps = find_missing_steps(features, sources, language)?;
    if !missing_steps.is_empty() {
        findings.push(format!(
            "{} step(s) have no definition - run spec steps generate.",
            missing_steps.len()
        ));
    }
    let steps_path = steps_path(&source_files, language, layout);
    assets.push(ImplementAsset {
        role: "step definitions (every step defined)".into(),
        path: steps_path.clone(),
        present: missing_steps.is_empty(),
    });

    let unit_path = unit_test_path(&source_files, language, req_id, layout);
    let conventional_unit = unit_test_target_path(language, req_id);
    let unit_test_present = source_files.iter().any(|file| {
        file.path == conventional_unit
            || (file.path == unit_path && mentions_requirement(&file.content, req_id))
    });
    if !unit_test_present {
        findings.push(format!(
            "The unit test {unit_path} does not exist - run spec unittest \
             generate {req_id}."
        ));
    }
    assets.push(ImplementAsset {
        role: "unit test".into(),
        path: unit_path.clone(),
        present: unit_test_present,
    });

    // The code that runs this requirement's scenarios is the evidence of
    // where its production code belongs: step definitions reach the
    // behavior, so whatever they reach is what the work is missing from.
    let evidence = scenario_evidence(features, &source_files, language, &tag)?;
    let production = production_path(
        &source_files,
        language,
        project,
        layout,
        &evidence,
        explicit,
    );
    if production.is_none() {
        findings.push(format!(
            "Cannot tell which production file {req_id} belongs in - no step \
             definition its scenarios run through names any of them. Name it \
             with spec implement {req_id} --into <path>, or write the steps \
             first so they point at the code."
        ));
    }
    assets.push(ImplementAsset {
        role: "production code (the attempt creates it when missing)".into(),
        path: production
            .clone()
            .unwrap_or_else(|| "unknown - pass --into <path>".into()),
        present: production
            .as_ref()
            .is_some_and(|path| source_files.iter().any(|file| &file.path == path)),
    });
    Ok((assets, findings))
}

/// The code this requirement's own scenarios run through.
///
/// Each step of each scenario carrying the tag is matched to the step
/// definition that binds it, and that definition's body is collected.
/// The result names the behavior under test — the servers, services and
/// types the scenarios actually reach — rather than the whole project.
pub(crate) fn scenario_evidence(
    features: &impl FeatureCatalog,
    files: &[crate::ports::SourceFile],
    language: Language,
    tag: &str,
) -> Result<String, ServiceError> {
    let mut steps = Vec::new();
    for summary in features.list()? {
        let doc = features.read(&summary.path)?;
        let feature_tags = doc.tags.clone();
        for scenario in &doc.scenarios {
            if !scenario.tags.iter().chain(&feature_tags).any(|t| t == tag) {
                continue;
            }
            // Only the action and the assertion. A Given builds the
            // fixture - requirements, specs, temp directories - and its
            // code points at the harness's own plumbing rather than at
            // the behavior the requirement is about.
            let mut keyword = String::from("Given");
            for step in &scenario.steps {
                let (word, text) = split_step(step);
                if word != "And" && word != "But" {
                    keyword = word.to_string();
                }
                if keyword == "When" || keyword == "Then" {
                    steps.push(text.to_string());
                }
            }
        }
    }
    if steps.is_empty() {
        return Ok(String::new());
    }
    let mut evidence = String::new();
    for file in files {
        let mut bodies = String::new();
        for (pattern, body) in extract_definitions(language, &file.content) {
            // A step generated minutes ago and still pending says
            // nothing about where the code belongs. Dropping the whole
            // window when a marker appears in it can cost a little real
            // evidence where a window runs on into the next definition,
            // which is the safe direction to err: too little evidence
            // makes the caller ask, too much sends it somewhere wrong.
            if steps.iter().any(|step| pattern_matches(&pattern, step))
                && !is_pending_step_body(language, body)
            {
                bodies.push_str(body);
                bodies.push('\n');
            }
        }
        if bodies.is_empty() {
            continue;
        }
        // A step usually delegates: the line naming the production type
        // is one call further in, inside a helper the glue file keeps to
        // itself. Follow those once, or the evidence stops at the test's
        // own vocabulary.
        evidence.push_str(&local_helpers(&bodies, &file.content));
        evidence.push_str(&bodies);
    }
    Ok(evidence)
}

/// How much of a helper to follow. Shorter than a step's own window:
/// this is one hop of context, not a second body of evidence.
const HELPER_WINDOW: usize = 400;

/// The bodies of the helpers `bodies` calls that `source` declares
/// itself, one hop deep.
fn local_helpers(bodies: &str, source: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    for name in IDENTIFIER.find_iter(bodies) {
        let name = name.as_str();
        if !seen.insert(name) {
            continue;
        }
        let Some(at) = source.find(&format!("fn {name}")) else {
            continue;
        };
        let after = &source[at..];
        let end = after
            .char_indices()
            .nth(HELPER_WINDOW)
            .map_or(after.len(), |(i, _)| i);
        out.push_str(&after[..end]);
        out.push('\n');
    }
    out
}

static IDENTIFIER: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]{2,}").expect("valid"));

/// The file that actually binds the Gherkin steps: the one holding the
/// most definitions. Taking the first file with any pattern at all picks
/// up a fixture or a prompt template that merely quotes one, and the
/// whole survey then points at a file no scenario runs through.
fn steps_file(
    files: &[crate::ports::SourceFile],
    language: Language,
) -> Option<&crate::ports::SourceFile> {
    files
        .iter()
        .map(|file| (extract_patterns(language, &file.content).len(), file))
        .filter(|(defined, _)| *defined > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.path.cmp(&a.1.path)))
        .map(|(_, file)| file)
}

/// Where generated step definitions go: the file that already holds
/// them, else the path the layout resolved. Adding a second file whose
/// patterns overlap the first is what Cucumber reports as a duplicate
/// step definition, so an existing file always wins.
pub(crate) fn steps_path(
    files: &[crate::ports::SourceFile],
    language: Language,
    layout: &ProjectStructure,
) -> String {
    steps_file(files, language)
        .map(|file| file.path.clone())
        .or_else(|| layout.step_definitions.clone())
        .unwrap_or_else(|| steps_target_path(language).to_string())
}

fn is_unit_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    (name.ends_with("Test.java")
        || name.ends_with("Test.cs")
        || name.ends_with(".test.js")
        || name.ends_with(".test.ts")
        || name.ends_with("_test.rs"))
        && !name.contains("RunCucumber")
}

fn mentions_requirement(content: &str, req_id: &str) -> bool {
    content.contains(req_id)
}

/// Where to write or look for this requirement's unit test: an existing
/// test that already names the id, else the project's calculator-style
/// test class, else the `Req00NTest` path inside the layout's test root.
pub(crate) fn unit_test_path(
    files: &[crate::ports::SourceFile],
    language: Language,
    req_id: &str,
    layout: &ProjectStructure,
) -> String {
    files
        .iter()
        .find(|file| is_unit_test_path(&file.path) && mentions_requirement(&file.content, req_id))
        .or_else(|| {
            files.iter().find(|file| {
                is_unit_test_path(&file.path)
                    && !file
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .starts_with("Req")
            })
        })
        .map(|file| file.path.clone())
        .unwrap_or_else(|| in_test_root(layout, &unit_test_file_name(language, req_id)))
}

/// The production file the work belongs in.
///
/// `evidence` is code that shows where the behavior is missing — the
/// bodies of the step definitions this requirement's scenarios run
/// through. When it is given, the production file whose symbols it
/// mentions most wins. `explicit` is the developer's own answer and
/// outranks everything.
///
/// Convention is the last resort and is trusted only where it cannot be
/// wrong: a conventional path that already exists, a project with no
/// production code at all (the attempt creates the file anyway), or one
/// that holds a single production file and so offers no choice.
///
/// `None` when the project has several production files and nothing
/// points at any of them - the honest answer, and the one that makes
/// the caller ask rather than guess.
pub(crate) fn production_path(
    files: &[crate::ports::SourceFile],
    language: Language,
    project: &str,
    layout: &ProjectStructure,
    evidence: &str,
    explicit: Option<&str>,
) -> Option<String> {
    // The developer's own answer ends the question.
    if let Some(path) = explicit {
        return Some(path.to_string());
    }
    let root = layout.production.as_deref();
    let under_root = |path: &str| match root {
        Some(root) => path.starts_with(&format!("{root}/")),
        None => path.contains("src/main/"),
    };
    // Evidence first. The conventional name is a guess that has to be
    // made before a project exists - for Rust it is always `src/lib.rs`
    // - and on anything past a kata the guess is wrong. When the
    // scenarios say which file the behavior is missing from, they
    // outrank it.
    if let Some(target) = from_evidence(files, language, evidence, &under_root) {
        return Some(target);
    }
    let conventional = implementation_target_path(language, project);
    if convention_names_this_project(language, project)
        && files.iter().any(|file| file.path == conventional)
    {
        return Some(conventional);
    }
    let mut under: Vec<&crate::ports::SourceFile> =
        files.iter().filter(|file| under_root(&file.path)).collect();
    match under.len() {
        // No production code yet, so there is nothing to disambiguate:
        // convention names the file the attempt is about to create.
        0 => Some(match root {
            Some(_) => in_production_root(layout, &implementation_file_name(language, project)),
            None => conventional,
        }),
        // One candidate is not a guess.
        1 => Some(under.remove(0).path.clone()),
        // Several, and nothing points at any of them. Taking the first
        // is a coin flip whose loser is an implementation written into
        // an unrelated module.
        _ => None,
    }
}

/// Whether convention names a file for *this* project, or only the
/// ecosystem's entry point.
///
/// Java and C# fold the project name into the path, so an existing
/// `StringCalculator.java` really is this kata's production file. Rust
/// and the JS family have a fixed entry point instead - `src/lib.rs` is
/// the answer whatever the project is called, and it exists in every
/// crate ever generated. Trusting that would let convention win every
/// time and put the work in the module list.
///
/// Asked by changing the project name and seeing whether the answer
/// follows, rather than by listing which ecosystems are which.
fn convention_names_this_project(language: Language, project: &str) -> bool {
    let renamed = format!("{project} renamed");
    implementation_file_name(language, project) != implementation_file_name(language, &renamed)
}

/// The file the scenarios' own code points at, when it points clearly.
fn from_evidence(
    files: &[crate::ports::SourceFile],
    language: Language,
    evidence: &str,
    under_root: &impl Fn(&str) -> bool,
) -> Option<String> {
    if evidence.trim().is_empty() {
        return None;
    }
    let pairs: Vec<(String, String)> = files
        .iter()
        .map(|file| (file.path.clone(), file.content.clone()))
        .collect();
    neighborhood::nearest_to_evidence(language, &pairs, evidence, under_root).map(str::to_string)
}

/// Simple class name of the production type (`StringCalculator.java` →
/// `StringCalculator`).
pub(crate) fn production_type_name(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    (!stem.is_empty()).then(|| stem.to_string())
}

/// The spec, in the error type the services speak.
pub(crate) fn load_spec(repository: &impl SpecRepository) -> Result<Spec, ServiceError> {
    repository.load().map_err(|e| ServiceError(e.0))
}

/// The spec tree file by file, in the error type the services speak.
pub(crate) fn load_catalog(repository: &impl SpecRepository) -> Result<SpecCatalog, ServiceError> {
    repository.load_catalog().map_err(|e| ServiceError(e.0))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::ports::SourceFile;
    use crate::test_support::{
        FakeSources, InMemoryFeatureCatalog, calculator_catalog, flat_layout,
    };

    fn one_scenario_tagged(tag: &str, when: &str, then: &str) -> InMemoryFeatureCatalog {
        InMemoryFeatureCatalog {
            files: HashMap::from([(
                "features/coverage.feature".to_string(),
                format!(
                    "Feature: Coverage\n\n  {tag}\n  Scenario: Reported\n    \
                     When {when}\n    Then {then}\n"
                ),
            )]),
        }
    }

    fn req(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            title: "Two numbers".into(),
            status: "pending".into(),
            story: "As a user, I want sums so that I can add.".into(),
            acceptance_criteria: vec!["Given a, when b, then 3".into()],
            feature_file: Some("features/calc.feature".into()),
        }
    }

    /// The definitions this requirement's scenarios bind to were
    /// generated minutes ago and are still `todo!`. They echo the
    /// Gherkin back and name no production code, so reading them as a
    /// statement about where the work belongs is reading noise.
    #[test]
    fn step_definitions_nobody_has_written_yet_are_no_evidence() {
        let features = one_scenario_tagged(
            "@REQ-001",
            "coverage is requested",
            "the verdict is reported",
        );
        let files = vec![SourceFile {
            path: "tests/cucumber.rs".into(),
            content: "#[when(expr = \"coverage is requested\")]\n\
                 fn a(_w: &mut SpecWorld) {\n    \
                 todo!(\"implement step: coverage is requested\");\n}\n\
                 #[then(expr = \"the verdict is reported\")]\n\
                 fn b(_w: &mut SpecWorld) {\n    \
                 todo!(\"implement step: the verdict is reported\");\n}\n"
                .into(),
        }];
        let evidence =
            scenario_evidence(&features, &files, Language::Rust, "@REQ-001").expect("evidence");
        assert!(evidence.trim().is_empty(), "got: {evidence}");
    }

    /// The same shape once the steps are written: now there is code to
    /// read, and the evidence names it.
    #[test]
    fn a_written_step_is_read_as_evidence() {
        let features = one_scenario_tagged(
            "@REQ-001",
            "coverage is requested",
            "the verdict is reported",
        );
        let files = vec![SourceFile {
            path: "tests/cucumber.rs".into(),
            content: "#[when(expr = \"coverage is requested\")]\n\
                 fn a(w: &mut SpecWorld) {\n    \
                 w.verdict = CoverageReport::for_requirement(&w.id);\n}\n"
                .into(),
        }];
        let evidence =
            scenario_evidence(&features, &files, Language::Rust, "@REQ-001").expect("evidence");
        assert!(evidence.contains("CoverageReport"), "got: {evidence}");
    }

    #[test]
    fn the_glue_file_is_the_one_binding_the_most_steps() {
        // The bug this closes: a prompt template that quotes a single
        // step pattern sorted ahead of the real glue file, so the
        // survey, and the production target inferred from it, pointed at
        // a file no scenario ever runs through.
        let files = vec![
            SourceFile {
                path: "src/domain/generation.rs".into(),
                content: r##"const SAMPLE: &str = "x";
                    #[given("a calculator")]
                    fn fixture() {}"##
                    .into(),
            },
            SourceFile {
                path: "tests/cucumber.rs".into(),
                content: r##"
                    #[given("a calculator")]
                    fn a(w: &mut W) {}
                    #[when("2 and 3 are added")]
                    fn b(w: &mut W) {}
                    #[then(regex = r#"^the result is "(\d+)"$"#)]
                    fn c(w: &mut W) {}
                "##
                .into(),
            },
        ];
        assert_eq!(
            steps_path(&files, Language::Rust, &flat_layout(Language::Rust)),
            "tests/cucumber.rs"
        );
    }

    #[test]
    fn a_brownfield_test_without_the_req_id_is_the_generate_target_but_missing() {
        let sources = FakeSources(vec![SourceFile {
            path: "src/test/java/com/example/StringCalculatorTest.java".into(),
            content: "class StringCalculatorTest { @Test void existing() {} }".into(),
        }]);
        let (assets, findings) = asset_survey(
            &calculator_catalog(),
            &sources,
            Language::Java,
            &req("REQ-003"),
            "String Calculator Kata",
            &flat_layout(Language::Java),
            None,
        )
        .unwrap();
        let unit = assets.iter().find(|a| a.role == "unit test").unwrap();
        assert_eq!(
            unit.path,
            "src/test/java/com/example/StringCalculatorTest.java"
        );
        assert!(!unit.present);
        assert!(
            findings
                .iter()
                .any(|f| f.contains("spec unittest generate REQ-003"))
        );
    }

    #[test]
    fn a_brownfield_test_that_names_the_requirement_is_present() {
        let sources = FakeSources(vec![SourceFile {
            path: "src/test/java/com/example/StringCalculatorTest.java".into(),
            content: "@DisplayName(\"REQ-003: two numbers\") @Test void two() {}".into(),
        }]);
        let (assets, _) = asset_survey(
            &calculator_catalog(),
            &sources,
            Language::Java,
            &req("REQ-003"),
            "String Calculator Kata",
            &flat_layout(Language::Java),
            None,
        )
        .unwrap();
        let unit = assets.iter().find(|a| a.role == "unit test").unwrap();
        assert!(unit.present);
    }

    #[test]
    fn production_path_prefers_an_existing_src_main_class() {
        let files = vec![SourceFile {
            path: "src/main/java/com/example/StringCalculator.java".into(),
            content: "class StringCalculator {}".into(),
        }];
        assert_eq!(
            production_path(
                &files,
                Language::Java,
                "String Calculator Kata",
                &flat_layout(Language::Java),
                "",
                None,
            )
            .as_deref(),
            Some("src/main/java/com/example/StringCalculator.java")
        );
    }

    /// The kata case: nothing is written yet, so there is nothing to
    /// disambiguate and convention names the file to create.
    #[test]
    fn a_project_with_no_production_code_still_gets_the_conventional_target() {
        let files = vec![SourceFile {
            path: "src/test/java/com/example/StringCalculatorTest.java".into(),
            content: "class StringCalculatorTest {}".into(),
        }];
        assert!(
            production_path(
                &files,
                Language::Java,
                "String Calculator Kata",
                &flat_layout(Language::Java),
                "",
                None,
            )
            .is_some()
        );
    }

    /// Production files exist and nothing points at one of them.
    /// Answering anyway writes the implementation into whichever file
    /// happened to sort first.
    #[test]
    fn a_project_whose_tests_point_nowhere_names_no_target() {
        let files = vec![
            SourceFile {
                path: "src/main/java/com/example/Alpha.java".into(),
                content: "class Alpha {}".into(),
            },
            SourceFile {
                path: "src/main/java/com/example/Beta.java".into(),
                content: "class Beta {}".into(),
            },
        ];
        assert_eq!(
            production_path(
                &files,
                Language::Java,
                "String Calculator Kata",
                &flat_layout(Language::Java),
                "",
                None,
            ),
            None
        );
    }

    /// `src/lib.rs` is every crate's entry point, not this
    /// requirement's production file. Letting it win because it happens
    /// to exist is how the work ends up in the module list.
    #[test]
    fn a_rust_crates_entry_point_is_not_mistaken_for_the_target() {
        let files = vec![
            SourceFile {
                path: "src/lib.rs".into(),
                content: "pub mod mcp;\npub mod domain;".into(),
            },
            SourceFile {
                path: "src/mcp.rs".into(),
                content: "pub struct WorkflowServer {}".into(),
            },
        ];
        assert_eq!(
            production_path(
                &files,
                Language::Rust,
                "spec harness",
                &flat_layout(Language::Rust),
                "",
                None,
            ),
            None
        );
    }

    /// A Java kata's conventional name carries the project in it, so an
    /// existing file by that name really is the one.
    #[test]
    fn a_name_built_from_the_project_is_still_worth_trusting() {
        let files = vec![
            SourceFile {
                path: "src/main/java/StringCalculatorKata.java".into(),
                content: "class StringCalculatorKata {}".into(),
            },
            SourceFile {
                path: "src/main/java/Other.java".into(),
                content: "class Other {}".into(),
            },
        ];
        assert_eq!(
            production_path(
                &files,
                Language::Java,
                "String Calculator Kata",
                &flat_layout(Language::Java),
                "",
                None,
            )
            .as_deref(),
            Some("src/main/java/StringCalculatorKata.java")
        );
    }

    /// The developer's answer ends the question, whatever the evidence
    /// would have said.
    #[test]
    fn an_explicit_target_outranks_everything_else() {
        let files = vec![SourceFile {
            path: "src/main/java/com/example/StringCalculator.java".into(),
            content: "class StringCalculator {}".into(),
        }];
        assert_eq!(
            production_path(
                &files,
                Language::Java,
                "String Calculator Kata",
                &flat_layout(Language::Java),
                "",
                Some("src/main/java/com/example/Elsewhere.java"),
            )
            .as_deref(),
            Some("src/main/java/com/example/Elsewhere.java")
        );
    }

    /// The preflight has to say it cannot tell, and name the way out.
    #[test]
    fn the_survey_asks_instead_of_guessing_when_nothing_points_at_the_code() {
        let sources = FakeSources(vec![
            SourceFile {
                path: "src/main/java/com/example/Alpha.java".into(),
                content: "class Alpha {}".into(),
            },
            SourceFile {
                path: "src/main/java/com/example/Beta.java".into(),
                content: "class Beta {}".into(),
            },
        ]);
        let (assets, findings) = asset_survey(
            &calculator_catalog(),
            &sources,
            Language::Java,
            &req("REQ-003"),
            "String Calculator Kata",
            &flat_layout(Language::Java),
            None,
        )
        .unwrap();
        assert!(
            findings.iter().any(|f| f.contains("--into")),
            "got: {findings:?}"
        );
        let production = assets
            .iter()
            .find(|a| a.role.starts_with("production code"))
            .expect("the production asset");
        assert!(!production.present);
    }
}

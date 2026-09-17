//! The requirement/asset queries shared by step generation, the
//! implement preflight, and `spec status`: looking a requirement up by
//! id, finding undefined steps, and surveying the assets an
//! implementation rests on.

use crate::application::spec_service::ServiceError;
use crate::domain::feature::FeatureDoc;
use crate::domain::generation::{
    ImplementAsset, implementation_target_path, steps_target_path, unit_test_target_path,
};
use crate::domain::language::Language;
use crate::domain::model::{Requirement, Spec, SpecCatalog, resolve_catalog};
use crate::domain::steps::{MissingStep, extract_patterns, find_missing, source_extension};
use crate::ports::{ChangeStore, FeatureCatalog, SourceFiles, SpecRepository};

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
    req_id: &str,
    requirement: &Requirement,
    project: &str,
) -> Result<(Vec<ImplementAsset>, Vec<String>), ServiceError> {
    let mut assets = Vec::new();
    let mut findings = Vec::new();
    let tag = format!("@{req_id}");
    let tagged_feature = feature_tagged(features, &tag)?;
    if tagged_feature.is_none() {
        findings.push(format!(
            "No scenario is tagged {tag} - add one with spec scenario add, \
             then spec changes commit."
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
            "{} step(s) have no definition - run spec steps generate, \
             then spec changes commit.",
            missing_steps.len()
        ));
    }
    let steps_path = steps_file(&source_files, language)
        .map(|f| f.path.clone())
        .unwrap_or_else(|| steps_target_path(language).to_string());
    assets.push(ImplementAsset {
        role: "step definitions (every step defined)".into(),
        path: steps_path,
        present: missing_steps.is_empty(),
    });

    let unit_path = unit_test_path(&source_files, language, req_id);
    let conventional_unit = unit_test_target_path(language, req_id);
    let unit_test_present = source_files.iter().any(|file| {
        file.path == conventional_unit
            || (file.path == unit_path && mentions_requirement(&file.content, req_id))
    });
    if !unit_test_present {
        findings.push(format!(
            "The unit test {unit_path} does not exist - run spec unittest \
             generate {req_id}, then spec changes commit."
        ));
    }
    assets.push(ImplementAsset {
        role: "unit test".into(),
        path: unit_path,
        present: unit_test_present,
    });

    let production = production_path(&source_files, language, project);
    assets.push(ImplementAsset {
        role: "production code (the attempt creates it when missing)".into(),
        path: production.clone(),
        present: source_files.iter().any(|file| file.path == production),
    });
    Ok((assets, findings))
}

fn steps_file(
    files: &[crate::ports::SourceFile],
    language: Language,
) -> Option<&crate::ports::SourceFile> {
    files
        .iter()
        .find(|file| !extract_patterns(language, &file.content).is_empty())
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
/// test class, else the greenfield `Req00NTest` path.
pub(crate) fn unit_test_path(
    files: &[crate::ports::SourceFile],
    language: Language,
    req_id: &str,
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
        .unwrap_or_else(|| unit_test_target_path(language, req_id))
}

/// The production file: an existing `src/main` source, otherwise the
/// conventional greenfield path named after the spec project.
pub(crate) fn production_path(
    files: &[crate::ports::SourceFile],
    language: Language,
    project: &str,
) -> String {
    let conventional = implementation_target_path(language, project);
    if files.iter().any(|file| file.path == conventional) {
        return conventional;
    }
    files
        .iter()
        .find(|file| file.path.contains("src/main/"))
        .map(|file| file.path.clone())
        .unwrap_or(conventional)
}

/// Simple class name of the production type (`StringCalculator.java` →
/// `StringCalculator`).
pub(crate) fn production_type_name(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    (!stem.is_empty()).then(|| stem.to_string())
}

/// The spec as it would look after commit: staged content wins so a
/// just-drafted requirement is visible to list, status, and generation.
pub(crate) fn load_effective_spec(
    repository: &impl SpecRepository,
    store: &impl ChangeStore,
) -> Result<Spec, ServiceError> {
    load_effective_catalog(repository, store, crate::workspace::SPEC_PATH)
        .map(|catalog| catalog.merged())
}

/// The spec tree as it would look after commit: every file of the
/// catalog resolves staged-first, so a staged include (or a staged
/// child file that is not in the working tree yet) is already part of
/// the effective view.
pub(crate) fn load_effective_catalog(
    repository: &impl SpecRepository,
    store: &impl ChangeStore,
    spec_path: &str,
) -> Result<SpecCatalog, ServiceError> {
    let (dir, root) = match spec_path.rfind('/') {
        Some(cut) => (&spec_path[..cut], &spec_path[cut + 1..]),
        None => ("", spec_path),
    };
    resolve_catalog(root, &mut |path| {
        let project_path = if dir.is_empty() {
            path.to_string()
        } else {
            format!("{dir}/{path}")
        };
        match store.content(&project_path).map_err(|e| e.0)? {
            Some(text) => Ok((text, format!("staged {project_path}"))),
            None => repository
                .read_raw(path)
                .map(|text| (text, path.to_string()))
                .map_err(|e| e.0),
        }
    })
    .map_err(ServiceError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::SourceFile;
    use crate::test_support::{FakeSources, calculator_catalog};

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
            "REQ-003",
            &req("REQ-003"),
            "String Calculator Kata",
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
            "REQ-003",
            &req("REQ-003"),
            "String Calculator Kata",
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
            production_path(&files, Language::Java, "String Calculator Kata"),
            "src/main/java/com/example/StringCalculator.java"
        );
    }
}

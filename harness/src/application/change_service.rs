//! Staged-change use cases: show, commit, discard, and `validate` — the
//! pre-commit check that parses every staged feature file and validates
//! the *effective* spec (staged content when present, the working tree
//! otherwise), so a staged requirement and its staged scenario validate
//! together before either touches the working tree.

use serde::Serialize;

use crate::application::assets::load_effective_catalog;
use crate::application::spec_service::{ServiceError, ValidationReport};
use crate::domain::feature;
use crate::domain::spec_validator::{SpecValidator, is_structural_issue, structural_repair};
use crate::ports::{ChangeStore, FeatureFiles, SpecRepository, StagedChange};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ChangesReport {
    pub changes: Vec<StagedChange>,
    /// Validation issues still open after a commit - a warning, never a
    /// refusal, so an invalid spec can no longer land silently.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct ChangeService<C: ChangeStore, R: SpecRepository, F: FeatureFiles> {
    store: C,
    repository: R,
    feature_files: F,
    spec_path: String,
}

impl<C: ChangeStore, R: SpecRepository, F: FeatureFiles> ChangeService<C, R, F> {
    pub fn new(store: C, repository: R, feature_files: F, spec_path: String) -> Self {
        Self {
            store,
            repository,
            feature_files,
            spec_path,
        }
    }

    pub fn show(&self) -> Result<ChangesReport, ServiceError> {
        let changes = self.store.changes()?;
        let next_step = if changes.is_empty() {
            "Nothing is staged. Authoring commands (spec draft, scenario add, \
             feature create) stage their edits here."
        } else {
            "Review the staged files, run validate, then apply with changes commit \
             or drop with changes discard."
        };
        Ok(ChangesReport {
            changes,
            issues: Vec::new(),
            next_step: next_step.to_string(),
        })
    }

    /// Apply the staged changes, then re-validate the working tree. Open
    /// issues ride along as a warning - the commit already happened, but
    /// an invalid spec never lands silently.
    pub fn commit(&self) -> Result<ChangesReport, ServiceError> {
        let changes = self.store.commit()?;
        if changes.is_empty() {
            return Ok(ChangesReport {
                changes,
                issues: Vec::new(),
                next_step: "Nothing was staged, so nothing was applied.".to_string(),
            });
        }
        let validation = self.validate()?;
        let (issues, next_step) = if validation.valid {
            (
                Vec::new(),
                "Staged changes applied to the working tree. Run spec test to see where \
                 the bar stands."
                    .to_string(),
            )
        } else {
            (
                validation.issues,
                "Staged changes applied, but the working tree does not validate - \
                 fix the issues above, then run spec changes validate again."
                    .to_string(),
            )
        };
        Ok(ChangesReport {
            changes,
            issues,
            next_step,
        })
    }

    pub fn discard(&self) -> Result<ChangesReport, ServiceError> {
        let changes = self.store.discard()?;
        let next_step = if changes.is_empty() {
            "Nothing was staged, so nothing was dropped."
        } else {
            "Staged changes dropped. The working tree is untouched."
        };
        Ok(ChangesReport {
            changes,
            issues: Vec::new(),
            next_step: next_step.to_string(),
        })
    }

    pub fn validate(&self) -> Result<ValidationReport, ServiceError> {
        let mut issues = Vec::new();
        let changes = self.store.changes()?;
        for change in changes.iter().filter(|c| c.path.ends_with(".feature")) {
            // A manifest entry with no content behind it means the staging
            // area is half-written. That is exactly what validate exists
            // to report, so it is an issue rather than a crash.
            let Some(content) = self.store.content(&change.path)? else {
                issues.push(format!(
                    "staged {} is listed but has no content - the staging area is \
                     incomplete. Run changes discard to clear it.",
                    change.path
                ));
                continue;
            };
            if let Err(error) = feature::parse(&change.path, &content) {
                issues.push(error);
            }
        }
        match load_effective_catalog(&self.repository, &self.store, &self.spec_path) {
            Ok(catalog) => {
                let overlay = OverlayFeatures {
                    store: &self.store,
                    fallback: &self.feature_files,
                };
                issues.extend(SpecValidator::new(&overlay).validate_catalog(&catalog));
            }
            // A spec tree that does not resolve (malformed staged file,
            // missing include, cycle) is a validation issue, not a crash.
            Err(spec_issue) => issues.push(spec_issue.0),
        }
        let valid = issues.is_empty();
        // Restaging cannot repair a duplicate id or a repeated include -
        // no tool writes either edit - so those issues get the guidance
        // that names what does.
        let next_step = match structural_repair(&issues) {
            Some(repair) if issues.iter().all(|issue| is_structural_issue(issue)) => repair,
            Some(repair) => {
                format!("{repair} Restage the remaining issues, then run validate again.")
            }
            None if valid => "Spec and staged Gherkin are valid. Apply the staged \
                 changes with changes commit."
                .to_string(),
            None => "Fix the issues (restage as needed), then run validate again \
                 before committing."
                .to_string(),
        };
        Ok(ValidationReport {
            valid,
            issues,
            next_step,
        })
    }
}

/// Answers feature-file questions from the staging area first, falling
/// back to the working tree — so validation sees the post-commit world.
struct OverlayFeatures<'a> {
    store: &'a dyn ChangeStore,
    fallback: &'a dyn FeatureFiles,
}

impl FeatureFiles for OverlayFeatures<'_> {
    fn exists(&self, path: &str) -> bool {
        match self.store.content(path) {
            Ok(Some(_)) => true,
            Ok(None) => self.fallback.exists(path),
            Err(error) => {
                tracing::error!(error = %error, path, "staging unreadable while checking a feature path");
                false
            }
        }
    }

    fn has_tag(&self, path: &str, tag: &str) -> bool {
        match self.store.content(path) {
            Ok(Some(content)) => feature::parse(path, &content)
                .map(|doc| doc.all_tags().iter().any(|t| t == tag))
                .unwrap_or(false),
            Ok(None) => self.fallback.has_tag(path, tag),
            Err(error) => {
                tracing::error!(error = %error, path, "staging unreadable while checking a feature tag");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{Requirement, Spec};
    use crate::ports::SpecError;
    use crate::test_support::{FakeFeatureFiles, InMemoryChangeStore, InMemorySpecRepository};

    const SPEC_PATH: &str = "requirements/requirements.json";

    fn requirement(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            title: "A title".into(),
            status: "pending".into(),
            story: "As a user, I want things so that value.".into(),
            acceptance_criteria: vec!["Given a, when b, then 3".into()],
            feature_file: None,
        }
    }

    fn valid_spec() -> Spec {
        Spec {
            project: "Kata".into(),
            requirements: vec![requirement("REQ-001")],
            ..Spec::default()
        }
    }

    fn service(
        store: InMemoryChangeStore,
        spec: Result<Spec, SpecError>,
    ) -> ChangeService<InMemoryChangeStore, InMemorySpecRepository, FakeFeatureFiles> {
        ChangeService::new(
            store,
            InMemorySpecRepository(spec),
            FakeFeatureFiles::default(),
            SPEC_PATH.into(),
        )
    }

    #[test]
    fn validation_falls_back_to_working_tree_features_for_unstaged_paths() {
        let mut spec = valid_spec();
        spec.requirements[0].status = "implemented".into();
        spec.requirements[0].feature_file = Some("features/w.feature".into());
        let mut features = FakeFeatureFiles::default();
        features.existing.insert("features/w.feature".into());
        features
            .tags
            .entry("features/w.feature".into())
            .or_default()
            .insert("@REQ-001".into());
        let service = ChangeService::new(
            InMemoryChangeStore::default(),
            InMemorySpecRepository(Ok(spec)),
            features,
            SPEC_PATH.into(),
        );
        let report = service.validate().unwrap();
        assert!(report.valid, "issues: {:?}", report.issues);
    }

    #[test]
    fn show_with_nothing_staged_points_at_the_authoring_commands() {
        let report = service(InMemoryChangeStore::default(), Ok(valid_spec()))
            .show()
            .unwrap();
        assert!(report.changes.is_empty());
        assert!(report.next_step.starts_with("Nothing is staged."));
    }

    #[test]
    fn show_with_staged_changes_points_at_validate_and_commit() {
        let store = InMemoryChangeStore::default();
        store.stage("a.feature", "Feature: A\n", "new").unwrap();
        let report = service(store, Ok(valid_spec())).show().unwrap();
        assert_eq!(report.changes.len(), 1);
        assert!(report.next_step.starts_with("Review the staged files"));
    }

    #[test]
    fn commit_reports_what_was_applied() {
        let store = InMemoryChangeStore::default();
        store.stage("a.feature", "Feature: A\n", "new").unwrap();
        let service = service(store, Ok(valid_spec()));
        let report = service.commit().unwrap();
        assert_eq!(report.changes.len(), 1);
        assert!(report.next_step.starts_with("Staged changes applied"));
        assert!(report.issues.is_empty());
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("issues"), "a clean commit carries no issues");
        let empty = service.commit().unwrap();
        assert!(empty.changes.is_empty());
        assert_eq!(
            empty.next_step,
            "Nothing was staged, so nothing was applied."
        );
    }

    #[test]
    fn commit_warns_when_the_working_tree_does_not_validate() {
        let store = InMemoryChangeStore::default();
        let mut spec = valid_spec();
        spec.requirements[0].status = "implemented".into(); // no featureFile
        store
            .stage(SPEC_PATH, &serde_json::to_string(&spec).unwrap(), "mark")
            .unwrap();
        // The fake store does not write through on commit, so the
        // repository plays the post-commit working tree.
        let service = ChangeService::new(
            store,
            InMemorySpecRepository(Ok(spec)),
            FakeFeatureFiles::default(),
            SPEC_PATH.into(),
        );
        let report = service.commit().unwrap();
        assert_eq!(report.changes.len(), 1, "the commit still applied");
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.contains("must name their featureFile")),
            "issues: {:?}",
            report.issues
        );
        assert!(report.next_step.contains("does not validate"));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("issues"));
    }

    #[test]
    fn discard_reports_what_was_dropped() {
        let store = InMemoryChangeStore::default();
        store.stage("a.feature", "Feature: A\n", "new").unwrap();
        let service = service(store, Ok(valid_spec()));
        let report = service.discard().unwrap();
        assert_eq!(report.changes.len(), 1);
        assert!(report.next_step.starts_with("Staged changes dropped."));
        let empty = service.discard().unwrap();
        assert_eq!(
            empty.next_step,
            "Nothing was staged, so nothing was dropped."
        );
    }

    #[test]
    fn a_failing_store_propagates_through_every_use_case() {
        let broken = || InMemoryChangeStore::failing("staging manifest is not valid JSON - x");
        let expected = ServiceError("staging manifest is not valid JSON - x".into());
        assert_eq!(
            service(broken(), Ok(valid_spec())).show().unwrap_err(),
            expected
        );
        assert_eq!(
            service(broken(), Ok(valid_spec())).validate().unwrap_err(),
            expected
        );
    }

    #[test]
    fn validate_parses_every_staged_feature_file() {
        let store = InMemoryChangeStore::default();
        store
            .stage("features/bad.feature", "not gherkin", "oops")
            .unwrap();
        let report = service(store, Ok(valid_spec())).validate().unwrap();
        assert!(!report.valid);
        assert!(
            report.issues[0].starts_with("features/bad.feature: not valid Gherkin -"),
            "got: {:?}",
            report.issues
        );
        assert!(report.next_step.starts_with("Fix the issues"));
    }

    #[test]
    fn validate_uses_the_staged_spec_when_one_is_staged() {
        let store = InMemoryChangeStore::default();
        let mut spec = valid_spec();
        spec.requirements.push(requirement("REQ-002"));
        store
            .stage(SPEC_PATH, &serde_json::to_string(&spec).unwrap(), "draft")
            .unwrap();
        // The working-tree spec is broken; the staged one must win.
        let report = service(store, Err(SpecError("spec: boom".into())))
            .validate()
            .unwrap();
        assert!(report.valid, "issues: {:?}", report.issues);
        assert!(
            report
                .next_step
                .starts_with("Spec and staged Gherkin are valid.")
        );
    }

    /// Restaging cannot repair a duplicate id either, so the staged-aware
    /// twin gives the same structural remedy.
    #[test]
    fn a_staged_duplicate_id_gets_the_structural_remedy() {
        let store = InMemoryChangeStore::default();
        let mut spec = valid_spec();
        spec.requirements.push(spec.requirements[0].clone());
        store
            .stage(SPEC_PATH, &serde_json::to_string(&spec).unwrap(), "draft")
            .unwrap();
        let report = service(store, Ok(valid_spec())).validate().unwrap();
        assert!(!report.valid);
        assert!(
            report
                .next_step
                .contains("no tool can delete a requirement")
                && report
                    .next_step
                    .contains("Editing the spec file directly is the remedy"),
            "{}",
            report.next_step
        );
        assert!(
            !report.next_step.starts_with("Fix the issues"),
            "{}",
            report.next_step
        );
    }

    #[test]
    fn a_staged_duplicate_id_beside_a_wording_issue_keeps_the_restage_advice() {
        let store = InMemoryChangeStore::default();
        let mut spec = valid_spec();
        let mut clash = spec.requirements[0].clone();
        clash.title = String::new();
        spec.requirements.push(clash);
        store
            .stage(SPEC_PATH, &serde_json::to_string(&spec).unwrap(), "draft")
            .unwrap();
        let report = service(store, Ok(valid_spec())).validate().unwrap();
        assert!(
            report
                .next_step
                .contains("no tool can delete a requirement")
                && report.next_step.contains("Restage the remaining issues"),
            "{}",
            report.next_step
        );
    }

    #[test]
    fn validate_reports_a_malformed_staged_spec_as_an_issue() {
        let store = InMemoryChangeStore::default();
        store.stage(SPEC_PATH, "not json", "draft").unwrap();
        let report = service(store, Ok(valid_spec())).validate().unwrap();
        assert!(!report.valid);
        assert!(
            report.issues[0]
                .starts_with("spec: staged requirements/requirements.json is not readable JSON -"),
            "got: {:?}",
            report.issues
        );
    }

    #[test]
    fn validate_reports_a_broken_working_tree_spec_when_nothing_is_staged() {
        let report = service(
            InMemoryChangeStore::default(),
            Err(SpecError("spec: boom".into())),
        )
        .validate()
        .unwrap();
        assert_eq!(report.issues, vec!["spec: boom"]);
    }

    #[test]
    fn validation_sees_staged_features_as_existing_with_their_tags() {
        let store = InMemoryChangeStore::default();
        let mut spec = valid_spec();
        spec.requirements[0].status = "implemented".into();
        spec.requirements[0].feature_file = Some("features/calc.feature".into());
        store
            .stage(SPEC_PATH, &serde_json::to_string(&spec).unwrap(), "mark")
            .unwrap();
        store
            .stage(
                "features/calc.feature",
                "Feature: Calc\n\n  @REQ-001\n  Scenario: S\n    Given a\n",
                "scenario",
            )
            .unwrap();
        let report = service(store, Err(SpecError("unused".into())))
            .validate()
            .unwrap();
        assert!(report.valid, "issues: {:?}", report.issues);
    }

    #[test]
    fn a_staged_feature_that_does_not_parse_never_answers_tag_questions() {
        let store = InMemoryChangeStore::default();
        let mut spec = valid_spec();
        spec.requirements[0].status = "implemented".into();
        spec.requirements[0].feature_file = Some("features/calc.feature".into());
        store
            .stage(SPEC_PATH, &serde_json::to_string(&spec).unwrap(), "mark")
            .unwrap();
        store
            .stage("features/calc.feature", "not gherkin", "broken")
            .unwrap();
        let report = service(store, Err(SpecError("unused".into())))
            .validate()
            .unwrap();
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.contains("no scenario tagged @REQ-001")),
            "issues: {:?}",
            report.issues
        );
    }
}

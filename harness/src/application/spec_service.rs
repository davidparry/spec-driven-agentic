//! Spec use cases: list, show (enriched), validate, refine. Frozen
//! `validate_spec` / `list_requirements` / `get_requirement` reply shapes
//! stay in `harness/tests/mcp_conformance.rs`.

use serde::Serialize;

use crate::domain::model::Requirement;
use crate::domain::refiner::RequirementRefiner;
use crate::domain::spec_validator::SpecValidator;
use crate::ports::{FeatureFiles, SpecRepository};

/// Where the project keeps its artifacts. Injected by the composition
/// root (later: detected by `project_inspect`), enriching `show` replies.
#[derive(Debug, Clone)]
pub struct ProjectLayout {
    pub step_definitions: String,
    pub test_location: String,
    pub production_location: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RequirementSummary {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ValidationReport {
    pub valid: bool,
    pub issues: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RefinementReport {
    pub id: String,
    pub clean: bool,
    pub findings: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// The `get_requirement` reply: not a copy of the spec entry — the server
/// enriches it with locations and a workflow hint.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EnrichedRequirement {
    pub id: String,
    pub title: String,
    pub status: String,
    pub story: String,
    #[serde(rename = "acceptanceCriteria")]
    pub acceptance_criteria: Vec<String>,
    #[serde(rename = "featureLocation", skip_serializing_if = "Option::is_none")]
    pub feature_location: Option<String>,
    #[serde(rename = "stepDefinitions")]
    pub step_definitions: String,
    #[serde(rename = "testLocation")]
    pub test_location: String,
    #[serde(rename = "productionLocation")]
    pub production_location: String,
    #[serde(rename = "workflowHint")]
    pub workflow_hint: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ServiceError(pub String);

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ServiceError {}

macro_rules! from_port_error {
    ($($t:ty),+ $(,)?) => {$(
        impl From<$t> for ServiceError {
            fn from(error: $t) -> Self {
                Self(error.0)
            }
        }
    )+};
}

from_port_error!(
    crate::ports::StageError,
    crate::ports::SpecError,
    crate::ports::FeatureError,
    crate::ports::SourceError,
    crate::ports::StateError,
    crate::ports::PromptError,
    crate::ports::ExecError,
    crate::ports::ScaffoldError,
    crate::ports::LlmError,
    crate::ports::ToolError,
    crate::ports::MemoryError,
    crate::domain::command_policy::CommandRefusal,
);

pub struct SpecService<R: SpecRepository, F: FeatureFiles> {
    repository: R,
    feature_files: F,
    layout: ProjectLayout,
}

impl<R: SpecRepository, F: FeatureFiles> SpecService<R, F> {
    pub fn new(repository: R, feature_files: F, layout: ProjectLayout) -> Self {
        Self {
            repository,
            feature_files,
            layout,
        }
    }

    pub fn list_requirements(&self) -> Result<Vec<RequirementSummary>, ServiceError> {
        let spec = self.repository.load()?;
        Ok(spec
            .requirements
            .into_iter()
            .map(|r| RequirementSummary {
                id: r.id,
                title: r.title,
                status: r.status,
            })
            .collect())
    }

    pub fn get_requirement(&self, id: &str) -> Result<EnrichedRequirement, ServiceError> {
        let spec = self.repository.load()?;
        spec.requirements
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| self.enrich(r))
            .ok_or_else(|| {
                ServiceError(format!(
                    "No requirement with id '{id}'. Call list_requirements to see valid ids."
                ))
            })
    }

    pub fn validate_spec(&self) -> ValidationReport {
        let issues = match self.repository.load_catalog() {
            Ok(catalog) => SpecValidator::new(&self.feature_files).validate_catalog(&catalog),
            Err(e) => vec![e.0],
        };
        let valid = issues.is_empty();
        let next_step = if valid {
            "The spec is valid. Call get_requirement for a pending requirement and write \
             its Gherkin scenario from the acceptance criteria."
        } else {
            "Call requirement_reword to fix the issues - never edit the requirements \
             file by hand - then call validate_spec again. Iterate until valid is true \
             before writing scenarios or code."
        };
        ValidationReport {
            valid,
            issues,
            next_step: next_step.to_string(),
        }
    }

    pub fn refine_requirement(&self, id: &str) -> Result<RefinementReport, ServiceError> {
        let spec = self.repository.load()?;
        let requirement = spec
            .requirements
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| {
                ServiceError(format!(
                    "No requirement with id '{id}'. Call list_requirements to see valid ids."
                ))
            })?;
        let findings = RequirementRefiner.review(requirement);
        let clean = findings.is_empty();
        let next_step = if clean {
            "The wording reads clean. Confirm it with the developer, then write the \
             Gherkin scenario from the acceptance criteria."
        } else {
            "Call requirement_reword to address each finding - never edit the \
             requirements file by hand - then run validate_spec and call \
             refine_requirement again. Iterate until there are no findings."
        };
        Ok(RefinementReport {
            id: id.to_string(),
            clean,
            findings,
            next_step: next_step.to_string(),
        })
    }

    fn enrich(&self, r: Requirement) -> EnrichedRequirement {
        let workflow_hint = format!(
            "Write the Gherkin scenario for this requirement in the feature file first \
             (tag it @{id}), reuse or add step definitions, then run_tests to see RED.",
            id = r.id
        );
        EnrichedRequirement {
            id: r.id,
            title: r.title,
            status: r.status,
            story: r.story,
            acceptance_criteria: r.acceptance_criteria,
            feature_location: r.feature_file,
            step_definitions: self.layout.step_definitions.clone(),
            test_location: self.layout.test_location.clone(),
            production_location: self.layout.production_location.clone(),
            workflow_hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::Spec;
    use crate::ports::SpecError;
    use std::collections::HashSet;

    struct InMemorySpec(Result<Spec, SpecError>);

    impl SpecRepository for InMemorySpec {
        fn load(&self) -> Result<Spec, SpecError> {
            self.0.clone()
        }
    }

    #[derive(Default)]
    struct NoFeatures(HashSet<String>);

    impl FeatureFiles for NoFeatures {
        fn exists(&self, path: &str) -> bool {
            self.0.contains(path)
        }
        fn has_tag(&self, _: &str, _: &str) -> bool {
            false
        }
    }

    fn layout() -> ProjectLayout {
        ProjectLayout {
            step_definitions: "steps/Steps.java".into(),
            test_location: "tests/Test.java".into(),
            production_location: "src/Prod.java".into(),
        }
    }

    fn requirement(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            title: "A title".into(),
            status: "pending".into(),
            story: "As a user, I want things so that value.".into(),
            acceptance_criteria: vec!["Given a, when b, then 3".into()],
            feature_file: Some("features/x.feature".into()),
        }
    }

    fn service_with(spec: Spec) -> SpecService<InMemorySpec, NoFeatures> {
        let mut features = NoFeatures::default();
        features.0.insert("features/x.feature".into());
        SpecService::new(InMemorySpec(Ok(spec)), features, layout())
    }

    fn one_requirement_spec() -> Spec {
        Spec {
            project: "Kata".into(),
            requirements: vec![requirement("REQ-001")],
            ..Spec::default()
        }
    }

    #[test]
    fn list_returns_id_title_and_status() {
        let service = service_with(one_requirement_spec());
        assert_eq!(
            service.list_requirements().unwrap(),
            vec![RequirementSummary {
                id: "REQ-001".into(),
                title: "A title".into(),
                status: "pending".into(),
            }]
        );
    }

    #[test]
    fn show_enriches_the_requirement_instead_of_copying_the_spec_entry() {
        let service = service_with(one_requirement_spec());
        let enriched = service.get_requirement("REQ-001").unwrap();
        assert_eq!(
            enriched.feature_location.as_deref(),
            Some("features/x.feature")
        );
        assert_eq!(enriched.step_definitions, "steps/Steps.java");
        assert_eq!(
            enriched.workflow_hint,
            "Write the Gherkin scenario for this requirement in the feature file first \
             (tag it @REQ-001), reuse or add step definitions, then run_tests to see RED."
        );
        let json = serde_json::to_string(&enriched).unwrap();
        assert!(json.contains("featureLocation"));
        assert!(json.contains("workflowHint"));
        assert!(!json.contains("featureFile"));
    }

    #[test]
    fn show_of_an_unknown_id_names_the_recovery_tool() {
        let service = service_with(one_requirement_spec());
        assert_eq!(
            service.get_requirement("REQ-999").unwrap_err(),
            ServiceError(
                "No requirement with id 'REQ-999'. Call list_requirements to see valid ids.".into()
            )
        );
    }

    #[test]
    fn a_valid_spec_reports_valid_with_the_forward_looking_next_step() {
        let report = service_with(one_requirement_spec()).validate_spec();
        assert!(report.valid);
        assert!(report.issues.is_empty());
        assert_eq!(
            report.next_step,
            "The spec is valid. Call get_requirement for a pending requirement and write \
             its Gherkin scenario from the acceptance criteria."
        );
    }

    #[test]
    fn an_invalid_spec_reports_the_issues_and_the_repair_next_step() {
        let mut spec = one_requirement_spec();
        spec.requirements[0].acceptance_criteria =
            vec!["the result should be 6 for 1\\n2,3".into()];
        let report = service_with(spec).validate_spec();
        assert!(!report.valid);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(
            report.next_step,
            "Call requirement_reword to fix the issues - never edit the requirements \
             file by hand - then call validate_spec again. Iterate until valid is true \
             before writing scenarios or code."
        );
    }

    #[test]
    fn every_use_case_propagates_a_failing_repository() {
        let broken = || {
            SpecService::new(
                InMemorySpec(Err(SpecError("spec: boom".into()))),
                NoFeatures::default(),
                layout(),
            )
        };
        assert_eq!(
            broken().list_requirements().unwrap_err(),
            ServiceError("spec: boom".into())
        );
        assert_eq!(
            broken().get_requirement("REQ-001").unwrap_err(),
            ServiceError("spec: boom".into())
        );
        assert_eq!(
            broken().refine_requirement("REQ-001").unwrap_err(),
            ServiceError("spec: boom".into())
        );
    }

    #[test]
    fn an_unreadable_spec_surfaces_the_repository_error_as_the_issue() {
        let service = SpecService::new(
            InMemorySpec(Err(SpecError(
                "spec: requirements.json is not readable JSON - oops".into(),
            ))),
            NoFeatures::default(),
            layout(),
        );
        let report = service.validate_spec();
        assert!(!report.valid);
        assert_eq!(
            report.issues,
            vec!["spec: requirements.json is not readable JSON - oops"]
        );
    }

    #[test]
    fn refine_of_an_unknown_id_names_the_recovery_tool() {
        let service = service_with(one_requirement_spec());
        assert_eq!(
            service.refine_requirement("REQ-999").unwrap_err(),
            ServiceError(
                "No requirement with id 'REQ-999'. Call list_requirements to see valid ids.".into()
            )
        );
    }

    #[test]
    fn validation_asks_the_feature_files_port_about_scenario_tags() {
        let mut spec = one_requirement_spec();
        spec.requirements[0].status = "implemented".into();
        let report = service_with(spec).validate_spec();
        assert!(!report.valid);
        assert_eq!(
            report.issues,
            vec![
                "REQ-001: no scenario tagged @REQ-001 in features/x.feature - \
                 implemented requirements need executable scenarios"
            ]
        );
    }

    #[test]
    fn refine_reports_clean_for_good_wording() {
        let mut spec = one_requirement_spec();
        spec.requirements[0].story =
            "As a user, I want newline sums so that multi-line input works.".into();
        spec.requirements[0].acceptance_criteria =
            vec!["Given an empty string \"\", when add is called, then the result is 0".into()];
        let report = service_with(spec).refine_requirement("REQ-001").unwrap();
        assert!(report.clean);
        assert_eq!(
            report.next_step,
            "The wording reads clean. Confirm it with the developer, then write the \
             Gherkin scenario from the acceptance criteria."
        );
    }

    #[test]
    fn refine_reports_findings_with_the_iterate_next_step() {
        let mut spec = one_requirement_spec();
        spec.requirements[0].story = "the calculator should handle newlines quickly".into();
        let report = service_with(spec).refine_requirement("REQ-001").unwrap();
        assert!(!report.clean);
        assert_eq!(report.findings.len(), 6);
        assert_eq!(
            report.next_step,
            "Call requirement_reword to address each finding - never edit the \
             requirements file by hand - then run validate_spec and call \
             refine_requirement again. Iterate until there are no findings."
        );
    }
}

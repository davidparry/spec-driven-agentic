//! Spec use cases: list, show (enriched), validate, refine. Frozen
//! `validate_spec` / `list_requirements` / `get_requirement` reply shapes
//! stay in `harness/tests/mcp_conformance.rs`.

use serde::Serialize;

use crate::application::assets::load_spec;
use crate::domain::human::{Human, bullets, columns, counted, sections, titled};
use crate::domain::model::Requirement;
use crate::domain::refiner::RequirementRefiner;
use crate::domain::spec_validator::{SpecValidator, is_structural_issue, structural_repair};
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
    /// The spec file declaring this requirement, relative to the project
    /// root. Last so the frozen `id`/`title`/`status` order is untouched.
    ///
    /// Without it an agent reading a split catalog knows a requirement
    /// exists but not where it lives, which `spec list` has always
    /// answered.
    pub file: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ValidationReport {
    pub valid: bool,
    pub issues: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

impl Human for ValidationReport {
    /// The issues, or nothing at all when there are none.
    ///
    /// Nothing, rather than "the spec is valid", because the advice
    /// that prints directly beneath already opens with that sentence -
    /// it has to, since an agent reading the JSON sees the advice
    /// alone. Saying it twice in a row is worse than saying it once.
    fn human(&self) -> String {
        if self.valid {
            return String::new();
        }
        sections(&[
            counted(self.issues.len(), "issue", "issues"),
            bullets(&self.issues),
        ])
    }

    fn next_step(&self) -> Option<&str> {
        Some(&self.next_step)
    }
}

/// `clean`, `findings`, `source`, and `nextStep` are the deterministic
/// verdict and are produced without a model.
///
/// The `judgment*` fields carry what a configured decision model said
/// about the same wording. They travel *beside* the deterministic
/// verdict rather than inside it: every one of them is omitted from the
/// JSON when no decision model is configured, so the reply shape every
/// existing consumer pattern-matches on is byte-for-byte what it was.
/// `clean` means exactly what it always meant — the deterministic rules
/// found nothing — and no probability is ever folded into it.
#[derive(Debug, Serialize, PartialEq)]
pub struct RefinementReport {
    pub id: String,
    pub clean: bool,
    pub findings: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
    /// The full audit record for each criterion that was judged.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub judgments: Vec<crate::domain::decision::Judgment>,
    /// The human-readable advisory line for each criterion the model
    /// read as unmeasurable.
    #[serde(rename = "judgmentAdvisories", skip_serializing_if = "Vec::is_empty")]
    pub judgment_advisories: Vec<String>,
    /// What the harness does about the judgments. `CONTINUE` in
    /// advisory mode, whatever the answers were.
    #[serde(rename = "judgmentAction", skip_serializing_if = "Option::is_none")]
    pub judgment_action: Option<crate::domain::decision::Transition>,
    /// Why there is no judgment, when one was wanted and did not
    /// arrive. Present instead of the judgments, never alongside them —
    /// and never silently absent, which would leave a reader unable to
    /// tell "nothing to report" from "nobody asked".
    #[serde(rename = "judgmentNote", skip_serializing_if = "Option::is_none")]
    pub judgment_note: Option<String>,
}

impl Human for RefinementReport {
    /// The verdict, the findings, and what the decision model made of
    /// them.
    ///
    /// `judgments` is deliberately left out: it is the audit record -
    /// model tag, threshold, provenance, token usage - and the reader
    /// watching a terminal wants the sentence it produced, which is
    /// what `judgment_advisories` already holds. The record is still
    /// there in full for anything reading the JSON.
    fn human(&self) -> String {
        let verdict = if self.clean {
            format!("{} is clean.", self.id)
        } else {
            format!(
                "{}: {}",
                self.id,
                counted(self.findings.len(), "finding", "findings")
            )
        };
        let advisories = if self.judgment_advisories.is_empty() {
            String::new()
        } else {
            sections(&[
                "The decision model could not measure these:".to_string(),
                bullets(&self.judgment_advisories),
            ])
        };
        let judgment = match (&self.judgment_action, &self.judgment_note) {
            (Some(action), _) => format!("Judgment: {action}"),
            (None, Some(note)) => format!("No judgment: {note}"),
            (None, None) => String::new(),
        };
        sections(&[verdict, bullets(&self.findings), advisories, judgment])
    }

    fn next_step(&self) -> Option<&str> {
        Some(&self.next_step)
    }
}

/// What to do about a refinement, given its verdict.
///
/// Its own function because a judgment can turn a clean report unclean
/// after this service has already worded the advice - see
/// [`crate::application::decision_service::apply_review`]. Two copies
/// of this wording would let the reply tell a reader there is nothing
/// to fix directly above the finding that has to be fixed.
pub fn refinement_next_step(clean: bool) -> &'static str {
    if clean {
        "The wording reads clean. Confirm it with the developer, then write the \
         Gherkin scenario from the acceptance criteria."
    } else {
        "Call requirement_reword to address each finding - never edit the \
         requirements file by hand - then run validate_spec and call \
         refine_requirement again. Iterate until there are no findings."
    }
}

/// A catalog-relative spec path as the project sees it. Catalog paths are
/// relative to the directory holding the root document, so replies have
/// to re-root them or they name a file the caller cannot open.
fn project_path(catalog_path: &str) -> String {
    match crate::workspace::SPEC_PATH.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{catalog_path}"),
        None => catalog_path.to_string(),
    }
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

impl Human for EnrichedRequirement {
    fn human(&self) -> String {
        let criteria: Vec<String> = self
            .acceptance_criteria
            .iter()
            .enumerate()
            .map(|(index, criterion)| format!("  {}. {criterion}", index + 1))
            .collect();
        let mut locations = vec![
            vec!["Steps".to_string(), self.step_definitions.clone()],
            vec!["Tests".to_string(), self.test_location.clone()],
            vec!["Production".to_string(), self.production_location.clone()],
        ];
        // A requirement with no scenario yet has no feature file, and a
        // blank line against the label would read as one that is
        // missing rather than one that was never written.
        if let Some(feature) = &self.feature_location {
            locations.insert(0, vec!["Feature".to_string(), feature.clone()]);
        }
        sections(&[
            format!("{}: {}\nStatus: {}", self.id, self.title, self.status),
            self.story.clone(),
            titled("Acceptance criteria", &criteria.join("\n")),
            columns(&locations),
            self.workflow_hint.clone(),
        ])
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ServiceError(pub String);

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ServiceError {}

impl ServiceError {
    /// Whether this is a prompt that will never be answered, carried
    /// up from [`crate::ports::PromptError::is_end_of_input`]. A
    /// wizard treats it as a decline rather than a failure: the
    /// developer piped in what they had, it ran out, and nothing
    /// should be written on a guess.
    pub fn is_end_of_input(&self) -> bool {
        self.0.starts_with(crate::ports::END_OF_INPUT)
    }
}

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
    crate::ports::WriteError,
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

    /// Every requirement of the committed spec, each naming the catalog
    /// file it lives in. Row order is catalog order, the same order
    /// [`crate::domain::model::SpecCatalog::merged`] produces.
    pub fn list_requirements(&self) -> Result<Vec<RequirementSummary>, ServiceError> {
        let catalog = self.repository.load_catalog()?;
        Ok(catalog
            .files()
            .iter()
            .flat_map(|file| {
                let path = project_path(&file.path);
                file.spec
                    .requirements
                    .iter()
                    .map(move |r| RequirementSummary {
                        id: r.id.clone(),
                        title: r.title.clone(),
                        status: r.status.clone(),
                        file: path.clone(),
                    })
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

    /// The acceptance criteria of `id`, which is all a judgment needs.
    pub fn criteria(&self, id: &str) -> Result<Vec<String>, ServiceError> {
        let spec = load_spec(&self.repository)?;
        spec.requirements
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| r.acceptance_criteria)
            .ok_or_else(|| {
                ServiceError(format!(
                    "No requirement with id '{id}'. Call list_requirements to see valid ids."
                ))
            })
    }

    /// Validate the spec on disk, which is the only copy there is.
    pub fn validate_spec(&self) -> ValidationReport {
        let issues = match self.repository.load_catalog() {
            Ok(catalog) => SpecValidator::new(&self.feature_files).validate_catalog(&catalog),
            Err(e) => vec![e.0],
        };
        let valid = issues.is_empty();
        let next_step = match structural_repair(&issues) {
            Some(repair) if issues.iter().all(|issue| is_structural_issue(issue)) => repair,
            Some(repair) => format!(
                "{repair} Call requirement_reword for the remaining wording issues - \
                 never edit the requirements file by hand for those."
            ),
            None if valid => "The spec is valid. Call get_requirement for a pending \
                 requirement and write its Gherkin scenario from the acceptance criteria."
                .to_string(),
            None => "Call requirement_reword to fix the issues - never edit the \
                 requirements file by hand - then call validate_spec again. Iterate \
                 until valid is true before writing scenarios or code."
                .to_string(),
        };
        ValidationReport {
            valid,
            issues,
            next_step,
        }
    }

    /// Review one requirement's wording. `requirement_reword` writes
    /// the file, so the next pass reads the text the developer just
    /// authored without anything having to be applied in between.
    pub fn refine_requirement(&self, id: &str) -> Result<RefinementReport, ServiceError> {
        let spec = load_spec(&self.repository)?;
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
        Ok(RefinementReport {
            id: id.to_string(),
            clean,
            findings,
            next_step: refinement_next_step(clean).to_string(),
            judgments: Vec::new(),
            judgment_advisories: Vec::new(),
            judgment_action: None,
            judgment_note: None,
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
    fn list_returns_id_title_status_and_the_file_it_lives_in() {
        let service = service_with(one_requirement_spec());
        assert_eq!(
            service.list_requirements().unwrap(),
            vec![RequirementSummary {
                id: "REQ-001".into(),
                title: "A title".into(),
                status: "pending".into(),
                file: "requirements/requirements.json".into(),
            }]
        );
    }

    /// A split catalog is the case the field exists for: an agent reading
    /// the list has to be able to tell which document holds which
    /// requirement, and `id`/`title`/`status` come first regardless.
    #[test]
    fn a_split_catalog_names_each_requirements_own_file_in_catalog_order() {
        struct SplitSpec;
        impl SpecRepository for SplitSpec {
            fn load(&self) -> Result<Spec, SpecError> {
                Ok(Spec {
                    project: "Kata".into(),
                    requirements: vec![requirement("REQ-001"), requirement("REQ-002")],
                    ..Spec::default()
                })
            }
            fn load_catalog(&self) -> Result<crate::domain::model::SpecCatalog, SpecError> {
                let files = [
                    (
                        "requirements.json",
                        r#"{"project":"Kata","includes":["core/math.json"],
                            "requirements":[{"id":"REQ-001","title":"A title",
                            "status":"pending","story":"s","acceptanceCriteria":["c"]}]}"#,
                    ),
                    (
                        "core/math.json",
                        r#"{"requirements":[{"id":"REQ-002","title":"Another",
                            "status":"implemented","story":"s","acceptanceCriteria":["c"]}]}"#,
                    ),
                ];
                crate::domain::model::resolve_catalog("requirements.json", &mut |path| {
                    files
                        .iter()
                        .find(|(p, _)| *p == path)
                        .map(|(p, c)| (c.to_string(), p.to_string()))
                        .ok_or_else(|| format!("spec: {path} is not readable"))
                })
                .map_err(SpecError)
            }
        }

        let service = SpecService::new(SplitSpec, NoFeatures::default(), layout());
        let listed = service.list_requirements().unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|r| (r.id.as_str(), r.file.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("REQ-001", "requirements/requirements.json"),
                ("REQ-002", "requirements/core/math.json"),
            ]
        );
        // The frozen fields keep their names and their order.
        let json = serde_json::to_string(&listed[1]).unwrap();
        assert_eq!(
            json,
            r#"{"id":"REQ-002","title":"Another","status":"implemented","file":"requirements/core/math.json"}"#
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

    /// A duplicate id cannot be reworded away, so the next step stops
    /// naming the reword tool and stops forbidding the file edit that is
    /// the only remedy.
    #[test]
    fn a_duplicate_id_gets_the_structural_remedy_instead_of_reword() {
        let mut spec = one_requirement_spec();
        spec.requirements.push(requirement("REQ-001"));
        let report = service_with(spec).validate_spec();
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
            !report.next_step.contains("requirement_reword")
                && !report.next_step.contains("never edit"),
            "{}",
            report.next_step
        );
    }

    /// Mixed issues keep both remedies: the file edit for the structure,
    /// the reword tool - hand-edit ban intact - for the words.
    #[test]
    fn structure_and_wording_together_keep_both_remedies() {
        let mut spec = one_requirement_spec();
        spec.requirements.push(Requirement {
            title: String::new(),
            ..requirement("REQ-001")
        });
        let report = service_with(spec).validate_spec();
        assert!(!report.valid);
        assert!(
            report
                .next_step
                .contains("no tool can delete a requirement"),
            "{}",
            report.next_step
        );
        assert!(
            report
                .next_step
                .contains("requirement_reword for the remaining wording issues"),
            "{}",
            report.next_step
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

    fn refinement(id: &str, clean: bool, findings: Vec<String>) -> RefinementReport {
        RefinementReport {
            id: id.into(),
            clean,
            findings,
            next_step: "Run spec reword REQ-001.".into(),
            judgments: Vec::new(),
            judgment_advisories: Vec::new(),
            judgment_action: None,
            judgment_note: None,
        }
    }

    /// The advice beneath already opens with "The spec is valid.", so
    /// a body saying the same thing would print it twice in a row.
    #[test]
    fn a_valid_spec_leaves_the_answer_to_its_advice() {
        let report = ValidationReport {
            valid: true,
            issues: Vec::new(),
            next_step: "The spec is valid. Run spec list.".into(),
        };
        assert_eq!(report.human(), "");
        assert_eq!(
            report.next_step(),
            Some("The spec is valid. Run spec list.")
        );
    }

    #[test]
    fn an_invalid_spec_counts_its_issues_and_lists_them() {
        let report = ValidationReport {
            valid: false,
            issues: vec![
                "REQ-001 has no criteria".into(),
                "REQ-002 has no story".into(),
            ],
            next_step: "Run spec reword.".into(),
        };
        assert_eq!(
            report.human(),
            "2 issues\n\n  - REQ-001 has no criteria\n  - REQ-002 has no story"
        );
    }

    #[test]
    fn a_clean_refinement_reads_as_one_sentence() {
        let report = refinement("REQ-001", true, Vec::new());
        assert_eq!(report.human(), "REQ-001 is clean.");
        assert_eq!(report.next_step(), Some("Run spec reword REQ-001."));
    }

    #[test]
    fn an_unclean_refinement_counts_its_findings_and_lists_them() {
        let report = refinement("REQ-001", false, vec!["criterion 1 is vague".into()]);
        assert_eq!(
            report.human(),
            "REQ-001: 1 finding\n\n  - criterion 1 is vague"
        );
    }

    /// The advisory is the sentence the judgment produced; the audit
    /// record behind it belongs to the JSON, not to a terminal.
    #[test]
    fn advisories_are_shown_and_the_audit_record_is_not() {
        let mut report = refinement("REQ-001", true, Vec::new());
        report.judgment_advisories = vec!["criterion 2 has no observable outcome".into()];
        report.judgment_action = Some(crate::domain::decision::Transition::Rework);
        let rendered = report.human();
        assert!(
            rendered.contains("criterion 2 has no observable outcome"),
            "{rendered}"
        );
        assert!(rendered.contains("Judgment: REWORK"), "{rendered}");
        assert!(!rendered.contains("threshold"), "{rendered}");
    }

    /// "Nothing to report" and "nobody asked" are different answers, so
    /// the note is said rather than left as a blank.
    #[test]
    fn a_missing_judgment_says_why_instead_of_going_quiet() {
        let mut report = refinement("REQ-001", true, Vec::new());
        report.judgment_note = Some("the decision model did not answer".into());
        assert!(
            report
                .human()
                .contains("No judgment: the decision model did not answer"),
            "{}",
            report.human()
        );
    }

    #[test]
    fn a_refinement_nobody_judged_says_nothing_about_judgments() {
        let rendered = refinement("REQ-001", true, Vec::new()).human();
        assert_eq!(rendered, "REQ-001 is clean.");
    }

    fn enriched(feature: Option<&str>) -> EnrichedRequirement {
        EnrichedRequirement {
            id: "REQ-001".into(),
            title: "Add two numbers".into(),
            status: "pending".into(),
            story: "As a user I want to add numbers".into(),
            acceptance_criteria: vec!["Given 1 and 2 Then 3".into(), "Given 0 and 0 Then 0".into()],
            feature_location: feature.map(str::to_string),
            step_definitions: "features/steps".into(),
            test_location: "tests".into(),
            production_location: "src".into(),
            workflow_hint: "Write the scenario first.".into(),
        }
    }

    #[test]
    fn a_requirement_numbers_its_criteria_and_lines_up_its_locations() {
        let rendered = enriched(Some("features/add.feature")).human();
        assert!(
            rendered.starts_with("REQ-001: Add two numbers\nStatus: pending"),
            "{rendered}"
        );
        // The heading hugs its list; a blank line between them reads as
        // two sections rather than one.
        assert!(
            rendered.contains("Acceptance criteria\n  1. Given 1 and 2 Then 3"),
            "{rendered}"
        );
        assert!(rendered.contains("  2. Given 0 and 0 Then 0"), "{rendered}");
        assert!(
            rendered.contains("Feature     features/add.feature"),
            "{rendered}"
        );
        assert!(rendered.contains("Production  src"), "{rendered}");
    }

    /// A requirement with no scenario yet has no feature file, and an
    /// empty value beside the label would read as one that went
    /// missing.
    #[test]
    fn a_requirement_without_a_feature_file_omits_the_label() {
        let rendered = enriched(None).human();
        assert!(!rendered.contains("Feature"), "{rendered}");
        assert!(rendered.contains("features/steps"), "{rendered}");
        // A requirement is an answer, not a step in a loop; the
        // workflow hint it already carries is the guidance.
        assert_eq!(enriched(None).next_step(), None);
    }
}

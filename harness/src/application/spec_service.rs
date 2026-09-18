//! Spec use cases: list, show (enriched), validate, refine. Frozen
//! `validate_spec` / `list_requirements` / `get_requirement` reply shapes
//! stay in `harness/tests/mcp_conformance.rs`.

use serde::Serialize;

use crate::application::assets::load_effective_spec;
use crate::domain::model::Requirement;
use crate::domain::refiner::RequirementRefiner;
use crate::domain::spec_validator::{SpecValidator, is_structural_issue, structural_repair};
use crate::ports::{ChangeStore, FeatureFiles, SpecRepository};

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

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RefinementReport {
    pub id: String,
    pub clean: bool,
    pub findings: Vec<String>,
    /// Which copy of the wording was reviewed: [`STAGED`] when the
    /// requirement has an uncommitted edit, [`WORKING_TREE`] otherwise.
    /// Stated rather than implied, so a reader can always tell whether
    /// `clean` is a verdict on the text they just wrote.
    pub source: &'static str,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// `source` of a reply that reviewed an uncommitted edit.
pub const STAGED: &str = "staged";
/// `source` of a reply that reviewed the committed spec on disk.
pub const WORKING_TREE: &str = "working tree";

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
    /// should be staged on a guess.
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

pub struct SpecService<R: SpecRepository, F: FeatureFiles, C: ChangeStore> {
    repository: R,
    feature_files: F,
    store: C,
    layout: ProjectLayout,
}

impl<R: SpecRepository, F: FeatureFiles, C: ChangeStore> SpecService<R, F, C> {
    pub fn new(repository: R, feature_files: F, store: C, layout: ProjectLayout) -> Self {
        Self {
            repository,
            feature_files,
            store,
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

    /// Validate the committed spec on disk. This is the frozen workshop
    /// tool and it deliberately does not see staging - `changes_validate`
    /// is the staged-aware twin. It does say so when an uncommitted spec
    /// edit exists, because "valid" about a file the developer has
    /// already moved past is the one answer worth qualifying.
    pub fn validate_spec(&self) -> ValidationReport {
        let issues = match self.repository.load_catalog() {
            Ok(catalog) => SpecValidator::new(&self.feature_files).validate_catalog(&catalog),
            Err(e) => vec![e.0],
        };
        let valid = issues.is_empty();
        let mut next_step = match structural_repair(&issues) {
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
        if self.has_staged_spec_edit() {
            next_step.push_str(
                " This read the committed spec on disk, and a spec edit is staged - \
                 call changes_validate to check the staged spec before changes_commit.",
            );
        }
        ValidationReport {
            valid,
            issues,
            next_step,
        }
    }

    /// Review one requirement's wording - the staged edit when it has
    /// one, so the text the developer just authored is the text that
    /// gets the verdict.
    ///
    /// Reading the working tree instead was the silent wrong answer:
    /// `requirement_reword` stages, so with no commit between passes a
    /// reworded requirement earned the same findings forever, and - the
    /// half that matters - a deliberately vague story staged over a
    /// clean one came back `clean: true`. The reply names its source
    /// either way.
    pub fn refine_requirement(&self, id: &str) -> Result<RefinementReport, ServiceError> {
        let spec = load_effective_spec(&self.repository, &self.store)?;
        let requirement = spec
            .requirements
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| {
                ServiceError(format!(
                    "No requirement with id '{id}'. Call list_requirements to see valid ids."
                ))
            })?;
        let committed = self
            .repository
            .load()
            .ok()
            .and_then(|spec| spec.requirements.into_iter().find(|r| r.id == id));
        let source = if committed.as_ref() == Some(requirement) {
            WORKING_TREE
        } else {
            STAGED
        };
        let findings = RequirementRefiner.review(requirement);
        let clean = findings.is_empty();
        let next_step = match (clean, source) {
            (true, STAGED) => {
                "The staged wording reads clean. Confirm it with the developer, apply it \
                 with changes_commit, then write the Gherkin scenario from the \
                 acceptance criteria."
            }
            (true, _) => {
                "The wording reads clean. Confirm it with the developer, then write the \
                 Gherkin scenario from the acceptance criteria."
            }
            (false, STAGED) => {
                "Call requirement_reword to address each finding - never edit the \
                 requirements file by hand - then call refine_requirement again. It \
                 reviews your staged edit, so there is no need to commit between passes. \
                 Iterate until there are no findings."
            }
            (false, _) => {
                "Call requirement_reword to address each finding - never edit the \
                 requirements file by hand - then run validate_spec and call \
                 refine_requirement again. Iterate until there are no findings."
            }
        };
        Ok(RefinementReport {
            id: id.to_string(),
            clean,
            findings,
            source,
            next_step: next_step.to_string(),
        })
    }

    /// Is an edit to a spec document waiting in staging? Spec files are
    /// the `.json` documents under the spec directory; a staged Gherkin
    /// or Java file says nothing about the spec.
    fn has_staged_spec_edit(&self) -> bool {
        let prefix = match crate::workspace::SPEC_PATH.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/"),
            None => String::new(),
        };
        self.store
            .changes()
            .unwrap_or_default()
            .iter()
            .any(|change| change.path.starts_with(&prefix) && change.path.ends_with(".json"))
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
    use crate::test_support::InMemoryChangeStore;
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

    fn service_with(spec: Spec) -> SpecService<InMemorySpec, NoFeatures, InMemoryChangeStore> {
        service_staging(spec, InMemoryChangeStore::default())
    }

    fn service_staging(
        spec: Spec,
        store: InMemoryChangeStore,
    ) -> SpecService<InMemorySpec, NoFeatures, InMemoryChangeStore> {
        let mut features = NoFeatures::default();
        features.0.insert("features/x.feature".into());
        SpecService::new(InMemorySpec(Ok(spec)), features, store, layout())
    }

    const SPEC_PATH: &str = "requirements/requirements.json";

    /// Stage `spec` as the uncommitted edit of the root spec document.
    fn stage(store: &InMemoryChangeStore, spec: &Spec) {
        store
            .stage(
                SPEC_PATH,
                &crate::domain::model::render(spec).unwrap(),
                "reword",
            )
            .unwrap();
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

        let service = SpecService::new(
            SplitSpec,
            NoFeatures::default(),
            InMemoryChangeStore::default(),
            layout(),
        );
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
                InMemoryChangeStore::default(),
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
            InMemoryChangeStore::default(),
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

    /// The serious half of the staged/working-tree split: a vague story
    /// staged over a clean one came back `{"clean": true, "findings":
    /// []}` because refine read the on-disk copy and never saw the edit.
    #[test]
    fn vague_wording_staged_over_a_clean_requirement_is_never_reported_clean() {
        let mut committed = one_requirement_spec();
        committed.requirements[0].story =
            "As a user, I want newline sums so that multi-line input works.".into();
        committed.requirements[0].acceptance_criteria =
            vec!["Given an empty string \"\", when add is called, then the result is 0".into()];
        let mut vague = committed.clone();
        vague.requirements[0].story = "the calculator should handle newlines quickly".into();

        let store = InMemoryChangeStore::default();
        stage(&store, &vague);
        let report = service_staging(committed, store)
            .refine_requirement("REQ-001")
            .unwrap();
        assert!(!report.clean, "findings: {:?}", report.findings);
        assert_eq!(report.source, STAGED);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.contains("missing the actor")),
            "findings: {:?}",
            report.findings
        );
        // And the loop is told it converges without a commit, which is
        // what the documented iterate-until-clean cycle needs.
        assert!(
            report
                .next_step
                .contains("no need to commit between passes"),
            "{}",
            report.next_step
        );
    }

    /// The other half: a rewording that fixed the findings is clean on
    /// the pass right after it is staged, with no commit in between.
    #[test]
    fn a_staged_rewording_reads_clean_without_a_commit_first() {
        let mut vague = one_requirement_spec();
        vague.requirements[0].story = "the calculator should handle newlines quickly".into();
        let mut fixed = vague.clone();
        fixed.requirements[0].story =
            "As a user, I want newline sums so that multi-line input works.".into();
        fixed.requirements[0].acceptance_criteria =
            vec!["Given an empty string \"\", when add is called, then the result is 0".into()];

        let store = InMemoryChangeStore::default();
        stage(&store, &fixed);
        let report = service_staging(vague, store)
            .refine_requirement("REQ-001")
            .unwrap();
        assert!(report.clean, "findings: {:?}", report.findings);
        assert_eq!(report.source, STAGED);
        assert!(
            report
                .next_step
                .starts_with("The staged wording reads clean.")
                && report.next_step.contains("changes_commit"),
            "{}",
            report.next_step
        );
    }

    /// A requirement that exists only in staging - just drafted, never
    /// committed - is refinable rather than "no requirement with id".
    #[test]
    fn a_requirement_that_exists_only_in_staging_can_be_refined() {
        let mut staged_spec = one_requirement_spec();
        staged_spec.requirements.push(Requirement {
            id: "REQ-002".into(),
            story: "the parser should handle things".into(),
            ..requirement("REQ-002")
        });
        let store = InMemoryChangeStore::default();
        stage(&store, &staged_spec);
        let report = service_staging(one_requirement_spec(), store)
            .refine_requirement("REQ-002")
            .unwrap();
        assert_eq!(report.source, STAGED);
        assert!(!report.clean, "findings: {:?}", report.findings);
    }

    /// A staged edit to some *other* requirement leaves this one read
    /// from the working tree, so the label stays honest.
    #[test]
    fn a_requirement_with_no_staged_edit_of_its_own_reads_the_working_tree() {
        let mut spec = one_requirement_spec();
        spec.requirements[0].story =
            "As a user, I want newline sums so that multi-line input works.".into();
        spec.requirements[0].acceptance_criteria =
            vec!["Given an empty string \"\", when add is called, then the result is 0".into()];
        spec.requirements.push(requirement("REQ-002"));
        let mut staged_spec = spec.clone();
        staged_spec.requirements[1].title = "Reworded elsewhere".into();

        let store = InMemoryChangeStore::default();
        stage(&store, &staged_spec);
        let report = service_staging(spec, store)
            .refine_requirement("REQ-001")
            .unwrap();
        assert_eq!(report.source, WORKING_TREE);
        assert!(report.clean, "findings: {:?}", report.findings);
    }

    /// `validate_spec` reads the committed spec by contract -
    /// `changes_validate` is the staged-aware twin - but it says so
    /// rather than letting "valid" stand for a file the developer has
    /// already moved past.
    #[test]
    fn validate_names_the_staged_twin_when_a_spec_edit_is_waiting() {
        let store = InMemoryChangeStore::default();
        stage(&store, &one_requirement_spec());
        let report = service_staging(one_requirement_spec(), store).validate_spec();
        assert!(report.valid);
        assert!(
            report.next_step.starts_with("The spec is valid.")
                && report.next_step.contains("changes_validate"),
            "{}",
            report.next_step
        );

        // A staged Gherkin file says nothing about the spec, so it does
        // not earn the qualifier.
        let store = InMemoryChangeStore::default();
        store
            .stage("features/calc.feature", "Feature: Calc\n", "scenario")
            .unwrap();
        let report = service_staging(one_requirement_spec(), store).validate_spec();
        assert!(
            !report.next_step.contains("changes_validate"),
            "{}",
            report.next_step
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
}

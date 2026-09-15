//! `bdd status`: the workflow query aggregating the TDD phase, the
//! staging area, and every requirement's asset gaps into the one next
//! step that moves the project toward all requirements implemented.
//! The report itself is deterministic reading; when a model is
//! resolved, `advice` additionally briefs it with the workflow process
//! and the full state so it names the next command.

use serde::Serialize;

use crate::application::LlmReplyError;
use crate::application::assets::{asset_survey, load_effective_spec};
use crate::application::generation_service::ResolvedLlm;
use crate::application::spec_service::ServiceError;
use crate::domain::generation::strip_code_fences;
use crate::domain::language::Language;
use crate::domain::workflow::next_step_prompt;
use crate::ports::{
    ChangeStore, FeatureCatalog, LlmConversation, Prompter, SourceFiles, SpecRepository, ToolBroker,
};

/// One requirement's position on the road to implemented: its status
/// and the asset gaps still open (empty once only the GREEN-gated
/// mark-implemented remains).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RequirementStatus {
    pub id: String,
    pub title: String,
    pub status: String,
    pub findings: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub staged: bool,
}

/// Reply of `bdd status`: the TDD phase, what waits in staging, every
/// requirement's position, and the one next step that moves the
/// project toward all requirements implemented.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StatusReport {
    pub phase: String,
    pub staged: Vec<crate::ports::StagedChange>,
    pub requirements: Vec<RequirementStatus>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct StatusService<F, S, C, R, L, B = crate::application::agent_service::NullBroker>
where
    F: FeatureCatalog,
    S: SourceFiles,
    C: ChangeStore,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    features: F,
    sources: S,
    store: C,
    spec: R,
    language: Language,
    llm: Option<ResolvedLlm<L, B>>,
}

impl<F, S, C, R, L, B> StatusService<F, S, C, R, L, B>
where
    F: FeatureCatalog,
    S: SourceFiles,
    C: ChangeStore,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    pub fn new(
        features: F,
        sources: S,
        store: C,
        spec: R,
        language: Language,
        llm: Option<ResolvedLlm<L, B>>,
    ) -> Self {
        Self {
            features,
            sources,
            store,
            spec,
            language,
            llm,
        }
    }

    /// Whether a model is resolved; callers narrate the advice call only
    /// when one will actually happen.
    pub fn has_model(&self) -> bool {
        self.llm.is_some()
    }

    /// Where the project stands on the road to every requirement being
    /// implemented, and the one next step that moves it forward. The
    /// priority order mirrors the loop itself: staged changes await
    /// review first; then the requirement in flight (assets complete)
    /// is tested or marked; then the earliest asset gap; and when
    /// nothing is left, the next draft.
    pub fn status(&self, phase: &str) -> Result<StatusReport, ServiceError> {
        let spec = load_effective_spec(&self.spec, &self.store)?;
        let disk_ids: std::collections::HashSet<String> = self
            .spec
            .load()
            .map(|s| s.requirements.into_iter().map(|r| r.id).collect())
            .unwrap_or_default();
        let staged = self.store.changes()?;
        let mut requirements = Vec::new();
        let mut in_flight: Option<String> = None;
        let mut first_gap: Option<String> = None;
        for requirement in &spec.requirements {
            let mut findings = Vec::new();
            if requirement.status != "implemented" {
                let (_, gaps) = asset_survey(
                    &self.features,
                    &self.sources,
                    self.language,
                    &requirement.id,
                    requirement,
                    &spec.project,
                )?;
                findings = gaps;
                if findings.is_empty() {
                    in_flight.get_or_insert_with(|| requirement.id.clone());
                } else if first_gap.is_none() {
                    first_gap = Some(findings[0].clone());
                }
            }
            requirements.push(RequirementStatus {
                id: requirement.id.clone(),
                title: requirement.title.clone(),
                status: requirement.status.clone(),
                findings,
                staged: !disk_ids.contains(&requirement.id),
            });
        }
        let next_step = if !staged.is_empty() {
            format!(
                "{} staged file(s) await review - inspect with bdd changes show, \
                 apply with bdd changes commit, then run bdd test.",
                staged.len()
            )
        } else if let Some(id) = in_flight {
            if phase == "GREEN" {
                format!(
                    "The bar is GREEN - close the loop: bdd spec mark-implemented \
                     {id}, then bdd validate, then bdd changes commit."
                )
            } else {
                format!(
                    "{id} has every asset in place and the bar is {phase} - run \
                     bdd test; on RED let the model try with bdd implement {id}."
                )
            }
        } else if let Some(gap) = first_gap {
            gap
        } else {
            "Every requirement is implemented. Draft the next one with bdd spec draft.".to_string()
        };
        Ok(StatusReport {
            phase: phase.to_string(),
            staged,
            requirements,
            next_step,
        })
    }

    /// Ask the model for the next step: the workflow process document
    /// plus the whole report and the last run's counts go into one
    /// advice call. `None` without a model.
    pub fn advice(
        &self,
        prompter: &mut dyn Prompter,
        report: &StatusReport,
        last_run: impl Serialize,
    ) -> Result<Option<String>, ServiceError> {
        let Some(llm) = &self.llm else {
            return Ok(None);
        };
        let prompt = next_step_prompt(
            &report.phase,
            last_run,
            &report.staged,
            &report.requirements,
        );
        let reply = match llm.ask(
            prompter,
            &prompt,
            |text| {
                let body = strip_code_fences(text);
                if body.trim().is_empty() {
                    Err("the advice reply was empty".into())
                } else {
                    Ok(body)
                }
            },
            |_, _, _| {},
        ) {
            Ok(reply) => reply,
            Err(LlmReplyError::Call(e)) => {
                return Err(ServiceError(format!("the model call failed - {}", e.0)));
            }
            Err(LlmReplyError::Invalid { reason }) => {
                return Err(ServiceError(reason));
            }
        };
        Ok(Some(reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_service::NullPrompter;
    use crate::domain::model::{Requirement, Spec, TestRunSummary};
    use crate::ports::SourceFile;
    use crate::test_support::{
        FakeLlm, FakeSources, InMemoryChangeStore, InMemoryFeatureCatalog, InMemorySpecRepository,
        calculator_catalog, calculator_spec, covered_steps_source, unit_test_source,
    };

    fn service_with_llm(
        sources: Vec<SourceFile>,
        llm: Option<ResolvedLlm<FakeLlm>>,
    ) -> StatusService<
        InMemoryFeatureCatalog,
        FakeSources,
        InMemoryChangeStore,
        InMemorySpecRepository,
        FakeLlm,
    > {
        StatusService::new(
            calculator_catalog(),
            FakeSources(sources),
            InMemoryChangeStore::default(),
            InMemorySpecRepository(Ok(calculator_spec())),
            Language::Java,
            llm,
        )
    }

    fn service(
        sources: Vec<SourceFile>,
    ) -> StatusService<
        InMemoryFeatureCatalog,
        FakeSources,
        InMemoryChangeStore,
        InMemorySpecRepository,
        FakeLlm,
    > {
        service_with_llm(sources, None)
    }

    #[test]
    fn status_puts_staged_changes_before_everything_else() {
        let service = service(vec![covered_steps_source(), unit_test_source()]);
        service
            .store
            .stage("src/main/java/Kata.java", "class Kata {}", "attempt")
            .unwrap();
        let report = service.status("RED").unwrap();
        assert_eq!(report.phase, "RED");
        assert_eq!(report.staged.len(), 1);
        assert!(
            report.next_step.contains("1 staged file(s) await review"),
            "next step: {}",
            report.next_step
        );
        assert!(report.next_step.contains("bdd changes commit"));
    }

    #[test]
    fn status_on_green_names_the_whole_close_the_loop_chain() {
        let service = service(vec![covered_steps_source(), unit_test_source()]);
        let report = service.status("GREEN").unwrap();
        assert!(
            report
                .next_step
                .contains("bdd spec mark-implemented REQ-001"),
            "next step: {}",
            report.next_step
        );
        assert!(
            report.next_step.contains("then bdd validate"),
            "validate is part of the chain: {}",
            report.next_step
        );
        assert!(report.next_step.contains("then bdd changes commit"));
        let by_id = |id: &str| report.requirements.iter().find(|r| r.id == id).unwrap();
        assert!(by_id("REQ-001").findings.is_empty(), "REQ-001 is in flight");
        assert!(!by_id("REQ-002").findings.is_empty(), "REQ-002 has gaps");
        assert_eq!(by_id("REQ-003").status, "implemented");
        assert!(by_id("REQ-003").findings.is_empty());
    }

    #[test]
    fn status_off_green_with_a_requirement_in_flight_points_to_the_test_run() {
        let service = service(vec![covered_steps_source(), unit_test_source()]);
        let report = service.status("RED").unwrap();
        assert!(
            report.next_step.contains("run bdd test"),
            "next step: {}",
            report.next_step
        );
        assert!(report.next_step.contains("bdd implement REQ-001"));
    }

    #[test]
    fn status_names_the_earliest_gap_when_nothing_is_in_flight() {
        let report = service(vec![]).status("START").unwrap();
        assert!(
            report.next_step.contains("bdd steps generate"),
            "REQ-001's first gap leads: {}",
            report.next_step
        );
    }

    #[test]
    fn status_with_every_requirement_implemented_points_to_the_next_draft() {
        let spec = Spec {
            project: "Kata".into(),
            requirements: vec![Requirement {
                id: "REQ-001".into(),
                title: "Adds two numbers".into(),
                status: "implemented".into(),
                story: "As a user, I want sums so that I can add.".into(),
                acceptance_criteria: vec!["Given a, when b, then 3".into()],
                feature_file: Some("features/calc.feature".into()),
            }],
            ..Spec::default()
        };
        let service: StatusService<_, _, _, _, FakeLlm> = StatusService::new(
            calculator_catalog(),
            FakeSources(vec![]),
            InMemoryChangeStore::default(),
            InMemorySpecRepository(Ok(spec)),
            Language::Java,
            None,
        );
        let report = service.status("GREEN").unwrap();
        assert_eq!(
            report.next_step,
            "Every requirement is implemented. Draft the next one with bdd spec draft."
        );
    }

    #[test]
    fn the_status_report_serializes_next_step_in_camel_case() {
        let report = service(vec![]).status("START").unwrap();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("nextStep"));
        assert!(json.contains("requirements"));
    }

    #[test]
    fn advice_without_a_model_is_none() {
        let service = service(vec![]);
        assert!(!service.has_model());
        let report = service.status("START").unwrap();
        assert_eq!(
            service
                .advice(&mut NullPrompter, &report, TestRunSummary::default())
                .unwrap(),
            None
        );
    }

    #[test]
    fn advice_briefs_the_model_with_the_workflow_and_the_whole_state() {
        let service = service_with_llm(
            vec![],
            Some(FakeLlm::replying(
                "Run bdd steps generate, then bdd changes commit.",
            )),
        );
        assert!(service.has_model());
        service
            .store
            .stage("features/calc.feature", "Feature: Calc\n", "scenario")
            .unwrap();
        let report = service.status("RED").unwrap();
        let advice = service
            .advice(
                &mut NullPrompter,
                &report,
                TestRunSummary {
                    tests: 6,
                    failures: 2,
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(advice, "Run bdd steps generate, then bdd changes commit.");
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(
            prompts[0].contains("THE LOOP FOR ONE REQUIREMENT"),
            "the workflow process briefs the model"
        );
        assert!(prompts[0].contains("The TDD phase: RED"));
        assert!(prompts[0].contains("tests=6 failures=2 errors=0 skipped=0"));
        assert!(prompts[0].contains("features/calc.feature"));
        assert!(prompts[0].contains("REQ-001"));
        assert!(prompts[0].contains("status=implemented") || prompts[0].contains("status=pending"));
    }

    #[test]
    fn a_model_failure_during_advice_is_reported() {
        let service = service_with_llm(vec![], Some(FakeLlm::failing()));
        let report = service.status("RED").unwrap();
        assert_eq!(
            service
                .advice(&mut NullPrompter, &report, TestRunSummary::default())
                .unwrap_err()
                .0,
            "the model call failed - model crashed"
        );
    }

    #[test]
    fn an_empty_advice_reply_is_reported_as_invalid() {
        let service = service_with_llm(vec![], Some(FakeLlm::replying("   ")));
        let report = service.status("RED").unwrap();
        let error = service
            .advice(&mut NullPrompter, &report, TestRunSummary::default())
            .unwrap_err();
        assert!(
            error.0.contains("empty") || error.0.contains("invalid"),
            "got: {}",
            error.0
        );
    }
}

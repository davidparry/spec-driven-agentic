//! Implementation attempts: the readiness preflight, the model-driven
//! attempt that stages file updates, and the advice call when the
//! preflight found problems. There is no template fallback: without a
//! model, implementing stays in the developer's hands.

use serde::Serialize;

use crate::application::LlmReplyError;
use crate::application::assets::{
    asset_survey, find_requirement, load_effective_spec, production_path,
};
use crate::application::generation_service::ResolvedLlm;
use crate::application::spec_service::ServiceError;
use crate::domain::generation::{
    FileUpdate, ImplementAsset, advice_prompt, implementation_prompt, parse_file_updates_checked,
    strip_code_fences,
};
use crate::domain::language::Language;
use crate::domain::steps::source_extension;
use crate::domain::tdd::{ImplementAttempt, StateEntry};
use crate::ports::{
    ChangeStore, FeatureCatalog, LlmConversation, Prompter, SourceFiles, SpecRepository, ToolBroker,
};

/// Reply of an implementation attempt: the files the model updated.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ImplementationReport {
    pub targets: Vec<String>,
    pub staged: bool,
    pub source: String,
    /// Set when the reply left the production code untouched - the
    /// attempt is incomplete and the caller narrates it loudly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// The implement preflight: whether every prerequisite of an
/// implementation attempt is in place, the asset survey, and the
/// findings naming the step to take instead when one is not.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ReadinessReport {
    pub ready: bool,
    pub assets: Vec<ImplementAsset>,
    pub findings: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct ImplementService<F, S, C, R, L, B = crate::application::agent_service::NullBroker>
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

impl<F, S, C, R, L, B> ImplementService<F, S, C, R, L, B>
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

    /// Whether a model is resolved; callers narrate model calls only
    /// when one will actually happen.
    pub fn has_model(&self) -> bool {
        self.llm.is_some()
    }

    /// Ask the model to make the failing tests pass: production code plus
    /// real bodies for the TODO placeholders in the test scaffolding.
    /// Every update is staged; the caller commits and reruns the tests -
    /// the test run is the real validator.
    pub fn generate(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
        failures: &[String],
        history: &[ImplementAttempt],
        states: &[StateEntry],
    ) -> Result<ImplementationReport, ServiceError> {
        let Some(llm) = &self.llm else {
            return Err(ServiceError(
                "No model resolved - implement by hand and rerun bdd test.".into(),
            ));
        };
        let spec = load_effective_spec(&self.spec, &self.store)?;
        let requirement = find_requirement(&spec, req_id)?;
        let sources = self.sources.sources(source_extension(self.language))?;
        let files: Vec<(String, String)> = sources
            .iter()
            .cloned()
            .map(|file| (file.path, file.content))
            .collect();
        let production = production_path(&sources, self.language, &spec.project);
        let prompt = implementation_prompt(
            self.language,
            requirement,
            failures,
            history,
            states,
            &files,
            &production,
        );
        tracing::debug!(requirement = %req_id, "calling LLM for an implementation attempt");
        let updates = match llm.ask(
            prompter,
            &prompt,
            |reply| {
                let updates: Vec<FileUpdate> = parse_file_updates_checked(reply)?
                    .into_iter()
                    .filter(|update| {
                        update.path == production
                            || files.iter().any(|(path, _)| *path == update.path)
                    })
                    .collect();
                if updates.is_empty() {
                    Err("the reply held no usable file update for this project".into())
                } else {
                    Ok(updates)
                }
            },
            |_, _, _| {},
        ) {
            Ok(updates) => updates,
            Err(LlmReplyError::Call(e)) => {
                return Err(ServiceError(format!("the model call failed - {}", e.0)));
            }
            Err(LlmReplyError::Invalid { .. }) => {
                return Err(ServiceError(
                    "The model's reply held no usable file update.".into(),
                ));
            }
        };
        let summary = format!("implementation attempt for {req_id} (llm)");
        let mut targets = Vec::new();
        for update in &updates {
            self.store.stage(&update.path, &update.content, &summary)?;
            targets.push(update.path.clone());
        }
        // A reply without the production file is an incomplete attempt:
        // the tests will stay RED. Stage what arrived, but say so.
        let production_written = targets.contains(&production);
        let warning = (!production_written).then(|| {
            format!(
                "The model left the production code untouched ({production}) - \
                 it only wrote: {}.",
                targets.join(", ")
            )
        });
        let next_step = if production_written {
            "Apply with bdd changes commit, then bdd test - the run decides.".to_string()
        } else {
            format!(
                "The attempt is incomplete without {production}. Apply what was \
                 staged with bdd changes commit, rerun bdd test, then bdd implement \
                 {req_id} again - or implement {production} by hand."
            )
        };
        Ok(ImplementationReport {
            targets,
            staged: true,
            source: "llm".into(),
            warning,
            next_step,
        })
    }

    /// The implement preflight: survey every prerequisite of an
    /// implementation attempt - the tagged scenario, the step
    /// definitions, the unit test, and a recorded RED bar - and name
    /// the step to take instead of implementing when one is missing.
    pub fn readiness(
        &self,
        req_id: &str,
        phase: &str,
        failures: &[String],
    ) -> Result<ReadinessReport, ServiceError> {
        let spec = load_effective_spec(&self.spec, &self.store)?;
        let requirement = find_requirement(&spec, req_id)?;
        let mut findings = Vec::new();
        if requirement.status == "implemented" {
            findings.push(format!(
                "{req_id} is already implemented - pick the next pending requirement \
                 with bdd spec list."
            ));
        }
        if phase != "RED" || failures.is_empty() {
            findings.push(match phase {
                "GREEN" => "The bar is GREEN - there is nothing to implement. Refactor \
                            with bdd refactor or close the loop with bdd spec mark-implemented."
                    .to_string(),
                _ => "No RED test run is recorded - run bdd test first so its failures \
                      brief the model."
                    .to_string(),
            });
        }

        let (assets, asset_findings) = asset_survey(
            &self.features,
            &self.sources,
            self.language,
            req_id,
            requirement,
            &spec.project,
        )?;
        findings.extend(asset_findings);

        let ready = findings.is_empty();
        let next_step = findings.first().cloned().unwrap_or_else(|| {
            format!("Every prerequisite is in place - bdd implement {req_id} can run.")
        });
        Ok(ReadinessReport {
            ready,
            assets,
            findings,
            next_step,
        })
    }

    /// Ask the model what to do next when the preflight found problems:
    /// the requirement, the asset survey, the findings, and the last
    /// failures go into one advice call. `None` without a model.
    pub fn advice(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
        readiness: &ReadinessReport,
        failures: &[String],
    ) -> Result<Option<String>, ServiceError> {
        let Some(llm) = &self.llm else {
            return Ok(None);
        };
        let spec = load_effective_spec(&self.spec, &self.store)?;
        let requirement = find_requirement(&spec, req_id)?;
        let prompt = advice_prompt(
            self.language,
            requirement,
            &readiness.findings,
            &readiness.assets,
            failures,
        );
        tracing::debug!(requirement = %req_id, "calling LLM for implement advice");
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
    use crate::ports::SourceFile;
    use crate::test_support::{
        FakeLlm, FakeSources, InMemoryChangeStore, InMemoryFeatureCatalog, InMemorySpecRepository,
        calculator_catalog, calculator_spec, covered_steps_source, unit_test_source,
    };

    fn service(
        sources: Vec<SourceFile>,
        llm: Option<ResolvedLlm<FakeLlm>>,
    ) -> ImplementService<
        InMemoryFeatureCatalog,
        FakeSources,
        InMemoryChangeStore,
        InMemorySpecRepository,
        FakeLlm,
    > {
        ImplementService::new(
            calculator_catalog(),
            FakeSources(sources),
            InMemoryChangeStore::default(),
            InMemorySpecRepository(Ok(calculator_spec())),
            Language::Java,
            llm,
        )
    }

    #[test]
    fn implement_without_a_model_is_refused() {
        let error = service(vec![], None)
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[])
            .unwrap_err();
        assert_eq!(
            error.0,
            "No model resolved - implement by hand and rerun bdd test."
        );
    }

    #[test]
    fn an_implementation_attempt_stages_allowed_updates_and_drops_the_rest() {
        let reply = r#"[
            {"path": "src/main/java/Kata.java", "content": "public class Kata {}"},
            {"path": "src/test/java/Steps.java", "content": "class Steps {}"},
            {"path": "/etc/passwd", "content": "nope"}
        ]"#;
        let sources = vec![SourceFile {
            path: "src/test/java/Steps.java".into(),
            content: "old steps".into(),
        }];
        let service = service(sources, Some(FakeLlm::replying(reply)));
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["Req001Test: TODO: assert".into()],
                &[],
                &[],
            )
            .unwrap();
        assert_eq!(
            report.targets,
            vec!["src/main/java/Kata.java", "src/test/java/Steps.java"]
        );
        assert!(report.staged);
        assert_eq!(report.source, "llm");
        let production = service
            .store
            .content("src/main/java/Kata.java")
            .unwrap()
            .unwrap();
        assert_eq!(production, "public class Kata {}");
        assert!(service.store.content("/etc/passwd").unwrap().is_none());
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(prompts[0].contains("Req001Test: TODO: assert"));
        assert!(prompts[0].contains("--- src/test/java/Steps.java ---"));
        assert!(prompts[0].contains("Write the production code at src/main/java/Kata.java"));
        assert!(
            prompts[0].contains("Java best practices to follow:")
                && prompts[0].contains("Package names are lowercase"),
            "prompt pins the language's best practices"
        );
    }

    #[test]
    fn a_reply_without_the_production_file_carries_a_loud_warning() {
        let reply =
            r#"[{"path": "src/test/java/Steps.java", "content": "class Steps { real body }"}]"#;
        let sources = vec![SourceFile {
            path: "src/test/java/Steps.java".into(),
            content: "old steps".into(),
        }];
        let service = service(sources, Some(FakeLlm::replying(reply)));
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["Req001Test: TODO: assert".into()],
                &[],
                &[],
            )
            .unwrap();
        assert_eq!(report.targets, vec!["src/test/java/Steps.java"]);
        let warning = report.warning.expect("the incomplete attempt warns");
        assert!(warning.contains("left the production code untouched"));
        assert!(warning.contains("src/main/java/Kata.java"));
        assert!(
            report
                .next_step
                .contains("incomplete without src/main/java/Kata.java")
        );
        assert!(report.next_step.contains("bdd implement REQ-001"));
    }

    #[test]
    fn a_complete_reply_stays_warning_free() {
        let reply = r#"[{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]"#;
        let service = service(vec![], Some(FakeLlm::replying(reply)));
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["Req001Test: TODO: assert".into()],
                &[],
                &[],
            )
            .unwrap();
        assert_eq!(report.warning, None);
        assert_eq!(
            report.next_step,
            "Apply with bdd changes commit, then bdd test - the run decides."
        );
    }

    #[test]
    fn implement_targets_an_existing_production_class() {
        let reply = r#"[{"path": "src/main/java/com/example/StringCalculator.java", "content": "class StringCalculator { int add(String n) { return 0; } }"}]"#;
        let sources = vec![SourceFile {
            path: "src/main/java/com/example/StringCalculator.java".into(),
            content: "class StringCalculator {}".into(),
        }];
        let service = service(sources, Some(FakeLlm::replying(reply)));
        let report = service
            .generate(&mut NullPrompter, "REQ-001", &["todo".into()], &[], &[])
            .unwrap();
        assert_eq!(
            report.targets,
            vec!["src/main/java/com/example/StringCalculator.java"]
        );
        assert_eq!(report.warning, None);
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(prompts[0].contains(
            "Write the production code at src/main/java/com/example/StringCalculator.java"
        ));
    }

    #[test]
    fn prior_attempts_reach_the_model_in_the_implementation_prompt() {
        let reply = r#"[{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]"#;
        let service = service(vec![], Some(FakeLlm::replying(reply)));
        let history = vec![ImplementAttempt {
            requirement: "REQ-001".into(),
            targets: vec!["src/main/java/Kata.java".into()],
            failures: vec!["Req001Test: expected 0 but was 1\nat Req001Test.java:9".into()],
            outcome: vec!["Req001Test: cannot find symbol\nat Req001Test.java:3".into()],
        }];
        service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["Req001Test: cannot find symbol".into()],
                &history,
                &[],
            )
            .unwrap();
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(prompts[0].contains("This is attempt 2 on this requirement"));
        assert!(prompts[0].contains("Attempt 1 wrote: src/main/java/Kata.java"));
        assert!(
            prompts[0].contains("Req001Test: expected 0 but was 1"),
            "the prior failure's first line reaches the prompt"
        );
        assert!(
            !prompts[0].contains("at Req001Test.java:9"),
            "prior stack traces are briefed away - only current failures carry full detail"
        );
        assert!(
            prompts[0]
                .contains("The run after attempt 1 reported:\n- Req001Test: cannot find symbol"),
            "the attempt's actual result reaches the prompt: {}",
            prompts[0]
        );
        assert!(
            prompts[0].contains("Req001Test: cannot find symbol"),
            "the current failures stay complete"
        );
    }

    #[test]
    fn an_unusable_implementation_reply_is_refused() {
        for reply in [
            "Sure, here you go!",
            r#"[{"path": "not/a/project/file.java", "content": "x"}]"#,
        ] {
            let error = service(vec![], Some(FakeLlm::replying(reply)))
                .generate(&mut NullPrompter, "REQ-001", &[], &[], &[])
                .unwrap_err();
            assert_eq!(error.0, "The model's reply held no usable file update.");
        }
    }

    #[test]
    fn a_model_failure_during_implementation_is_reported() {
        let error = service(vec![], Some(FakeLlm::failing()))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[])
            .unwrap_err();
        assert_eq!(error.0, "the model call failed - model crashed");
    }

    #[test]
    fn an_implementation_for_an_unknown_requirement_is_refused() {
        let error = service(vec![], Some(FakeLlm::replying("[]")))
            .generate(&mut NullPrompter, "REQ-404", &[], &[], &[])
            .unwrap_err();
        assert_eq!(
            error.0,
            "No requirement with id REQ-404. Call spec list to see valid ids."
        );
    }

    #[test]
    fn readiness_is_clean_when_every_prerequisite_is_in_place() {
        let service = service(vec![covered_steps_source(), unit_test_source()], None);
        let report = service
            .readiness("REQ-001", "RED", &["Req001Test: TODO: assert".into()])
            .unwrap();
        assert!(report.ready, "report: {report:?}");
        assert!(report.findings.is_empty());
        assert_eq!(
            report.next_step,
            "Every prerequisite is in place - bdd implement REQ-001 can run."
        );
        let asset = |path: &str| {
            report
                .assets
                .iter()
                .find(|a| a.path == path)
                .unwrap_or_else(|| panic!("no asset {path}: {:?}", report.assets))
        };
        assert!(asset("features/calc.feature").present);
        assert!(asset("src/test/java/Req001Test.java").present);
        assert!(
            !asset("src/main/java/Kata.java").present,
            "production code does not exist yet - and that is not a finding"
        );
    }

    #[test]
    fn readiness_names_every_gap_and_the_step_to_take_instead() {
        let report = service(vec![], None)
            .readiness("REQ-001", "START", &[])
            .unwrap();
        assert!(!report.ready);
        let has = |fragment: &str| {
            assert!(
                report.findings.iter().any(|f| f.contains(fragment)),
                "no finding with {fragment:?}: {:?}",
                report.findings
            );
        };
        has("run bdd test first");
        has("bdd steps generate");
        has("bdd unittest generate REQ-001");
        assert!(
            report.next_step.contains("bdd test"),
            "the earliest gap leads: {}",
            report.next_step
        );
    }

    #[test]
    fn readiness_on_green_says_there_is_nothing_to_implement() {
        let report = service(vec![], None)
            .readiness("REQ-001", "GREEN", &[])
            .unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.contains("The bar is GREEN"))
        );
    }

    #[test]
    fn readiness_flags_a_missing_tag_and_an_already_implemented_requirement() {
        let report = service(vec![], None)
            .readiness("REQ-002", "RED", &["boom".into()])
            .unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.contains("No scenario is tagged @REQ-002"))
        );
        let report = service(vec![], None)
            .readiness("REQ-003", "RED", &["boom".into()])
            .unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.contains("REQ-003 is already implemented"))
        );
    }

    #[test]
    fn readiness_and_advice_for_an_unknown_requirement_are_refused() {
        let refusal = "No requirement with id REQ-404. Call spec list to see valid ids.";
        let service = service(vec![], Some(FakeLlm::replying("irrelevant")));
        assert_eq!(
            service.readiness("REQ-404", "RED", &[]).unwrap_err().0,
            refusal
        );
        let readiness = service.readiness("REQ-001", "START", &[]).unwrap();
        assert_eq!(
            service
                .advice(&mut NullPrompter, "REQ-404", &readiness, &[])
                .unwrap_err()
                .0,
            refusal
        );
    }

    #[test]
    fn advice_without_a_model_is_none() {
        let service = service(vec![], None);
        assert!(!service.has_model());
        let readiness = service.readiness("REQ-001", "START", &[]).unwrap();
        assert_eq!(
            service
                .advice(&mut NullPrompter, "REQ-001", &readiness, &[])
                .unwrap(),
            None
        );
    }

    #[test]
    fn advice_sends_the_survey_to_the_model_and_returns_its_reply() {
        let service = service(
            vec![],
            Some(FakeLlm::replying(
                "No - run bdd test first to record the RED bar.",
            )),
        );
        assert!(service.has_model());
        let readiness = service.readiness("REQ-001", "START", &[]).unwrap();
        let advice = service
            .advice(&mut NullPrompter, "REQ-001", &readiness, &[])
            .unwrap()
            .unwrap();
        assert_eq!(advice, "No - run bdd test first to record the RED bar.");
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(prompts[0].contains("The project assets:"));
        assert!(prompts[0].contains("No RED test run is recorded"));
        assert!(prompts[0].contains("at most four short sentences"));
    }

    #[test]
    fn a_model_failure_during_advice_is_reported() {
        let service = service(vec![], Some(FakeLlm::failing()));
        let readiness = service.readiness("REQ-001", "START", &[]).unwrap();
        assert_eq!(
            service
                .advice(&mut NullPrompter, "REQ-001", &readiness, &[])
                .unwrap_err()
                .0,
            "the model call failed - model crashed"
        );
    }

    #[test]
    fn an_empty_advice_reply_is_reported_as_invalid() {
        let service = service(vec![], Some(FakeLlm::replying("   ")));
        let readiness = service.readiness("REQ-001", "START", &[]).unwrap();
        let error = service
            .advice(&mut NullPrompter, "REQ-001", &readiness, &[])
            .unwrap_err();
        assert!(
            error.0.contains("empty") || error.0.contains("invalid"),
            "got: {}",
            error.0
        );
    }
}

//! `spec greenfield`: the orchestrated loop from an empty directory to an
//! implemented requirement. Exactly two human gates shape the run — the
//! spec wording approval (inside `spec draft`) and the generated-test
//! review before anything is committed. Everything else is derived from
//! the approved spec. Like [`crate::mcp`], this is a delivery module: it
//! wires the same application services, so it may name concrete adapters.
//! The prompter, runner factory, and LLM are injected so the whole
//! orchestration is testable with fakes.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

use crate::adapters::fs_spec::FsSpecRepository;
use crate::adapters::fs_staging::FsChangeStore;
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::runners::detect_runner;
use crate::application::DEFAULT_LLM_ATTEMPTS;
use crate::application::change_service::ChangeService;
use crate::application::generation_service::GenerationService;
use crate::application::implement_service::ImplementService;
use crate::application::scenario_service::ScenarioService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::tdd_service::{TddError, TddService, TestReport};
use crate::bootstrap::{ensure_project, ensure_spec, refresh_project_memory};
use crate::domain::language::Language;
use crate::domain::model::Spec;
use crate::domain::scaffold::slug;
use crate::domain::steps::criterion_to_steps;
use crate::domain::tdd::ImplementAttempt;
use crate::ports::{ChangeStore as _, Prompter, TestFilter, TestRunner};
use crate::wiring::{DynLlm, OverlayFeatures, OverlayTree, RunnerFactory};
use crate::workspace::SPEC_PATH;

/// Where the run ended, and what the human does next.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GreenfieldReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    pub completed: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct Greenfield {
    root: PathBuf,
    runner_factory: RunnerFactory,
    llm: Option<(String, DynLlm)>,
    llm_attempts: u32,
}

/// Which pending requirement to deliver next. Enter means the first;
/// a number picks from the listed 1-based rows; `n` / `no` / `stop`
/// ends the run. `None` is decline; `Some` is the chosen id.
fn pick_pending(
    prompter: &mut dyn Prompter,
    pending: &[(String, String)],
) -> Result<Option<String>, String> {
    let count = pending.len();
    let question = format!("Which pending requirement next? [1-{count}, Enter for 1] (n stops):");
    loop {
        let answer = prompter.ask(&question).map_err(|e| e.to_string())?;
        match parse_pending_pick(&answer, count) {
            Ok(Some(index)) => return Ok(Some(pending[index].0.clone())),
            Ok(None) => return Ok(None),
            Err(warning) => prompter.warn(warning),
        }
    }
}

/// Parse the next-pending answer. `Ok(None)` means stop; `Ok(Some(i))`
/// is a 0-based index into the listed pending rows.
pub fn parse_pending_pick(answer: &str, count: usize) -> Result<Option<usize>, &'static str> {
    let trimmed = answer.trim();
    if trimmed.eq_ignore_ascii_case("n")
        || trimmed.eq_ignore_ascii_case("no")
        || trimmed.eq_ignore_ascii_case("stop")
    {
        return Ok(None);
    }
    if trimmed.is_empty() {
        return Ok(Some(0));
    }
    match trimmed.parse::<usize>() {
        Ok(pick) if count > 0 && (1..=count).contains(&pick) => Ok(Some(pick - 1)),
        _ => Err("Pick a number from the list, or n to stop."),
    }
}

impl Greenfield {
    pub fn new(root: PathBuf, llm: Option<(String, DynLlm)>) -> Self {
        Self::with_runner_factory(root, Arc::new(detect_runner), llm)
    }

    /// Constructor for tests: a scripted runner factory replaces real
    /// build-tool execution.
    pub fn with_runner_factory(
        root: PathBuf,
        runner_factory: RunnerFactory,
        llm: Option<(String, DynLlm)>,
    ) -> Self {
        Self {
            root,
            runner_factory,
            llm,
            llm_attempts: DEFAULT_LLM_ATTEMPTS,
        }
    }

    /// How many times a model reply is tried when validation fails.
    pub fn with_llm_attempts(mut self, attempts: u32) -> Self {
        self.llm_attempts = attempts.max(1);
        self
    }

    pub fn run(&self, prompter: &mut dyn Prompter) -> Result<GreenfieldReport, String> {
        prompter.tell(
            "Greenfield loop: approved spec -> tagged scenario -> failing tests -> \
             implement -> GREEN -> refactor -> mark implemented.",
        );
        match &self.llm {
            Some((model, _)) => prompter.tell(&format!(
                "Generation uses model {model}; templates are the fallback."
            )),
            None => prompter.tell(
                "No LLM model resolved - deterministic templates will be used for generation.",
            ),
        }

        let language = self.ensure_project(prompter)?;
        self.ensure_spec(prompter)?;
        refresh_project_memory(&self.root, Some(language));
        prompter.tell(&format!(
            "Project language: {} ({}).",
            language.display(),
            language.bdd_framework()
        ));

        // Gate 1 (inside draft): the human words the spec and approves it.
        // With a model, drafting starts from a plain-words description the
        // model splits into requirement proposals the wizard walks through.
        let mutation = self.mutation_service();
        let draft = match self.memory_llm() {
            Some((model, llm)) => mutation.draft_assisted(prompter, &model, &llm),
            None => mutation.draft(prompter),
        }
        .map_err(|e| e.to_string())?;
        if !draft.staged {
            return Ok(GreenfieldReport {
                requirement: None,
                feature: None,
                phase: None,
                completed: false,
                next_step:
                    "Nothing was staged. Run spec greenfield again when the wording is ready."
                        .into(),
            });
        }
        self.commit()?;
        prompter.tell(&format!("{} committed to the spec.", draft.id));
        let mut report = self.deliver(prompter, language, &draft.id)?;

        // A closed loop offers every remaining pending requirement as a
        // numbered choice: pick one to refine and deliver, or n to stop.
        while report.completed {
            let pending = self.pending_requirements()?;
            if pending.is_empty() {
                report.next_step =
                    "Every requirement is implemented. Draft the next one with spec draft.".into();
                break;
            }
            prompter.tell("Still pending in the spec:");
            for (index, (id, title)) in pending.iter().enumerate() {
                prompter.tell(&format!("  {}. {} {title}", index + 1, id));
            }
            let Some(next_id) = pick_pending(prompter, &pending)? else {
                break;
            };
            let reworded = match self.memory_llm() {
                Some((model, llm)) => self
                    .mutation_service()
                    .reword_assisted(prompter, &next_id, &model, &llm),
                None => self.mutation_service().reword(prompter, &next_id),
            }
            .map_err(|e| e.to_string())?;
            if !reworded.staged {
                return Ok(GreenfieldReport {
                    requirement: Some(next_id),
                    feature: None,
                    phase: None,
                    completed: false,
                    next_step: "Nothing was staged. Run spec greenfield again when the \
                                wording is ready."
                        .into(),
                });
            }
            self.commit()?;
            prompter.tell(&format!("{next_id} committed to the spec."));
            report = self.deliver(prompter, language, &next_id)?;
        }
        Ok(report)
    }

    /// One requirement's delivery: tagged scenarios, generated tests
    /// behind the review gate, the RED loop to GREEN, the optional
    /// refactor step, and the GREEN-gated mark-implemented.
    fn deliver(
        &self,
        prompter: &mut dyn Prompter,
        language: Language,
        req_id: &str,
    ) -> Result<GreenfieldReport, String> {
        let feature_path = self.author_scenarios(prompter, req_id)?;
        self.commit()?;
        prompter.tell(&format!("Scenarios committed to {feature_path}."));

        // Generation into staging, then gate 2: review before commit.
        let generation = self.generation_service(language);
        let implement = self.implement_service(language);
        let missing = generation.steps_missing().map_err(|e| e.to_string())?;
        if !missing.missing.is_empty() {
            let work = prompter.working("Generating step definitions - working");
            let report = generation
                .steps_generate(prompter)
                .map_err(|e| e.to_string())?;
            drop(work);
            prompter.tell(&format!("Staged {} ({}).", report.target, report.source));
        }
        let work = prompter.working(&format!("Generating the unit test for {req_id} - working"));
        let unit_test = generation
            .unittest_generate(prompter, req_id)
            .map_err(|e| e.to_string())?;
        drop(work);
        prompter.tell(&format!(
            "Staged {} ({}).",
            unit_test.target, unit_test.source
        ));
        if let Some(content) = self
            .change_store()
            .content(&unit_test.target)
            .map_err(|e| e.to_string())?
        {
            prompter.tell("Generated unit test (the assertions are yours to sharpen):");
            prompter.tell(&content);
        }
        if !prompter
            .confirm("Commit the generated tests and step definitions?")
            .map_err(|e| e.to_string())?
        {
            self.change_service().discard().map_err(|e| e.to_string())?;
            return Ok(GreenfieldReport {
                requirement: Some(req_id.to_string()),
                feature: Some(feature_path),
                phase: None,
                completed: false,
                next_step: "Generation was discarded. Author the tests by hand or rerun \
                            spec greenfield."
                    .into(),
            });
        }
        self.commit()?;

        // Execution only when the runtime is present; authoring is done
        // either way.
        let runner = match (self.runner_factory)(&self.root) {
            Ok(runner) => runner,
            Err(message) => {
                prompter.warn(&message);
                return Ok(self.authoring_done(req_id.to_string(), feature_path));
            }
        };
        let tdd = self.tdd_service();
        let Some(mut report) = self.try_run(&tdd, runner.as_ref(), prompter)? else {
            return Ok(self.authoring_done(req_id.to_string(), feature_path));
        };

        while report.phase != "GREEN" {
            let question = if self.llm.is_some() {
                "Press Enter to let the model attempt the implementation and rerun \
                 the tests, enter a number to attempt up to that many times without \
                 asking again, or type stop to pause here:"
            } else {
                "Implement the production code now. Press Enter to run the tests \
                 again, or type stop to pause here:"
            };
            let answer = prompter.ask(question).map_err(|e| e.to_string())?;
            if answer.eq_ignore_ascii_case("stop") {
                let next_step = format!(
                    "Paused on RED. Implement by hand or run spec implement {req_id}, \
                     then spec test until GREEN, spec refactor, and spec \
                     mark-implemented."
                );
                return Ok(GreenfieldReport {
                    requirement: Some(req_id.to_string()),
                    feature: Some(feature_path),
                    phase: Some(report.phase),
                    completed: false,
                    next_step,
                });
            }
            // Without a model a number cannot buy extra attempts - the
            // developer is the implementation, so every rerun is asked for.
            let budget = if self.llm.is_some() {
                attempt_budget(&answer)
            } else {
                1
            };
            for attempt in 1..=budget {
                if budget > 1 {
                    prompter.tell(&format!("Attempt {attempt} of {budget}."));
                }
                if self.llm.is_some() {
                    self.attempt_implementation(prompter, &implement, &tdd, req_id)?;
                }
                report = match self.try_run(&tdd, runner.as_ref(), prompter)? {
                    Some(report) => report,
                    None => return Ok(self.authoring_done(req_id.to_string(), feature_path)),
                };
                if report.phase == "GREEN" {
                    break;
                }
            }
        }

        if prompter
            .confirm("Green bar. Start a refactor step before closing the loop?")
            .map_err(|e| e.to_string())?
        {
            prompter.tell(
                "A refactor changes structure, never behavior - and the edits \
                 are yours to make. Apply them in your editor now; your note \
                 goes in the refactor log, then the tests rerun to prove the \
                 bar stayed green.",
            );
            let note = prompter
                .ask("When your edits are in place, describe what you changed and why:")
                .map_err(|e| e.to_string())?;
            tdd.refactor(Some(&note)).map_err(tdd_message)?;
            report = self
                .try_run(&tdd, runner.as_ref(), prompter)?
                .ok_or_else(|| "the runtime disappeared mid-loop".to_string())?;
            if report.phase != "GREEN" {
                return Ok(GreenfieldReport {
                    requirement: Some(req_id.to_string()),
                    feature: Some(feature_path),
                    phase: Some(report.phase),
                    completed: false,
                    next_step: "The refactor broke the bar. Make the tests pass again, \
                                then spec mark-implemented."
                        .into(),
                });
            }
        }

        let work = prompter.working("Saving status - working");
        self.mutation_service()
            .mark_implemented(req_id)
            .map_err(|e| e.to_string())?;
        self.commit()?;
        drop(work);
        prompter.tell(&format!("{req_id} is implemented. Loop closed."));
        Ok(GreenfieldReport {
            requirement: Some(req_id.to_string()),
            feature: Some(feature_path),
            phase: Some("GREEN".into()),
            completed: true,
            next_step: "The next requirement is waiting. Type greenfield to continue, \
                        or spec list."
                .into(),
        })
    }

    /// Detect the project language, scaffolding a new project first when
    /// the directory has no marker files.
    fn ensure_project(&self, prompter: &mut dyn Prompter) -> Result<Language, String> {
        ensure_project(&self.root, prompter)
    }

    /// A detected project can still have no spec. Drafting reads
    /// `requirements/requirements.json`, so a missing or unreadable file
    /// is written as an empty catalog before the wizard starts. A file
    /// that can be read is left as it is.
    fn ensure_spec(&self, prompter: &mut dyn Prompter) -> Result<(), String> {
        ensure_spec(&self.root, prompter)
    }

    /// Turn the requirement's criteria into a tagged feature file. The
    /// title comes from the committed spec, so a rewording that changed
    /// it still names the feature correctly.
    fn author_scenarios(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
    ) -> Result<String, String> {
        let spec: Spec = {
            let repository = FsSpecRepository::new(self.root.join(SPEC_PATH));
            crate::ports::SpecRepository::load(&repository).map_err(|e| e.to_string())?
        };
        let requirement = spec
            .requirements
            .iter()
            .find(|r| r.id == req_id)
            .ok_or_else(|| format!("{req_id} disappeared from the committed spec"))?;
        let title = requirement.title.as_str();
        let feature_path = format!("features/{}.feature", slug(title));
        let scenarios = self.scenario_service();
        scenarios
            .create_feature(&feature_path, title)
            .map_err(|e| e.to_string())?;
        for (index, criterion) in requirement.acceptance_criteria.iter().enumerate() {
            let Some(steps) = criterion_to_steps(criterion) else {
                prompter.warn(&format!(
                    "Skipping criterion (not Given/when/then shaped): {criterion}"
                ));
                continue;
            };
            let name = format!("{} case {}", title, index + 1);
            scenarios
                .add_scenario(&feature_path, req_id, &name, steps)
                .map_err(|e| e.to_string())?;
        }
        Ok(feature_path)
    }

    /// The committed spec's still-pending requirements (id, title), in
    /// catalog order.
    fn pending_requirements(&self) -> Result<Vec<(String, String)>, String> {
        let repository = FsSpecRepository::new(self.root.join(SPEC_PATH));
        let spec = crate::ports::SpecRepository::load(&repository).map_err(|e| e.to_string())?;
        Ok(spec
            .requirements
            .into_iter()
            .filter(|r| r.status == "pending")
            .map(|r| (r.id, r.title))
            .collect())
    }

    fn authoring_done(&self, requirement: String, feature: String) -> GreenfieldReport {
        GreenfieldReport {
            requirement: Some(requirement),
            feature: Some(feature),
            phase: None,
            completed: false,
            next_step: "Authoring is complete. Install the runtime, then spec test \
                        (expect RED), implement, and close the loop."
                .into(),
        }
    }

    /// Run the tests and narrate the outcome. `Ok(None)` means the
    /// runtime is missing: execution stops but authoring stands.
    fn try_run(
        &self,
        tdd: &TddService<FsStateStore>,
        runner: &dyn TestRunner,
        prompter: &mut dyn Prompter,
    ) -> Result<Option<TestReport>, String> {
        let work = prompter.working("Running the tests - working");
        let outcome = tdd.run_tests(runner, &TestFilter::default());
        drop(work);
        match outcome {
            Ok(report) => {
                prompter.tell(&format!(
                    "{}: {} tests, {} failures, {} errors.",
                    report.phase, report.tests, report.failures, report.errors
                ));
                for detail in &report.failure_details {
                    prompter.tell(&format!("  - {detail}"));
                }
                Ok(Some(report))
            }
            Err(TddError::RuntimeMissing { runtime, hint }) => {
                prompter.warn(&format!("Runtime missing ({runtime}): {hint}"));
                Ok(None)
            }
            Err(TddError::Other(message)) => Err(message),
        }
    }

    /// Ask the model to make the failing tests pass and commit whatever it
    /// staged. The brief carries the persisted failure details (stack
    /// traces included), prior attempts on this requirement, and only the
    /// three latest dated state entries. The attempt is logged so the next
    /// one learns from it. A model failure is narrated, not fatal - the
    /// developer can still implement by hand and press Enter again.
    fn attempt_implementation(
        &self,
        prompter: &mut dyn Prompter,
        implement: &ImplementService<
            crate::wiring::OverlayFeatures,
            crate::wiring::OverlayTree,
            FsChangeStore,
            FsSpecRepository,
            DynLlm,
        >,
        tdd: &TddService<FsStateStore>,
        req_id: &str,
    ) -> Result<(), String> {
        let brief = tdd.implementation_brief(req_id).map_err(tdd_message)?;
        let work = prompter.working("Generating an implementation attempt - working");
        let outcome = implement.generate(
            prompter,
            req_id,
            &brief.failures,
            &brief.history,
            &brief.states,
        );
        drop(work);
        match outcome {
            Ok(attempt) => {
                for target in &attempt.targets {
                    // The complete path: targets are project-relative,
                    // but the reader may be anywhere on the machine.
                    let full = std::path::absolute(self.root.join(target))
                        .unwrap_or_else(|_| self.root.join(target));
                    prompter.tell(&format!("Updated {} (llm).", full.display()));
                }
                if let Some(warning) = &attempt.warning {
                    prompter.warn(warning);
                }
                tdd.record_attempt(ImplementAttempt {
                    requirement: req_id.to_string(),
                    targets: attempt.targets.clone(),
                    failures: brief.failures,
                    ..Default::default()
                })
                .map_err(tdd_message)?;
                self.commit()?;
            }
            Err(error) => prompter.warn(&format!("{} Implement by hand instead.", error.0)),
        }
        Ok(())
    }

    fn commit(&self) -> Result<(), String> {
        self.change_service().commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn change_store(&self) -> FsChangeStore {
        FsChangeStore::new(self.root.clone())
    }

    fn change_service(
        &self,
    ) -> ChangeService<FsChangeStore, FsSpecRepository, crate::wiring::OverlayFeatures> {
        crate::wiring::change_service(&self.root)
    }

    fn mutation_service(
        &self,
    ) -> SpecMutationService<
        FsSpecRepository,
        crate::wiring::OverlayFeatures,
        FsChangeStore,
        FsStateStore,
    > {
        crate::wiring::mutation_service(&self.root, self.llm_attempts)
    }

    fn scenario_service(&self) -> ScenarioService<FsChangeStore, crate::wiring::OverlayFeatures> {
        crate::wiring::scenario_service(&self.root)
    }

    fn generation_service(
        &self,
        language: Language,
    ) -> GenerationService<OverlayFeatures, OverlayTree, FsChangeStore, FsSpecRepository, DynLlm>
    {
        crate::wiring::generation_service(
            &self.root,
            language,
            self.llm.as_ref(),
            self.llm_attempts,
        )
    }

    fn implement_service(
        &self,
        language: Language,
    ) -> ImplementService<OverlayFeatures, OverlayTree, FsChangeStore, FsSpecRepository, DynLlm>
    {
        crate::wiring::implement_service(&self.root, language, self.llm.as_ref(), self.llm_attempts)
    }

    /// The session LLM with project memory prepended to every system
    /// prompt, for the drafting calls that take one directly.
    fn memory_llm(&self) -> crate::wiring::SessionLlm {
        crate::wiring::memory_llm(&self.root, self.llm.as_ref())
    }

    fn tdd_service(&self) -> TddService<FsStateStore> {
        crate::wiring::tdd_service(&self.root)
    }
}

/// The RED prompt's answer, read as an attempt budget: Enter buys a
/// single attempt, a positive number buys up to that many attempts
/// without asking again, and anything unreadable stays a single
/// attempt - exactly what Enter would have done.
fn attempt_budget(answer: &str) -> u32 {
    answer.parse::<u32>().ok().filter(|n| *n > 0).unwrap_or(1)
}

/// The directory name, for the empty spec's `project` field.
/// Flatten a TDD error into the message the human sees.
fn tdd_message(error: TddError) -> String {
    match error {
        TddError::Other(message) => message,
        TddError::RuntimeMissing { hint, .. } => hint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::PromptError;

    struct Told {
        lines: Vec<String>,
    }

    impl Prompter for Told {
        fn tell(&mut self, message: &str) {
            self.lines.push(message.to_string());
        }
        fn ask(&mut self, _question: &str) -> Result<String, crate::ports::PromptError> {
            Ok(String::new())
        }
        fn confirm(&mut self, _question: &str) -> Result<bool, crate::ports::PromptError> {
            Ok(false)
        }
    }

    #[test]
    fn a_project_without_a_readable_spec_gets_an_empty_requirements_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        let greenfield = Greenfield::new(dir.path().to_path_buf(), None);
        let mut prompter = Told { lines: Vec::new() };
        greenfield.ensure_spec(&mut prompter).unwrap();
        let path = dir.path().join(SPEC_PATH);
        let text = std::fs::read_to_string(&path).unwrap();
        let spec: Spec = serde_json::from_str(&text).unwrap();
        assert!(spec.requirements.is_empty());
        assert_eq!(
            spec.project,
            dir.path().file_name().unwrap().to_string_lossy()
        );
        assert_eq!(
            prompter.lines,
            vec!["Created requirements/requirements.json — there was no readable spec yet."]
        );

        std::fs::write(&path, "   \n").unwrap();
        let mut empty = Told { lines: Vec::new() };
        greenfield.ensure_spec(&mut empty).unwrap();
        assert_eq!(empty.lines.len(), 1, "an empty file is not a spec");
        let replaced: Spec =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(replaced.requirements.is_empty());

        std::fs::write(&path, "{ nope").unwrap();
        let mut again = Told { lines: Vec::new() };
        greenfield.ensure_spec(&mut again).unwrap();
        assert!(again.lines.is_empty(), "a readable file is left alone");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ nope");
    }

    #[test]
    fn pending_picks_are_the_listed_row_or_a_stop() {
        assert_eq!(parse_pending_pick("", 3), Ok(Some(0)));
        assert_eq!(parse_pending_pick("1", 3), Ok(Some(0)));
        assert_eq!(parse_pending_pick("3", 3), Ok(Some(2)));
        assert_eq!(parse_pending_pick("n", 3), Ok(None));
        assert_eq!(parse_pending_pick("STOP", 2), Ok(None));
        assert!(parse_pending_pick("4", 3).is_err());
        assert!(parse_pending_pick("y", 1).is_err());
    }

    #[test]
    fn red_prompt_answers_parse_into_an_attempt_budget() {
        assert_eq!(attempt_budget(""), 1, "Enter is a single attempt");
        assert_eq!(attempt_budget("5"), 5);
        assert_eq!(attempt_budget("1"), 1);
        assert_eq!(attempt_budget("0"), 1, "zero cannot mean no attempt");
        assert_eq!(attempt_budget("-3"), 1);
        assert_eq!(attempt_budget("five"), 1, "junk behaves like Enter");
    }

    #[test]
    fn tdd_errors_flatten_to_their_human_message() {
        assert_eq!(tdd_message(TddError::Other("boom".into())), "boom");
        assert_eq!(
            tdd_message(TddError::RuntimeMissing {
                runtime: "JDK".into(),
                hint: "Install a JDK.".into(),
            }),
            "Install a JDK."
        );
    }

    #[test]
    fn the_default_constructor_wires_the_real_runner_detection() {
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = Greenfield::new(dir.path().to_path_buf(), None);
        // An empty directory has no build markers, so detection refuses.
        assert!((orchestrator.runner_factory)(dir.path()).is_err());
    }

    struct ScriptedPrompter {
        answers: std::collections::VecDeque<String>,
        warned: Vec<String>,
    }

    impl ScriptedPrompter {
        fn answering(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|a| a.to_string()).collect(),
                warned: Vec::new(),
            }
        }
    }

    impl Prompter for ScriptedPrompter {
        fn tell(&mut self, _message: &str) {}
        fn warn(&mut self, message: &str) {
            self.warned.push(message.to_string());
        }
        fn ask(&mut self, _question: &str) -> Result<String, PromptError> {
            self.answers
                .pop_front()
                .ok_or_else(|| PromptError("exhausted".into()))
        }
        fn confirm(&mut self, _question: &str) -> Result<bool, PromptError> {
            Ok(true)
        }
    }

    #[test]
    fn llm_attempts_are_at_least_one() {
        let dir = tempfile::tempdir().unwrap();
        let _ = Greenfield::new(dir.path().to_path_buf(), None).with_llm_attempts(0);
        let _ = Greenfield::new(dir.path().to_path_buf(), None).with_llm_attempts(5);
    }

    #[test]
    fn an_invalid_pending_pick_is_warned_and_retried() {
        let mut prompter = ScriptedPrompter::answering(&["nope", "1"]);
        let pending = vec![("REQ-001".into(), "Title".into())];
        let picked = pick_pending(&mut prompter, &pending).unwrap();
        assert_eq!(picked.as_deref(), Some("REQ-001"));
        assert!(!prompter.warned.is_empty());
    }
}

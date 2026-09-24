//! `spec deliver`: the top-level orchestrator. One command takes a
//! requirement id, a plain-words requirement, or an empty directory all
//! the way to implemented, and says at the end whether it got there.
//!
//! What separates this from [`crate::greenfield`] is the entry, the
//! verdict, and who answers. Greenfield starts at the drafting wizard and
//! ends whenever the developer stops answering; deliver resolves a *plan*
//! first - one id, the ids a description was split into, or the whole
//! pending backlog - then works the plan and reports which requirements
//! landed and which did not. A run that did not finish the plan exits
//! nonzero, so the same command is usable as a gate.
//!
//! **A run never stops to ask.** Every review gate is approved and every
//! proposal accepted, through the auto-answering prompter wired at the
//! composition root; anything that genuinely needs an answer no default
//! can supply - which language to scaffold, what to build when there is
//! no spec and no description - is refused up front with the reason and
//! the command that settles it. So a run either completes or says why it
//! could not, and it does neither halfway through a question.
//!
//! Each step is run and then **verified** against the asset survey -
//! the same deterministic reading `spec status` reports - rather than
//! trusted because the command returned `Ok`. A step whose gap is still
//! open is retried, bounded by [`VERIFY_ROUNDS`]; a step whose command
//! already loops internally (`spec draft`'s validate/refine rewording,
//! `spec implement`'s attempts, `spec refactor`'s rounds) is invoked
//! once and its own loop is trusted.
//!
//! Like [`crate::greenfield`] and [`crate::mcp`] this is a delivery
//! module: it wires the same application services, so it may name
//! concrete adapters. The prompter, runner factory, and LLM are
//! injected, so the whole orchestration is testable with fakes.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

use crate::adapters::fs_spec::FsSpecRepository;
use crate::adapters::fs_staging::FsChangeStore;
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::runners::detect_runner;
use crate::application::DEFAULT_LLM_ATTEMPTS;
use crate::application::assets::{asset_survey, load_effective_spec};
use crate::application::change_service::ChangeService;
use crate::application::generation_service::GenerationService;
use crate::application::implement_service::ImplementService;
use crate::application::refactor_service::RefactorService;
use crate::application::scenario_service::ScenarioService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::tdd_service::{TddError, TddService, TestReport};
use crate::bootstrap::{
    ensure_project, ensure_spec, has_readable_spec, project_detected, refresh_project_memory,
};
use crate::domain::language::Language;
use crate::domain::model::{Requirement, Spec};
use crate::domain::scaffold::slug;
use crate::domain::steps::criterion_to_steps;
use crate::domain::tdd::ImplementAttempt;
use crate::ports::{
    ChangeStore as _, FeatureCatalog as _, Prompter, SpecRepository as _, TestFilter, TestRunner,
};
use crate::wiring::{DynLlm, OverlayFeatures, OverlayTree, RunnerFactory};
use crate::workspace::project_layout;

/// How many times one authoring step is re-run while the asset survey
/// still reports its gap.
///
/// Two, not "until it works": these steps are deterministic template or
/// model generation against an unchanged spec, so a gap that survives
/// the retry will survive a third attempt as well, and the run is more
/// useful reporting the gap than spinning on it.
pub const VERIFY_ROUNDS: u32 = 2;

/// The default RED-to-GREEN budget for one requirement.
pub const DEFAULT_ATTEMPTS: u32 = 3;

/// What `spec deliver` was pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// One requirement already in the catalog.
    Requirement(String),
    /// Plain words to be split into requirements first.
    Description(String),
    /// Nothing named: every pending requirement, or - with no spec at
    /// all - the greenfield questions followed by a description.
    Backlog,
}

/// Read the command's argument.
///
/// An argument shaped like a requirement id is one, prose is a
/// description, and nothing at all is the backlog. A single word that was
/// reaching for an id and missed is refused rather than drafted: handing
/// `R-003` to the model as prose invents a requirement nobody asked for.
pub fn parse_target(raw: Option<&str>) -> Result<Target, String> {
    let Some(text) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(Target::Backlog);
    };
    if is_requirement_id(text) {
        return Ok(Target::Requirement(text.to_ascii_uppercase()));
    }
    if is_misspelled_id(text) {
        return Err(format!(
            "{text} is not a requirement id, and it is one word rather than a requirement \
             to break down, so nothing here can be delivered. Ids look like REQ-003 - run \
             spec list to see them. To describe new work instead, use plain words: spec \
             deliver \"a custom delimiter on the first line\"."
        ));
    }
    Ok(Target::Description(text.to_string()))
}

/// `REQ-` followed by at least one digit, and nothing else. Matched
/// case-insensitively so `req-3` on the command line still means the
/// requirement rather than a one-word description of a feature.
fn is_requirement_id(text: &str) -> bool {
    let Some(digits) = text
        .get(..4)
        .filter(|head| head.eq_ignore_ascii_case("REQ-"))
        .map(|_| &text[4..])
    else {
        return false;
    };
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// A single word that was aiming at an id: it carries a digit, or it
/// starts with `req`. Prose is several words, so `the REQ-003 one` still
/// reads as a description and a real description is never refused.
fn is_misspelled_id(text: &str) -> bool {
    !text.contains(char::is_whitespace)
        && (text.chars().any(|c| c.is_ascii_digit())
            || text
                .get(..3)
                .is_some_and(|head| head.eq_ignore_ascii_case("req")))
}

/// How hard the run tries, and when it gives up on the plan. What it may
/// ask is not an option: it may not ask anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverOptions {
    /// RED-to-GREEN rounds per requirement.
    pub attempts: u32,
    /// Stop at the first requirement that falls short instead of
    /// carrying on to the next.
    pub fail_fast: bool,
    /// Offer the model-driven refactor step on GREEN.
    pub refactor: bool,
    /// Which catalog document drafted requirements land in.
    pub file: Option<String>,
}

impl Default for DeliverOptions {
    fn default() -> Self {
        Self {
            attempts: DEFAULT_ATTEMPTS,
            fail_fast: false,
            refactor: true,
            file: None,
        }
    }
}

/// A planned requirement that did not reach implemented, and why.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Outstanding {
    pub id: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// Where the run got to.
///
/// `completed` is true only when every planned requirement landed, so a
/// partial run cannot read as a success; `outstanding` names what is
/// left and why each one stopped.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DeliverReport {
    pub planned: Vec<String>,
    pub delivered: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<Outstanding>,
    pub completed: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// One requirement's outcome.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Implemented,
    /// Already `implemented` in the spec before this run touched it.
    AlreadyImplemented,
    Stopped {
        reason: String,
        phase: Option<String>,
    },
}

pub struct Deliver {
    root: PathBuf,
    runner_factory: RunnerFactory,
    llm: Option<(String, DynLlm)>,
    llm_attempts: u32,
    options: DeliverOptions,
}

impl Deliver {
    pub fn new(root: PathBuf, llm: Option<(String, DynLlm)>, options: DeliverOptions) -> Self {
        Self::with_runner_factory(root, Arc::new(detect_runner), llm, options)
    }

    /// Constructor for tests: a scripted runner factory replaces real
    /// build-tool execution.
    pub fn with_runner_factory(
        root: PathBuf,
        runner_factory: RunnerFactory,
        llm: Option<(String, DynLlm)>,
        options: DeliverOptions,
    ) -> Self {
        Self {
            root,
            runner_factory,
            llm,
            llm_attempts: DEFAULT_LLM_ATTEMPTS,
            options,
        }
    }

    /// How many times a model reply is tried when validation fails.
    pub fn with_llm_attempts(mut self, attempts: u32) -> Self {
        self.llm_attempts = attempts.max(1);
        self
    }

    pub fn run(
        &self,
        prompter: &mut dyn Prompter,
        target: &Target,
    ) -> Result<DeliverReport, String> {
        prompter.tell(
            "Deliver: every planned requirement to implemented - scenario, steps, \
             unit test, RED, implement, GREEN, refactor, mark-implemented.",
        );
        match &self.llm {
            Some((model, _)) => prompter.tell(&format!(
                "Generation uses model {model}; templates are the fallback."
            )),
            None => prompter.tell(
                "No LLM model resolved - deterministic templates will be used for generation.",
            ),
        }

        // Scaffolding needs a language and a project name, and no default
        // can supply either. A run that may not ask says so here rather
        // than partway in, at a question nobody is there to answer.
        if !project_detected(&self.root) {
            return Err(
                "No project was detected here, and a delivery run cannot answer the \
                 language and name questions that scaffolding one asks. Run spec init \
                 --language <java|javascript|typescript|dotnet|rust> first, or spec \
                 greenfield to be walked through it, then spec deliver."
                    .into(),
            );
        }
        let language = ensure_project(&self.root, prompter)?;
        ensure_spec(&self.root, prompter)?;
        refresh_project_memory(&self.root, Some(language));
        prompter.tell(&format!(
            "Project language: {} ({}).",
            language.display(),
            language.bdd_framework()
        ));

        let planned = self.plan(prompter, target)?;
        if planned.is_empty() {
            return Ok(DeliverReport {
                planned,
                delivered: Vec::new(),
                outstanding: Vec::new(),
                completed: true,
                next_step: "Nothing is pending. Describe the next requirement with \
                            spec deliver \"<what to build>\", or draft it by hand with \
                            spec draft."
                    .into(),
            });
        }
        prompter.tell(&format!(
            "Plan: {} requirement(s) - {}.",
            planned.len(),
            planned.join(", ")
        ));

        let mut delivered = Vec::new();
        let mut outstanding = Vec::new();
        for (index, id) in planned.iter().enumerate() {
            prompter.tell(&format!("[{} of {}] {id}", index + 1, planned.len()));
            match self.deliver_one(prompter, language, id)? {
                Outcome::Implemented => {
                    prompter.tell(&format!("{id} is implemented. Loop closed."));
                    delivered.push(id.clone());
                }
                Outcome::AlreadyImplemented => {
                    prompter.tell(&format!("{id} is already implemented - nothing to do."));
                    delivered.push(id.clone());
                }
                Outcome::Stopped { reason, phase } => {
                    prompter.warn(&format!("{id} stopped: {reason}"));
                    outstanding.push(Outstanding {
                        id: id.clone(),
                        reason,
                        phase,
                    });
                    if self.options.fail_fast {
                        prompter.warn(
                            "Stopping here - the rest of the plan is untouched (--fail-fast).",
                        );
                        break;
                    }
                }
            }
        }
        Ok(self.verdict(planned, delivered, outstanding))
    }

    /// The report, worded for what actually happened. Anything left in
    /// `outstanding` - or a plan cut short by `--fail-fast` - is an
    /// incomplete run, however many requirements did land.
    fn verdict(
        &self,
        planned: Vec<String>,
        delivered: Vec<String>,
        outstanding: Vec<Outstanding>,
    ) -> DeliverReport {
        let untouched: Vec<&String> = planned
            .iter()
            .filter(|id| !delivered.contains(id) && !outstanding.iter().any(|left| &&left.id == id))
            .collect();
        let completed = outstanding.is_empty() && untouched.is_empty();
        let next_step = if completed {
            format!(
                "All {} planned requirement(s) are implemented. Describe the next one \
                 with spec deliver \"<what to build>\".",
                planned.len()
            )
        } else {
            let mut left: Vec<String> = outstanding.iter().map(|o| o.id.clone()).collect();
            left.extend(untouched.into_iter().cloned());
            format!(
                "{} of {} delivered. Still pending: {}. Read why with spec status, \
                 then run spec deliver {} again.",
                delivered.len(),
                planned.len(),
                left.join(", "),
                left.first().map(String::as_str).unwrap_or("")
            )
        };
        DeliverReport {
            planned,
            delivered,
            outstanding,
            completed,
            next_step,
        }
    }

    /// Resolve the target into the ids this run will work, in catalog
    /// order.
    fn plan(&self, prompter: &mut dyn Prompter, target: &Target) -> Result<Vec<String>, String> {
        match target {
            Target::Requirement(id) => {
                let spec = self.spec()?;
                if !spec.requirements.iter().any(|r| &r.id == id) {
                    return Err(format!(
                        "No requirement with id {id}. Run spec list to see valid ids, or \
                         describe a new one with spec deliver \"<what to build>\"."
                    ));
                }
                Ok(vec![id.clone()])
            }
            Target::Description(description) => self.draft_plan(prompter, description),
            Target::Backlog => {
                let pending = self.pending()?;
                // Nothing in the catalog and nothing described: the only
                // way on is to ask what to build, and asking is what this
                // command does not do.
                if pending.is_empty() && !self.had_spec_on_entry() {
                    return Err(
                        "There is no requirement to deliver and nothing was described, so \
                         there is nothing to work from. Say what to build with spec deliver \
                         \"<what to build>\", or run spec greenfield to be walked through \
                         wording the first requirement."
                            .into(),
                    );
                }
                Ok(pending)
            }
        }
    }

    /// Break plain words into requirements and plan the ones that were
    /// accepted. `spec draft`'s own validate/refine loop runs inside
    /// this call, so it is made once and its looping is trusted.
    fn draft_plan(
        &self,
        prompter: &mut dyn Prompter,
        description: &str,
    ) -> Result<Vec<String>, String> {
        // Breaking words into requirements is the model's job. Without
        // one, drafting is the wizard asking a human to word it, and a run
        // that may not ask has to hand that back instead.
        let Some((model, llm)) = self.memory_llm() else {
            return Err(format!(
                "Breaking \"{description}\" into requirements needs a model, and none is \
                 resolved. Point one at the project with spec model use <name>, or word \
                 the requirement yourself with spec draft and then run spec deliver."
            ));
        };
        let before = self.pending()?;
        let mutation = self.mutation_service();
        let file = self.options.file.as_deref();
        let draft = mutation
            .draft_assisted_from(prompter, &model, llm.as_ref(), file, description)
            .map_err(|e| e.to_string())?;
        self.commit()?;
        // The plan is what actually reached the catalog, read back rather
        // than taken from the draft's own report: every pending id it
        // added, so a description that held three requirements plans all
        // three, and one that produced none says so instead of planning
        // an id that is not there to work.
        let after = self.pending()?;
        let planned: Vec<String> = after
            .iter()
            .filter(|id| !before.contains(id))
            .cloned()
            .collect();
        if planned.is_empty() {
            return Err(nothing_to_plan(description, &draft.findings));
        }
        prompter.tell(&format!("{} committed to the spec.", planned.join(", ")));
        Ok(planned)
    }

    /// One requirement's delivery. Every step verifies itself against
    /// the asset survey before the next one starts.
    fn deliver_one(
        &self,
        prompter: &mut dyn Prompter,
        language: Language,
        req_id: &str,
    ) -> Result<Outcome, String> {
        let spec = self.spec()?;
        let Some(requirement) = spec.requirements.iter().find(|r| r.id == req_id) else {
            return Ok(Outcome::Stopped {
                reason: format!("{req_id} is no longer in the spec."),
                phase: None,
            });
        };
        if requirement.status == "implemented" {
            return Ok(Outcome::AlreadyImplemented);
        }

        // Staged work from an earlier session would ride along on this
        // run's first commit, and the refactor loop refuses outright
        // while anything is staged. It is the author's to settle.
        let staged = self.change_store().changes().map_err(|e| e.to_string())?;
        if !staged.is_empty() {
            return Ok(Outcome::Stopped {
                reason: format!(
                    "{} change(s) are already staged - review with spec changes show, \
                     then spec changes commit or spec changes discard before delivering.",
                    staged.len()
                ),
                phase: None,
            });
        }

        if let Some(stopped) = self.author_scenarios(prompter, language, req_id, requirement)? {
            return Ok(stopped);
        }
        if let Some(stopped) = self.author_steps(prompter, language, req_id)? {
            return Ok(stopped);
        }
        match self.author_unit_test(prompter, language, req_id)? {
            AuthoringStep::Stopped(stopped) => return Ok(stopped),
            AuthoringStep::Done => {}
        }

        // Execution only when the runtime is present; the authoring
        // above stands on its own either way.
        let runner = match (self.runner_factory)(&self.root) {
            Ok(runner) => runner,
            Err(message) => {
                prompter.warn(&message);
                return Ok(Outcome::Stopped {
                    reason: format!("Authoring is complete but the tests cannot run - {message}"),
                    phase: None,
                });
            }
        };
        let tdd = self.tdd_service();
        let Some(report) = self.try_run(&tdd, runner.as_ref(), prompter)? else {
            return Ok(Outcome::Stopped {
                reason: "Authoring is complete but the language runtime is missing, so \
                         the tests never ran."
                    .into(),
                phase: None,
            });
        };

        if let Bar::Stopped(stopped) =
            self.drive_to_green(prompter, &tdd, runner.as_ref(), language, req_id, report)?
        {
            return Ok(stopped);
        }

        if self.options.refactor
            && let Some(stopped) = self.refactor(prompter, runner.as_ref(), language, req_id)?
        {
            return Ok(stopped);
        }

        self.mark_implemented(prompter, req_id)
    }

    /// The tagged scenario. Skipped when one already exists, so
    /// delivering a requirement whose Gherkin was written earlier is not
    /// an error.
    fn author_scenarios(
        &self,
        prompter: &mut dyn Prompter,
        language: Language,
        req_id: &str,
        requirement: &Requirement,
    ) -> Result<Option<Outcome>, String> {
        if !self.scenario_missing(language, req_id)? {
            prompter.tell(&format!("A scenario tagged @{req_id} is already in place."));
            return Ok(None);
        }
        let feature_path = requirement
            .feature_file
            .clone()
            .unwrap_or_else(|| format!("features/{}.feature", slug(&requirement.title)));
        let scenarios = self.scenario_service();
        let known = self
            .feature_catalog()
            .list()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|summary| summary.path == feature_path);
        if !known {
            scenarios
                .create_feature(&feature_path, &requirement.title)
                .map_err(|e| e.to_string())?;
        }
        let mut added = 0;
        for (index, criterion) in requirement.acceptance_criteria.iter().enumerate() {
            let Some(steps) = criterion_to_steps(criterion) else {
                prompter.warn(&format!(
                    "Skipping criterion (not Given/when/then shaped): {criterion}"
                ));
                continue;
            };
            let name = format!("{} case {}", requirement.title, index + 1);
            scenarios
                .add_scenario(&feature_path, req_id, &name, steps)
                .map_err(|e| e.to_string())?;
            added += 1;
        }
        if added == 0 {
            self.discard()?;
            return Ok(Some(Outcome::Stopped {
                reason: format!(
                    "No acceptance criterion of {req_id} is Given/When/Then shaped, so no \
                     scenario could be written. Reword it with spec reword {req_id}."
                ),
                phase: None,
            }));
        }
        self.commit()?;
        prompter.tell(&format!("Scenarios committed to {feature_path}."));
        Ok(verified(self.scenario_missing(language, req_id)?, || {
            unverified_scenario(req_id, &feature_path)
        }))
    }

    /// Step definitions for every undefined step. `spec steps generate`
    /// is deterministic, so a gap that survives [`VERIFY_ROUNDS`] is
    /// reported rather than retried again.
    fn author_steps(
        &self,
        prompter: &mut dyn Prompter,
        language: Language,
        req_id: &str,
    ) -> Result<Option<Outcome>, String> {
        let generation = self.generation_service(language);
        for round in 1..=VERIFY_ROUNDS {
            let missing = generation.steps_missing().map_err(|e| e.to_string())?;
            if missing.missing.is_empty() {
                return Ok(None);
            }
            if round > 1 {
                prompter.warn(&steps_retry_notice(missing.missing.len(), round));
            }
            let work = prompter.working("Generating step definitions - working");
            let generated = generation.steps_generate(prompter);
            drop(work);
            let report = generated.map_err(|e| e.to_string())?;
            prompter.tell(&format!("Staged {} ({}).", report.target, report.source));
            self.commit()?;
        }
        let missing = generation.steps_missing().map_err(|e| e.to_string())?;
        if missing.missing.is_empty() {
            return Ok(None);
        }
        Ok(Some(Outcome::Stopped {
            reason: undefined_steps_remain(req_id, missing.missing.len()),
            phase: None,
        }))
    }

    /// The failing unit test. The generated assertions are printed before
    /// they run, because they are the one thing here a human will want to
    /// sharpen afterwards - but they are not put behind a gate, since a
    /// run that cannot be answered would only ever approve it.
    fn author_unit_test(
        &self,
        prompter: &mut dyn Prompter,
        language: Language,
        req_id: &str,
    ) -> Result<AuthoringStep, String> {
        if !self.unit_test_missing(language, req_id)? {
            prompter.tell(&format!("The unit test for {req_id} is already in place."));
            return Ok(AuthoringStep::Done);
        }
        let generation = self.generation_service(language);
        let work = prompter.working(&format!("Generating the unit test for {req_id} - working"));
        let generated = generation.unittest_generate(prompter, req_id);
        drop(work);
        let report = generated.map_err(|e| e.to_string())?;
        prompter.tell(&format!("Staged {} ({}).", report.target, report.source));
        if let Some(content) = self
            .change_store()
            .content(&report.target)
            .map_err(|e| e.to_string())?
        {
            prompter.tell("Generated unit test (the assertions are yours to sharpen):");
            prompter.tell(&content);
        }
        self.commit()?;
        match verified(self.unit_test_missing(language, req_id)?, || {
            unverified_unit_test(req_id, &report.target)
        }) {
            Some(stopped) => Ok(AuthoringStep::Stopped(stopped)),
            None => Ok(AuthoringStep::Done),
        }
    }

    /// The RED-to-GREEN loop, bounded by `--attempts`. `spec implement`
    /// runs its own model attempts inside each round, so a round here is
    /// one implementation attempt plus the test run that judges it.
    ///
    /// A bar that stayed red is a [`Bar::Stopped`] outcome rather than an
    /// error: the run reports it and carries on to the next requirement.
    fn drive_to_green(
        &self,
        prompter: &mut dyn Prompter,
        tdd: &TddService<FsStateStore>,
        runner: &dyn TestRunner,
        language: Language,
        req_id: &str,
        first: TestReport,
    ) -> Result<Bar, String> {
        let mut report = first;
        if report.phase == "GREEN" {
            return Ok(Bar::Green);
        }
        if self.llm.is_none() {
            return Ok(Bar::stopped(
                format!(
                    "The bar is RED and no model is resolved, so nothing can write the \
                     production code. Implement {req_id} by hand, then run spec deliver \
                     {req_id} again."
                ),
                Some(report.phase),
            ));
        }
        let budget = self.options.attempts.max(1);
        let implement = self.implement_service(language);
        for attempt in 1..=budget {
            prompter.tell(&format!("Attempt {attempt} of {budget}."));
            self.attempt_implementation(prompter, &implement, tdd, req_id)?;
            report = match self.try_run(tdd, runner, prompter)? {
                Some(report) => report,
                None => {
                    return Ok(Bar::stopped(
                        "The language runtime disappeared mid-loop, so the bar cannot be \
                         read."
                            .into(),
                        None,
                    ));
                }
            };
            if report.phase == "GREEN" {
                return Ok(Bar::Green);
            }
        }
        Ok(Bar::stopped(
            format!(
                "The bar is still RED after {budget} attempt(s). The failures and every \
                 attempt are recorded - read them with spec state, then run spec deliver \
                 {req_id} again for another {budget}, or implement by hand."
            ),
            Some(report.phase),
        ))
    }

    /// The refactor step, on a green bar. Skipped by `--no-refactor` and
    /// when no model is resolved, since there would be nothing to drive
    /// it; otherwise it runs, because the alternative is a question.
    ///
    /// This is the model-driven [`RefactorService`] loop, which runs the
    /// suite itself once per round and restores the code when a round
    /// cannot stay green - not greenfield's hand-off to the developer's
    /// editor, which has nobody to hand off to here. Its rounds are its
    /// own, so it is called once. A refactor that cannot land is not a
    /// failed delivery: the behaviour is already green and the
    /// requirement still gets marked.
    fn refactor(
        &self,
        prompter: &mut dyn Prompter,
        runner: &dyn TestRunner,
        language: Language,
        req_id: &str,
    ) -> Result<Option<Outcome>, String> {
        let service = self.refactor_service(language);
        if !service.has_model() {
            return Ok(None);
        }
        let goal = format!("Clean up the production code that implements {req_id}");
        // The phase gate first: it is what refuses off GREEN, and it
        // records the note in the phase log.
        if let Err(error) = self.tdd_service().refactor(Some(&goal)) {
            prompter.warn(&format!("{} Skipping the refactor.", tdd_message(error)));
            return Ok(None);
        }
        match service.run(prompter, runner, Some(&goal), Some(req_id)) {
            Ok(report) => {
                if let Some(warning) = &report.warning {
                    prompter.warn(warning);
                }
                for target in &report.targets {
                    prompter.tell(&format!("Refactored {target}."));
                }
                if report.reverted {
                    prompter.warn(
                        "No refactor round stayed green, so the code was restored. The \
                         behaviour is unchanged and the loop continues.",
                    );
                }
                // A refactor leaves the bar green by construction, but
                // mark-implemented reads the recorded phase, and the
                // gate above moved it to REFACTOR. Put it back on the
                // bar the refactor proved.
                let runner_report = self.try_run(&self.tdd_service(), runner, prompter)?;
                match runner_report {
                    Some(report) if report.phase == "GREEN" => Ok(None),
                    Some(report) => Ok(Some(Outcome::Stopped {
                        reason: format!(
                            "The refactor left the bar {}. Make the tests pass again, then \
                             run spec deliver {req_id}.",
                            report.phase
                        ),
                        phase: Some(report.phase),
                    })),
                    None => Ok(Some(Outcome::Stopped {
                        reason: "The language runtime disappeared after the refactor.".into(),
                        phase: None,
                    })),
                }
            }
            Err(error) => {
                prompter.warn(&format!("{} Skipping the refactor.", error.0));
                // The gate moved the phase to REFACTOR and the loop
                // never ran, so the green bar has to be re-established
                // before the requirement can be marked.
                match self.try_run(&self.tdd_service(), runner, prompter)? {
                    Some(report) if report.phase == "GREEN" => Ok(None),
                    Some(report) => Ok(Some(Outcome::Stopped {
                        reason: format!("The bar is {} after the refactor attempt.", report.phase),
                        phase: Some(report.phase),
                    })),
                    None => Ok(Some(Outcome::Stopped {
                        reason: "The language runtime disappeared after the refactor attempt."
                            .into(),
                        phase: None,
                    })),
                }
            }
        }
    }

    /// Close the loop: flip the status, validate the staged edit, apply
    /// it, and read the spec back to be sure it landed.
    fn mark_implemented(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
    ) -> Result<Outcome, String> {
        let work = prompter.working("Saving status - working");
        let marked = self.mutation_service().mark_implemented(req_id);
        drop(work);
        if let Err(error) = marked {
            return Ok(Outcome::Stopped {
                reason: format!("{req_id} could not be marked implemented - {}", error.0),
                phase: None,
            });
        }
        let validation = self
            .change_service()
            .validate()
            .map_err(|e| e.to_string())?;
        if !validation.valid {
            self.discard()?;
            return Ok(Outcome::Stopped {
                reason: invalid_mark(req_id, &validation.issues),
                phase: None,
            });
        }
        self.commit()?;
        let implemented = self
            .spec()?
            .requirements
            .iter()
            .any(|r| r.id == req_id && r.status == "implemented");
        Ok(verified(!implemented, || unverified_mark(req_id)).unwrap_or(Outcome::Implemented))
    }

    /// Ask the model to make the failing tests pass and commit whatever
    /// it staged. A model failure is narrated, not fatal - the next
    /// round, or a human, can still implement.
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

    /// Whether the asset survey still reports a gap whose finding
    /// contains `needle`. The survey is the same deterministic reading
    /// `spec status` reports, so a step is verified by what the next
    /// command would see rather than by its own return value.
    fn survey_gap(&self, language: Language, req_id: &str, needle: &str) -> Result<bool, String> {
        let layout = project_layout(&self.root);
        let spec =
            load_effective_spec(&self.spec_repository(), &self.change_store()).map_err(|e| e.0)?;
        let Some(requirement) = spec.requirements.iter().find(|r| r.id == req_id) else {
            return Ok(false);
        };
        let (_, findings) = asset_survey(
            &self.feature_catalog(),
            &crate::wiring::overlay_sources(&self.root, layout.module_root.as_deref()),
            language,
            req_id,
            requirement,
            &spec.project,
            &layout,
        )
        .map_err(|e| e.0)?;
        Ok(findings.iter().any(|finding| finding.contains(needle)))
    }

    fn scenario_missing(&self, language: Language, req_id: &str) -> Result<bool, String> {
        self.survey_gap(
            language,
            req_id,
            &format!("No scenario is tagged @{req_id}"),
        )
    }

    fn unit_test_missing(&self, language: Language, req_id: &str) -> Result<bool, String> {
        self.survey_gap(
            language,
            req_id,
            "does not exist - run spec unittest generate",
        )
    }

    /// The pending requirement ids of the committed spec, in catalog
    /// order.
    fn pending(&self) -> Result<Vec<String>, String> {
        Ok(self
            .spec()?
            .requirements
            .into_iter()
            .filter(|r| r.status == "pending")
            .map(|r| r.id)
            .collect())
    }

    /// Whether the root had a readable spec before this run wrote an
    /// empty one. Recorded by reading the file's requirements: an empty
    /// catalog that `ensure_spec` just created has none.
    fn had_spec_on_entry(&self) -> bool {
        has_readable_spec(&self.root)
            && self
                .spec()
                .map(|spec| !spec.requirements.is_empty())
                .unwrap_or(false)
    }

    fn spec(&self) -> Result<Spec, String> {
        self.spec_repository().load().map_err(|e| e.0)
    }

    fn spec_repository(&self) -> FsSpecRepository {
        crate::wiring::spec_repository(&self.root)
    }

    fn feature_catalog(&self) -> crate::wiring::OverlayFeatures {
        crate::wiring::overlay_catalog(&self.root)
    }

    fn commit(&self) -> Result<(), String> {
        self.change_service().commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn discard(&self) -> Result<(), String> {
        self.change_service().discard().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn change_store(&self) -> FsChangeStore {
        crate::wiring::change_store(&self.root)
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

    /// The refactor loop's own rounds are bounded by `--attempts`, the
    /// same budget the RED-to-GREEN loop spends.
    fn refactor_service(
        &self,
        language: Language,
    ) -> RefactorService<OverlayTree, FsChangeStore, FsSpecRepository, DynLlm> {
        crate::wiring::refactor_service(
            &self.root,
            language,
            self.llm.as_ref(),
            self.llm_attempts,
            self.options.attempts.max(1),
        )
    }

    /// The session LLM with project memory prepended to every system
    /// prompt, for the drafting call that takes one directly.
    fn memory_llm(&self) -> crate::wiring::SessionLlm {
        crate::wiring::memory_llm(&self.root, self.llm.as_ref())
    }

    fn tdd_service(&self) -> TddService<FsStateStore> {
        crate::wiring::tdd_service(&self.root)
    }
}

/// Whether an authoring step finished or stopped the delivery. A plain
/// `Option<Outcome>` reads as "maybe nothing happened" at the call site;
/// this says which of the two it was.
enum AuthoringStep {
    Done,
    Stopped(Outcome),
}

/// Where the RED-to-GREEN loop left the bar. A red bar is an outcome the
/// run reports, not an error, so this is the loop's `Ok` rather than a
/// `Result` inside a `Result`.
enum Bar {
    Green,
    Stopped(Outcome),
}

impl Bar {
    fn stopped(reason: String, phase: Option<String>) -> Self {
        Self::Stopped(Outcome::Stopped { reason, phase })
    }
}

/// How every step ends: the gap it was supposed to close is read again,
/// and a gap still open means the step did not do what it said.
///
/// `Some` is that stop. Nothing is retried here - the commands are
/// deterministic, so a second identical run would land in the same
/// place; `reason` is built only when it is needed.
fn verified(still_open: bool, reason: impl FnOnce() -> String) -> Option<Outcome> {
    still_open.then(|| Outcome::Stopped {
        reason: reason(),
        phase: None,
    })
}

/// The stops a verification reports when the command that wrote an asset
/// returned `Ok` but the survey cannot see its work.
///
/// A step that says it succeeded and did not is worse than one that
/// failed, because the next step would build on nothing. None of these
/// is retried: the same deterministic command would do the same thing
/// again, so each names the asset, the requirement, and the command that
/// shows the human what actually landed.
fn unverified_scenario(req_id: &str, feature_path: &str) -> String {
    format!(
        "The scenario was written to {feature_path} but no scenario tagged @{req_id} can be \
         read back. Inspect it with spec feature show {feature_path}."
    )
}

fn unverified_unit_test(req_id: &str, target: &str) -> String {
    format!(
        "{target} was committed but the unit test for {req_id} still reads as missing. \
         Inspect it before delivering again."
    )
}

fn unverified_mark(req_id: &str) -> String {
    format!(
        "{req_id} was committed but the spec still does not read as implemented. Check it \
         with spec show {req_id}."
    )
}

fn invalid_mark(req_id: &str, issues: &[String]) -> String {
    format!(
        "The staged mark-implemented for {req_id} did not validate ({}), so it was discarded.",
        issues.join("; ")
    )
}

/// A breakdown that put nothing in the catalog. Whatever the wording
/// review was still objecting to goes back with it, because that is the
/// only thing that says what to fix.
fn nothing_to_plan(description: &str, findings: &[String]) -> String {
    let because = match findings.is_empty() {
        true => String::new(),
        false => format!(
            " The wording review still objects: {}.",
            findings.join("; ")
        ),
    };
    format!(
        "Breaking \"{description}\" down added no requirement to the catalog, so there is \
         nothing to deliver.{because} Word it yourself with spec draft, then run spec deliver."
    )
}

/// Step generation is deterministic, so a gap that survived
/// [`VERIFY_ROUNDS`] needs a human rather than another round.
fn undefined_steps_remain(req_id: &str, count: usize) -> String {
    format!(
        "{count} step(s) still have no definition after {VERIFY_ROUNDS} rounds of spec steps \
         generate, so {req_id} cannot go RED honestly. Define them by hand and run spec \
         deliver {req_id} again."
    )
}

fn steps_retry_notice(count: usize, round: u32) -> String {
    format!(
        "{count} step(s) are still undefined - generating again (round {round} of {VERIFY_ROUNDS})."
    )
}

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
    use crate::adapters::auto_prompt::AutoPrompter;
    use crate::workspace::SPEC_PATH;
    use std::path::Path;

    #[test]
    fn an_argument_shaped_like_an_id_is_one() {
        assert_eq!(
            parse_target(Some("REQ-003")),
            Ok(Target::Requirement("REQ-003".into()))
        );
        assert_eq!(
            parse_target(Some("req-3")),
            Ok(Target::Requirement("REQ-3".into()))
        );
        assert_eq!(
            parse_target(Some("  REQ-0042  ")),
            Ok(Target::Requirement("REQ-0042".into()))
        );
    }

    #[test]
    fn prose_is_a_description() {
        assert_eq!(
            parse_target(Some("a custom delimiter on the first line")),
            Ok(Target::Description(
                "a custom delimiter on the first line".into()
            ))
        );
        // Several words are prose even when one of them is an id, so a
        // real description is never mistaken for a typo.
        for prose in ["the REQ-003 one", "sum 2 numbers", "REQ- and REQ-2"] {
            assert!(
                matches!(parse_target(Some(prose)), Ok(Target::Description(_))),
                "{prose} should read as a description"
            );
        }
    }

    /// The failure this refuses: `R-003` is one word reaching for an id,
    /// and drafting from it asks the model to invent a requirement out of
    /// the typo. Every one of these has to be refused with the ids named.
    #[test]
    fn a_single_word_reaching_for_an_id_is_refused_not_drafted() {
        for typo in ["R-003", "REQ-1a", "REQ", "REQ-", "REQUIRE-1", "003", "req7"] {
            let error = parse_target(Some(typo))
                .expect_err(&format!("{typo} should be refused, not drafted"));
            assert!(error.contains(typo), "{error}");
            assert!(error.contains("REQ-003"), "{error}");
            assert!(error.contains("spec list"), "{error}");
        }
    }

    #[test]
    fn nothing_named_is_the_backlog() {
        assert_eq!(parse_target(None), Ok(Target::Backlog));
        assert_eq!(parse_target(Some("")), Ok(Target::Backlog));
        assert_eq!(parse_target(Some("   ")), Ok(Target::Backlog));
    }

    /// A multi-byte first character used to panic the `get(..4)` slice,
    /// and the one-word check slices too.
    #[test]
    fn a_non_ascii_argument_is_a_description_not_a_panic() {
        assert!(matches!(
            parse_target(Some("données de test")),
            Ok(Target::Description(_))
        ));
        assert!(matches!(
            parse_target(Some("é")),
            Ok(Target::Description(_))
        ));
        assert!(matches!(
            parse_target(Some("aaé")),
            Ok(Target::Description(_))
        ));
    }

    fn deliver(root: &Path) -> Deliver {
        Deliver::new(root.to_path_buf(), None, DeliverOptions::default())
    }

    #[test]
    fn a_plan_fully_delivered_reads_as_completed() {
        let dir = tempfile::tempdir().unwrap();
        let report = deliver(dir.path()).verdict(
            vec!["REQ-001".into(), "REQ-002".into()],
            vec!["REQ-001".into(), "REQ-002".into()],
            Vec::new(),
        );
        assert!(report.completed);
        assert!(
            report.next_step.starts_with("All 2 planned requirement(s)"),
            "{}",
            report.next_step
        );
        // An empty list is not serialized, so a clean run's reply has
        // no outstanding key at all.
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("outstanding"), "{json}");
    }

    /// The failure this guards: a run that delivered one of three used
    /// to be able to report success because nothing errored.
    #[test]
    fn a_partial_plan_is_not_completed_and_names_what_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let report = deliver(dir.path()).verdict(
            vec!["REQ-001".into(), "REQ-002".into(), "REQ-003".into()],
            vec!["REQ-001".into()],
            vec![Outstanding {
                id: "REQ-002".into(),
                reason: "still RED".into(),
                phase: Some("RED".into()),
            }],
        );
        assert!(!report.completed);
        assert!(report.next_step.contains("1 of 3 delivered"));
        assert!(
            report.next_step.contains("REQ-002, REQ-003"),
            "the untouched tail is named too: {}",
            report.next_step
        );
        assert!(report.next_step.contains("spec deliver REQ-002"));
    }

    /// `--fail-fast` breaks the loop, so the ids after the failure were
    /// never attempted. They are still outstanding work.
    #[test]
    fn requirements_never_reached_count_as_outstanding() {
        let dir = tempfile::tempdir().unwrap();
        let report = deliver(dir.path()).verdict(
            vec!["REQ-001".into(), "REQ-002".into()],
            Vec::new(),
            vec![Outstanding {
                id: "REQ-001".into(),
                reason: "declined".into(),
                phase: None,
            }],
        );
        assert!(!report.completed);
        assert_eq!(report.delivered, Vec::<String>::new());
        assert!(report.next_step.contains("0 of 2 delivered"));
        assert!(report.next_step.contains("REQ-001, REQ-002"));
    }

    #[test]
    fn the_default_options_try_three_times_and_offer_the_refactor() {
        let options = DeliverOptions::default();
        assert_eq!(options.attempts, DEFAULT_ATTEMPTS);
        assert!(options.refactor);
        assert!(!options.fail_fast);
        assert!(options.file.is_none());
    }

    #[test]
    fn the_default_constructor_wires_the_real_runner_detection() {
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = deliver(dir.path());
        // An empty directory has no build markers, so detection refuses.
        assert!((orchestrator.runner_factory)(dir.path()).is_err());
    }

    #[test]
    fn llm_attempts_are_at_least_one() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(deliver(dir.path()).with_llm_attempts(0).llm_attempts, 1);
        assert_eq!(deliver(dir.path()).with_llm_attempts(7).llm_attempts, 7);
    }

    /// A closed gap is not an outcome at all, so the step carries on;
    /// only an open one becomes a stop, and the reason is not even built
    /// for the closed case.
    #[test]
    fn a_closed_gap_is_not_an_outcome_and_an_open_one_carries_its_reason() {
        assert!(verified(false, || panic!("no reason is needed")).is_none());
        let stopped = verified(true, || "the scenario cannot be read back".into());
        assert_eq!(
            stopped,
            Some(Outcome::Stopped {
                reason: "the scenario cannot be read back".into(),
                phase: None,
            })
        );
    }

    /// Every verification stop has to name the requirement and the
    /// command that shows the human what really landed; a stop that only
    /// says "something went wrong" leaves nowhere to go.
    #[test]
    fn a_verification_stop_names_the_requirement_and_the_command_to_run() {
        let scenario = unverified_scenario("REQ-001", "features/kata.feature");
        assert!(scenario.contains("@REQ-001"), "{scenario}");
        assert!(
            scenario.contains("spec feature show features/kata.feature"),
            "{scenario}"
        );

        let unit_test = unverified_unit_test("REQ-001", "src/test/java/Req001Test.java");
        assert!(
            unit_test.starts_with("src/test/java/Req001Test.java"),
            "{unit_test}"
        );
        assert!(unit_test.contains("REQ-001"), "{unit_test}");

        let mark = unverified_mark("REQ-001");
        assert!(mark.contains("spec show REQ-001"), "{mark}");
    }

    /// The validation issues are the only clue to why the edit was
    /// refused, so they travel with the stop rather than being swallowed.
    #[test]
    fn a_refused_mark_carries_the_issues_that_refused_it() {
        let reason = invalid_mark(
            "REQ-001",
            &[
                "REQ-001: no scenario tagged @REQ-001".into(),
                "REQ-002: story is empty".into(),
            ],
        );
        assert!(reason.contains("no scenario tagged @REQ-001"), "{reason}");
        assert!(reason.contains("story is empty"), "{reason}");
        assert!(reason.contains("was discarded"), "{reason}");
    }

    #[test]
    fn an_undefined_step_that_survives_the_rounds_is_handed_back_with_its_count() {
        let reason = undefined_steps_remain("REQ-001", 3);
        assert!(
            reason.starts_with("3 step(s) still have no definition"),
            "{reason}"
        );
        assert!(
            reason.contains(&format!("{VERIFY_ROUNDS} rounds")),
            "{reason}"
        );
        assert!(reason.contains("spec deliver REQ-001 again"), "{reason}");

        let notice = steps_retry_notice(2, 2);
        assert_eq!(
            notice,
            format!(
                "2 step(s) are still undefined - generating again (round 2 of {VERIFY_ROUNDS})."
            )
        );
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
    fn an_unknown_id_is_refused_naming_spec_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            r#"{"project":"Kata","requirements":[]}"#,
        )
        .unwrap();
        let error = deliver(dir.path())
            .plan(
                &mut Script::default(),
                &Target::Requirement("REQ-404".into()),
            )
            .unwrap_err();
        assert!(error.contains("No requirement with id REQ-404"), "{error}");
        assert!(error.contains("spec list"), "{error}");
    }

    #[test]
    fn the_backlog_plan_is_every_pending_id_in_catalog_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            r#"{"project":"Kata","requirements":[
                {"id":"REQ-001","title":"One","status":"implemented","story":"s","acceptanceCriteria":["Given a, when b, then c"]},
                {"id":"REQ-002","title":"Two","status":"pending","story":"s","acceptanceCriteria":["Given a, when b, then c"]},
                {"id":"REQ-003","title":"Three","status":"pending","story":"s","acceptanceCriteria":["Given a, when b, then c"]}
            ]}"#,
        )
        .unwrap();
        let planned = deliver(dir.path())
            .plan(&mut Script::default(), &Target::Backlog)
            .unwrap();
        assert_eq!(planned, vec!["REQ-002", "REQ-003"]);
    }

    /// A spec whose requirements are all implemented plans nothing, and
    /// that is a completed run rather than a failure.
    #[test]
    fn an_exhausted_backlog_plans_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            r#"{"project":"Kata","requirements":[
                {"id":"REQ-001","title":"One","status":"implemented","story":"s","acceptanceCriteria":["Given a, when b, then c"]}
            ]}"#,
        )
        .unwrap();
        let orchestrator = deliver(dir.path());
        assert!(
            orchestrator
                .plan(&mut Script::default(), &Target::Backlog)
                .unwrap()
                .is_empty()
        );
        assert!(orchestrator.had_spec_on_entry());
    }

    /// The hang this refuses instead of: `prompt_language` re-asks until
    /// the answer parses, and a run that answers every question with the
    /// empty string never parses one.
    #[test]
    fn an_empty_directory_refuses_rather_than_asking_which_language() {
        let error = deliver(tempfile::tempdir().unwrap().path())
            .run(&mut Script::default(), &Target::Backlog)
            .unwrap_err();
        assert!(error.contains("No project was detected"), "{error}");
        assert!(error.contains("spec init --language"), "{error}");
        assert!(error.contains("spec greenfield"), "{error}");
    }

    #[test]
    fn an_empty_catalog_is_not_a_spec_to_deliver_from() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            r#"{"project":"Kata","requirements":[]}"#,
        )
        .unwrap();
        assert!(!deliver(dir.path()).had_spec_on_entry());
    }

    /// A prompter that keeps what it was told and refuses to be asked.
    ///
    /// `spec deliver` never stops for an answer, so a question reaching a
    /// double is the bug this catches. The two paths that do put a
    /// question to a *service* - drafting a description - go through
    /// [`nobody`], which answers the way the shipped prompter does.
    #[derive(Default)]
    struct Script {
        told: Vec<String>,
    }

    impl Script {
        fn said(&self, fragment: &str) -> bool {
            self.told.iter().any(|line| line.contains(fragment))
        }
    }

    impl Prompter for Script {
        fn tell(&mut self, message: &str) {
            self.told.push(message.to_string());
        }
        fn ask(&mut self, question: &str) -> Result<String, crate::ports::PromptError> {
            panic!("a delivery must not ask, but {question:?} was asked");
        }
        fn confirm(&mut self, question: &str) -> Result<bool, crate::ports::PromptError> {
            panic!("a delivery must not ask, but {question:?} was confirmed");
        }
    }

    /// One scripted outcome per test run, in order. The queue is shared,
    /// so every runner the factory hands out draws from the same script.
    struct Queue(Arc<std::sync::Mutex<std::collections::VecDeque<TestOutcome>>>);

    type TestOutcome = Result<crate::domain::model::TestRunSummary, crate::ports::RunnerError>;

    impl TestRunner for Queue {
        fn run(&self, _: &TestFilter) -> TestOutcome {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .expect("another scripted test run")
        }
    }

    fn green() -> TestOutcome {
        Ok(crate::domain::model::TestRunSummary {
            tests: 1,
            ..Default::default()
        })
    }

    fn red() -> TestOutcome {
        Ok(crate::domain::model::TestRunSummary {
            tests: 1,
            failures: 1,
            failure_details: vec!["Req001Test: TODO: assert".into()],
            ..Default::default()
        })
    }

    fn runtime_gone() -> TestOutcome {
        Err(crate::ports::RunnerError::RuntimeMissing {
            runtime: "JDK".into(),
            hint: "Install a JDK 17+.".into(),
        })
    }

    /// A runner factory that serves the scripted outcomes in order.
    fn queued(outcomes: Vec<TestOutcome>) -> RunnerFactory {
        let queue = Arc::new(std::sync::Mutex::new(outcomes.into_iter().collect()));
        Arc::new(move |_: &Path| Ok(Box::new(Queue(Arc::clone(&queue))) as Box<dyn TestRunner>))
    }

    /// A model that always replies with the same text, or always fails.
    struct Fixed(Result<String, String>);

    impl crate::ports::LlmConversation for Fixed {
        fn chat(
            &self,
            _model: &str,
            _messages: &[crate::domain::tools::ChatMessage],
            _tools: &[crate::domain::tools::ToolDefinition],
        ) -> Result<crate::domain::tools::ChatTurn, crate::ports::LlmError> {
            match &self.0 {
                Ok(reply) => Ok(crate::domain::tools::text_turn(reply.clone())),
                Err(message) => Err(crate::ports::LlmError(message.clone())),
            }
        }
    }

    fn model(reply: Result<&str, &str>) -> Option<(String, DynLlm)> {
        let fixed = Fixed(match reply {
            Ok(text) => Ok(text.to_string()),
            Err(message) => Err(message.to_string()),
        });
        Some(("scripted-model".into(), Arc::new(fixed) as DynLlm))
    }

    /// A Java project with one pending requirement, which is the state
    /// every delivery step below starts from.
    fn kata(criteria: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            format!(
                r#"{{"project":"Kata","requirements":[{{"id":"REQ-001","title":"Adds","status":"pending",
                   "story":"As a user, I want to add so that totals are right.",
                   "featureFile":"features/kata.feature","acceptanceCriteria":[{criteria}]}}]}}"#
            ),
        )
        .unwrap();
        dir
    }

    fn one_criterion() -> &'static str {
        r#""Given an empty string, when add is called, then the result is 0""#
    }

    fn orchestrator(
        root: &Path,
        runs: Vec<TestOutcome>,
        llm: Option<(String, DynLlm)>,
        options: DeliverOptions,
    ) -> Deliver {
        Deliver::with_runner_factory(root.to_path_buf(), queued(runs), llm, options)
    }

    fn no_refactor() -> DeliverOptions {
        DeliverOptions {
            refactor: false,
            ..DeliverOptions::default()
        }
    }

    /// The whole delivery for one requirement, which is what the
    /// branch tests below vary one condition of.
    fn deliver_one(
        root: &Path,
        prompter: &mut Script,
        runs: Vec<TestOutcome>,
        llm: Option<(String, DynLlm)>,
        options: DeliverOptions,
    ) -> Outcome {
        orchestrator(root, runs, llm, options)
            .deliver_one(prompter, Language::Java, "REQ-001")
            .expect("the delivery reports rather than errors")
    }

    fn stopped_because(outcome: &Outcome) -> &str {
        match outcome {
            Outcome::Stopped { reason, .. } => reason,
            other => panic!("expected a stop, got {other:?}"),
        }
    }

    /// Closing the loop on an id the spec does not hold is reported as
    /// that requirement's outcome, not raised as the run's error: the
    /// rest of the plan is still deliverable.
    #[test]
    fn a_status_that_cannot_be_flipped_stops_that_requirement() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let outcome = deliver(dir.path())
            .mark_implemented(&mut prompter, "REQ-404")
            .unwrap();
        assert!(
            stopped_because(&outcome).contains("REQ-404 could not be marked implemented"),
            "{outcome:?}"
        );
    }

    /// Marking is staged and validated like every other edit, and the
    /// validation reads the whole spec. A requirement left implemented
    /// without a scenario by some earlier session makes every later mark
    /// invalid, so this one is discarded and the issue is named rather
    /// than a half-valid spec being committed.
    #[test]
    fn a_mark_that_would_commit_an_invalid_spec_is_discarded_with_its_issues() {
        let dir = kata(one_criterion());
        let mut spec = deliver(dir.path()).spec().unwrap();
        spec.requirements.push(Requirement {
            id: "REQ-002".into(),
            title: "Unproven".into(),
            status: "implemented".into(),
            story: "As a user, I want this so that history is honest.".into(),
            feature_file: Some("features/kata.feature".into()),
            acceptance_criteria: vec!["Given a, when b, then c".into()],
        });
        std::fs::write(
            dir.path().join(SPEC_PATH),
            serde_json::to_string(&spec).unwrap(),
        )
        .unwrap();
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![green()],
            None,
            no_refactor(),
        );
        let reason = stopped_because(&outcome);
        assert!(reason.contains("did not validate"), "{reason}");
        assert!(reason.contains("REQ-002"), "{reason}");
        assert!(reason.contains("was discarded"), "{reason}");
        assert!(
            deliver(dir.path())
                .change_store()
                .changes()
                .unwrap()
                .is_empty(),
            "the refused edit left nothing staged"
        );
    }

    /// The spec can change under a long run - a requirement named in the
    /// plan may be gone by the time its turn comes.
    #[test]
    fn a_requirement_that_left_the_spec_mid_run_stops_that_requirement_only() {
        let dir = kata(one_criterion());
        let outcome = orchestrator(dir.path(), Vec::new(), None, no_refactor())
            .deliver_one(&mut Script::default(), Language::Java, "REQ-404")
            .unwrap();
        assert!(stopped_because(&outcome).contains("REQ-404 is no longer in the spec."));
    }

    /// A refactor cannot start off a green bar, and the phase gate is
    /// what says so. That is not a failed delivery - the behaviour is
    /// already proven, so the loop closes without the cleanup.
    #[test]
    fn a_refactor_the_phase_gate_refuses_is_skipped_not_fatal() {
        let dir = kata(one_criterion());
        let orchestrator = orchestrator(
            dir.path(),
            Vec::new(),
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"class Kata {}"}]"#,
            )),
            DeliverOptions::default(),
        );
        let runner = (orchestrator.runner_factory)(dir.path()).unwrap();
        let mut prompter = Script::default();
        // No bar has been run, so the recorded phase is START and the
        // gate refuses before the model is ever asked.
        let stopped = orchestrator
            .refactor(&mut prompter, runner.as_ref(), Language::Rust, "REQ-001")
            .unwrap();
        assert!(stopped.is_none(), "{stopped:?}");
        assert!(prompter.said("Skipping the refactor."));
        assert!(prompter.said("current phase: START"), "{:?}", prompter.told);
    }

    #[test]
    fn a_gap_is_never_reported_for_a_requirement_that_is_not_in_the_spec() {
        let dir = kata(one_criterion());
        assert!(
            !deliver(dir.path())
                .survey_gap(Language::Rust, "REQ-404", "anything")
                .unwrap(),
            "an absent requirement has no gaps to report"
        );
    }

    /// Both assets already written by hand: the delivery says so and
    /// goes straight to the bar rather than writing them twice.
    #[test]
    fn assets_that_are_already_in_place_are_reported_and_skipped() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let first = deliver_one(
            dir.path(),
            &mut prompter,
            vec![green()],
            None,
            no_refactor(),
        );
        assert!(matches!(first, Outcome::Implemented), "{first:?}");

        // Back to pending with the scenario and the unit test still on
        // disk, which is the state a hand-authored requirement is in.
        let spec = std::fs::read_to_string(dir.path().join(SPEC_PATH)).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            spec.replace("implemented", "pending"),
        )
        .unwrap();
        let mut prompter = Script::default();
        let again = deliver_one(
            dir.path(),
            &mut prompter,
            vec![green()],
            None,
            no_refactor(),
        );
        assert!(matches!(again, Outcome::Implemented), "{again:?}");
        assert!(prompter.said("A scenario tagged @REQ-001 is already in place."));
        assert!(prompter.said("The unit test for REQ-001 is already in place."));
    }

    /// The runtime can go away between the first run and a later one -
    /// a container restart, a toolchain switch. The bar cannot be read,
    /// so the loop stops rather than guessing.
    #[test]
    fn a_runtime_that_disappears_mid_loop_stops_the_requirement() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![red(), runtime_gone()],
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"class Kata {}"}]"#,
            )),
            no_refactor(),
        );
        assert!(stopped_because(&outcome).contains("runtime disappeared mid-loop"));
    }

    /// A model failure is narrated and the attempt is spent, not turned
    /// into an error: a human can still implement by hand.
    #[test]
    fn a_model_that_fails_is_narrated_and_the_budget_is_spent() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![red(), red()],
            model(Err("the endpoint refused the connection")),
            DeliverOptions {
                attempts: 1,
                ..no_refactor()
            },
        );
        assert!(prompter.said("Implement by hand instead."));
        assert!(stopped_because(&outcome).contains("still RED after 1 attempt(s)"));
    }

    /// A build that fails outright is not a red bar and not a missing
    /// runtime; it is an error the run cannot interpret.
    #[test]
    fn a_build_that_cannot_run_is_an_error_not_an_outcome() {
        let dir = kata(one_criterion());
        let error = orchestrator(
            dir.path(),
            vec![Err(crate::ports::RunnerError::Failed(
                "mvn exited 1: dependency resolution failed".into(),
            ))],
            None,
            no_refactor(),
        )
        .deliver_one(&mut Script::default(), Language::Java, "REQ-001")
        .unwrap_err();
        assert!(error.contains("dependency resolution failed"), "{error}");
    }

    /// Whatever the wording review was still objecting to is the only
    /// thing that says what to fix, so it goes back with the refusal.
    #[test]
    fn a_refused_breakdown_carries_the_findings_that_refused_it() {
        let with = nothing_to_plan("adding", &["criteria: only happy paths".into()]);
        assert!(
            with.contains("still objects: criteria: only happy paths."),
            "{with}"
        );
        let without = nothing_to_plan("adding", &[]);
        assert!(!without.contains("still objects"), "{without}");
        assert!(without.contains("spec draft"), "{without}");
    }

    /// A breakdown that adds nothing leaves no plan to work, and the run
    /// says that rather than planning an id the catalog does not hold.
    #[test]
    fn a_breakdown_that_adds_no_requirement_is_refused_as_a_plan() {
        let error = empty_catalog_orchestrator(model(Ok("[]")))
            .draft_plan(&mut nobody(), "adding")
            .unwrap_err();
        assert!(error.contains("added no requirement"), "{error}");
        assert!(error.contains("spec draft"), "{error}");
    }

    /// The plan is every id the breakdown added, read back from the
    /// catalog, so a description holding two requirements plans both.
    #[test]
    fn the_plan_is_every_requirement_the_breakdown_added() {
        let planned = empty_catalog_orchestrator(model(Ok(
            r#"[{"title": "Adds two numbers", "story": "As a user, I want to add so that totals are right.", "acceptanceCriteria": ["Given an empty string, when add is called, then the result is 0"]},
                {"title": "Rejects blanks", "story": "As a user, I want blanks rejected so that mistakes surface.", "acceptanceCriteria": ["Given a blank string, when add is called, then an error is raised"]}]"#,
        )))
        .draft_plan(&mut nobody(), "adding")
        .unwrap();
        assert_eq!(planned, vec!["REQ-001", "REQ-002"]);
    }

    /// The prompter the CLI actually wires, for the one step that puts
    /// questions to a service: every proposal accepted, nothing read.
    fn nobody() -> AutoPrompter<Script> {
        AutoPrompter::new(Script::default(), "spec deliver never stops to ask")
    }

    /// Words have to be broken down by something, and the run cannot ask
    /// a human to do it, so it hands the work back naming both ways on.
    #[test]
    fn a_description_with_no_model_to_break_it_down_is_handed_back() {
        let error = empty_catalog_orchestrator(None)
            .draft_plan(&mut Script::default(), "adding two numbers")
            .unwrap_err();
        assert!(error.contains("adding two numbers"), "{error}");
        assert!(error.contains("needs a model"), "{error}");
        assert!(error.contains("spec model use"), "{error}");
        assert!(error.contains("spec draft"), "{error}");
    }

    /// A Java project whose catalog is readable but holds nothing, which
    /// is where every description-mode run starts.
    fn empty_catalog_orchestrator(llm: Option<(String, DynLlm)>) -> Deliver {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join(SPEC_PATH),
            r#"{"project":"Kata","requirements":[]}"#,
        )
        .unwrap();
        let root = dir.keep();
        Deliver::new(root, llm, DeliverOptions::default())
    }

    /// A refactor round that cannot stay green is restored, and that is
    /// reported without failing the delivery: the behaviour is green.
    #[test]
    fn a_refactor_that_cannot_land_is_restored_and_the_loop_still_closes() {
        let dir = kata(one_criterion());
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        std::fs::write(
            dir.path().join("src/main/java/Kata.java"),
            "public class Kata { int add(String input) { return 0; } }\n",
        )
        .unwrap();
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            // The gate run, the refactor's baseline, a round that goes
            // red, and the run that proves the restore is green again.
            vec![green(), green(), red(), green(), green()],
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"public class Kata { int add(String i) { return 0; } }\n"}]"#,
            )),
            DeliverOptions {
                attempts: 1,
                ..DeliverOptions::default()
            },
        );
        assert!(matches!(outcome, Outcome::Implemented), "{outcome:?}");
        assert!(prompter.said("the code was restored"));
    }

    /// A bar the refactor left red is the one refactor outcome that does
    /// stop the delivery: the requirement cannot be marked on red.
    #[test]
    fn a_refactor_that_leaves_the_bar_red_stops_before_marking() {
        let dir = kata(one_criterion());
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        std::fs::write(
            dir.path().join("src/main/java/Kata.java"),
            "public class Kata { int add(String input) { return 0; } }\n",
        )
        .unwrap();
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            // The gate run, the baseline, the round that lands green,
            // then a red reading when the phase is restored.
            vec![green(), green(), green(), red()],
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"public class Kata {\n    int add(String input) {\n        return 0;\n    }\n}\n"}]"#,
            )),
            DeliverOptions {
                attempts: 1,
                ..DeliverOptions::default()
            },
        );
        assert!(
            stopped_because(&outcome).contains("The refactor left the bar RED"),
            "{outcome:?}"
        );
    }

    /// The runtime going away right after a refactor leaves the bar
    /// unreadable, so the requirement is not marked.
    #[test]
    fn a_runtime_that_disappears_after_the_refactor_stops_before_marking() {
        let dir = kata(one_criterion());
        std::fs::create_dir_all(dir.path().join("src/main/java")).unwrap();
        std::fs::write(
            dir.path().join("src/main/java/Kata.java"),
            "public class Kata { int add(String input) { return 0; } }\n",
        )
        .unwrap();
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![green(), green(), green(), runtime_gone()],
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"public class Kata {\n    int add(String input) {\n        return 0;\n    }\n}\n"}]"#,
            )),
            DeliverOptions {
                attempts: 1,
                ..DeliverOptions::default()
            },
        );
        assert!(
            stopped_because(&outcome).contains("runtime disappeared after the refactor"),
            "{outcome:?}"
        );
    }

    /// The refactor service refuses when there is no production code to
    /// work on. The bar has to be re-read afterwards because the phase
    /// gate already moved off GREEN - and a red reading then stops the
    /// delivery.
    #[test]
    fn a_red_bar_after_a_skipped_refactor_stops_before_marking() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![green(), red()],
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"class Kata {}"}]"#,
            )),
            DeliverOptions {
                attempts: 1,
                ..DeliverOptions::default()
            },
        );
        assert!(prompter.said("Skipping the refactor."));
        assert!(
            stopped_because(&outcome).contains("The bar is RED after the refactor attempt."),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_runtime_that_disappears_after_a_skipped_refactor_stops_before_marking() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![green(), runtime_gone()],
            model(Ok(
                r#"[{"path":"src/main/java/Kata.java","content":"class Kata {}"}]"#,
            )),
            DeliverOptions {
                attempts: 1,
                ..DeliverOptions::default()
            },
        );
        assert!(
            stopped_because(&outcome).contains("runtime disappeared after the refactor attempt"),
            "{outcome:?}"
        );
    }
}

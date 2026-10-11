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
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::fs_worktree::FsWorkTree;
use crate::adapters::runners::detect_runner;
use crate::application::DEFAULT_LLM_ATTEMPTS;
use crate::application::assets::{asset_survey, load_spec};
use crate::application::generation_service::{GenerationService, ResolvedLlm};
use crate::application::implement_service::{ImplementService, ImplementTarget};
use crate::application::refactor_service::RefactorService;
use crate::application::scenario_service::ScenarioService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::tdd_service::{TddError, TddService, TestReport};
use crate::bootstrap::{
    LOOP_CLOSED, attempt_work, ensure_project, ensure_spec, has_readable_spec, project_detected,
    refresh_project_memory, run_and_narrate,
};
use crate::domain::attribution::foreign_failures;
use crate::domain::decision::Judgment;
use crate::domain::generation::brief_failure;
use crate::domain::language::Language;
use crate::domain::layout::in_feature_root;
use crate::domain::memory::ProjectStructure;
use crate::domain::model::{Requirement, Spec};
use crate::domain::requirement_id::{DEFAULT_PREFIX, is_id_shape, split_id};
use crate::domain::scaffold::slug;
use crate::domain::steps::{criterion_to_steps, source_extension};
use crate::domain::tdd::ImplementAttempt;
use crate::ports::{
    FeatureCatalog as _, Prompter, SourceFile, SourceFiles as _, SpecRepository as _, TestRunner,
    WorkTree as _,
};
use crate::wiring::{DynLlm, ProjectFeatures, ProjectTree, RunnerFactory};
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

/// Read the command's argument against the ids the catalog actually
/// holds.
///
/// An argument naming a requirement is one, prose is a description, and
/// nothing at all is the backlog. A single word that was reaching for an
/// id and missed is refused rather than drafted: handing `R-003` to the
/// model as prose invents a requirement nobody asked for.
///
/// `known` is why this reads the catalog instead of matching a prefix:
/// ids are `REQ-003` in the kata and `HARNESS-014` in this crate's own
/// spec, and a command that only recognised one of those could not
/// deliver the other.
pub fn parse_target(raw: Option<&str>, known: &[String]) -> Result<Target, String> {
    let Some(text) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(Target::Backlog);
    };
    if let Some(id) = known.iter().find(|id| id.eq_ignore_ascii_case(text)) {
        return Ok(Target::Requirement(id.clone()));
    }
    // Shaped like an id but naming nothing: the plan reports it by name,
    // which is a better answer than a lecture on the shape.
    let upper = text.to_ascii_uppercase();
    if is_id_shape(&upper) {
        return Ok(Target::Requirement(upper));
    }
    if is_misspelled_id(text, known) {
        let example = known
            .iter()
            .find(|id| is_id_shape(id))
            .cloned()
            .unwrap_or_else(|| format!("{DEFAULT_PREFIX}-003"));
        return Err(format!(
            "{text} is not a requirement id, and it is one word rather than a requirement \
             to break down, so nothing here can be delivered. Ids look like {example} - run \
             spec list to see them. To describe new work instead, use plain words: spec \
             deliver \"a custom delimiter on the first line\"."
        ));
    }
    Ok(Target::Description(text.to_string()))
}

/// A single word that was aiming at an id and missed: it carries a
/// digit, or it opens with a prefix the catalog already uses. Prose is
/// several words, so `the REQ-003 one` still reads as a description and
/// a real description is never refused.
fn is_misspelled_id(text: &str, known: &[String]) -> bool {
    if text.contains(char::is_whitespace) {
        return false;
    }
    let opens_with_a_known_prefix = || {
        known
            .iter()
            .filter_map(|id| split_id(id).map(|(prefix, _)| prefix))
            .chain(std::iter::once(DEFAULT_PREFIX))
            .any(|prefix| {
                text.get(..prefix.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
            })
    };
    text.chars().any(|c| c.is_ascii_digit()) || opens_with_a_known_prefix()
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
#[derive(Debug, Serialize, PartialEq)]
pub struct DeliverReport {
    pub planned: Vec<String>,
    pub delivered: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<Outstanding>,
    pub completed: bool,
    #[serde(rename = "nextStep")]
    pub next_step: String,
    /// What the decision model said about each stage that was gated, in
    /// the order the stages ran. Empty when no decision model was named,
    /// which is the shipped default; present so a run nobody watched can
    /// still be read for what the plane made of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub judgments: Vec<StageJudgments>,
}

/// The judgments one stage of one requirement earned.
///
/// Kept per stage rather than flattened, because the same gate reads
/// differently at different points: a `REWORK` on attempt one that the
/// retry addressed is a different fact from a `REWORK` on the attempt
/// that was finally accepted.
#[derive(Debug, Serialize, PartialEq)]
pub struct StageJudgments {
    pub id: String,
    /// `author_steps`, `drive_to_green`, `refactor` - the stage names
    /// the manual uses.
    pub stage: &'static str,
    pub judgments: Vec<Judgment>,
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
    /// `--decision-model`, when the caller was given one. `None`
    /// resolves from config, the same as every other decision caller.
    decision_model: Option<String>,
    /// `--judge-draft`. Off by default, so an unflagged run sends the
    /// drafting model exactly the prompts it sent before this existed.
    judge_draft: bool,
    options: DeliverOptions,
    /// Every gated stage's judgments so far, drained into the report.
    judgments: std::cell::RefCell<Vec<StageJudgments>>,
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
            decision_model: None,
            judge_draft: false,
            options,
            judgments: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// How many times a model reply is tried when validation fails.
    pub fn with_llm_attempts(mut self, attempts: u32) -> Self {
        self.llm_attempts = attempts.max(1);
        self
    }

    /// The decision model the drafting step puts its criteria to, and
    /// whether it is asked at all.
    pub fn with_decision_model(mut self, model: Option<String>, judge_draft: bool) -> Self {
        self.decision_model = model;
        self.judge_draft = judge_draft;
        self
    }

    /// The ids the catalog holds, for [`parse_target`] to read the
    /// command's argument against. Empty when there is no readable spec
    /// yet — the description path that drafts the first requirement.
    pub fn known_ids(&self) -> Vec<String> {
        self.spec()
            .map(|spec| spec.requirements.into_iter().map(|r| r.id).collect())
            .unwrap_or_default()
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
                judgments: Vec::new(),
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
                    prompter.tell(&format!("{id} {LOOP_CLOSED}"));
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
            judgments: self.judgments.take(),
        }
    }

    /// Record what the decision model said about one stage, and say it
    /// while the run is still on the screen. Nothing is recorded for a
    /// stage that earned no judgment, so a run without a decision model
    /// reports exactly what it reported before there was one.
    fn reviewed(
        &self,
        prompter: &mut dyn Prompter,
        id: &str,
        stage: &'static str,
        judgments: Vec<Judgment>,
    ) {
        if judgments.is_empty() {
            return;
        }
        prompter.tell(&crate::domain::decision::second_review(&judgments));
        self.judgments.borrow_mut().push(StageJudgments {
            id: id.to_string(),
            stage,
            judgments,
        });
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

        let structure = project_layout(&self.root);
        if let Some(stopped) =
            self.author_scenarios(prompter, language, req_id, requirement, &structure)?
        {
            return Ok(stopped);
        }
        if let Some(stopped) = self.author_steps(prompter, language, req_id)? {
            return Ok(stopped);
        }
        let unit_test = match self.author_unit_test(prompter, language, req_id)? {
            AuthoringStep::Stopped(stopped) => return Ok(stopped),
            authored => authored,
        };

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
        let Some(mut report) = run_and_narrate(&tdd, runner.as_ref(), prompter)? else {
            return Ok(Outcome::Stopped {
                reason: "Authoring is complete but the language runtime is missing, so \
                         the tests never ran."
                    .into(),
                phase: None,
            });
        };
        // A build the model's unit test broke is the one break the run
        // can mend itself: the template it polished compiles.
        if report.build_broken()
            && let AuthoringStep::Polished { target, before } = &unit_test
        {
            self.fall_back_to_template_unit_test(prompter, language, req_id, target, before)?;
            report = match run_and_narrate(&tdd, runner.as_ref(), prompter)? {
                Some(report) => report,
                None => {
                    return Ok(Outcome::Stopped {
                        reason: "The language runtime disappeared mid-run, so the bar \
                                 cannot be read."
                            .into(),
                        phase: None,
                    });
                }
            };
        }

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
        structure: &ProjectStructure,
    ) -> Result<Option<Outcome>, String> {
        if !self.scenario_missing(language, req_id)? {
            prompter.tell(&format!("A scenario tagged @{req_id} is already in place."));
            return Ok(None);
        }
        let feature_path = requirement.feature_file.clone().unwrap_or_else(|| {
            in_feature_root(structure, &format!("{}.feature", slug(&requirement.title)))
        });
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
            return Ok(Some(Outcome::Stopped {
                reason: format!(
                    "No acceptance criterion of {req_id} is Given/When/Then shaped, so no \
                     scenario could be written. Reword it with spec reword {req_id}."
                ),
                phase: None,
            }));
        }
        if let Err(error) = self.mutation_service().set_feature(req_id, &feature_path) {
            prompter.warn(&format!(
                "{req_id} still does not record its feature file - {}",
                error.0
            ));
        }
        prompter.tell(&format!("Scenarios written to {feature_path}."));
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
            let report = match generated {
                Ok(report) => report,
                Err(error) => {
                    self.reviewed(
                        prompter,
                        req_id,
                        "author_steps",
                        generation.take_judgments(),
                    );
                    return Err(error.to_string());
                }
            };
            self.reviewed(prompter, req_id, "author_steps", report.judgments);
            prompter.tell(&format!("Wrote {} ({}).", report.target, report.source));
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
        // Taken before the model writes, so that if what it wrote does
        // not build the tree can go back to this and take the template.
        let before = self.source_snapshot(language)?;
        let generation = self.generation_service(language);
        let work = prompter.working(&format!("Generating the unit test for {req_id} - working"));
        let generated = generation.unittest_generate(prompter, req_id);
        drop(work);
        let report = generated.map_err(|e| e.to_string())?;
        prompter.tell(&format!("Wrote {} ({}).", report.target, report.source));
        if let Some(content) = self
            .work_tree()
            .read(&report.target)
            .map_err(|e| e.to_string())?
        {
            prompter.tell("Generated unit test (the assertions are yours to sharpen):");
            prompter.tell(&content);
        }
        if let Some(stopped) = verified(self.unit_test_missing(language, req_id)?, || {
            unverified_unit_test(req_id, &report.target)
        }) {
            return Ok(AuthoringStep::Stopped(stopped));
        }
        Ok(match report.source.as_str() {
            "llm" => AuthoringStep::Polished {
                target: report.target,
                before,
            },
            _ => AuthoringStep::Done,
        })
    }

    /// The template unit test in place of the one the model polished,
    /// when that one stopped the build.
    ///
    /// The tree goes back to what the unit-test step read, so a class
    /// the model's members were spliced into is the class it was, and
    /// the template is written by the same service with no model
    /// attached - the fallback the run promised on its second line.
    /// One more test run and no model call; a build still broken after
    /// it is somebody else's break, and the stop stands.
    fn fall_back_to_template_unit_test(
        &self,
        prompter: &mut dyn Prompter,
        language: Language,
        req_id: &str,
        target: &str,
        before: &[SourceFile],
    ) -> Result<(), String> {
        prompter.warn(&template_unit_test_fallback(req_id, target));
        self.restore_sources(language, before)?;
        let template = crate::wiring::generation_service(
            &self.root,
            language,
            None::<ResolvedLlm<DynLlm>>,
            None,
        );
        let report = template
            .unittest_generate(prompter, req_id)
            .map_err(|e| e.to_string())?;
        prompter.tell(&format!("Wrote {} ({}).", report.target, report.source));
        Ok(())
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
        // RED and "it did not compile" are both red bars to the state
        // machine, and only one of them is a test that can be made to
        // pass. Implementing against a build error spends the budget on
        // production code while the break is usually in the generated
        // unit test, which the loop is not aimed at.
        if report.build_broken() {
            return Ok(Bar::stopped(broken_build(req_id), Some(report.phase)));
        }
        // The other red bar no attempt can turn green: failures that are
        // not this requirement's. Implementing it leaves them failing,
        // so the budget would be spent reading the same bar three
        // times - minutes per read on a local model. Read fresh, so the
        // feature file the scenario step just recorded is a marker too.
        let foreign: Vec<String> = self
            .spec()?
            .requirements
            .iter()
            .find(|r| r.id == req_id)
            .map(|requirement| {
                foreign_failures(&report.failure_details, requirement)
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if !foreign.is_empty() {
            return Ok(Bar::stopped(
                foreign_red_bar(req_id, &foreign),
                Some(report.phase),
            ));
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
            let before = self.source_snapshot(language)?;
            if self.attempt_implementation(prompter, &implement, tdd, req_id)? {
                return Ok(Bar::stopped(
                    format!(
                        "The model never answered the implementation prompt, so {req_id} \
                         was not attempted. Implement by hand, then run spec deliver \
                         {req_id} again - or raise timeout_seconds under [llm] if the \
                         model was still generating when the wait ran out."
                    ),
                    Some(report.phase.clone()),
                ));
            }
            self.fill_steps(prompter, &implement, tdd, req_id);
            let judged = match run_and_narrate(tdd, runner, prompter)? {
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
            if judged.phase == "GREEN" {
                return Ok(Bar::Green);
            }
            // An attempt that stops the suite compiling is worse than the
            // one before it: the next attempt is briefed with a compiler
            // error instead of a failing test, and a run that spends its
            // budget leaves the break on disk. Put the code back and let
            // the remaining attempts read the bar this one was given.
            if judged.build_broken() {
                let restored = self.restore_sources(language, &before)?;
                if !restored.is_empty() {
                    prompter.warn(&reverted_attempt(attempt, &restored));
                    continue;
                }
            }
            report = judged;
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
                self.reviewed(prompter, req_id, "refactor", report.judgments);
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
                let runner_report = run_and_narrate(&self.tdd_service(), runner, prompter)?;
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
                self.reviewed(prompter, req_id, "refactor", service.take_judgments());
                prompter.warn(&format!("{} Skipping the refactor.", error.0));
                // The gate moved the phase to REFACTOR and the loop
                // never ran, so the green bar has to be re-established
                // before the requirement can be marked.
                match run_and_narrate(&self.tdd_service(), runner, prompter)? {
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

    /// Close the loop: flip the status, then read the spec back to be
    /// sure it landed and still validates.
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
        let validation = self.spec_service().validate_spec();
        if !validation.valid {
            return Ok(Outcome::Stopped {
                reason: invalid_mark(req_id, &validation.issues),
                phase: None,
            });
        }
        let implemented = self
            .spec()?
            .requirements
            .iter()
            .any(|r| r.id == req_id && r.status == "implemented");
        Ok(verified(!implemented, || unverified_mark(req_id)).unwrap_or(Outcome::Implemented))
    }

    /// The second half of an attempt: the step bodies this requirement's
    /// scenarios still run through as placeholders, written against the
    /// production code the first half just put down.
    ///
    /// Narrated, never fatal. A refused reply leaves the stubs as they
    /// were and the bar reads them as it did; the next attempt, or a
    /// human, can still fill them in. Three measured runs on this crate
    /// ended RED on exactly these bodies with the production code
    /// written and every attempt spent, which is what this pass is for.
    fn fill_steps(
        &self,
        prompter: &mut dyn Prompter,
        implement: &ImplementService<
            crate::wiring::ProjectFeatures,
            crate::wiring::ProjectTree,
            FsWorkTree,
            FsSpecRepository,
            DynLlm,
        >,
        tdd: &TddService<FsStateStore>,
        req_id: &str,
    ) {
        let failures = tdd
            .implementation_brief(req_id)
            .map(|brief| brief.failures)
            .unwrap_or_default();
        match implement.fill_pending_steps(prompter, req_id, &failures) {
            Ok(Some(report)) => prompter.tell(&format!(
                "Filled {} pending step bodies in {} ({}).",
                report.filled, report.target, report.source
            )),
            Ok(None) => {}
            Err(error) => prompter.warn(&error.0),
        }
    }

    /// Ask the model to make the failing tests pass and commit whatever
    /// it wrote. A refused reply is narrated, not fatal - the next
    /// round, or a human, can still implement. But when the model never
    /// answered at all the loop stops: nothing was staged, and the same
    /// prompt would only wait out another timeout.
    ///
    /// `Ok(true)` is that stop; `Ok(false)` ran the attempt to whatever
    /// verdict the reply earned.
    fn attempt_implementation(
        &self,
        prompter: &mut dyn Prompter,
        implement: &ImplementService<
            crate::wiring::ProjectFeatures,
            crate::wiring::ProjectTree,
            FsWorkTree,
            FsSpecRepository,
            DynLlm,
        >,
        tdd: &TddService<FsStateStore>,
        req_id: &str,
    ) -> Result<bool, String> {
        let brief = tdd.implementation_brief(req_id).map_err(tdd_message)?;
        // Nobody is at the keyboard to answer --into. Evidence still
        // decides wherever it can; only when it names nothing does an
        // unattended run fall back to convention, and say which file it
        // settled on so the choice is in the transcript.
        let into = match implement.target(req_id) {
            Ok(ImplementTarget::Resolved(_)) => None,
            Ok(ImplementTarget::Unresolved { conventional }) => {
                prompter.warn(&unattended_target(req_id, &conventional));
                Some(conventional)
            }
            Err(error) => {
                prompter.warn(&format!("{} Implement by hand instead.", error.0));
                return Ok(false);
            }
        };
        let destinations = implement.attempt_destinations(req_id).unwrap_or_default();
        let work = prompter.working(&attempt_work(&self.root, &destinations));
        let outcome = implement.generate(
            prompter,
            req_id,
            &brief.failures,
            &brief.history,
            &brief.states,
            into.as_deref(),
        );
        drop(work);
        match outcome {
            Ok(attempt) => {
                self.reviewed(prompter, req_id, "drive_to_green", attempt.judgments);
                for target in &attempt.targets {
                    let full = std::path::absolute(self.root.join(target))
                        .unwrap_or_else(|_| self.root.join(target));
                    prompter.tell(&format!("Updated {} (llm).", full.display()));
                }
                if let Some(warning) = &attempt.warning {
                    prompter.warn(warning);
                }
                // Remember where this attempt put the production code,
                // so the next one writes there instead of re-deriving a
                // target from whatever the scenarios reach today. The
                // same thing `scenario generate` does with featureFile.
                // Losing the record is not a reason to lose the attempt
                // that earned it, so a failure here is narrated.
                if !attempt.production.is_empty()
                    && let Err(error) = self
                        .mutation_service()
                        .record_production(req_id, &attempt.production)
                {
                    prompter.warn(&format!(
                        "{req_id} still does not record where its production code lives - {}",
                        error.0
                    ));
                }
                tdd.record_attempt(ImplementAttempt {
                    requirement: req_id.to_string(),
                    targets: attempt.targets.clone(),
                    failures: brief.failures,
                    ..Default::default()
                })
                .map_err(tdd_message)?;
            }
            Err(error) => {
                self.reviewed(
                    prompter,
                    req_id,
                    "drive_to_green",
                    implement.take_judgments(),
                );
                // The model never answered - a timeout on a prompt this
                // size, usually. The next round would ask the same prompt
                // and wait out another one, so the loop stops here with
                // the requirement still pending instead of spending the
                // remaining attempts proving that.
                if implement.take_unanswered().is_some() {
                    prompter.warn(&format!("{} Implement by hand instead.", error.0));
                    return Ok(true);
                }
                prompter.warn(&format!("{} Implement by hand instead.", error.0));
            }
        }
        Ok(false)
    }

    /// Every source file an implement attempt could write, read before
    /// the attempt so a build it breaks can be put back.
    fn source_snapshot(&self, language: Language) -> Result<Vec<SourceFile>, String> {
        let layout = project_layout(&self.root);
        crate::wiring::source_tree(&self.root, layout.module_root.as_deref())
            .sources(source_extension(language))
            .map_err(|e| e.0)
    }

    /// Put back every snapshotted file the attempt moved, and name them.
    ///
    /// A file the attempt *created* is not in the snapshot and stays:
    /// [`WorkTree`] writes and reads, and deliberately cannot delete.
    /// The build breaks this guards against came from rewrites of files
    /// that were already there.
    fn restore_sources(
        &self,
        language: Language,
        snapshot: &[SourceFile],
    ) -> Result<Vec<String>, String> {
        let now = self.source_snapshot(language)?;
        let tree = self.work_tree();
        let mut restored = Vec::new();
        for file in snapshot {
            let unchanged = now
                .iter()
                .any(|current| current.path == file.path && current.content == file.content);
            if unchanged {
                continue;
            }
            tree.write(&file.path, &file.content, RESTORE_SUMMARY)
                .map_err(|e| e.0)?;
            restored.push(file.path.clone());
        }
        Ok(restored)
    }

    /// Whether the asset survey still reports a gap whose finding
    /// contains `needle`. The survey is the same deterministic reading
    /// `spec status` reports, so a step is verified by what the next
    /// command would see rather than by its own return value.
    fn survey_gap(&self, language: Language, req_id: &str, needle: &str) -> Result<bool, String> {
        let layout = project_layout(&self.root);
        let spec = load_spec(&self.spec_repository()).map_err(|e| e.0)?;
        let Some(requirement) = spec.requirements.iter().find(|r| r.id == req_id) else {
            return Ok(false);
        };
        let (_, findings) = asset_survey(
            &self.feature_catalog(),
            &crate::wiring::source_tree(&self.root, layout.module_root.as_deref()),
            language,
            requirement,
            &spec.project,
            &layout,
            None,
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

    fn feature_catalog(&self) -> crate::wiring::ProjectFeatures {
        crate::wiring::feature_catalog(&self.root)
    }

    fn work_tree(&self) -> FsWorkTree {
        crate::wiring::work_tree(&self.root)
    }

    fn spec_service(
        &self,
    ) -> crate::application::spec_service::SpecService<
        FsSpecRepository,
        crate::adapters::fs_spec::FsFeatureFiles,
    > {
        crate::wiring::spec_service(&self.root, crate::workspace::workshop_layout())
    }

    /// The drafting service, with the decision model attached only when
    /// `--judge-draft` asked for it.
    ///
    /// This run has no human reading its drafts, which is the argument
    /// for asking here and not the argument for obeying the answer:
    /// the judgment arrives as a rejection reason the drafting model
    /// gets one round to address, never as a gate. See
    /// [`SpecMutationService::with_judge`].
    ///
    /// Opt-in rather than on, and the reason is the measured one. A
    /// drafting model writes `then the roll-up verdict is "covered"`,
    /// which is the shape the question reads worst: put one real draft
    /// of six criteria through it and four come back flagged. That
    /// earns a redraft the question was usually wrong to ask for, and
    /// a redraft changes the criteria every later stage is prompted
    /// from - so an unattended run that was reproducible stops being
    /// reproducible. The flag is for someone who wants the second
    /// opinion and has the rounds to spend on it.
    fn mutation_service(
        &self,
    ) -> SpecMutationService<
        FsSpecRepository,
        crate::wiring::ProjectFeatures,
        FsWorkTree,
        FsStateStore,
    > {
        let service = crate::wiring::mutation_service(&self.root, self.llm_attempts);
        if !self.judge_draft {
            return service;
        }
        match crate::wiring::decision_service(&self.root, self.decision_model.as_deref())
            .and_then(|service| service.when_asking())
        {
            Some(judge) => service.with_judge(Box::new(judge)),
            None => service,
        }
    }

    fn scenario_service(&self) -> ScenarioService<FsWorkTree, crate::wiring::ProjectFeatures> {
        crate::wiring::scenario_service(&self.root)
    }

    fn generation_service(
        &self,
        language: Language,
    ) -> GenerationService<ProjectFeatures, ProjectTree, FsWorkTree, FsSpecRepository, DynLlm> {
        crate::wiring::generation_service(
            &self.root,
            language,
            self.resolved_llm(),
            self.decision_model.as_deref(),
        )
    }

    fn implement_service(
        &self,
        language: Language,
    ) -> ImplementService<ProjectFeatures, ProjectTree, FsWorkTree, FsSpecRepository, DynLlm> {
        crate::wiring::implement_service(
            &self.root,
            language,
            self.resolved_llm(),
            self.decision_model.as_deref(),
        )
    }

    /// The refactor loop's own rounds are bounded by `--attempts`, the
    /// same budget the RED-to-GREEN loop spends.
    fn refactor_service(
        &self,
        language: Language,
    ) -> RefactorService<ProjectTree, FsWorkTree, FsSpecRepository, DynLlm> {
        crate::wiring::refactor_service(
            &self.root,
            language,
            self.resolved_llm(),
            self.options.attempts.max(1),
            self.decision_model.as_deref(),
        )
    }

    /// The session LLM in the shape the generating services take it.
    fn resolved_llm(&self) -> Option<ResolvedLlm<DynLlm>> {
        crate::wiring::resolved_llm(&self.root, self.llm.as_ref(), self.llm_attempts)
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
    /// Nothing to write, or the template was written as it stands.
    Done,
    /// The model polished the template into `target`. `before` is the
    /// source tree as the step read it, so the template can replace the
    /// polish if the polish turns out not to build.
    Polished {
        target: String,
        before: Vec<SourceFile>,
    },
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
        "{req_id} was marked implemented, but the spec no longer validates ({}). Fix it, \
         or undo the edit with git.",
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

/// Said when no step definition points at a production file and the run
/// picks the conventional one anyway. A warning rather than a quiet
/// `tell`: the choice is convention, not evidence, and the next reader
/// of the transcript should know which it was.
/// The summary written against every file an abandoned attempt moved.
const RESTORE_SUMMARY: &str = "restore the code the attempt started from";

/// Said when the model's unit test stopped the build and the template
/// is written in its place. A warning, because the polish the run paid
/// a model call for is gone and the assertions it printed a moment ago
/// are not the ones on disk any more.
fn template_unit_test_fallback(req_id: &str, target: &str) -> String {
    format!(
        "The build failed with the unit test the model wrote for {req_id} in it, so {target} \
         is being rewritten from the template, which compiles. The tests run again before \
         anything is implemented."
    )
}

/// Why the loop will not spend attempts on a tree that does not build.
fn broken_build(req_id: &str) -> String {
    format!(
        "The build failed before any test ran, so there is no red bar to implement {req_id} \
         against - the generated unit test is the usual cause. Fix the build, then run \
         spec deliver {req_id} again."
    )
}

/// How many foreign failures the stop names in full before counting the
/// rest. Enough to recognize the suite; the whole list is in spec state.
const FOREIGN_FAILURES_NAMED: usize = 5;

/// Why the loop will not spend attempts on a bar that is red for some
/// other reason. Each failure is named by its first line, so the
/// developer can tell a drifted test from a broken fixture without
/// opening anything.
fn foreign_red_bar(req_id: &str, foreign: &[String]) -> String {
    let mut named: Vec<String> = foreign
        .iter()
        .take(FOREIGN_FAILURES_NAMED)
        .map(|failure| brief_failure(failure))
        .collect();
    if foreign.len() > FOREIGN_FAILURES_NAMED {
        named.push(format!(
            "and {} more",
            foreign.len() - FOREIGN_FAILURES_NAMED
        ));
    }
    format!(
        "The bar is RED with {} failure(s) that are not {req_id}'s: {}. Implementing \
         {req_id} cannot turn that bar green, so no attempt was made. Make the suite pass \
         on its own (spec test shows every failure), then run spec deliver {req_id} again.",
        foreign.len(),
        named.join("; ")
    )
}

/// What an attempt that broke the build cost, and what was kept instead.
fn reverted_attempt(attempt: u32, restored: &[String]) -> String {
    format!(
        "Attempt {attempt} left the build not compiling, so {} was restored to what the \
         attempt read. The bar is where it was before it ran.",
        restored.join(", ")
    )
}

fn unattended_target(req_id: &str, conventional: &str) -> String {
    format!(
        "No step definition {req_id}'s scenarios run through names a production file. \
         Implementing into {conventional} by convention - rerun with \
         spec implement {req_id} --into <path> to put it elsewhere."
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
    use crate::ports::TestFilter;
    use crate::workspace::SPEC_PATH;
    use std::path::Path;

    fn known(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    /// The transcript has to show both halves: which file was written,
    /// and that convention rather than evidence chose it.
    #[test]
    fn the_unattended_target_notice_names_the_file_and_why_it_was_picked() {
        let notice = unattended_target("REQ-001", "src/main/java/Kata.java");
        assert!(notice.contains("src/main/java/Kata.java"), "{notice}");
        assert!(notice.contains("by convention"), "{notice}");
        assert!(notice.contains("spec implement REQ-001 --into"), "{notice}");
    }

    /// The stop has to separate the two red bars, because the way out of
    /// a build error is not another implement attempt.
    #[test]
    fn the_broken_build_stop_says_the_suite_never_ran_and_names_the_way_out() {
        let stop = broken_build("HARNESS-019");
        assert!(stop.contains("before any test ran"), "{stop}");
        assert!(stop.contains("generated unit test"), "{stop}");
        assert!(stop.contains("spec deliver HARNESS-019 again"), "{stop}");
    }

    /// An attempt that is thrown away has to be in the transcript with
    /// the files it touched, or the next reader cannot tell a reverted
    /// run from one that never wrote anything.
    #[test]
    fn the_revert_notice_names_the_attempt_and_every_file_put_back() {
        let notice = reverted_attempt(2, &["src/lib.rs".into(), "tests/req_001_test.rs".into()]);
        assert!(notice.contains("Attempt 2"), "{notice}");
        assert!(notice.contains("src/lib.rs"), "{notice}");
        assert!(notice.contains("tests/req_001_test.rs"), "{notice}");
        assert!(notice.contains("not compiling"), "{notice}");
    }

    #[test]
    fn an_argument_shaped_like_an_id_is_one() {
        let catalog = known(&["REQ-003", "REQ-3", "REQ-0042"]);
        assert_eq!(
            parse_target(Some("REQ-003"), &catalog),
            Ok(Target::Requirement("REQ-003".into()))
        );
        assert_eq!(
            parse_target(Some("req-3"), &catalog),
            Ok(Target::Requirement("REQ-3".into()))
        );
        assert_eq!(
            parse_target(Some("  REQ-0042  "), &catalog),
            Ok(Target::Requirement("REQ-0042".into()))
        );
    }

    /// The prefix is the catalog's, not the tool's: this crate's own
    /// spec numbers `HARNESS-014`, and `spec deliver` has to reach it.
    #[test]
    fn an_id_carrying_the_catalogs_own_prefix_is_one() {
        let catalog = known(&["HARNESS-001", "HARNESS-014"]);
        assert_eq!(
            parse_target(Some("HARNESS-014"), &catalog),
            Ok(Target::Requirement("HARNESS-014".into()))
        );
        assert_eq!(
            parse_target(Some("harness-014"), &catalog),
            Ok(Target::Requirement("HARNESS-014".into()))
        );
    }

    /// Shaped like an id and naming nothing: the plan reports it by
    /// name, so it must survive parsing rather than be refused here.
    #[test]
    fn a_well_shaped_id_the_catalog_does_not_hold_is_still_a_requirement() {
        assert_eq!(
            parse_target(Some("REQ-404"), &known(&["REQ-001"])),
            Ok(Target::Requirement("REQ-404".into()))
        );
        assert_eq!(
            parse_target(Some("HARNESS-404"), &known(&["HARNESS-001"])),
            Ok(Target::Requirement("HARNESS-404".into()))
        );
    }

    #[test]
    fn prose_is_a_description() {
        let catalog = known(&["REQ-001"]);
        assert_eq!(
            parse_target(Some("a custom delimiter on the first line"), &catalog),
            Ok(Target::Description(
                "a custom delimiter on the first line".into()
            ))
        );
        // Several words are prose even when one of them is an id, so a
        // real description is never mistaken for a typo.
        for prose in ["the REQ-003 one", "sum 2 numbers", "REQ- and REQ-2"] {
            assert!(
                matches!(
                    parse_target(Some(prose), &catalog),
                    Ok(Target::Description(_))
                ),
                "{prose} should read as a description"
            );
        }
    }

    /// The failure this refuses: `req7` is one word reaching for an id,
    /// and drafting from it asks the model to invent a requirement out
    /// of the typo. Each is refused with a real id named as the shape.
    #[test]
    fn a_single_word_reaching_for_an_id_is_refused_not_drafted() {
        let catalog = known(&["HARNESS-001"]);
        for typo in [
            "REQ-1a", "REQ", "REQ-", "003", "req7", "HARNESS", "harness-",
        ] {
            let error = parse_target(Some(typo), &catalog)
                .expect_err(&format!("{typo} should be refused, not drafted"));
            assert!(error.contains(typo), "{error}");
            assert!(error.contains("HARNESS-001"), "{error}");
            assert!(error.contains("spec list"), "{error}");
        }
    }

    /// With nothing to read a prefix off, the refusal still has to name
    /// a shape rather than trail off.
    #[test]
    fn an_empty_catalog_still_names_a_shape_when_it_refuses() {
        let error = parse_target(Some("req7"), &[]).expect_err("req7 should be refused");
        assert!(error.contains("REQ-003"), "{error}");
    }

    #[test]
    fn nothing_named_is_the_backlog() {
        assert_eq!(parse_target(None, &[]), Ok(Target::Backlog));
        assert_eq!(parse_target(Some(""), &[]), Ok(Target::Backlog));
        assert_eq!(parse_target(Some("   "), &[]), Ok(Target::Backlog));
    }

    /// A multi-byte first character used to panic the `get(..4)` slice,
    /// and the one-word check slices too.
    #[test]
    fn a_non_ascii_argument_is_a_description_not_a_panic() {
        let catalog = known(&["REQ-001"]);
        assert!(matches!(
            parse_target(Some("données de test"), &catalog),
            Ok(Target::Description(_))
        ));
        assert!(matches!(
            parse_target(Some("é"), &catalog),
            Ok(Target::Description(_))
        ));
        assert!(matches!(
            parse_target(Some("aaé"), &catalog),
            Ok(Target::Description(_))
        ));
    }

    fn deliver(root: &Path) -> Deliver {
        Deliver::new(root.to_path_buf(), None, DeliverOptions::default())
    }

    /// An unflagged run drafts exactly as it did before the decision
    /// model reached this command.
    ///
    /// Worth a test of its own rather than a reading of the branch,
    /// because what it protects is not in this crate: a rehearsed
    /// demo whose every later stage is cached on prompts built from
    /// the criteria this stage writes. One judgment here earns a
    /// redraft, a redraft rewrites the criteria, and the whole cached
    /// chain behind it misses.
    #[test]
    fn deliver_asks_the_decision_model_nothing_unless_judge_draft_is_given() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!deliver(dir.path()).judge_draft);
        assert!(
            !deliver(dir.path())
                .with_decision_model(Some("nimble:latest".into()), false)
                .judge_draft,
            "naming a decision model is not asking for one during drafting"
        );
        assert!(
            deliver(dir.path())
                .with_decision_model(None, true)
                .judge_draft,
            "--judge-draft asks, and resolves the model from config"
        );
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
        assert!(reason.contains("undo the edit with git"), "{reason}");
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

    /// Marking is written and validated like every other edit, and the
    /// validation reads the whole spec. A requirement left implemented
    /// without a scenario by some earlier session makes every later mark
    /// invalid; the run stops and names the issue rather than carrying
    /// on over a spec that no longer validates.
    #[test]
    fn a_mark_that_leaves_the_spec_invalid_stops_with_its_issues() {
        let dir = kata(one_criterion());
        let mut spec = deliver(dir.path()).spec().unwrap();
        spec.requirements.push(Requirement {
            id: "REQ-002".into(),
            title: "Unproven".into(),
            status: "implemented".into(),
            story: "As a user, I want this so that history is honest.".into(),
            feature_file: Some("features/kata.feature".into()),
            acceptance_criteria: vec!["Given a, when b, then c".into()],
            ..Default::default()
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
        assert!(reason.contains("no longer validates"), "{reason}");
        assert!(reason.contains("REQ-002"), "{reason}");
        assert!(reason.contains("undo the edit with git"), "{reason}");
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

    /// The model never answered - a timeout on a prompt this size,
    /// usually. The failure is narrated and the loop stops with the
    /// requirement still pending: nothing was staged, and the same
    /// prompt would only wait out another timeout.
    #[test]
    fn a_model_that_never_answers_stops_the_loop_instead_of_spending_the_budget() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![red()],
            model(Err("the endpoint refused the connection")),
            DeliverOptions {
                attempts: 3,
                ..no_refactor()
            },
        );
        assert!(prompter.said("Implement by hand instead."));
        let reason = stopped_because(&outcome);
        assert!(reason.contains("never answered"), "{reason}");
        assert!(reason.contains("spec deliver REQ-001 again"), "{reason}");
    }

    /// A suite that is red for some other reason is not a bar this
    /// requirement can turn green. The loop stops before the first
    /// attempt - the model is never asked - and names what is failing.
    #[test]
    fn a_red_bar_with_failures_that_are_not_the_requirements_stops_before_any_attempt() {
        let dir = kata(one_criterion());
        let mut prompter = Script::default();
        let foreign_red = Ok(crate::domain::model::TestRunSummary {
            tests: 2,
            failures: 2,
            failure_details: vec![
                "Req001Test.emptyString: TODO: assert".into(),
                "every_documented_version_floor_is_the_version_this_crate_ships: FAILED\n\
                 claims floor 0.7.15, but this crate ships 0.7.17"
                    .into(),
            ],
            ..Default::default()
        });
        let outcome = deliver_one(
            dir.path(),
            &mut prompter,
            vec![foreign_red],
            model(Err("the model must not be asked about a bar it cannot fix")),
            DeliverOptions {
                attempts: 3,
                ..no_refactor()
            },
        );
        let reason = stopped_because(&outcome);
        assert!(
            reason.contains("1 failure(s) that are not REQ-001's"),
            "{reason}"
        );
        assert!(
            reason.contains("every_documented_version_floor_is_the_version_this_crate_ships"),
            "{reason}"
        );
        assert!(
            !reason.contains("claims floor"),
            "first line only: {reason}"
        );
        assert!(reason.contains("spec deliver REQ-001 again"), "{reason}");
        assert!(
            !prompter.said("Attempt 1 of 3."),
            "no attempt is made: {:?}",
            prompter.told
        );
    }

    /// The stop names a handful and counts the rest, so a suite with
    /// forty drifted tests reads as one line rather than a page.
    #[test]
    fn the_foreign_bar_stop_names_a_few_failures_and_counts_the_rest() {
        let foreign: Vec<String> = (1..=7).map(|n| format!("other_test_{n}: FAILED")).collect();
        let stop = foreign_red_bar("REQ-001", &foreign);
        assert!(stop.contains("7 failure(s)"), "{stop}");
        assert!(stop.contains("other_test_5: FAILED; and 2 more"), "{stop}");
        assert!(!stop.contains("other_test_6"), "{stop}");
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

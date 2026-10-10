//! Implementation attempts: the readiness preflight, the model-driven
//! attempt that stages file updates, and the advice call when the
//! preflight found problems. There is no template fallback: without a
//! model, implementing stays in the developer's hands.

use serde::Serialize;

use crate::application::LlmReplyError;
use crate::application::assets::{
    asset_survey, find_requirement, load_spec, production_path, scenario_evidence,
    unattended_production_file, unit_test_path,
};
use crate::application::decision_service::{Brief, TaskJudge, gated};
use crate::application::generation_service::ResolvedLlm;
use crate::application::incremental::Exam;
use crate::application::incremental::placeholder_exam;
use crate::application::spec_service::ServiceError;
use crate::domain::coverage::covers_all;
use crate::domain::decision::{
    IMPLEMENTATION_COMPLETE, Judgment, Policies, UNIT_TEST_ASSERTS, changed_code,
    implementation_state, test_asserts_state,
};
use crate::domain::generation::{
    FileUpdate, ImplementAsset, advice_prompt, implementation_prompt, is_pending_step_body,
    parse_file_updates_checked, strip_code_fences, unasserted_criteria,
};
use crate::domain::language::Language;
use crate::domain::layout::allowed_targets;
use crate::domain::memory::ProjectStructure;
use crate::domain::model::Spec;
use crate::domain::reply_guard::{Damage, damage};
use crate::domain::step_fill;
use crate::domain::steps::{extract_patterns, source_extension};
use crate::domain::tdd::{ImplementAttempt, StateEntry};
use crate::ports::{
    FeatureCatalog, LlmConversation, Prompter, SourceFiles, SpecRepository, ToolBroker, WorkTree,
};

/// Reply of the step-filling pass: which step file had its pending
/// bodies written, and how many.
#[derive(Debug, Serialize, PartialEq)]
pub struct StepFillReport {
    pub target: String,
    pub filled: usize,
    pub source: String,
}

/// Reply of an implementation attempt: the files the model updated.
#[derive(Debug, Serialize, PartialEq)]
pub struct ImplementationReport {
    pub targets: Vec<String>,
    /// Which of `targets` were production files rather than tests or
    /// step definitions. A delivery records these on the requirement so
    /// the next attempt writes where this one did.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub production: Vec<String>,
    pub written: bool,
    pub source: String,
    /// Set when the reply left the production code untouched - the
    /// attempt is incomplete and the caller narrates it loudly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Every judgment this attempt earned, in the order they were made.
    /// Empty when no decision model was attached, which is the shipped
    /// state. The accepted reply is the one that satisfied them; the
    /// ones that did not are the complaints it was re-asked with.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub judgments: Vec<Judgment>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

/// What [`ImplementService::target`] found: the file the evidence
/// names, or the admission that nothing does, carrying the conventional
/// path so a caller that must still choose has something to choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImplementTarget {
    Resolved(String),
    Unresolved { conventional: String },
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
    C: WorkTree,
    R: SpecRepository,
    L: LlmConversation,
    B: ToolBroker,
{
    features: F,
    sources: S,
    store: C,
    spec: R,
    language: Language,
    layout: ProjectStructure,
    llm: Option<ResolvedLlm<L, B>>,
    /// Asked about an attempt before it is accepted, when the caller has
    /// a decision model. `None` means ask nothing - the composition root
    /// decides that, so no service ever consults a mode.
    judge: Option<Box<dyn TaskJudge>>,
    /// How strictly each gate's answers are read. Inert without a judge.
    policies: Policies,
    /// Judgments from attempts that were refused and never became a
    /// report, kept so a caller can still read what the plane said
    /// about them. A report carries its own.
    judgments: std::cell::RefCell<Vec<Judgment>>,
}

/// One definition from a fill reply, when it keeps `pattern` and the
/// placeholder is gone. A reply that also carries the neighbours is
/// fine: only the definition for this pattern is taken.
fn accept_filled_definition(
    language: Language,
    reply: &str,
    pattern: &str,
) -> Result<String, String> {
    let code = strip_code_fences(reply);
    let Some(found) = step_fill::definition_spans(language, &code)
        .into_iter()
        .find(|span| span.pattern == pattern)
    else {
        return Err(format!("the reply did not keep the expression {pattern:?}"));
    };
    let body = found.text(&code).to_string();
    match placeholder_exam(pattern, is_pending_step_body(language, &body)) {
        Exam::Accept => Ok(body),
        Exam::Retry(reason) => Err(reason),
        Exam::Ask(_) => Ok(body),
    }
}

impl<F, S, C, R, L, B> ImplementService<F, S, C, R, L, B>
where
    F: FeatureCatalog,
    S: SourceFiles,
    C: WorkTree,
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
        layout: ProjectStructure,
        llm: Option<ResolvedLlm<L, B>>,
    ) -> Self {
        Self {
            features,
            sources,
            store,
            spec,
            language,
            layout,
            llm,
            judge: None,
            policies: Policies::default(),
            judgments: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// The judgments of attempts that were refused outright, handed over
    /// and cleared. An accepted attempt reports its own judgments
    /// instead, so the two never double up.
    pub fn take_judgments(&self) -> Vec<Judgment> {
        std::mem::take(&mut *self.judgments.borrow_mut())
    }

    /// The decision model this service puts its gates' questions to, and
    /// the policies their answers are read against.
    ///
    /// Attached by the caller rather than read from config, because
    /// whether to ask at all is the policy's call and
    /// [`crate::application::decision_service::DecisionService::when_asking`]
    /// has already made it by the time there is anything to pass here.
    pub fn with_judge(mut self, judge: Box<dyn TaskJudge>, policies: Policies) -> Self {
        self.judge = Some(judge);
        self.policies = policies;
        self
    }

    /// Whether a decision model gates this service's attempts. The
    /// composition root decides that; this only reports it.
    pub fn judges(&self) -> bool {
        self.judge.is_some()
    }

    /// Whether a model is resolved; callers narrate model calls only
    /// when one will actually happen.
    pub fn has_model(&self) -> bool {
        self.llm.is_some()
    }

    /// Which production file an attempt would write to, asked before
    /// attempting.
    ///
    /// [`production_path`] answers "which file does the evidence point
    /// at?" and refuses to guess when nothing does. That refusal is
    /// right for `spec implement`, where a developer can answer
    /// `--into`. It leaves an unattended caller with a question and
    /// nobody to ask, so the refusal is reported alongside the
    /// conventional path - what to do about it is the caller's policy,
    /// not this service's.
    pub fn target(&self, req_id: &str) -> Result<ImplementTarget, ServiceError> {
        let spec = load_spec(&self.spec)?;
        let requirement = find_requirement(&spec, req_id)?;
        let declared = requirement.production_files.as_slice();
        let sources = self.sources.sources(source_extension(self.language))?;
        let evidence = scenario_evidence(
            &self.features,
            &sources,
            self.language,
            &format!("@{req_id}"),
        )?;
        let resolved = production_path(
            &sources,
            self.language,
            &spec.project,
            &self.layout,
            &evidence,
            None,
            declared,
        );
        Ok(match resolved {
            Some(path) => ImplementTarget::Resolved(path),
            None => ImplementTarget::Unresolved {
                conventional: unattended_production_file(
                    &self.layout,
                    self.language,
                    &spec.project,
                    &requirement.title,
                ),
            },
        })
    }

    /// The production files this attempt will write, as project-relative
    /// paths, named before the model is asked. The primary target, then
    /// any other file the requirement already declares. Empty is not an
    /// answer `target` gives: a conventional file stands in when nothing
    /// else does.
    pub fn attempt_destinations(&self, req_id: &str) -> Result<Vec<String>, ServiceError> {
        let spec = load_spec(&self.spec)?;
        let requirement = find_requirement(&spec, req_id)?;
        let primary = match self.target(req_id)? {
            ImplementTarget::Resolved(path) => path,
            ImplementTarget::Unresolved { conventional } => conventional,
        };
        Ok(allowed_targets(&primary, &requirement.production_files))
    }

    /// Write the bodies of the step definitions this requirement's
    /// scenarios run through that are still generated placeholders.
    ///
    /// The second half of making the bar green, after [`Self::generate`]
    /// has written the production code. It is a separate pass with a
    /// separate brief because the step-definition file is the one file
    /// the implementation prompt can never get back whole: it is shared
    /// by every requirement, it is the largest file in a project that
    /// has been at this for a while, and three measured runs on this
    /// crate spent every attempt rewriting the production file while
    /// the bar stayed RED on three `todo!()` bodies in it. Only the
    /// pending definitions go to the model, with the production code
    /// they should call and the head of their file; only they come
    /// back, and they are spliced in where they were.
    ///
    /// `Ok(None)` when nothing bound to the requirement is pending, or
    /// when every pending body stayed a placeholder. Each definition is
    /// asked on its own. A reply that keeps that one's placeholder is
    /// sent back for that definition only; when the attempt budget is
    /// spent the stub stays and the definitions already filled are kept.
    pub fn fill_pending_steps(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
        failures: &[String],
    ) -> Result<Option<StepFillReport>, ServiceError> {
        let Some(llm) = &self.llm else {
            return Ok(None);
        };
        let spec = load_spec(&self.spec)?;
        let requirement = find_requirement(&spec, req_id)?;
        let (scenarios, step_lines) = self.tagged_scenarios(&format!("@{req_id}"))?;
        if step_lines.is_empty() {
            return Ok(None);
        }
        let sources = self.sources.sources(source_extension(self.language))?;
        // The file with the most of this requirement's pending steps.
        // One file is what a project has; picking the fullest keeps a
        // stray stub elsewhere from splitting the pass in two.
        let Some((file, pending)) = sources
            .iter()
            .map(|file| {
                (
                    file,
                    step_fill::pending_step_definitions(self.language, &file.content, &step_lines),
                )
            })
            .filter(|(_, pending)| !pending.is_empty())
            .max_by_key(|(_, pending)| pending.len())
        else {
            return Ok(None);
        };
        // The production files the requirement records - a delivery
        // writes them after its first attempt - or, before anything is
        // recorded, the file the evidence resolves to today.
        let mut wanted = requirement.production_files.clone();
        if wanted.is_empty()
            && let Ok(
                ImplementTarget::Resolved(path)
                | ImplementTarget::Unresolved { conventional: path },
            ) = self.target(req_id)
        {
            wanted.push(path);
        }
        let production: Vec<(String, String)> = sources
            .iter()
            .filter(|source| wanted.contains(&source.path))
            .map(|source| (source.path.clone(), source.content.clone()))
            .collect();
        let preamble = step_fill::step_file_preamble(self.language, &file.content);
        let declared = extract_patterns(self.language, &file.content);
        let work = prompter.working(&format!(
            "Asking {} to write {} pending step bodies in {} - working",
            llm.model(),
            pending.len(),
            file.path
        ));
        // Last definition first, so a splice does not move the offsets
        // of the definitions still waiting.
        let mut ordered = pending.clone();
        ordered.sort_by_key(|span| std::cmp::Reverse(span.start));
        let mut content = file.content.clone();
        let mut filled = 0usize;
        for span in &ordered {
            let fragment = span.text(&content).trim_end().to_string();
            let pattern = span.pattern.clone();
            let prompt = step_fill::fill_steps_prompt(
                self.language,
                requirement,
                &step_fill::FillBrief {
                    scenarios: &scenarios,
                    fragment: &fragment,
                    count: 1,
                    preamble,
                    production: &production,
                    failures,
                },
            );
            let language = self.language;
            let outcome = llm.ask(
                prompter,
                &prompt,
                |reply| accept_filled_definition(language, reply, &pattern),
                |_, _, _| {},
            );
            match outcome {
                Ok(body) => {
                    content.replace_range(span.start..span.end, &body);
                    filled += 1;
                }
                Err(LlmReplyError::Call(error)) => {
                    drop(work);
                    return Err(ServiceError(LlmReplyError::call_failed(&error)));
                }
                Err(LlmReplyError::Invalid { .. }) => {}
            }
        }
        drop(work);
        if filled == 0 {
            return Ok(None);
        }
        let kept = extract_patterns(self.language, &content);
        if kept != declared {
            return Err(ServiceError(
                "splicing the filled steps in would change which steps the file declares".into(),
            ));
        }
        let summary = format!("fill {filled} pending step definition(s) for {req_id}");
        self.store.write(&file.path, &content, &summary)?;
        tracing::info!(
            requirement = %req_id,
            target = %file.path,
            filled,
            "pending step bodies written"
        );
        Ok(Some(StepFillReport {
            target: file.path.clone(),
            filled,
            source: "llm".into(),
        }))
    }

    /// The requirement's scenarios as Gherkin text, with their step
    /// texts (keywords dropped) for matching against definitions.
    fn tagged_scenarios(&self, tag: &str) -> Result<(String, Vec<String>), ServiceError> {
        let mut text = String::new();
        let mut steps = Vec::new();
        for summary in self.features.list()? {
            let doc = self.features.read(&summary.path)?;
            for scenario in &doc.scenarios {
                if !scenario.tags.iter().chain(&doc.tags).any(|t| t == tag) {
                    continue;
                }
                text.push_str(&format!("  {tag}\n  Scenario: {}\n", scenario.name));
                for step in &scenario.steps {
                    text.push_str(&format!("    {step}\n"));
                }
                text.push('\n');
                steps.extend(scenario.steps.iter().cloned());
            }
        }
        Ok((text, step_fill::step_texts(&steps)))
    }

    /// Ask the model to make the failing tests pass: production code plus
    /// real bodies for the TODO placeholders in the test scaffolding.
    /// Every update is written; the caller reruns the tests -
    /// the test run is the real validator.
    pub fn generate(
        &self,
        prompter: &mut dyn Prompter,
        req_id: &str,
        failures: &[String],
        history: &[ImplementAttempt],
        states: &[StateEntry],
        into: Option<&str>,
    ) -> Result<ImplementationReport, ServiceError> {
        let Some(llm) = &self.llm else {
            return Err(ServiceError(
                "No model resolved - implement by hand and rerun spec test.".into(),
            ));
        };
        let spec = load_spec(&self.spec)?;
        let requirement = find_requirement(&spec, req_id)?;
        let sources = self.sources.sources(source_extension(self.language))?;
        let files: Vec<(String, String)> = sources
            .iter()
            .cloned()
            .map(|file| (file.path, file.content))
            .collect();
        // The same evidence the preflight used, so the file it told the
        // developer about is the file the attempt actually writes.
        let evidence = scenario_evidence(
            &self.features,
            &sources,
            self.language,
            &format!("@{req_id}"),
        )?;
        let Some(production) = production_path(
            &sources,
            self.language,
            &spec.project,
            &self.layout,
            &evidence,
            into,
            &requirement.production_files,
        ) else {
            return Err(ServiceError(format!(
                "Cannot tell which production file {req_id} belongs in - no step \
                 definition its scenarios run through names any of them. Name it \
                 with spec implement {req_id} --into <path>, or write the steps \
                 first so they point at the code."
            )));
        };
        // The primary plus whatever else the requirement declares. A
        // declared file that does not exist yet is the one an attempt is
        // there to create, so it has to survive the reply filter below.
        let allowed = allowed_targets(&production, &requirement.production_files);
        let prompt = implementation_prompt(
            self.language,
            requirement,
            failures,
            history,
            states,
            &files,
            &allowed,
        );
        // The assertions this attempt is expected to write. The prompt
        // has always asked for them; nothing checked, and six measured
        // attempts left every placeholder standing while the failure
        // count climbed.
        //
        // Scoped to this requirement's own criteria, not to the file:
        // the kata's test class is shared, and refusing over the next
        // requirement's stub would be a complaint no reply to this
        // prompt could answer.
        let unit_test = unit_test_path(&sources, self.language, req_id, &self.layout);
        let criteria = requirement.acceptance_criteria.as_slice();
        tracing::debug!(requirement = %req_id, "calling LLM for an implementation attempt");
        // Two gates on one reply, nested so each keeps its own narrow
        // question: did the test get a real assertion, and is the
        // production code more than a stub? The deterministic check
        // below catches a literal placeholder left standing; neither
        // gate can be reached until it passes, and neither rule can see
        // what the gates are for - `assertTrue(true)` and a method that
        // returns a constant both read as perfectly good code.
        let judge = self.judge.as_deref();
        let mut judgments = Vec::new();
        let mut asserts = Vec::new();
        let asserts_gate = gated(
            judge,
            self.policies.policy_for(&UNIT_TEST_ASSERTS),
            &UNIT_TEST_ASSERTS,
            &mut asserts,
            |updates: &Vec<FileUpdate>| test_briefs(req_id, criteria, updates, &unit_test),
            |reply| {
                let updates: Vec<FileUpdate> = parse_file_updates_checked(reply)?
                    .into_iter()
                    .filter(|update| {
                        allowed.contains(&update.path)
                            || files.iter().any(|(path, _)| *path == update.path)
                    })
                    .collect();
                if updates.is_empty() {
                    return Err("the reply held no usable file update for this project".into());
                }
                let unasserted = unasserted_criteria(&files, &updates, &unit_test, criteria);
                if !unasserted.is_empty() {
                    return Err(format!(
                        "{unit_test} still carries the generated placeholder for {} - replace \
                         each one with an assertion that calls the production code",
                        unasserted
                            .iter()
                            .map(|criterion| format!("{criterion:?}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                Ok(updates)
            },
        );
        let mut complete = Vec::new();
        let attempt = llm.ask(
            prompter,
            &prompt,
            gated(
                judge,
                self.policies.policy_for(&IMPLEMENTATION_COMPLETE),
                &IMPLEMENTATION_COMPLETE,
                &mut complete,
                |updates: &Vec<FileUpdate>| {
                    production_briefs(req_id, criteria, updates, &allowed, &files)
                },
                asserts_gate,
            ),
            |_, _, _| {},
        );
        judgments.append(&mut asserts);
        judgments.append(&mut complete);
        let updates = match attempt {
            Ok(updates) => updates,
            Err(error) => {
                // A refused attempt has no report to carry its
                // judgments, so they are kept for the caller to read.
                self.judgments.borrow_mut().append(&mut judgments);
                return Err(ServiceError(match error {
                    LlmReplyError::Call(e) => LlmReplyError::call_failed(&e),
                    // The reason names the file still holding a stub,
                    // which is the one thing a developer rerunning this
                    // by hand needs.
                    LlmReplyError::Invalid { reason } => {
                        format!("The model's reply was refused: {reason}")
                    }
                }));
            }
        };
        let summary = format!("implementation attempt for {req_id} (llm)");
        let mut targets = Vec::new();
        let mut refused = Vec::new();
        for update in &updates {
            // The model is shown every source file and often hands one
            // back word for word. Exclusive that puts a "modify" on the
            // review surface with nothing in it to review, and the
            // reviewer has to diff it to find out.
            if is_unchanged(&files, update) {
                tracing::debug!(path = %update.path, "implement: reply matches the file on disk");
                continue;
            }
            if let Some(damage) = replacing(&files, update, self.language) {
                tracing::warn!(path = %update.path, ?damage, "implement: destructive reply refused");
                refused.push((update.path.as_str(), damage));
                continue;
            }
            self.store.write(&update.path, &update.content, &summary)?;
            targets.push(update.path.clone());
        }
        // One of three declared files written is not an incomplete
        // attempt - the test run decides that. Only an attempt that
        // touched none of them has demonstrably left the bar where it
        // was.
        let written_production: Vec<String> = allowed
            .iter()
            .filter(|path| targets.contains(path))
            .cloned()
            .collect();
        let production_refused = refused
            .iter()
            .any(|(path, _)| allowed.contains(&(*path).to_string()));
        let mut warnings: Vec<String> = refused
            .iter()
            .map(|(path, damage)| damage.describe(path))
            .collect();
        // A reply without the production code is an incomplete attempt:
        // the tests will stay RED. Stage what arrived, but say so. A
        // refusal has already said it, and said more.
        if written_production.is_empty() && !production_refused {
            warnings.push(if targets.is_empty() {
                format!(
                    "The model left every file as it found it, including the \
                     production code ({}) - nothing was written.",
                    allowed.join(", ")
                )
            } else {
                format!(
                    "The model left the production code untouched ({}) - \
                     it only wrote: {}.",
                    allowed.join(", "),
                    targets.join(", ")
                )
            });
        }
        warnings.extend(ran_ahead_of_the_spec(&updates, &spec, req_id));
        let next_step = if !written_production.is_empty() {
            "Run spec test - the run decides.".to_string()
        } else if targets.is_empty() {
            format!(
                "There is nothing to apply. Run spec implement {req_id} again - \
                 or implement {production} by hand."
            )
        } else {
            format!(
                "The attempt is incomplete without {production}. Rerun spec test, \
                 then spec implement {req_id} again - or implement {production} by hand."
            )
        };
        Ok(ImplementationReport {
            written: !targets.is_empty(),
            targets,
            production: written_production,
            source: "llm".into(),
            warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
            judgments,
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
        into: Option<&str>,
    ) -> Result<ReadinessReport, ServiceError> {
        let spec = load_spec(&self.spec)?;
        let requirement = find_requirement(&spec, req_id)?;
        let mut findings = Vec::new();
        if requirement.status == "implemented" {
            findings.push(format!(
                "{req_id} is already implemented - pick the next pending requirement \
                 with spec list."
            ));
        }
        if phase != "RED" || failures.is_empty() {
            findings.push(match phase {
                "GREEN" => "The bar is GREEN - there is nothing to implement. Refactor \
                            with spec refactor or close the loop with spec mark-implemented."
                    .to_string(),
                _ => "No RED test run is recorded - run spec test first so its failures \
                      brief the model."
                    .to_string(),
            });
        }

        let (assets, asset_findings) = asset_survey(
            &self.features,
            &self.sources,
            self.language,
            requirement,
            &spec.project,
            &self.layout,
            into,
        )?;
        findings.extend(asset_findings);

        let ready = findings.is_empty();
        let next_step = findings.first().cloned().unwrap_or_else(|| {
            format!("Every prerequisite is in place - spec implement {req_id} can run.")
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
        let spec = load_spec(&self.spec)?;
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
                return Err(ServiceError(LlmReplyError::call_failed(&e)));
            }
            Err(LlmReplyError::Invalid { reason }) => {
                return Err(ServiceError(reason));
            }
        };
        Ok(Some(reply))
    }
}

/// Did the attempt reach past the requirement it was asked for?
///
/// Observed live: `spec implement REQ-006` implemented REQ-005 too, the
/// bar went green, and nothing said a word — the run had drifted ahead
/// of the spec it is supposed to be driven by. The written code is
/// matched against the other pending requirements with the literal
/// Whether the model handed back a file exactly as it was given it.
///
/// `files` is what the prompt showed the model, which is the working
/// tree as it stands - so a match means writing this update would
/// change nothing.
/// What [`UNIT_TEST_ASSERTS`] is asked about: one question per
/// criterion, against the test file this attempt wrote.
///
/// Nothing to ask when the attempt left the test file alone - the
/// question is about work this reply did, and a file it did not touch
/// is the previous attempt's business.
fn test_briefs(
    req_id: &str,
    criteria: &[String],
    updates: &[FileUpdate],
    unit_test: &str,
) -> Vec<Brief> {
    briefs_for(
        req_id,
        criteria,
        written(updates, unit_test),
        test_asserts_state,
    )
}

/// What [`IMPLEMENTATION_COMPLETE`] is graded on: one question per
/// criterion, against what this attempt changed in the production file.
///
/// Only the primary file, and only the lines the attempt changed in it,
/// with a little context. The brief is the criterion plus the one
/// change on purpose - `MAX_REQUEST_BYTES` is 64KB and the shipped
/// decision models load an 8,192-token context and refuse what
/// overflows it. A brief that grows to the whole module is a brief
/// that is never answered, and because this gate ships advisory the
/// refusal is a line in the log and a verdict nobody gave. Measured
/// against this crate's router, the whole file was 19,885 tokens.
///
/// A file handed back word for word changed nothing, and the gate is
/// asked nothing: the question is about work this reply did.
fn production_briefs(
    req_id: &str,
    criteria: &[String],
    updates: &[FileUpdate],
    allowed: &[String],
    files: &[(String, String)],
) -> Vec<Brief> {
    let Some(primary) = allowed.first() else {
        return Vec::new();
    };
    let prior = files
        .iter()
        .find(|(path, _)| path == primary)
        .map(|(_, content)| content.as_str());
    let change = written(updates, primary).map(|after| changed_code(prior, after));
    briefs_for(
        req_id,
        criteria,
        change.as_deref().filter(|change| !change.is_empty()),
        implementation_state,
    )
}

/// What this reply wrote to `path`, if it wrote it at all.
fn written<'a>(updates: &'a [FileUpdate], path: &str) -> Option<&'a str> {
    updates
        .iter()
        .find(|update| update.path == path)
        .map(|update| update.content.as_str())
}

/// One brief per criterion over the same body, numbered the way
/// `review_criteria` numbers them so provenance reads the same wherever
/// a judgment came from.
fn briefs_for(
    req_id: &str,
    criteria: &[String],
    body: Option<&str>,
    state: impl Fn(&str, &str) -> serde_json::Value,
) -> Vec<Brief> {
    let Some(body) = body else {
        return Vec::new();
    };
    criteria
        .iter()
        .enumerate()
        .map(|(index, criterion)| {
            let input = match criteria.len() {
                1 => req_id.to_string(),
                _ => format!("{req_id} acceptance criterion {}", index + 1),
            };
            Brief::new(input, state(criterion, body))
        })
        .collect()
}

fn is_unchanged(files: &[(String, String)], update: &FileUpdate) -> bool {
    files
        .iter()
        .any(|(path, content)| *path == update.path && *content == update.content)
}

/// What writing this update would destroy, if the file already exists.
///
/// A path the project has never seen is the attempt creating something,
/// and there is nothing there to lose.
fn replacing(
    files: &[(String, String)],
    update: &FileUpdate,
    language: Language,
) -> Option<Damage> {
    let (_, before) = files.iter().find(|(path, _)| *path == update.path)?;
    damage(language, before, &update.content)
}

/// [`covers_all`] heuristic, which is why this warns rather than
/// blocks: a shared input literal can make two requirements look alike,
/// and only the developer can say whether the extra code belongs.
fn ran_ahead_of_the_spec(updates: &[FileUpdate], spec: &Spec, req_id: &str) -> Option<String> {
    let written: String = updates
        .iter()
        .map(|update| update.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let reached: Vec<&str> = spec
        .requirements
        .iter()
        .filter(|requirement| requirement.id != req_id && requirement.status == "pending")
        .filter(|requirement| covers_all(&written, &requirement.acceptance_criteria))
        .map(|requirement| requirement.id.as_str())
        .collect();
    (!reached.is_empty()).then(|| {
        format!(
            "The code written also satisfies {}, still pending - {req_id} was the \
             requirement asked for. Read the diff with git diff and drop what \
             {req_id} does not need, so each requirement keeps its own RED bar.",
            reached.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_service::NullPrompter;
    use crate::ports::SourceFile;
    use crate::test_support::{
        FakeLlm, FakeSources, InMemoryFeatureCatalog, InMemorySpecRepository, InMemoryWorkTree,
        calculator_catalog, calculator_spec, covered_steps_source, flat_layout, unit_test_source,
    };

    fn service(
        sources: Vec<SourceFile>,
        llm: Option<ResolvedLlm<FakeLlm>>,
    ) -> ImplementService<
        InMemoryFeatureCatalog,
        FakeSources,
        InMemoryWorkTree,
        InMemorySpecRepository,
        FakeLlm,
    > {
        ImplementService::new(
            calculator_catalog(),
            FakeSources(sources),
            InMemoryWorkTree::default(),
            InMemorySpecRepository(Ok(calculator_spec())),
            Language::Java,
            flat_layout(Language::Java),
            llm,
        )
    }

    /// The same service with REQ-001 declaring where its production code
    /// lives - a port and the adapter beside it, which is the case one
    /// path cannot describe.
    fn service_declaring(
        declared: &[&str],
        sources: Vec<SourceFile>,
        llm: Option<ResolvedLlm<FakeLlm>>,
    ) -> ImplementService<
        InMemoryFeatureCatalog,
        FakeSources,
        InMemoryWorkTree,
        InMemorySpecRepository,
        FakeLlm,
    > {
        let mut spec = calculator_spec();
        spec.requirements[0].production_files =
            declared.iter().map(|path| (*path).to_string()).collect();
        ImplementService::new(
            calculator_catalog(),
            FakeSources(sources),
            InMemoryWorkTree::default(),
            InMemorySpecRepository(Ok(spec)),
            Language::Java,
            flat_layout(Language::Java),
            llm,
        )
    }

    /// Two production files and no step definition pointing at either.
    /// `spec implement` stops here and asks for `--into`; the service
    /// still has to hand back a usable conventional path so an
    /// unattended caller is not left with only the refusal.
    #[test]
    fn an_unresolvable_target_comes_back_with_the_conventional_path() {
        let sources = vec![
            SourceFile {
                path: "src/main/java/Alpha.java".into(),
                content: "public class Alpha {}".into(),
            },
            SourceFile {
                path: "src/main/java/Beta.java".into(),
                content: "public class Beta {}".into(),
            },
        ];
        assert_eq!(
            service(sources, None).target("REQ-001").unwrap(),
            ImplementTarget::Unresolved {
                conventional: "src/main/java/Kata.java".into()
            }
        );
    }

    /// Rust's conventional file is `lib.rs` for every crate, and this
    /// project already has modules. A new requirement with nothing
    /// pointing at one of them gets a file of its own.
    #[test]
    fn a_rust_project_with_nothing_to_point_at_gets_a_file_named_for_the_requirement() {
        let sources = vec![
            SourceFile {
                path: "src/lib.rs".into(),
                content: "pub mod a;".into(),
            },
            SourceFile {
                path: "src/a.rs".into(),
                content: "pub fn a() {}".into(),
            },
        ];
        let service = ImplementService::new(
            calculator_catalog(),
            FakeSources(sources),
            InMemoryWorkTree::default(),
            InMemorySpecRepository(Ok(calculator_spec())),
            Language::Rust,
            flat_layout(Language::Rust),
            None::<ResolvedLlm<FakeLlm>>,
        );
        assert_eq!(
            service.target("REQ-001").unwrap(),
            ImplementTarget::Unresolved {
                conventional: "src/adds_two_numbers.rs".into(),
            }
        );
    }

    /// Evidence still decides. The fallback is only reached when
    /// nothing points anywhere, so a project with a single production
    /// file resolves without it.
    #[test]
    fn a_single_production_file_resolves_without_the_fallback() {
        let sources = vec![SourceFile {
            path: "src/main/java/Only.java".into(),
            content: "public class Only {}".into(),
        }];
        assert_eq!(
            service(sources, None).target("REQ-001").unwrap(),
            ImplementTarget::Resolved("src/main/java/Only.java".into())
        );
    }

    #[test]
    fn implement_without_a_model_is_refused() {
        let error = service(vec![], None)
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap_err();
        assert_eq!(
            error.0,
            "No model resolved - implement by hand and rerun spec test."
        );
    }

    /// Observed live on REQ-003: the model was shown the step
    /// definitions, handed them straight back, and spec wrote them as
    /// a "modify" that was byte-identical to the working tree. The
    /// reviewer had to diff it to discover there was nothing in it.
    #[test]
    fn a_file_handed_back_word_for_word_is_not_written_as_a_change() {
        let steps = SourceFile {
            path: "src/test/java/KataSteps.java".into(),
            content: "public class KataSteps {}".into(),
        };
        let reply = format!(
            r#"[{{"path": "src/test/java/KataSteps.java", "content": {}}},
                {{"path": "src/main/java/Kata.java", "content": "public class Kata {{}}"}}]"#,
            serde_json::to_string(&steps.content).unwrap()
        );
        let report = service(vec![steps], Some(FakeLlm::replying(&reply)))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap();
        assert_eq!(report.targets, vec!["src/main/java/Kata.java".to_string()]);
        assert!(report.written);
    }

    /// A file changed only in whitespace is still a change - the model
    /// meant it, and the reviewer should see it.
    #[test]
    fn a_file_returned_with_any_difference_at_all_is_still_written() {
        let steps = SourceFile {
            path: "src/main/java/Kata.java".into(),
            content: "public class Kata {}".into(),
        };
        let reply = r#"[{"path": "src/main/java/Kata.java", "content": "public class Kata { }"}]"#;
        let report = service(vec![steps], Some(FakeLlm::replying(reply)))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap();
        assert_eq!(report.targets, vec!["src/main/java/Kata.java".to_string()]);
    }

    /// If every file came back unchanged there is nothing to review,
    /// and saying "written" would send the student to an empty diff.
    #[test]
    fn a_reply_that_changes_nothing_stages_nothing_and_says_so() {
        let production = SourceFile {
            path: "src/main/java/Kata.java".into(),
            content: "public class Kata {}".into(),
        };
        let reply = r#"[{"path": "src/main/java/Kata.java", "content": "public class Kata {}"}]"#;
        let report = service(vec![production], Some(FakeLlm::replying(reply)))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap();
        assert!(report.targets.is_empty());
        assert!(!report.written);
        let warning = report.warning.expect("a warning that nothing landed");
        assert!(
            warning.contains("left every file as it found it"),
            "{warning}"
        );
        assert!(
            report.next_step.contains("nothing to apply"),
            "{}",
            report.next_step
        );
    }

    /// Observed live: asked to add one tool to a 1124-line module, the
    /// model replied `placeholder`. It was written over the file, and the
    /// build stopped. The test run cannot be the validator for a reply
    /// that deletes the code the tests were going to run.
    #[test]
    fn a_reply_that_would_delete_the_production_file_is_not_written() {
        let production = SourceFile {
            path: "src/main/java/Kata.java".into(),
            content: "public class Kata {\n  int add(String in) { return 0; }\n}".into(),
        };
        let reply = r#"[{"path": "src/main/java/Kata.java", "content": "placeholder"}]"#;
        let service = service(vec![production], Some(FakeLlm::replying(reply)));
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap();
        assert!(report.targets.is_empty());
        assert!(!report.written);
        assert_eq!(
            service.store.read("src/main/java/Kata.java").unwrap(),
            None,
            "nothing reached the file"
        );
        let warning = report.warning.expect("the refusal is reported");
        assert!(warning.contains("Kata"), "{warning}");
        assert!(warning.contains("not written"), "{warning}");
        assert!(
            !warning.contains("left every file as it found it"),
            "a refusal is not the model leaving the file alone: {warning}"
        );
        assert!(
            report.next_step.contains("spec implement REQ-001"),
            "{}",
            report.next_step
        );
    }

    /// The guard is per file: one bad update does not discard the rest
    /// of an attempt that was otherwise fine.
    #[test]
    fn a_refusal_does_not_throw_away_the_rest_of_the_attempt() {
        let steps = SourceFile {
            path: "src/test/java/Steps.java".into(),
            content: "class Steps {}".into(),
        };
        let production = SourceFile {
            path: "src/main/java/Kata.java".into(),
            content: "public class Kata {}".into(),
        };
        let reply = r#"[
            {"path": "src/test/java/Steps.java", "content": "nothing left"},
            {"path": "src/main/java/Kata.java", "content": "public class Kata { int add() { return 1; } }"}
        ]"#;
        let service = service(vec![steps, production], Some(FakeLlm::replying(reply)));
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap();
        assert_eq!(report.targets, vec!["src/main/java/Kata.java"]);
        assert!(report.written);
        let warning = report.warning.expect("the refused file is reported");
        assert!(warning.contains("src/test/java/Steps.java"), "{warning}");
    }

    /// A path the project has never seen is the attempt creating a
    /// file, and nothing can be lost from a file that is not there.
    #[test]
    fn the_completeness_brief_is_the_change_not_the_file() {
        let prior: String = (0..600)
            .map(|n| format!("fn existing_{n}() -> u32 {{\n    {n}\n}}\n"))
            .collect();
        let added = "fn add(input: &str) -> u32 {\n    3\n}\n";
        let update = FileUpdate {
            path: "src/lib.rs".into(),
            content: format!("{prior}{added}"),
        };
        let criteria = vec!["Given \"1,2\", when add is called, then 3".to_string()];
        let briefs = production_briefs(
            "REQ-001",
            &criteria,
            std::slice::from_ref(&update),
            &["src/lib.rs".to_string()],
            &[("src/lib.rs".to_string(), prior.clone())],
        );
        assert_eq!(briefs.len(), 1);
        let code = briefs[0].state["production_code"].as_str().unwrap();
        assert!(code.contains(added), "{code}");
        assert!(
            code.len() < prior.len() / 10,
            "{} bytes of {}",
            code.len(),
            prior.len()
        );
    }

    #[test]
    fn a_file_handed_back_word_for_word_asks_the_completeness_gate_nothing() {
        let prior = "fn add() -> u32 { 3 }\n".to_string();
        let update = FileUpdate {
            path: "src/lib.rs".into(),
            content: prior.clone(),
        };
        let briefs = production_briefs(
            "REQ-001",
            &["a criterion".to_string()],
            std::slice::from_ref(&update),
            &["src/lib.rs".to_string()],
            &[("src/lib.rs".to_string(), prior)],
        );
        assert_eq!(briefs.len(), 0);
    }

    /// REQ-001's steps with the When and Then still pending, beside a
    /// hand-written Given and another requirement's stub.
    fn pending_steps() -> SourceFile {
        SourceFile {
            path: "src/test/java/steps/Steps.java".into(),
            content: "package steps;\n\nimport io.cucumber.java.PendingException;\nimport io.cucumber.java.en.*;\n\npublic class Steps {\n    private int result;\n\n    @Given(\"a calculator\")\n    public void a() { result = -1; }\n\n    @When(\"add is called with {string}\")\n    public void b(String s) {\n        throw new PendingException();\n    }\n\n    @Then(\"the result is {int}\")\n    public void c(int n) {\n        throw new PendingException();\n    }\n\n    @Then(\"something REQ-002 asks\")\n    public void d() {\n        throw new PendingException();\n    }\n}\n".into(),
        }
    }

    fn production() -> SourceFile {
        SourceFile {
            path: "src/main/java/Kata.java".into(),
            content: "public class Kata { int add(String in) { return 3; } }".into(),
        }
    }

    const FILLED_STEPS: &str = "    @When(\"add is called with {string}\")\n    public void b(String s) {\n        result = new Kata().add(s);\n    }\n\n    @Then(\"the result is {int}\")\n    public void c(int n) {\n        org.junit.jupiter.api.Assertions.assertEquals(n, result);\n    }\n";

    #[test]
    fn the_pending_step_bodies_bound_to_the_requirement_are_filled_in_place() {
        let service = service(
            vec![pending_steps(), production()],
            Some(FakeLlm::replying(FILLED_STEPS)),
        );
        let report = service
            .fill_pending_steps(
                &mut NullPrompter,
                "REQ-001",
                &["the result is 3: FAILED".into()],
            )
            .unwrap()
            .expect("two pending steps to fill");
        assert_eq!(report.target, "src/test/java/steps/Steps.java");
        assert_eq!(report.filled, 2);
        let written = service
            .store
            .read("src/test/java/steps/Steps.java")
            .unwrap()
            .expect("the step file was written");
        assert!(written.contains("result = new Kata().add(s);"), "{written}");
        assert!(written.contains("assertEquals(n, result);"), "{written}");
        assert!(
            written.contains("public void a() { result = -1; }"),
            "{written}"
        );
        assert!(written.contains("something REQ-002 asks"), "{written}");
        assert_eq!(
            written.matches("PendingException();").count(),
            1,
            "{written}"
        );
        let prompt = service.llm.as_ref().unwrap().chat().prompts.borrow()[0].clone();
        assert!(
            prompt.contains("int add(String in)"),
            "the production code is in the brief"
        );
        assert!(
            prompt.contains("private int result;"),
            "the file head is in the brief"
        );
        assert!(
            !prompt.contains("something REQ-002 asks"),
            "another requirement's stub is not"
        );
    }

    #[test]
    fn a_reply_that_leaves_a_step_pending_keeps_that_step_and_fills_the_rest() {
        let still_pending = "    @When(\"add is called with {string}\")\n    public void b(String s) {\n        result = new Kata().add(s);\n    }\n\n    @Then(\"the result is {int}\")\n    public void c(int n) {\n        throw new PendingException();\n    }\n";
        let service = service(
            vec![pending_steps(), production()],
            Some(FakeLlm::replying(still_pending)),
        );
        let report = service
            .fill_pending_steps(&mut NullPrompter, "REQ-001", &[])
            .unwrap()
            .expect("the When is filled even though the Then stays pending");
        assert_eq!(report.filled, 1);
        let written = service
            .store
            .read("src/test/java/steps/Steps.java")
            .unwrap()
            .expect("the step that bound is written");
        assert!(written.contains("result = new Kata().add(s);"), "{written}");
        assert!(
            written.contains("throw new PendingException();"),
            "{written}"
        );
    }

    #[test]
    fn nothing_pending_means_nothing_to_fill() {
        let service = service(
            vec![covered_steps_source(), production()],
            Some(FakeLlm::replying(FILLED_STEPS)),
        );
        let outcome = service
            .fill_pending_steps(&mut NullPrompter, "REQ-001", &[])
            .unwrap();
        assert!(outcome.is_none());
        assert!(
            service
                .llm
                .as_ref()
                .unwrap()
                .chat()
                .prompts
                .borrow()
                .is_empty()
        );
    }

    #[test]
    fn a_brand_new_file_is_not_measured_against_anything() {
        let reply = r#"[{"path": "src/main/java/Kata.java", "content": "placeholder"}]"#;
        let report = service(vec![], Some(FakeLlm::replying(reply)))
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap();
        assert_eq!(report.targets, vec!["src/main/java/Kata.java"]);
    }

    #[test]
    fn an_attempt_that_also_satisfies_another_pending_requirement_is_flagged() {
        // Observed live: spec implement REQ-006 implemented REQ-005 as
        // well, the bar went green, and nothing said a word.
        let reply = r#"[{"path": "src/main/java/Kata.java",
            "content": "public class Kata {\n  int add(String in) { return in.equals(\"1,2\") ? 3 : 0; }\n  int subtract(String in) { return in.equals(\"3,1\") ? 2 : 0; }\n}"}]"#;
        let report = service(vec![], Some(FakeLlm::replying(reply)))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap();
        let warning = report.warning.expect("the scope warning");
        assert!(warning.contains("also satisfies REQ-002"), "{warning}");
        assert!(warning.contains("git diff"), "{warning}");
        // A warning, not a gate: the work is on disk.
        assert!(report.written);
    }

    #[test]
    fn an_attempt_confined_to_its_own_requirement_is_not_flagged() {
        let reply = r#"[{"path": "src/main/java/Kata.java",
            "content": "public class Kata {\n  int add(String in) { return in.equals(\"1,2\") ? 3 : 0; }\n}"}]"#;
        let report = service(vec![], Some(FakeLlm::replying(reply)))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap();
        assert_eq!(report.warning, None);
    }

    #[test]
    fn the_scope_warning_is_added_to_the_untouched_production_warning() {
        // Both are true of the same attempt, and the report carries one
        // warning field - neither may swallow the other.
        let reply = r#"[{"path": "src/test/java/Steps.java",
            "content": "assertEquals(3, calc.add(\"1,2\"));\nassertEquals(2, calc.subtract(\"3,1\"));"}]"#;
        let sources = vec![SourceFile {
            path: "src/test/java/Steps.java".into(),
            content: "old steps".into(),
        }];
        let report = service(sources, Some(FakeLlm::replying(reply)))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap();
        let warning = report.warning.expect("both warnings");
        assert!(
            warning.contains("left the production code untouched"),
            "{warning}"
        );
        assert!(warning.contains("also satisfies REQ-002"), "{warning}");
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
                None,
            )
            .unwrap();
        assert_eq!(
            report.targets,
            vec!["src/main/java/Kata.java", "src/test/java/Steps.java"]
        );
        assert!(report.written);
        assert_eq!(report.source, "llm");
        let production = service
            .store
            .read("src/main/java/Kata.java")
            .unwrap()
            .unwrap();
        assert_eq!(production, "public class Kata {}");
        assert!(service.store.read("/etc/passwd").unwrap().is_none());
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
                None,
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
        assert!(report.next_step.contains("spec implement REQ-001"));
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
                None,
            )
            .unwrap();
        assert_eq!(report.warning, None);
        assert_eq!(report.next_step, "Run spec test - the run decides.");
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
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["todo".into()],
                &[],
                &[],
                None,
            )
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
                None,
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

    /// The refusal names what was wrong with the reply. Flattening every
    /// rejection to one sentence left a developer rerunning this by hand
    /// nothing to act on.
    #[test]
    fn an_unusable_implementation_reply_is_refused_with_the_reason() {
        // Two replies, two different faults. Each one's own reason has
        // to survive the trip out, which is the whole point.
        for (reply, expected) in [
            ("Sure, here you go!", "not a JSON array"),
            (
                r#"[{"path": "not/a/project/file.java", "content": "x"}]"#,
                "no usable file update for this project",
            ),
        ] {
            let error = service(vec![], Some(FakeLlm::replying(reply)))
                .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
                .unwrap_err();
            assert!(
                error.0.starts_with("The model's reply was refused:"),
                "{}",
                error.0
            );
            assert!(error.0.contains(expected), "{}", error.0);
        }
    }

    /// A generated unit test as `spec unittest generate` leaves it: a
    /// failing placeholder carrying REQ-001's criterion where the
    /// assertion belongs. It names REQ-001, so `unit_test_path` resolves
    /// to it. The criterion quotes `"1,2"`, so the file holds it escaped
    /// the way Java quotes it - which is the case worth fixturing.
    fn generated_test() -> SourceFile {
        let placeholder =
            r#"fail("TODO: assert - Given \"1,2\", when add is called, then the result is 3");"#;
        SourceFile {
            path: "src/test/java/Req001Test.java".into(),
            content: format!(
                "/** Generated from REQ-001: Adds two numbers */\n\
                 class Req001Test {{\n\
                 \x20   @Test\n\
                 \x20   void addsNumbers() {{\n\
                 \x20       {placeholder}\n\
                 \x20   }}\n\
                 }}\n"
            ),
        }
    }

    /// Observed across six measured attempts: the model writes the
    /// production code and hands the generated `fail("TODO: assert")`
    /// back untouched, so the bar stays red for a reason the attempt
    /// never addressed. The prompt has always asked for the assertion -
    /// this is the first thing that checks it arrived.
    #[test]
    fn a_reply_that_leaves_the_placeholders_standing_is_refused_and_asked_again() {
        let reply = r#"[{"path": "src/main/java/Kata.java",
            "content": "public class Kata { int add(String in) { return 0; } }"}]"#;
        let service = service(vec![generated_test()], Some(FakeLlm::replying(reply)));
        let error = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap_err();
        assert!(
            error.0.contains("src/test/java/Req001Test.java"),
            "the refusal names the file still holding the stub: {}",
            error.0
        );
        assert!(
            error.0.contains("still carries the generated placeholder")
                && error.0.contains("when add is called"),
            "the refusal names the criterion left unasserted: {}",
            error.0
        );
        assert!(
            service.llm.as_ref().unwrap().chat().prompts.borrow().len() > 1,
            "a refused reply is asked again rather than handed back"
        );
    }

    /// A declared file that does not exist yet is exactly the file an
    /// attempt is there to create. The reply filter used to admit one
    /// new path - the primary - so the second one was dropped without a
    /// word, and the requirement could never gain its adapter.
    #[test]
    fn a_declared_file_that_does_not_exist_yet_is_still_written() {
        let filled = "class Req001Test { @Test void t() { assertEquals(3, new Port().add()); } }";
        let reply = format!(
            r#"[{{"path": "src/main/java/Port.java", "content": "public class Port {{}}"}},
                {{"path": "src/main/java/Adapter.java", "content": "public class Adapter {{}}"}},
                {{"path": "src/test/java/Req001Test.java", "content": {}}}]"#,
            serde_json::to_string(filled).unwrap()
        );
        let service = service_declaring(
            &["src/main/java/Port.java", "src/main/java/Adapter.java"],
            vec![generated_test()],
            Some(FakeLlm::replying(&reply)),
        );
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap();
        assert!(
            report
                .targets
                .contains(&"src/main/java/Adapter.java".to_string()),
            "{:?}",
            report.targets
        );
        assert_eq!(
            report.production,
            ["src/main/java/Port.java", "src/main/java/Adapter.java"],
            "the report names which of the targets were production code"
        );
        assert_eq!(report.warning, None, "both declared files were written");
    }

    /// One of two declared files is not an incomplete attempt - the test
    /// run decides that. Touching none of them is, and the warning has
    /// to name the whole set so the developer knows what was expected.
    #[test]
    fn the_untouched_warning_fires_only_when_no_declared_file_was_written() {
        let one = r#"[{"path": "src/main/java/Port.java", "content": "public class Port {}"}]"#;
        let partial = service_declaring(
            &["src/main/java/Port.java", "src/main/java/Adapter.java"],
            vec![],
            Some(FakeLlm::replying(one)),
        );
        let report = partial
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap();
        assert_eq!(report.warning, None, "one of two written is not a finding");
        assert_eq!(report.next_step, "Run spec test - the run decides.");

        let steps = r#"[{"path": "src/test/java/Steps.java", "content": "class Steps { real }"}]"#;
        let none = service_declaring(
            &["src/main/java/Port.java", "src/main/java/Adapter.java"],
            vec![SourceFile {
                path: "src/test/java/Steps.java".into(),
                content: "old steps".into(),
            }],
            Some(FakeLlm::replying(steps)),
        );
        let warning = none
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap()
            .warning
            .expect("an attempt that wrote no production code warns");
        assert!(
            warning.contains("left the production code untouched"),
            "{warning}"
        );
        assert!(
            warning.contains("src/main/java/Port.java")
                && warning.contains("src/main/java/Adapter.java"),
            "the warning names every file that was expected: {warning}"
        );
    }

    /// The gate is satisfied by writing the assertion, and a reply that
    /// does is written as normal and asked for only once.
    #[test]
    fn a_reply_that_writes_the_assertion_is_accepted() {
        let filled = "/** Generated from REQ-001: Adds two numbers */\n\
                      class Req001Test {\n\
                      \x20   @Test\n\
                      \x20   void addsNumbers() {\n\
                      \x20       assertEquals(3, new Kata().add(\"1,2\"));\n\
                      \x20   }\n\
                      }\n";
        let reply = format!(
            r#"[{{"path": "src/main/java/Kata.java",
                  "content": "public class Kata {{ int add(String in) {{ return 0; }} }}"}},
                {{"path": "src/test/java/Req001Test.java", "content": {}}}]"#,
            serde_json::to_string(filled).unwrap()
        );
        let service = service(vec![generated_test()], Some(FakeLlm::replying(&reply)));
        let report = service
            .generate(
                &mut NullPrompter,
                "REQ-001",
                &["boom".into()],
                &[],
                &[],
                None,
            )
            .unwrap();
        assert!(
            report
                .targets
                .contains(&"src/test/java/Req001Test.java".to_string()),
            "{:?}",
            report.targets
        );
        assert_eq!(
            service.llm.as_ref().unwrap().chat().prompts.borrow().len(),
            1,
            "a reply that satisfied the gate is not asked again"
        );
    }

    #[test]
    fn a_model_failure_during_implementation_is_reported() {
        let error = service(vec![], Some(FakeLlm::failing()))
            .generate(&mut NullPrompter, "REQ-001", &[], &[], &[], None)
            .unwrap_err();
        assert_eq!(error.0, "the model call failed - model crashed");
    }

    #[test]
    fn an_implementation_for_an_unknown_requirement_is_refused() {
        let error = service(vec![], Some(FakeLlm::replying("[]")))
            .generate(&mut NullPrompter, "REQ-404", &[], &[], &[], None)
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
            .readiness("REQ-001", "RED", &["Req001Test: TODO: assert".into()], None)
            .unwrap();
        assert!(report.ready, "report: {report:?}");
        assert!(report.findings.is_empty());
        assert_eq!(
            report.next_step,
            "Every prerequisite is in place - spec implement REQ-001 can run."
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
            .readiness("REQ-001", "START", &[], None)
            .unwrap();
        assert!(!report.ready);
        let has = |fragment: &str| {
            assert!(
                report.findings.iter().any(|f| f.contains(fragment)),
                "no finding with {fragment:?}: {:?}",
                report.findings
            );
        };
        has("run spec test first");
        has("spec steps generate");
        has("spec unittest generate REQ-001");
        assert!(
            report.next_step.contains("spec test"),
            "the earliest gap leads: {}",
            report.next_step
        );
    }

    #[test]
    fn readiness_on_green_says_there_is_nothing_to_implement() {
        let report = service(vec![], None)
            .readiness("REQ-001", "GREEN", &[], None)
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
            .readiness("REQ-002", "RED", &["boom".into()], None)
            .unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.contains("No scenario is tagged @REQ-002"))
        );
        let report = service(vec![], None)
            .readiness("REQ-003", "RED", &["boom".into()], None)
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
            service
                .readiness("REQ-404", "RED", &[], None)
                .unwrap_err()
                .0,
            refusal
        );
        let readiness = service.readiness("REQ-001", "START", &[], None).unwrap();
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
        let readiness = service.readiness("REQ-001", "START", &[], None).unwrap();
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
                "No - run spec test first to record the RED bar.",
            )),
        );
        assert!(service.has_model());
        let readiness = service.readiness("REQ-001", "START", &[], None).unwrap();
        let advice = service
            .advice(&mut NullPrompter, "REQ-001", &readiness, &[])
            .unwrap()
            .unwrap();
        assert_eq!(advice, "No - run spec test first to record the RED bar.");
        let prompts = service.llm.as_ref().unwrap().chat().prompts.borrow();
        assert!(prompts[0].contains("The project assets:"));
        assert!(prompts[0].contains("No RED test run is recorded"));
        assert!(prompts[0].contains("at most four short sentences"));
    }

    #[test]
    fn a_model_failure_during_advice_is_reported() {
        let service = service(vec![], Some(FakeLlm::failing()));
        let readiness = service.readiness("REQ-001", "START", &[], None).unwrap();
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
        let readiness = service.readiness("REQ-001", "START", &[], None).unwrap();
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

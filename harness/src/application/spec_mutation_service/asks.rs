//! The ask side of spec mutations: a request preserved in the user's
//! words and broken into requirements that point back at it, the
//! decisions recorded on those proposals, and the bookkeeping that keeps
//! an ask and its derived requirements in step however either is
//! edited.
//!
//! Two small types carry the state the operations here share.
//! [`CatalogEdit`] is one mutation's view of the catalog: every
//! requirement written through it lands in the file it lives in, the
//! merged view follows, and the files touched are remembered for the
//! write - so no operation keeps a catalog and a merged copy in step by
//! hand. [`AskEdit`] is one ask being worked on inside such an edit: the
//! working copy of the ask, the file its children land in, and the
//! moves on its proposals (accept, withdraw, reword, answer) that touch
//! the derived requirements as well.

use super::*;
use crate::domain::ask::{
    accept, detach_proposal, proposal_mut, proposal_slot, set_state, take_wording,
    withdraw_proposal,
};

/// What `decompose` is pointed at: new words, or an ask on record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskTarget {
    Text(String),
    Existing(String),
}

/// How one interactive round ended.
enum Review {
    /// Answers or a missing piece were added to the brief: split again.
    Another,
    /// Confirmed, declined with nothing to add, or out of rounds.
    Done,
}

/// Decisions recorded on an ask without a wizard: proposals by
/// number, answers by question number, and the decomposition's state.
/// `by` says who is deciding, and the rules hold the automatic kind to
/// what they would accept on their own.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Decisions {
    pub accept: Vec<u32>,
    pub reject: Vec<(u32, Option<String>)>,
    pub exclude: Vec<(u32, Option<String>)>,
    pub answers: Vec<(u32, String)>,
    pub complete: Option<bool>,
    pub confirmed: Option<bool>,
    pub by: Option<AcceptedBy>,
}

impl Decisions {
    fn is_empty(&self) -> bool {
        self.accept.is_empty()
            && self.reject.is_empty()
            && self.exclude.is_empty()
            && self.answers.is_empty()
            && self.complete.is_none()
            && self.confirmed.is_none()
    }

    /// Every rejection and exclusion, with the status it records.
    fn withdrawals(&self) -> impl Iterator<Item = (u32, Option<String>, ProposalStatus)> + '_ {
        let rejected = self
            .reject
            .iter()
            .map(|(number, reason)| (*number, reason.clone(), ProposalStatus::Rejected));
        let excluded = self
            .exclude
            .iter()
            .map(|(number, reason)| (*number, reason.clone(), ProposalStatus::Excluded));
        rejected.chain(excluded)
    }
}

/// Where an ask stands after a decomposition, a decision, or an update.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DecomposeReport {
    pub ask: String,
    pub mode: Mode,
    pub request: String,
    /// The derived requirements, accepted or awaiting acceptance.
    pub derived: Vec<String>,
    pub accepted: Vec<String>,
    pub pending: Vec<u32>,
    pub rejected: Vec<u32>,
    pub excluded: Vec<u32>,
    /// Questions still open, blocking ones first.
    #[serde(rename = "openQuestions")]
    pub open_questions: Vec<Question>,
    pub ready: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    pub written: bool,
    pub file: String,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

impl DecomposeReport {
    pub(super) fn new(
        ask: &Requirement,
        readiness: Readiness,
        written: bool,
        file: String,
    ) -> Self {
        let numbers = |status: ProposalStatus| -> Vec<u32> {
            ask.proposals
                .iter()
                .filter(|p| p.status == status)
                .map(|p| p.number)
                .collect()
        };
        let mut open_questions: Vec<Question> = ask
            .questions
            .iter()
            .filter(|q| q.answer.is_none())
            .cloned()
            .collect();
        open_questions.sort_by_key(|q| !q.blocking);
        let accepted: Vec<String> = ask
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Accepted)
            .filter_map(|p| p.requirement.clone())
            .collect();
        let mode = ask
            .decomposition
            .as_ref()
            .map(|d| d.mode)
            .unwrap_or(Mode::Automatic);
        let next_step = if readiness.ready {
            format!(
                "{} is ready for implementation: run spec deliver {} to work its derived \
                 requirements, or spec show {} to read them.",
                ask.id, ask.id, ask.id
            )
        } else {
            let fix = match mode {
                Mode::Automatic => format!(
                    "Settle what is open with spec decide {id} (accept, reject, exclude, or \
                     answer by number), or review it yourself with spec draft {id} \
                     --interactive.",
                    id = ask.id
                ),
                Mode::Interactive => format!(
                    "Run spec draft {id} --interactive to finish the review, or spec decide \
                     {id} to record decisions directly.",
                    id = ask.id
                ),
            };
            format!(
                "{} is not ready: {}. {fix}",
                ask.id,
                readiness.reasons.join("; ")
            )
        };
        Self {
            ask: ask.id.clone(),
            mode,
            request: ask.request.clone().unwrap_or_default(),
            derived: ask.derived.clone(),
            accepted,
            pending: numbers(ProposalStatus::Pending),
            rejected: numbers(ProposalStatus::Rejected),
            excluded: numbers(ProposalStatus::Excluded),
            open_questions,
            ready: readiness.ready,
            reasons: readiness.reasons,
            written,
            file,
            next_step,
        }
    }

    /// The shape a manual draft takes when the assisted wizard fell
    /// back to it: one requirement, no ask.
    pub(super) fn from_draft(report: DraftReport) -> Self {
        let written = report.written.then(|| vec![report.id.clone()]);
        Self {
            ask: String::new(),
            mode: Mode::Interactive,
            request: String::new(),
            derived: written.clone().unwrap_or_default(),
            accepted: written.unwrap_or_default(),
            pending: Vec::new(),
            rejected: Vec::new(),
            excluded: Vec::new(),
            open_questions: Vec::new(),
            ready: report.written,
            reasons: report.findings,
            written: report.written,
            file: String::new(),
            next_step: report.next_step,
        }
    }

    /// The first requirement a run can work on, for an orchestrator
    /// that delivers one at a time.
    pub fn first_accepted(&self) -> Option<&str> {
        self.accepted.first().map(String::as_str)
    }
}

impl NextStep for DecomposeReport {
    fn next_step_mut(&mut self) -> &mut String {
        &mut self.next_step
    }
}

impl Human for DecomposeReport {
    fn human(&self) -> String {
        if self.ask.is_empty() {
            return self.next_step.clone();
        }
        let mut lines = vec![format!(
            "{} ({} mode): {}",
            self.ask,
            self.mode.as_str(),
            if self.ready { "ready" } else { "not ready" }
        )];
        lines.push(format!(
            "  {} accepted, {} pending, {} rejected, {} excluded, {} open question(s)",
            self.accepted.len(),
            self.pending.len(),
            self.rejected.len(),
            self.excluded.len(),
            self.open_questions.len()
        ));
        for reason in &self.reasons {
            lines.push(format!("  - {reason}"));
        }
        lines.push(self.next_step.clone());
        lines.join("\n")
    }
}

/// A proposal as the model wrote it, numbered by [`reconcile`] when it
/// joins the ask.
fn proposal_of(proposed: ProposedRequirement) -> Proposal {
    Proposal {
        number: 0,
        fingerprint: wording_fingerprint(
            &proposed.title,
            &proposed.story,
            &proposed.acceptance_criteria,
        ),
        title: proposed.title,
        story: proposed.story,
        acceptance_criteria: proposed.acceptance_criteria,
        status: ProposalStatus::Pending,
        requirement: None,
        reason: None,
        findings: Vec::new(),
    }
}

/// The blocking question an automatic run records when it has nothing
/// to split with: the honest alternative to inventing a requirement.
fn no_split_question(had_model: bool, ask_id: &str) -> String {
    if had_model {
        format!(
            "The model gave no usable split of the request. Word its requirements with \
             spec draft --parent {ask_id} --title ... or run spec draft {ask_id} --interactive."
        )
    } else {
        format!(
            "No model is resolved and the request is not an enumerated list, so nothing \
             was split. Point a model at the project with spec model use <name>, or word \
             the requirements with spec draft --parent {ask_id} --title ..."
        )
    }
}

fn no_proposal(ask_id: &str, number: u32) -> ServiceError {
    ServiceError(format!("{ask_id} has no proposal {number}."))
}

// ---- the working state ----------------------------------------------------

/// One mutation's view of the catalog. Every requirement written
/// through it lands in the file that declares it, the merged view
/// follows in the same move, and the files touched are remembered so
/// the write at the end covers exactly what changed.
pub(super) struct CatalogEdit {
    catalog: SpecCatalog,
    merged: Spec,
    touched: BTreeSet<String>,
}

impl CatalogEdit {
    pub(super) fn new(catalog: SpecCatalog) -> Self {
        let merged = catalog.merged();
        Self {
            catalog,
            merged,
            touched: BTreeSet::new(),
        }
    }

    pub(super) fn catalog(&self) -> &SpecCatalog {
        &self.catalog
    }

    pub(super) fn merged(&self) -> &Spec {
        &self.merged
    }

    pub(super) fn touched(&self) -> &BTreeSet<String> {
        &self.touched
    }

    /// Start over from a catalog read back from disk, keeping the
    /// record of what this edit already touched.
    fn reload(&mut self, catalog: SpecCatalog) {
        self.merged = catalog.merged();
        self.catalog = catalog;
    }

    pub(super) fn get(&self, id: &str) -> Option<&Requirement> {
        self.merged.requirements.iter().find(|r| r.id == id)
    }

    /// The requirement `id`, or the unknown-id refusal.
    pub(super) fn require(&self, id: &str) -> Result<Requirement, ServiceError> {
        self.get(id).cloned().ok_or_else(|| unknown_id(id))
    }

    /// The catalog file declaring `id`.
    pub(super) fn file_of(&self, id: &str) -> Result<String, ServiceError> {
        source_of_or_err(&self.catalog, id)
    }

    /// Write a requirement already in the catalog back into its file.
    pub(super) fn put(&mut self, requirement: Requirement) -> Result<String, ServiceError> {
        let file = self.file_of(&requirement.id)?;
        *requirement_mut_or_err(&mut self.catalog, &file, &requirement.id)? = requirement.clone();
        replace_requirement(&mut self.merged, &requirement);
        self.touched.insert(file.clone());
        Ok(file)
    }

    /// Add a new requirement to `file`.
    pub(super) fn insert(
        &mut self,
        file: &str,
        requirement: Requirement,
    ) -> Result<(), ServiceError> {
        file_mut_or_err(&mut self.catalog, file)?
            .requirements
            .push(requirement.clone());
        self.merged.requirements.push(requirement);
        self.touched.insert(file.to_string());
        Ok(())
    }

    /// Take a requirement out of the catalog.
    pub(super) fn remove(&mut self, id: &str) -> Result<(), ServiceError> {
        let file = self.file_of(id)?;
        file_mut_or_err(&mut self.catalog, &file)?
            .requirements
            .retain(|r| r.id != id);
        self.merged.requirements.retain(|r| r.id != id);
        self.touched.insert(file);
        Ok(())
    }

    /// A requirement's acceptance falls away.
    fn clear_acceptance(&mut self, id: &str) -> Result<(), ServiceError> {
        if let Some(mut child) = self.get(id).cloned() {
            child.acceptance = None;
            self.put(child)?;
        }
        Ok(())
    }

    /// Re-derive an ask from its children as they now stand - derived
    /// mirrors them, its criteria follow them, its readiness is
    /// recomputed - in the file it lives in.
    pub(super) fn settle_ask(&mut self, ask_id: &str) -> Result<Readiness, ServiceError> {
        let mut ask = self.require(ask_id)?;
        let readiness = settle(&mut ask, &self.merged);
        self.put(ask)?;
        Ok(readiness)
    }
}

/// One ask being worked on: the working copy, the file its children
/// land in, and the edit it is part of. The working copy is what the
/// moves below change; [`AskEdit::settle`] puts it back.
pub(super) struct AskEdit<'e> {
    pub(super) edit: &'e mut CatalogEdit,
    pub(super) ask: Requirement,
    pub(super) file: String,
}

impl<'e> AskEdit<'e> {
    /// Work on the ask `ask_id`, which the caller has checked is one.
    pub(super) fn open(edit: &'e mut CatalogEdit, ask_id: &str) -> Result<Self, ServiceError> {
        let ask = edit.require(ask_id)?;
        let file = edit.file_of(ask_id)?;
        Ok(Self { edit, ask, file })
    }

    fn proposal(&self, number: u32) -> Result<&Proposal, ServiceError> {
        self.ask
            .proposals
            .iter()
            .find(|p| p.number == number)
            .ok_or_else(|| no_proposal(&self.ask.id, number))
    }

    fn proposal_mut(&mut self, number: u32) -> Result<&mut Proposal, ServiceError> {
        let ask_id = self.ask.id.clone();
        proposal_mut(&mut self.ask, number).ok_or_else(|| no_proposal(&ask_id, number))
    }

    /// The proposal that `requirement` stands for, by number.
    pub(super) fn proposal_standing_for(&self, requirement: &str) -> Option<u32> {
        self.ask
            .proposals
            .iter()
            .find(|p| p.requirement.as_deref() == Some(requirement))
            .map(|p| p.number)
    }

    /// Accept proposal `number` by `by`: the requirement it becomes is
    /// written under the next free id of the ask's file - or, for a
    /// proposal that already stands for a requirement, that requirement
    /// takes the proposal's wording - carrying who accepted it and the
    /// fingerprint of what they accepted. Returns the requirement's id.
    fn accept(&mut self, number: u32, by: AcceptedBy) -> Result<String, ServiceError> {
        let ask_id = self.ask.id.clone();
        let proposal = self.proposal(number)?.clone();
        let standing = proposal
            .requirement
            .as_deref()
            .and_then(|id| self.edit.get(id).cloned());
        let id = match standing {
            Some(mut existing) => {
                take_wording(&mut existing, &proposal);
                existing.parent = Some(ask_id.clone());
                existing.kind = Some(ACTIONABLE_REQUIREMENT.into());
                accept(&mut existing, by);
                let id = existing.id.clone();
                self.edit.put(existing)?;
                id
            }
            None => {
                let id = next_id(self.edit.catalog(), &self.file);
                let mut child = requirement_from(&proposal, id.clone(), &ask_id);
                accept(&mut child, by);
                self.edit.insert(&self.file, child)?;
                id
            }
        };
        let proposal = self.proposal_mut(number)?;
        proposal.status = ProposalStatus::Accepted;
        proposal.requirement = Some(id.clone());
        proposal.reason = None;
        if !self.ask.derived.contains(&id) {
            self.ask.derived.push(id.clone());
        }
        Ok(id)
    }

    /// Reject or exclude proposal `number` with `reason`. A requirement
    /// that already stood for it is no longer accepted either way.
    fn withdraw(
        &mut self,
        number: u32,
        status: ProposalStatus,
        reason: Option<String>,
    ) -> Result<(), ServiceError> {
        let proposal = self.proposal_mut(number)?;
        if let Some(child) = withdraw_proposal(proposal, status, reason) {
            self.edit.clear_acceptance(&child)?;
        }
        Ok(())
    }

    /// The requirement standing for proposal `number` is gone (deleted
    /// or unlinked): the proposal no longer names it and takes `status`.
    pub(super) fn detach(
        &mut self,
        number: u32,
        status: ProposalStatus,
        reason: String,
    ) -> Result<(), ServiceError> {
        let proposal = self.proposal_mut(number)?;
        if let Some(child) = detach_proposal(proposal, status, Some(reason)) {
            self.edit.clear_acceptance(&child)?;
        }
        Ok(())
    }

    /// Proposal `number` takes new wording, and so does the requirement
    /// standing for it - which loses its acceptance until the next vote.
    fn reword_proposal(
        &mut self,
        number: u32,
        title: String,
        story: String,
        criteria: Vec<String>,
    ) -> Result<(), ServiceError> {
        let proposal = self.proposal_mut(number)?;
        proposal.title = title;
        proposal.story = story;
        proposal.acceptance_criteria = criteria;
        proposal.fingerprint = wording_fingerprint(
            &proposal.title,
            &proposal.story,
            &proposal.acceptance_criteria,
        );
        let proposal = proposal.clone();
        if let Some(mut child) = proposal
            .requirement
            .as_deref()
            .and_then(|id| self.edit.get(id).cloned())
        {
            take_wording(&mut child, &proposal);
            self.edit.put(child)?;
        }
        Ok(())
    }

    /// Record the answer to question `number`; an empty answer leaves
    /// it open.
    fn answer(&mut self, number: u32, answer: &str) -> Result<(), ServiceError> {
        let ask_id = self.ask.id.clone();
        let question = self
            .ask
            .questions
            .iter_mut()
            .find(|q| q.number == number)
            .ok_or_else(|| ServiceError(format!("{ask_id} has no question {number}.")))?;
        question.answer = Some(answer.trim().to_string()).filter(|a| !a.is_empty());
        Ok(())
    }

    /// Put the working copy into the catalog as it is, without
    /// re-deriving it.
    fn save(&mut self) -> Result<(), ServiceError> {
        self.edit.put(self.ask.clone()).map(drop)
    }

    /// Settle the working copy against its children and put it back.
    pub(super) fn settle(&mut self) -> Result<Readiness, ServiceError> {
        let readiness = settle(&mut self.ask, self.edit.merged());
        self.save()?;
        Ok(readiness)
    }
}

// ---- the operations -------------------------------------------------------

impl<R: SpecRepository, G: FeatureCatalog + FeatureFiles, C: WorkTree, S: StateStore>
    SpecMutationService<R, G, C, S>
{
    /// Preserve a request as an ask and break it into actionable
    /// requirements, or carry on with an ask already on record.
    ///
    /// The split is the model's when one is resolved and otherwise the
    /// deterministic one ([`split_enumerated`]), which reads only what
    /// the request spells out as a list. Every proposal is reviewed by
    /// the rules ([`review_proposal`] and the validator) before anyone
    /// decides on it.
    ///
    /// In [`Mode::Automatic`] nothing is asked: proposals the review
    /// passes are accepted on its authority, the rest stay pending with
    /// their findings, and the questions the model raised are recorded
    /// unanswered. In [`Mode::Interactive`] every proposal is put to the
    /// user to approve, reject, revise, or exclude, every blocking
    /// question is asked, and the user confirms the accepted
    /// requirements cover the request; an empty answer is never read
    /// as a vote. Either way the ask's readiness is derived from what
    /// was recorded, and running the same request again adds nothing
    /// that is already there.
    pub fn decompose(
        &self,
        prompter: &mut dyn Prompter,
        aid: ModelAid,
        file: Option<&str>,
        target: AskTarget,
        mode: Mode,
    ) -> Result<DecomposeReport, ServiceError> {
        // Claimed across the whole run: ids are handed out from what
        // the catalog holds, and the ask and its children land together.
        let _claim = self.store.claim()?;
        let mut edit = CatalogEdit::new(self.spec_catalog()?);
        let mut work = self.resolve_ask(&mut edit, file, target, mode)?;
        let request = work.ask.request.clone().unwrap_or_default();
        prompter.tell(&format!("{} preserves the request: {request}", work.ask.id));
        let mut written_children = Vec::new();
        for round in 1..=MAX_SPLIT_ROUNDS {
            let (proposals, questions) = match aid {
                Some(aid) => match self.split_with_model(prompter, aid, &work.ask) {
                    Some(reply) => (
                        reply.proposals.into_iter().map(proposal_of).collect(),
                        reply
                            .questions
                            .into_iter()
                            .map(|question| (question.question, question.blocking))
                            .collect(),
                    ),
                    None => (Vec::new(), Vec::new()),
                },
                None => (split_enumerated(&request), Vec::new()),
            };
            let added = reconcile(&mut work.ask, proposals);
            let asked = add_questions(&mut work.ask, questions);
            if round == 1 && added.is_empty() && asked == 0 && work.ask.proposals.is_empty() {
                // Nothing to decide on. Without a model that is the
                // honest end of what rules can do with prose, and the
                // interactive wizard can still word one requirement.
                if mode == Mode::Interactive {
                    if let Some(id) = self.manual_child(prompter, &mut work)? {
                        written_children.push(id);
                    }
                } else {
                    let question = no_split_question(aid.is_some(), &work.ask.id);
                    add_questions(&mut work.ask, vec![(question, true)]);
                }
            }
            let acceptable = self.review_pending(&mut work);
            match mode {
                Mode::Automatic => {
                    for number in acceptable {
                        written_children.push(work.accept(number, AcceptedBy::Automatic)?);
                    }
                    set_state(&mut work.ask, DecompositionState::Complete, None);
                    break;
                }
                Mode::Interactive => {
                    let another_possible = aid.is_some() && round < MAX_SPLIT_ROUNDS;
                    let outcome = self.review_interactively(
                        prompter,
                        &mut work,
                        &mut written_children,
                        another_possible,
                    );
                    match outcome {
                        Ok(Review::Another) => continue,
                        Ok(Review::Done) => break,
                        // The answers ran out. Nothing is decided on the
                        // user's behalf: what was recorded stands, and
                        // the readiness says what is still open.
                        Err(error) if error.is_end_of_input() => {
                            prompter.warn(
                                "The answers ran out before the review finished - what was \
                                 decided is kept, the rest stays pending.",
                            );
                            break;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        let readiness = work.settle()?;
        let (ask, file) = (work.ask, work.file);
        self.write_files(
            edit.catalog(),
            edit.touched(),
            &format!("decompose {} into {}", ask.id, naming(&ask.derived)),
        )?;
        let report = DecomposeReport::new(&ask, readiness, true, self.project_path(&file));
        prompter.tell(&report.human());
        Ok(report)
    }

    /// The ask a decomposition works on: the one named, the one already
    /// preserving the same request, or a new one written into the
    /// target file. Its decomposition is put in this run's mode, and a
    /// request edited behind the record reopens it.
    fn resolve_ask<'e>(
        &self,
        edit: &'e mut CatalogEdit,
        file: Option<&str>,
        target: AskTarget,
        mode: Mode,
    ) -> Result<AskEdit<'e>, ServiceError> {
        let ask_id = match target {
            AskTarget::Existing(id) => {
                let ask = edit.require(&id)?;
                if !ask.is_ask() {
                    return Err(ServiceError(format!(
                        "{id} is an actionable requirement, not an ask. Decompose an ask \
                         (ids like {ASK_PREFIX}-001), or describe new work in plain words."
                    )));
                }
                id
            }
            AskTarget::Text(text) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return Err(ServiceError(
                        "Describe what to build in plain words, or name an ask to continue.".into(),
                    ));
                }
                let known = fingerprint(&text);
                let existing = edit.merged().requirements.iter().find(|r| {
                    r.is_ask()
                        && r.decomposition
                            .as_ref()
                            .is_some_and(|d| d.fingerprint == known)
                });
                match existing {
                    Some(existing) => existing.id.clone(),
                    None => {
                        let file = self.target_file(edit.catalog(), file)?;
                        let ask = new_ask(next_ask_id(&edit.merged().requirements), &text, mode);
                        let id = ask.id.clone();
                        edit.insert(&file, ask)?;
                        id
                    }
                }
            }
        };
        let mut work = AskEdit::open(edit, &ask_id)?;
        let ask = &mut work.ask;
        if let Some(decomposition) = &mut ask.decomposition {
            // This run's mode is the one readiness is judged by: a run
            // that asked for the user's votes is not satisfied by the
            // automatic acceptances an earlier run recorded.
            decomposition.mode = mode;
        } else {
            ask.decomposition = Some(Decomposition {
                mode,
                state: DecompositionState::Open,
                confirmed: false,
                fingerprint: fingerprint(ask.request.as_deref().unwrap_or_default()),
                ready: false,
            });
        }
        // A request edited behind the record reopens the decomposition,
        // exactly as `spec reword --request` would have.
        let request = ask.request.clone().unwrap_or_default();
        let recorded = ask
            .decomposition
            .as_ref()
            .map(|d| d.fingerprint.clone())
            .unwrap_or_default();
        if recorded != fingerprint(&request) {
            ask.request = None;
            reopen(ask, &request);
        }
        Ok(work)
    }

    /// Ask the model to split the request, with the questions already
    /// answered and the requirements already accepted in its brief.
    /// `None` when no usable reply came, after saying so.
    fn split_with_model(
        &self,
        prompter: &mut dyn Prompter,
        aid: Model<'_>,
        ask: &Requirement,
    ) -> Option<ProposalReply> {
        let (model, llm) = aid;
        let request = ask.request.as_deref().unwrap_or_default();
        let context = ProposalContext {
            answered: ask
                .questions
                .iter()
                .filter_map(|q| q.answer.clone().map(|a| (q.text.clone(), a)))
                .collect(),
            accepted: ask
                .proposals
                .iter()
                .filter(|p| p.status == ProposalStatus::Accepted)
                .map(|p| p.title.clone())
                .collect(),
            questions_allowed: true,
        };
        let work = prompter.working(&format!(
            "Splitting the request into requirements with {model} - working"
        ));
        let prompt = proposal_prompt_with(request, &context);
        // Every rejection this check has made, so the last attempt it is
        // allowed can stop insisting: one rejection too many and the whole
        // split fails, which costs the author the draft. A happy-path draft
        // is worth keeping - the findings round asks for the edge case that
        // the retries could not win.
        let rejections = std::cell::Cell::new(0u32);
        let attempts = self.llm_attempts;
        // The decision model gets one rejection and no more. It is the
        // unreliable half of this check - see `with_judge` -
        // so it nudges a draft once and then takes what it is given,
        // leaving the remaining budget to the rule that is always
        // right about what it finds.
        let judged = std::cell::Cell::new(false);
        // A draft handed back unchanged after the model was told what it
        // lacked is the last word it has on the matter: the agent loop
        // stops asking on a verbatim repeat, so refusing it here again
        // would lose the draft to manual drafting instead of keeping
        // it for the author to finish, as the final attempt does.
        let last_refused = std::cell::RefCell::new(None::<String>);
        let check = |reply: &str| {
            let repeated = last_refused.borrow().as_deref() == Some(reply);
            let outcome = parse_reply_checked(reply).and_then(|parsed| {
                let proposals = &parsed.proposals;
                // Deterministic first: it costs nothing, and a draft
                // that is still missing its edge cases is not worth a
                // model's opinion on wording yet.
                let reason = missing_edge_case(proposals).or_else(|| {
                    // Asking on the last attempt buys a judgment the
                    // loop has no round left to act on, at several
                    // seconds a criterion.
                    let spendable = rejections.get() + 1 < attempts && !judged.get();
                    let judge = self.judge.as_deref().filter(|_| spendable)?;
                    judged.set(true);
                    unmeasurable_criteria(judge, proposals)
                });
                match reason {
                    Some(reason) if rejections.get() + 1 < attempts && !repeated => Err(reason),
                    _ => Ok(parsed),
                }
            });
            if outcome.is_err() {
                rejections.set(rejections.get() + 1);
                *last_refused.borrow_mut() = Some(reply.to_string());
            }
            outcome
        };
        let null = NullBroker;
        let broker: &dyn ToolBroker = self.broker.as_deref().unwrap_or(&null);
        let outcome = ask_llm(
            LlmCall {
                model,
                llm,
                tools: &self.tools,
                broker,
                config: AgentConfig::new(
                    "proposal",
                    self.llm_attempts,
                    self.max_rounds,
                    self.confirm.clone(),
                ),
            },
            prompter,
            &prompt,
            check,
            |prompter, attempt, of, reason| {
                prompter.warn(&format!(
                    "The model reply was invalid ({reason}) - asking again ({attempt} of {of})"
                ));
            },
        );
        drop(work);
        match outcome {
            Ok(parsed) => {
                prompter.tell(&format!(
                    "The request holds {} requirement(s) and {} question(s).",
                    parsed.proposals.len(),
                    parsed.questions.len()
                ));
                Some(parsed)
            }
            Err(LlmReplyError::Call(error)) => {
                prompter.warn(&format!("The model call failed ({}).", error.0));
                None
            }
            Err(LlmReplyError::Invalid { reason }) => {
                prompter.warn(&format!(
                    "The model did not return a valid requirement list ({reason})."
                ));
                None
            }
        }
    }

    /// Review every pending proposal of the ask by the rules: the
    /// validator's structural findings on the requirement it would
    /// become, the duplicate and conflict checks, and the wording
    /// review. The findings are recorded on the proposal; the numbers
    /// returned are the proposals the rules would accept without a
    /// vote, decided here and nowhere else.
    fn review_pending(&self, work: &mut AskEdit<'_>) -> Vec<u32> {
        let request = work.ask.request.clone().unwrap_or_default();
        let ask_id = work.ask.id.clone();
        let siblings = work.ask.proposals.clone();
        let provisional = next_id(work.edit.catalog(), &work.file);
        let merged = work.edit.merged();
        let mut acceptable = Vec::new();
        for proposal in work
            .ask
            .proposals
            .iter_mut()
            .filter(|p| p.status == ProposalStatus::Pending)
        {
            let own_id = proposal
                .requirement
                .clone()
                .unwrap_or_else(|| provisional.clone());
            let candidate = requirement_from(proposal, own_id.clone(), &ask_id);
            let findings = self.findings_for(merged, &candidate);
            let review = review_proposal(proposal, &request, merged, Some(&own_id), &siblings);
            if auto_acceptable(&findings.structural, &review) {
                acceptable.push(proposal.number);
            }
            let mut recorded = findings.structural;
            recorded.extend(review.blocking.iter().cloned());
            recorded.extend(findings.advisory);
            recorded.extend(review.advisory.iter().cloned());
            proposal.findings = recorded;
        }
        acceptable
    }

    /// The interactive stage of one round: each pending proposal is put
    /// to the user, each blocking question is asked, and the user says
    /// whether the accepted requirements cover the request. Returns
    /// whether another round is wanted - because answers arrived that
    /// the model can split with, or the coverage was not confirmed -
    /// and `another_possible` says whether one can be had.
    fn review_interactively(
        &self,
        prompter: &mut dyn Prompter,
        work: &mut AskEdit<'_>,
        written: &mut Vec<String>,
        another_possible: bool,
    ) -> Result<Review, ServiceError> {
        let pending: Vec<u32> = work
            .ask
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Pending)
            .map(|p| p.number)
            .collect();
        for number in pending {
            if let Some(id) = self.vote(prompter, work, number)? {
                written.push(id);
            }
        }
        let mut answered = 0;
        let open: Vec<Question> = work
            .ask
            .questions
            .iter()
            .filter(|q| q.answer.is_none())
            .cloned()
            .collect();
        for question in open {
            let answer = self.ask(
                prompter,
                &format!(
                    "Question {}{}: {} (Enter leaves it open):",
                    question.number,
                    if question.blocking { " (blocking)" } else { "" },
                    question.text
                ),
            )?;
            if !answer.trim().is_empty() {
                work.answer(question.number, &answer)?;
                answered += 1;
            }
        }
        if answered > 0 && another_possible {
            prompter.tell("Splitting again with your answers in the brief.");
            return Ok(Review::Another);
        }
        let accepted: Vec<String> = work
            .ask
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Accepted)
            .map(|p| format!("{} {}", p.requirement.clone().unwrap_or_default(), p.title))
            .collect();
        if accepted.is_empty() {
            prompter.tell("Nothing is accepted yet, so there is no coverage to confirm.");
            return Ok(Review::Done);
        }
        prompter.tell("Accepted so far:");
        for line in &accepted {
            prompter.tell(&format!("  {line}"));
        }
        if self.confirm(prompter, &format!("{COVERAGE_PROMPT} {}?", work.ask.id))? {
            set_state(&mut work.ask, DecompositionState::Complete, Some(true));
            return Ok(Review::Done);
        }
        set_state(&mut work.ask, DecompositionState::Open, Some(false));
        if !another_possible {
            prompter.tell(
                "The decomposition stays open. Add what is missing with spec draft \
                 --parent, or run the ask again with a model.",
            );
            return Ok(Review::Done);
        }
        let missing = self.ask(
            prompter,
            "What is missing? (added to the brief; Enter stops here):",
        )?;
        if missing.trim().is_empty() {
            return Ok(Review::Done);
        }
        let number = work
            .ask
            .questions
            .iter()
            .map(|q| q.number)
            .max()
            .unwrap_or(0)
            + 1;
        work.ask.questions.push(Question {
            number,
            text: "What is still missing from the accepted requirements?".into(),
            blocking: false,
            answer: Some(missing.trim().to_string()),
        });
        Ok(Review::Another)
    }

    /// One proposal put to the user. Approve writes it, reject and
    /// exclude record why, revise reopens its wording through the
    /// wizard's prompts and asks again. An empty answer is not a vote:
    /// it is asked for again, and after three the proposal is left
    /// pending rather than decided for the user. Returns the id of a
    /// requirement written.
    fn vote(
        &self,
        prompter: &mut dyn Prompter,
        work: &mut AskEdit<'_>,
        number: u32,
    ) -> Result<Option<String>, ServiceError> {
        let mut empty_answers = 0;
        loop {
            let proposal = work.proposal(number)?.clone();
            prompter.tell(&format!("Proposal {number}: {}", proposal.title));
            prompter.tell(&format!("  story: {}", proposal.story));
            for (index, criterion) in proposal.acceptance_criteria.iter().enumerate() {
                prompter.tell(&format!("  criterion {}: {criterion}", index + 1));
            }
            for finding in &proposal.findings {
                prompter.tell(&format!("  finding: {finding}"));
            }
            let answer = self
                .ask(
                    prompter,
                    &format!("Proposal {number} {REVIEW_PROMPT} [a/r/v/x]:"),
                )?
                .trim()
                .to_lowercase();
            match answer.as_str() {
                "a" => {
                    let id = work.accept(number, AcceptedBy::User)?;
                    prompter.tell(&format!("{id} accepted by you."));
                    return Ok(Some(id));
                }
                "r" | "x" => {
                    let reason = self.ask(prompter, "Why? (kept as the record):")?;
                    let status = if answer == "r" {
                        ProposalStatus::Rejected
                    } else {
                        ProposalStatus::Excluded
                    };
                    work.withdraw(number, status, Some(reason))?;
                    return Ok(None);
                }
                "v" => {
                    self.revise(prompter, work, number)?;
                    empty_answers = 0;
                }
                "" => {
                    empty_answers += 1;
                    if empty_answers >= MAX_EMPTY_VOTES {
                        prompter.warn(&format!(
                            "No answer for proposal {number} - it stays pending, undecided."
                        ));
                        return Ok(None);
                    }
                    prompter.warn(
                        "An empty answer is not a vote. Answer a (approve), r (reject), v \
                         (revise), or x (exclude from scope).",
                    );
                }
                _ => prompter.warn("Answer a, r, v, or x."),
            }
        }
    }

    /// Reword proposal `number` through the wizard's prompts, each
    /// pre-filled with the current wording. The revised wording is
    /// reviewed again before the next vote, and a requirement that
    /// already stood for the proposal takes the new wording and loses
    /// its acceptance until the vote.
    fn revise(
        &self,
        prompter: &mut dyn Prompter,
        work: &mut AskEdit<'_>,
        number: u32,
    ) -> Result<(), ServiceError> {
        let label = format!("proposal {number}");
        let proposal = work.proposal(number)?.clone();
        let title = self.ask_field(prompter, &label, "title", Some(&proposal.title))?;
        let story = self.ask_field(
            prompter,
            &label,
            "story (As a ..., I want ..., so that ...)",
            Some(&proposal.story),
        )?;
        let criteria = self.ask_criteria(prompter, &label, &proposal.acceptance_criteria)?;
        work.reword_proposal(number, title, story, criteria)?;
        self.review_pending(work);
        Ok(())
    }

    /// With no model and nothing enumerated, the interactive run can
    /// still word one requirement by hand. It is recorded on the ask as
    /// a proposal the user accepted - they wrote it - and written as a
    /// child. Returns the id, or `None` when the wizard declined.
    fn manual_child(
        &self,
        prompter: &mut dyn Prompter,
        work: &mut AskEdit<'_>,
    ) -> Result<Option<String>, ServiceError> {
        prompter.tell(&format!(
            "The request gave no complete requirement - drafting manually. Word the first \
             requirement of {} yourself.",
            work.ask.id
        ));
        let id = next_id(work.edit.catalog(), &work.file);
        // The wizard's catalog carries this run's view of the ask, so
        // the child it writes joins the ask's proposals as the user's
        // own acceptance.
        work.save()?;
        let report = self.draft_loop(
            prompter,
            DraftTarget {
                catalog: work.edit.catalog().clone(),
                file: work.file.clone(),
            },
            id.clone(),
            Some(Requirement {
                id: id.clone(),
                status: "pending".into(),
                kind: Some(ACTIONABLE_REQUIREMENT.into()),
                parent: Some(work.ask.id.clone()),
                ..Default::default()
            }),
            None,
            false,
        )?;
        if !report.written {
            return Ok(None);
        }
        // The wizard wrote the files; read them back into this run's
        // view of the catalog, which continues from it.
        work.edit.reload(self.spec_catalog()?);
        work.ask = work.edit.require(&work.ask.id)?;
        Ok(Some(id))
    }

    /// Record decisions on an ask directly - the deterministic half of
    /// the review, with no model and no wizard. Who decides matters:
    /// the automatic kind may only accept what the rules would accept
    /// on their own and may not confirm coverage, because that
    /// confirmation is the user's.
    pub fn decide(
        &self,
        ask_id: &str,
        decisions: Decisions,
    ) -> Result<DecomposeReport, ServiceError> {
        let _claim = self.store.claim()?;
        let mut edit = CatalogEdit::new(self.spec_catalog()?);
        let ask = edit.require(ask_id)?;
        if !ask.is_ask() {
            return Err(ServiceError(format!(
                "{ask_id} is an actionable requirement, not an ask; decisions are recorded \
                 on the ask it derives from{}.",
                ask.parent
                    .as_deref()
                    .map(|p| format!(" ({p})"))
                    .unwrap_or_default()
            )));
        }
        let by = decisions.by.unwrap_or(AcceptedBy::Automatic);
        if by == AcceptedBy::Automatic && decisions.confirmed == Some(true) {
            return Err(ServiceError(format!(
                "Confirming that the accepted requirements cover {ask_id} is the user's \
                 decision; pass it with --by user once they have said so."
            )));
        }
        let mut work = AskEdit::open(&mut edit, ask_id)?;
        for (number, answer) in &decisions.answers {
            work.answer(*number, answer)?;
        }
        for (number, reason, status) in decisions.withdrawals() {
            work.withdraw(number, status, reason)?;
        }
        let acceptable = self.review_pending(&mut work);
        for number in &decisions.accept {
            let proposal = work.proposal(*number)?.clone();
            if proposal.status == ProposalStatus::Accepted {
                // Already accepted: a user's vote on top of an automatic
                // acceptance is recorded as theirs, anything else is
                // already on record.
                let standing = proposal
                    .requirement
                    .as_deref()
                    .and_then(|id| work.edit.get(id))
                    .and_then(|r| r.acceptance.as_ref().map(|a| a.by));
                if standing == Some(by) || by == AcceptedBy::Automatic {
                    continue;
                }
            } else if by == AcceptedBy::Automatic && !acceptable.contains(number) {
                return Err(ServiceError(format!(
                    "Proposal {number} of {ask_id} does not pass the review ({}), so it \
                     cannot be accepted automatically. Revise it, or pass --by user with the \
                     user's approval.",
                    proposal.findings.join("; ")
                )));
            }
            work.accept(*number, by)?;
        }
        if let Some(complete) = decisions.complete {
            let state = if complete {
                DecompositionState::Complete
            } else {
                DecompositionState::Open
            };
            set_state(&mut work.ask, state, None);
        }
        if let Some(confirmed) = decisions.confirmed {
            let state = if confirmed {
                DecompositionState::Complete
            } else {
                work.ask
                    .decomposition
                    .as_ref()
                    .map(|d| d.state)
                    .unwrap_or(DecompositionState::Open)
            };
            set_state(&mut work.ask, state, Some(confirmed));
        }
        // The mode stands: a single vote recorded here is not the full
        // review `spec draft <ask> --interactive` is, and only that run
        // makes the user's approval of every child the bar.
        let readiness = work.settle()?;
        let (ask, file) = (work.ask, work.file);
        let written = !decisions.is_empty();
        if written {
            self.write_files(edit.catalog(), edit.touched(), &format!("decide {ask_id}"))?;
        }
        Ok(DecomposeReport::new(
            &ask,
            readiness,
            written,
            self.project_path(&file),
        ))
    }

    /// Bring a child's ask up to date after the child was written. A
    /// child newly placed under an ask joins its proposals, accepted by
    /// `by` when someone is named; a child reworded in meaning loses the
    /// acceptance it had; an ask the child left is settled without it.
    /// Returns a note the human should hear, if there is one.
    pub(super) fn sync_parent(
        &self,
        edit: &mut CatalogEdit,
        child_id: &str,
        former_parent: Option<&str>,
        by: Option<AcceptedBy>,
    ) -> Result<Option<String>, ServiceError> {
        let Some(mut child) = edit.get(child_id).cloned() else {
            return Ok(None);
        };
        if let Some(former) = former_parent
            && child.parent.as_deref() != Some(former)
            && edit.get(former).is_some_and(|r| r.is_ask())
        {
            let mut left = AskEdit::open(edit, former)?;
            if let Some(number) = left.proposal_standing_for(child_id) {
                left.detach(
                    number,
                    ProposalStatus::Excluded,
                    format!("{child_id} was unlinked from {former}"),
                )?;
            }
            left.settle()?;
            child = edit.require(child_id)?;
        }
        let Some(parent) = child.parent.clone() else {
            return Ok(None);
        };
        let mut work = AskEdit::open(edit, &parent)?;
        let note = if work.proposal_standing_for(child_id).is_some() {
            invalidate(&mut work.ask, &mut child).then(|| {
                format!(
                    "{child_id} was reworded since it was accepted, so its acceptance is \
                     withdrawn and {parent} is not ready until it is accepted again (spec \
                     decide {parent})."
                )
            })
        } else {
            // A child placed under the ask joins its proposals. The
            // user placing it is their acceptance; the automatic kind
            // is held to the same review as any proposal, and a child
            // the rules block on stays pending with the findings on
            // record.
            let slot = proposal_slot(&work.ask, &child);
            let number = match slot {
                Some(index) => work.ask.proposals[index].number,
                None => {
                    work.ask
                        .proposals
                        .iter()
                        .map(|p| p.number)
                        .max()
                        .unwrap_or(0)
                        + 1
                }
            };
            let mut proposal = proposal_from(&child, number, ProposalStatus::Pending);
            let request = work.ask.request.clone().unwrap_or_default();
            let others: Vec<Proposal> = work
                .ask
                .proposals
                .iter()
                .filter(|p| p.number != number)
                .cloned()
                .collect();
            let review = review_proposal(
                &proposal,
                &request,
                work.edit.merged(),
                Some(child_id),
                &others,
            );
            proposal.findings = review.all();
            let note = match by {
                Some(by) if by == AcceptedBy::User || review.blocking.is_empty() => {
                    proposal.status = ProposalStatus::Accepted;
                    accept(&mut child, by);
                    None
                }
                Some(_) => {
                    child.acceptance = None;
                    Some(format!(
                        "{child_id} is linked to {parent} but not accepted: {}. Decide on it \
                         with spec decide {parent}.",
                        review.blocking.join("; ")
                    ))
                }
                None => {
                    child.acceptance = None;
                    None
                }
            };
            match slot {
                Some(index) => work.ask.proposals[index] = proposal,
                None => work.ask.proposals.push(proposal),
            }
            note
        };
        work.edit.put(child)?;
        work.settle()?;
        Ok(note)
    }
}

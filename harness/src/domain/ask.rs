//! An original request preserved as a requirement of its own, and the
//! rules that connect it to the requirements derived from it.
//!
//! Everything here is deterministic and model-free: fingerprints of
//! wording, the review a proposal gets before it may be accepted
//! without a vote, the readiness of an ask derived from its children,
//! and the invalidation a changed wording causes. What the text of a
//! request *means* - how to split it, whether a split covers it - is
//! the calling agent's judgment and, in interactive mode, the user's.
//! This module records those decisions; it does not make them.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::model::{ACTIONABLE_REQUIREMENT, ASK_REQUIREMENT, Requirement, Spec};
use crate::domain::requirement_id::{MIN_WIDTH, split_id};

/// The prefix asks are numbered under, whatever prefix the catalog's
/// actionable requirements carry: `ASK-001` beside `REQ-001` or
/// `HARNESS-014`, so the two kinds are told apart by id alone.
pub const ASK_PREFIX: &str = "ASK";

/// Whether acceptance is the validation's to grant or the user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Valid proposals are accepted without a question; unresolved
    /// ambiguity is recorded, never answered on anyone's behalf.
    Automatic,
    /// Every proposal is approved, rejected, or revised by the user,
    /// and the user confirms the decomposition covers the request.
    Interactive,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Automatic => "automatic",
            Mode::Interactive => "interactive",
        }
    }
}

/// Whether the split of the request has been declared finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecompositionState {
    Open,
    Complete,
}

/// Where an ask's decomposition stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decomposition {
    pub mode: Mode,
    pub state: DecompositionState,
    /// In interactive mode, whether the user has confirmed the accepted
    /// requirements cover the request. Always false in automatic mode,
    /// where readiness does not read it.
    #[serde(default)]
    pub confirmed: bool,
    /// Fingerprint of the request text the proposals were made from. A
    /// request whose meaning changed reopens the decomposition.
    pub fingerprint: String,
    /// Derived from the children on every write - see [`readiness`].
    /// Stored so a reader of the file sees it, recomputed so it cannot
    /// drift from them.
    #[serde(default)]
    pub ready: bool,
}

/// Who accepted a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcceptedBy {
    /// The deterministic review passed and no one was asked.
    Automatic,
    /// A user typed the approval.
    User,
}

impl AcceptedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            AcceptedBy::Automatic => "automatic",
            AcceptedBy::User => "user",
        }
    }
}

impl std::str::FromStr for AcceptedBy {
    type Err = String;

    /// The two words the tools take, and nothing else - a typo is never
    /// read as either party's decision.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.trim() {
            "user" => Ok(AcceptedBy::User),
            "automatic" => Ok(AcceptedBy::Automatic),
            other => Err(format!(
                "by must be \"user\" or \"automatic\", not {other:?}."
            )),
        }
    }
}

/// The acceptance a derived requirement carries: who gave it, and the
/// fingerprint of the wording they accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub by: AcceptedBy,
    pub wording: String,
}

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    /// Proposed and not yet decided, or accepted once and then reworded.
    Pending,
    /// A requirement of the spec, named in `requirement`.
    Accepted,
    /// Turned down and waiting on a revision; blocks readiness.
    Rejected,
    /// Explicitly put out of scope; does not block readiness.
    Excluded,
}

/// One requirement proposed for an ask. Accepted proposals point at the
/// requirement they became; the others are the record of what was
/// proposed and why it is not in the spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub number: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub story: String,
    #[serde(rename = "acceptanceCriteria", default)]
    pub acceptance_criteria: Vec<String>,
    /// Fingerprint of the wording above; a re-run proposing the same
    /// wording again adds nothing.
    pub fingerprint: String,
    pub status: ProposalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    /// Why it was rejected or excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The review findings at the last decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

/// A clarification the decomposition could not settle on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub number: u32,
    pub text: String,
    /// Whether the ask may be ready while this stands unanswered.
    #[serde(default)]
    pub blocking: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

/// Whether an ask is ready for implementation, and when it is not,
/// every reason why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Readiness {
    pub ready: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

/// Sixteen hex digits of SHA-256 over `text` with its whitespace
/// collapsed: the identity of a piece of wording.
pub fn fingerprint(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The fingerprint of a requirement's wording: title, story, and
/// criteria, and nothing about where its files live or who accepted it.
/// Two wordings with the same fingerprint mean the same thing as far as
/// a test can tell.
pub fn wording_fingerprint(title: &str, story: &str, criteria: &[String]) -> String {
    let mut text = format!("{title}\n{story}");
    for criterion in criteria {
        text.push('\n');
        text.push_str(criterion);
    }
    fingerprint(&text)
}

/// [`wording_fingerprint`] of a requirement.
pub fn requirement_fingerprint(requirement: &Requirement) -> String {
    wording_fingerprint(
        &requirement.title,
        &requirement.story,
        &requirement.acceptance_criteria,
    )
}

/// The next free ask id in `spec`: [`ASK_PREFIX`] and one past the
/// highest number already carrying it, at the width the catalog uses.
pub fn next_ask_id(requirements: &[Requirement]) -> String {
    let (max, width) = requirements
        .iter()
        .filter_map(|r| split_id(&r.id))
        .filter(|(prefix, _)| *prefix == ASK_PREFIX)
        .filter_map(|(_, digits)| digits.parse::<u32>().ok().map(|n| (n, digits.len())))
        .fold((0, MIN_WIDTH), |(max, width), (number, digits)| {
            (max.max(number), width.max(digits))
        });
    format!("{ASK_PREFIX}-{next:0width$}", next = max + 1)
}

/// A new ask for `request`, worded deterministically from the text so
/// it passes every rule an actionable requirement does: the title is
/// the request's first clause, the story names the request, and the
/// criteria are [`settle`]d from the children.
pub fn new_ask(id: String, request: &str, mode: Mode) -> Requirement {
    let request = request.trim();
    let mut ask = Requirement {
        id,
        title: title_from_request(request),
        status: "pending".into(),
        story: story_from_request(request),
        kind: Some(ASK_REQUIREMENT.into()),
        request: Some(request.to_string()),
        decomposition: Some(Decomposition {
            mode,
            state: DecompositionState::Open,
            confirmed: false,
            fingerprint: fingerprint(request),
            ready: false,
        }),
        ..Default::default()
    };
    ask.acceptance_criteria = ask_criteria(&ask, &[]);
    ask
}

/// The request's first sentence or clause as a title, capitalised and
/// cut at a word boundary so it stays a title. List markers - `1.`,
/// `2)`, `-`, `*` - are not words, so an enumerated request is titled
/// by its first item rather than by the number in front of it.
pub fn title_from_request(request: &str) -> String {
    const MAX: usize = 72;
    let is_marker = |part: &str| {
        part.is_empty()
            || part
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, ')' | '-' | '*' | '(' | ' '))
    };
    let clause = request
        .split(['.', ';', '\n', ':'])
        .map(str::trim)
        .find(|part| !is_marker(part))
        .unwrap_or(request)
        .trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, ')' | '-' | '*' | ' '))
        .trim_end_matches(',');
    let mut title = String::new();
    for word in clause.split_whitespace() {
        if !title.is_empty() && title.len() + 1 + word.len() > MAX {
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => title,
    }
}

pub fn story_from_request(request: &str) -> String {
    let want = request.trim_end_matches(['.', ' ']);
    format!(
        "As a requester, I want \"{want}\" so that every requirement derived from it is \
         delivered."
    )
}

/// The ask's acceptance criteria, derived from its accepted children so
/// they cannot disagree with them: one per derived requirement, and the
/// refusal that holds for every ask - it is not delivered while a
/// derived requirement is still pending. Worded to pass the same
/// wording review every requirement passes: a concrete outcome after
/// `then`, and an edge case among the criteria.
pub fn ask_criteria(ask: &Requirement, children: &[&Requirement]) -> Vec<String> {
    let mut criteria: Vec<String> = children
        .iter()
        .map(|child| {
            format!(
                "Given the ask {}, when {} \"{}\" is implemented, then the derived \
                 requirement {} has the status \"implemented\"",
                ask.id, child.id, child.title, child.id
            )
        })
        .collect();
    criteria.push(format!(
        "Given the ask {} with a derived requirement still pending, when spec \
         mark-implemented {} runs, then it is refused with \"is still pending\"",
        ask.id, ask.id
    ));
    criteria
}

/// Review findings on one proposal that the rules can establish without
/// reading for meaning: a duplicate of something already in the spec, a
/// conflict with another proposal of the same ask, and - advisory, since
/// a broad request names its parts in other words - wording that shares
/// no content word with the request.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProposalReview {
    /// Findings that keep the proposal from automatic acceptance.
    pub blocking: Vec<String>,
    /// Findings recorded with the proposal, which a user may wave through.
    pub advisory: Vec<String>,
}

impl ProposalReview {
    pub fn all(&self) -> Vec<String> {
        let mut all = self.blocking.clone();
        all.extend(self.advisory.iter().cloned());
        all
    }
}

/// Review `proposal` against the requirements already in `spec` (other
/// than the one it may become, `own_id`) and the other proposals of the
/// same ask.
pub fn review_proposal(
    proposal: &Proposal,
    request: &str,
    spec: &Spec,
    own_id: Option<&str>,
    siblings: &[Proposal],
) -> ProposalReview {
    let mut review = ProposalReview::default();
    let title = normalize(&proposal.title);
    for existing in &spec.requirements {
        if Some(existing.id.as_str()) == own_id || existing.is_ask() {
            continue;
        }
        if normalize(&existing.title) == title {
            review
                .blocking
                .push(format!("duplicates {}: the same title", existing.id));
        } else if let Some(criterion) = existing.acceptance_criteria.iter().find(|e| {
            proposal
                .acceptance_criteria
                .iter()
                .any(|c| normalize(c) == normalize(e))
        }) {
            review.blocking.push(format!(
                "duplicates {}: shares the criterion \"{criterion}\"",
                existing.id
            ));
        }
    }
    for sibling in siblings {
        if sibling.number == proposal.number
            || matches!(
                sibling.status,
                ProposalStatus::Excluded | ProposalStatus::Rejected
            )
        {
            continue;
        }
        if sibling.fingerprint == proposal.fingerprint {
            review.blocking.push(format!(
                "duplicates proposal {}: the same wording",
                sibling.number
            ));
        } else if normalize(&sibling.title) == title {
            review.blocking.push(format!(
                "conflicts with proposal {}: the same title with different criteria",
                sibling.number
            ));
        }
    }
    if !shares_a_content_word(proposal, request) {
        review
            .advisory
            .push("alignment: names nothing the request says - check it is not new scope".into());
    }
    review
}

/// Does any content word of the proposal appear in the request?
fn shares_a_content_word(proposal: &Proposal, request: &str) -> bool {
    let request: Vec<String> = content_words(request).collect();
    if request.is_empty() {
        return true;
    }
    let mut proposal_text = format!("{} {}", proposal.title, proposal.story);
    for criterion in &proposal.acceptance_criteria {
        proposal_text.push(' ');
        proposal_text.push_str(criterion);
    }
    content_words(&proposal_text).any(|word| {
        request
            .iter()
            .any(|known| known == &word || stem(known) == stem(&word))
    })
}

const STOP_WORDS: &[&str] = &[
    "about", "after", "also", "been", "before", "being", "between", "called", "could", "does",
    "each", "every", "from", "given", "have", "into", "more", "must", "only", "other", "result",
    "returns", "same", "should", "that", "their", "then", "there", "these", "this", "those",
    "through", "under", "user", "want", "when", "which", "while", "will", "with", "would",
];

fn content_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 3)
        .map(str::to_lowercase)
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
}

/// A crude stem: enough to read `numbers` as `number` and `summed` as
/// `sum`, which is all the alignment check asks of it.
fn stem(word: &str) -> &str {
    for suffix in ["ing", "ed", "es", "s"] {
        if let Some(stemmed) = word.strip_suffix(suffix)
            && stemmed.len() >= 3
        {
            return stemmed;
        }
    }
    word
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim_end_matches('.')
        .to_string()
}

/// Whether a proposal may be accepted without a vote: nothing structural
/// against it and nothing the review blocks on. Advisory findings travel
/// with it, exactly as `spec draft --title` writes a requirement with
/// its wording findings open.
pub fn auto_acceptable(structural: &[String], review: &ProposalReview) -> bool {
    structural.is_empty() && review.blocking.is_empty()
}

/// Reconcile incoming proposals with those already on `ask`: one whose
/// wording - or whose title, which is how a proposal is known - is
/// already there (at any status) adds nothing, so a re-run of the same
/// request neither duplicates a requirement nor forgets a decision.
/// Returns the numbers of the proposals added.
pub fn reconcile(ask: &mut Requirement, incoming: Vec<Proposal>) -> Vec<u32> {
    let mut added = Vec::new();
    for mut proposal in incoming {
        if ask.proposals.iter().any(|known| {
            known.fingerprint == proposal.fingerprint
                || normalize(&known.title) == normalize(&proposal.title)
        }) {
            continue;
        }
        proposal.number = ask.proposals.iter().map(|p| p.number).max().unwrap_or(0) + 1;
        proposal.status = ProposalStatus::Pending;
        proposal.requirement = None;
        added.push(proposal.number);
        ask.proposals.push(proposal);
    }
    added
}

/// Add questions not already asked (by text), numbered after the last.
pub fn add_questions(ask: &mut Requirement, incoming: Vec<(String, bool)>) -> usize {
    let mut added = 0;
    for (text, blocking) in incoming {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty()
            || ask
                .questions
                .iter()
                .any(|known| normalize(&known.text) == normalize(&text))
        {
            continue;
        }
        let number = ask.questions.iter().map(|q| q.number).max().unwrap_or(0) + 1;
        ask.questions.push(Question {
            number,
            text,
            blocking,
            answer: None,
        });
        added += 1;
    }
    added
}

/// A proposal as the requirement it becomes once accepted.
pub fn requirement_from(proposal: &Proposal, id: String, parent: &str) -> Requirement {
    Requirement {
        id,
        title: proposal.title.clone(),
        status: "pending".into(),
        story: proposal.story.clone(),
        acceptance_criteria: proposal.acceptance_criteria.clone(),
        kind: Some(ACTIONABLE_REQUIREMENT.into()),
        parent: Some(parent.to_string()),
        ..Default::default()
    }
}

/// A requirement as a proposal: how a hand-drafted child, or a child
/// whose wording changed, is recorded on its ask.
pub fn proposal_from(requirement: &Requirement, number: u32, status: ProposalStatus) -> Proposal {
    Proposal {
        number,
        title: requirement.title.clone(),
        story: requirement.story.clone(),
        acceptance_criteria: requirement.acceptance_criteria.clone(),
        fingerprint: requirement_fingerprint(requirement),
        status,
        requirement: Some(requirement.id.clone()),
        reason: None,
        findings: Vec::new(),
    }
}

/// Two pieces of wording that read the same once whitespace and case
/// are set aside.
pub fn same_wording(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

/// The proposal of `ask` that `number` names.
pub fn proposal_mut(ask: &mut Requirement, number: u32) -> Option<&mut Proposal> {
    ask.proposals.iter_mut().find(|p| p.number == number)
}

/// The row a requirement placed under `ask` takes among its proposals,
/// when one is already there for it: the row with the same wording
/// (one it was unlinked or deleted from), or a pending row the
/// deterministic split left as a bare title that this requirement now
/// words. `None` means it joins as a new proposal.
pub fn proposal_slot(ask: &Requirement, child: &Requirement) -> Option<usize> {
    let wording = requirement_fingerprint(child);
    ask.proposals.iter().position(|p| {
        p.requirement.is_none()
            && (p.fingerprint == wording
                || (p.status == ProposalStatus::Pending
                    && p.story.is_empty()
                    && p.acceptance_criteria.is_empty()
                    && same_wording(&p.title, &child.title)))
    })
}

/// Record that `proposal` is rejected or excluded (or pending again),
/// with the reason given. Returns the id of the requirement that stood
/// for it, whose acceptance no longer holds either way.
pub fn withdraw_proposal(
    proposal: &mut Proposal,
    status: ProposalStatus,
    reason: Option<String>,
) -> Option<String> {
    proposal.status = status;
    proposal.reason = reason.filter(|r| !r.trim().is_empty());
    proposal.requirement.clone()
}

/// The requirement standing for `proposal` is gone (deleted, or unlinked
/// from the ask): the proposal no longer names it and takes `status`.
/// Returns that requirement's id, whose acceptance under this ask is
/// over.
pub fn detach_proposal(
    proposal: &mut Proposal,
    status: ProposalStatus,
    reason: Option<String>,
) -> Option<String> {
    let stood_for = withdraw_proposal(proposal, status, reason);
    proposal.requirement = None;
    stood_for
}

/// `child` takes the wording of `proposal`; whatever acceptance it had
/// was of other words.
pub fn take_wording(child: &mut Requirement, proposal: &Proposal) {
    child.title = proposal.title.clone();
    child.story = proposal.story.clone();
    child.acceptance_criteria = proposal.acceptance_criteria.clone();
    child.acceptance = None;
}

/// `by` accepts `requirement` as it is worded now.
pub fn accept(requirement: &mut Requirement, by: AcceptedBy) {
    requirement.acceptance = Some(Acceptance {
        by,
        wording: requirement_fingerprint(requirement),
    });
}

/// Move the decomposition of `ask` to `state`, and record the user's
/// confirmation when it changed.
pub fn set_state(ask: &mut Requirement, state: DecompositionState, confirmed: Option<bool>) {
    if let Some(decomposition) = &mut ask.decomposition {
        decomposition.state = state;
        if let Some(confirmed) = confirmed {
            decomposition.confirmed = confirmed;
        }
    }
}

/// Is `ask` ready for implementation? Ready means every condition below
/// holds; each one that does not is a reason.
///
/// - its decomposition is complete;
/// - every derived requirement exists, names the ask as its parent, and
///   carries an acceptance of its current wording - a user's acceptance
///   when the mode is interactive;
/// - no blocking question stands unanswered;
/// - no proposal is pending or rejected (excluded ones are out of scope
///   by decision, and do not count against it);
/// - in interactive mode, the user has confirmed the coverage.
///
/// Acceptance here means ready to implement, not implemented: status is
/// not read.
pub fn readiness(ask: &Requirement, spec: &Spec) -> Readiness {
    let mut reasons = Vec::new();
    let Some(decomposition) = &ask.decomposition else {
        return Readiness {
            ready: false,
            reasons: vec![format!("{} has no decomposition recorded", ask.id)],
        };
    };
    if decomposition.state != DecompositionState::Complete {
        reasons.push("the decomposition is still open".to_string());
    }
    if ask.derived.is_empty() {
        reasons.push("no derived requirement has been accepted".to_string());
    }
    for id in &ask.derived {
        let Some(child) = spec.requirements.iter().find(|r| &r.id == id) else {
            reasons.push(format!("derived requirement {id} is not in the spec"));
            continue;
        };
        if child.parent.as_deref() != Some(ask.id.as_str()) {
            reasons.push(format!("{id} does not name {} as its parent", ask.id));
        }
        match &child.acceptance {
            None => reasons.push(format!("{id} is not accepted")),
            Some(acceptance) => {
                if acceptance.wording != requirement_fingerprint(child) {
                    reasons.push(format!(
                        "{id} was reworded since it was accepted - accept it again"
                    ));
                }
                if decomposition.mode == Mode::Interactive && acceptance.by != AcceptedBy::User {
                    reasons.push(format!(
                        "{id} was accepted automatically, and interactive mode needs the \
                         user's approval"
                    ));
                }
            }
        }
    }
    for question in ask
        .questions
        .iter()
        .filter(|q| q.blocking && q.answer.is_none())
    {
        reasons.push(format!(
            "question {} is blocking and unanswered: {}",
            question.number, question.text
        ));
    }
    for proposal in &ask.proposals {
        match proposal.status {
            ProposalStatus::Pending => reasons.push(format!(
                "proposal {} \"{}\" is still pending",
                proposal.number, proposal.title
            )),
            ProposalStatus::Rejected => reasons.push(format!(
                "proposal {} \"{}\" was rejected and neither revised nor excluded",
                proposal.number, proposal.title
            )),
            ProposalStatus::Accepted | ProposalStatus::Excluded => {}
        }
    }
    if decomposition.mode == Mode::Interactive && !decomposition.confirmed {
        reasons
            .push("the user has not confirmed the accepted requirements cover the request".into());
    }
    Readiness {
        ready: reasons.is_empty(),
        reasons,
    }
}

/// Bring `ask` into line with its children in `spec`: `derived` is every
/// requirement naming it as parent that carries an acceptance (in the
/// order they appear), the criteria are derived from them, and `ready`
/// is recomputed. Returns the readiness.
pub fn settle(ask: &mut Requirement, spec: &Spec) -> Readiness {
    let children: Vec<&Requirement> = spec
        .requirements
        .iter()
        .filter(|r| r.parent.as_deref() == Some(ask.id.as_str()))
        .collect();
    let mut derived: Vec<String> = ask
        .derived
        .iter()
        .filter(|id| children.iter().any(|c| &c.id == *id))
        .cloned()
        .collect();
    for child in &children {
        if !derived.contains(&child.id) {
            derived.push(child.id.clone());
        }
    }
    ask.derived = derived;
    let accepted: Vec<&Requirement> = ask
        .derived
        .iter()
        .filter_map(|id| children.iter().find(|c| &c.id == id).copied())
        .collect();
    ask.acceptance_criteria = ask_criteria(ask, &accepted);
    let readiness = readiness(ask, spec);
    if let Some(decomposition) = &mut ask.decomposition {
        decomposition.ready = readiness.ready;
    }
    readiness
}

/// The wording of `child` changed in meaning: its acceptance no longer
/// stands, and the proposal that became it goes back to pending under
/// the new wording. Returns whether anything was invalidated.
pub fn invalidate(ask: &mut Requirement, child: &mut Requirement) -> bool {
    let current = requirement_fingerprint(child);
    let stood = child
        .acceptance
        .as_ref()
        .is_some_and(|acceptance| acceptance.wording != current);
    if !stood {
        return false;
    }
    child.acceptance = None;
    match ask
        .proposals
        .iter_mut()
        .find(|p| p.requirement.as_deref() == Some(child.id.as_str()))
    {
        Some(proposal) => {
            proposal.title = child.title.clone();
            proposal.story = child.story.clone();
            proposal.acceptance_criteria = child.acceptance_criteria.clone();
            proposal.fingerprint = current;
            proposal.status = ProposalStatus::Pending;
            proposal.reason = Some("reworded after acceptance".into());
        }
        None => {
            let number = ask.proposals.iter().map(|p| p.number).max().unwrap_or(0) + 1;
            ask.proposals
                .push(proposal_from(child, number, ProposalStatus::Pending));
        }
    }
    true
}

/// The request's text changed in meaning: the decomposition reopens and
/// any confirmation is withdrawn, while the children and their
/// acceptances - which are about the children's wording - stand.
pub fn reopen(ask: &mut Requirement, request: &str) -> bool {
    let request = request.trim();
    let changed = ask.request.as_deref().map(str::trim) != Some(request);
    if !changed {
        return false;
    }
    ask.request = Some(request.to_string());
    if let Some(decomposition) = &mut ask.decomposition {
        decomposition.state = DecompositionState::Open;
        decomposition.confirmed = false;
        decomposition.fingerprint = fingerprint(request);
    }
    true
}

/// The deterministic split: only what the request spells out as a list.
/// Numbered or bulleted lines and semicolon-separated clauses become
/// proposals with a title each and nothing else - a story and criteria
/// are meaning, and meaning is not read here. Each comes back with a
/// finding saying what it still needs. One clause, or no structure at
/// all, splits into nothing.
pub fn split_enumerated(request: &str) -> Vec<Proposal> {
    let lines: Vec<&str> = request
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let items: Vec<String> = if lines.len() > 1 {
        lines.iter().filter_map(|line| strip_marker(line)).collect()
    } else {
        request
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| part.trim_end_matches('.').to_string())
            .collect()
    };
    if items.len() < 2 {
        return Vec::new();
    }
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let title = title_from_request(&item);
            Proposal {
                number: index as u32 + 1,
                fingerprint: wording_fingerprint(&title, "", &[]),
                title,
                story: String::new(),
                acceptance_criteria: Vec::new(),
                status: ProposalStatus::Pending,
                requirement: None,
                reason: None,
                findings: vec![
                    "needs a user story and at least one Given/When/Then criterion".into(),
                ],
            }
        })
        .collect()
}

/// A list line without its marker: `1. x`, `1) x`, `- x`, `* x`.
fn strip_marker(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix(['-', '*', '•'])
        .map(str::trim)
        .or_else(|| {
            let digits = line.trim_start_matches(|c: char| c.is_ascii_digit());
            if digits.len() == line.len() {
                return None;
            }
            digits.strip_prefix(['.', ')']).map(str::trim)
        })?;
    (!rest.is_empty()).then(|| rest.trim_end_matches('.').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(id: &str, parent: &str, by: Option<AcceptedBy>) -> Requirement {
        let mut requirement = Requirement {
            id: id.into(),
            title: format!("Title {id}"),
            status: "pending".into(),
            story: "As a user, I want x so that y.".into(),
            acceptance_criteria: vec!["Given a, when b, then 3".into()],
            kind: Some(ACTIONABLE_REQUIREMENT.into()),
            parent: Some(parent.into()),
            ..Default::default()
        };
        requirement.acceptance = by.map(|by| Acceptance {
            by,
            wording: requirement_fingerprint(&requirement),
        });
        requirement
    }

    fn complete_ask(mode: Mode) -> Requirement {
        let mut ask = new_ask("ASK-001".into(), "sum numbers; empty is zero", mode);
        ask.decomposition.as_mut().unwrap().state = DecompositionState::Complete;
        ask
    }

    fn spec_of(requirements: Vec<Requirement>) -> Spec {
        Spec {
            project: "Kata".into(),
            requirements,
            ..Spec::default()
        }
    }

    #[test]
    fn a_fingerprint_ignores_whitespace_but_not_words() {
        assert_eq!(fingerprint("a  b\n c"), fingerprint("a b c"));
        assert_ne!(fingerprint("a b c"), fingerprint("a b d"));
        assert_eq!(fingerprint("x").len(), 16);
    }

    #[test]
    fn a_new_ask_passes_the_rules_every_requirement_does() {
        let ask = new_ask(
            "ASK-001".into(),
            "sum numbers from a comma separated string, empty input means zero.",
            Mode::Automatic,
        );
        assert!(ask.is_ask());
        assert_eq!(
            ask.title,
            "Sum numbers from a comma separated string, empty input means zero"
        );
        assert!(ask.story.starts_with("As a requester, I want"));
        assert_eq!(ask.acceptance_criteria.len(), 1);
        let criterion = ask.acceptance_criteria[0].to_lowercase();
        assert!(
            criterion.contains("given") && criterion.contains("when") && criterion.contains("then")
        );
        assert_eq!(ask.status, "pending");
        assert!(!ask.decomposition.unwrap().ready);
    }

    #[test]
    fn a_long_request_is_cut_to_a_title_at_a_word_boundary() {
        let title = title_from_request(
            "when the delimiter line starts with two slashes the character after them is \
             the delimiter for the rest of the input and nothing else changes",
        );
        assert!(title.len() <= 72, "{title}");
        assert!(!title.ends_with(' '));
        assert!(title.starts_with("When the delimiter"));
    }

    #[test]
    fn an_enumerated_request_is_titled_by_its_first_item_not_its_number() {
        assert_eq!(
            title_from_request("1. sum comma separated numbers; 2. empty input returns zero"),
            "Sum comma separated numbers"
        );
        assert_eq!(
            title_from_request("- sum numbers\n- empty means zero"),
            "Sum numbers"
        );
        assert_eq!(title_from_request("2) halve the total"), "Halve the total");
    }

    #[test]
    fn ask_ids_number_under_their_own_prefix() {
        assert_eq!(next_ask_id(&[]), "ASK-001");
        let existing = vec![child("REQ-007", "ASK-002", None), {
            let mut ask = new_ask("ASK-002".into(), "x", Mode::Automatic);
            ask.id = "ASK-002".into();
            ask
        }];
        assert_eq!(next_ask_id(&existing), "ASK-003");
    }

    #[test]
    fn readiness_needs_every_condition() {
        let mut ask = complete_ask(Mode::Automatic);
        let spec = spec_of(vec![child(
            "REQ-001",
            "ASK-001",
            Some(AcceptedBy::Automatic),
        )]);
        let settled = settle(&mut ask, &spec);
        assert!(settled.ready, "{:?}", settled.reasons);
        assert_eq!(ask.derived, vec!["REQ-001"]);
        // One criterion per derived requirement, plus the refusal that
        // holds for every ask - and the wording review has nothing to
        // say about any of them.
        assert_eq!(ask.acceptance_criteria.len(), 2);
        assert!(ask.acceptance_criteria[0].contains("REQ-001"));
        assert!(ask.acceptance_criteria[1].contains("still pending"));
        assert_eq!(
            crate::domain::refiner::RequirementRefiner.review(&ask),
            Vec::<String>::new()
        );

        let mut open = ask.clone();
        open.decomposition.as_mut().unwrap().state = DecompositionState::Open;
        assert!(
            readiness_reasons(&open, &spec)
                .iter()
                .any(|r| r.contains("still open"))
        );

        let mut questioned = ask.clone();
        add_questions(&mut questioned, vec![("Which delimiter?".into(), true)]);
        assert!(
            readiness_reasons(&questioned, &spec)
                .iter()
                .any(|r| r.contains("blocking"))
        );
        questioned.questions[0].answer = Some("comma".into());
        assert!(readiness(&questioned, &spec).ready);

        let mut pending = ask.clone();
        reconcile(
            &mut pending,
            vec![proposal_from(
                &child("REQ-009", "ASK-001", None),
                0,
                ProposalStatus::Pending,
            )],
        );
        assert!(
            readiness_reasons(&pending, &spec)
                .iter()
                .any(|r| r.contains("still pending"))
        );
        pending.proposals[0].status = ProposalStatus::Rejected;
        assert!(
            readiness_reasons(&pending, &spec)
                .iter()
                .any(|r| r.contains("rejected"))
        );
        pending.proposals[0].status = ProposalStatus::Excluded;
        assert!(readiness(&pending, &spec).ready);
    }

    fn readiness_reasons(ask: &Requirement, spec: &Spec) -> Vec<String> {
        let readiness = readiness(ask, spec);
        assert!(!readiness.ready);
        readiness.reasons
    }

    #[test]
    fn interactive_readiness_needs_the_users_vote_and_confirmation() {
        let mut ask = complete_ask(Mode::Interactive);
        let automatic = spec_of(vec![child(
            "REQ-001",
            "ASK-001",
            Some(AcceptedBy::Automatic),
        )]);
        settle(&mut ask, &automatic);
        let reasons = readiness_reasons(&ask, &automatic);
        assert!(reasons.iter().any(|r| r.contains("accepted automatically")));
        assert!(reasons.iter().any(|r| r.contains("not confirmed")));

        let user = spec_of(vec![child("REQ-001", "ASK-001", Some(AcceptedBy::User))]);
        ask.decomposition.as_mut().unwrap().confirmed = true;
        assert!(settle(&mut ask, &user).ready);
    }

    #[test]
    fn a_reworded_child_loses_its_acceptance_and_its_parents_readiness() {
        let mut ask = complete_ask(Mode::Automatic);
        let mut reworded = child("REQ-001", "ASK-001", Some(AcceptedBy::Automatic));
        ask.proposals
            .push(proposal_from(&reworded, 1, ProposalStatus::Accepted));
        assert!(settle(&mut ask, &spec_of(vec![reworded.clone()])).ready);

        // Whitespace is not meaning.
        reworded.title = format!("  {}  ", reworded.title);
        assert!(!invalidate(&mut ask, &mut reworded));
        assert!(reworded.acceptance.is_some());

        reworded
            .acceptance_criteria
            .push("Given a, when b, then 4".into());
        assert!(invalidate(&mut ask, &mut reworded));
        assert!(reworded.acceptance.is_none());
        assert_eq!(ask.proposals[0].status, ProposalStatus::Pending);
        assert_eq!(ask.proposals[0].acceptance_criteria.len(), 2);
        let readiness = settle(&mut ask, &spec_of(vec![reworded]));
        assert!(!readiness.ready);
        assert!(
            readiness
                .reasons
                .iter()
                .any(|r| r.contains("REQ-001 is not accepted"))
        );
        assert!(
            readiness
                .reasons
                .iter()
                .any(|r| r.contains("still pending"))
        );

        // A wording edited behind the record - the file changed by hand -
        // is caught the same way, by the fingerprint.
        let mut edited = child("REQ-002", "ASK-001", Some(AcceptedBy::Automatic));
        edited.title = "Edited by hand".into();
        let reasons = readiness_reasons(&complete_ask(Mode::Automatic), &spec_of(vec![edited]));
        assert!(reasons.iter().any(|r| r.contains("no derived requirement")));
        let mut ask = complete_ask(Mode::Automatic);
        ask.derived = vec!["REQ-002".into()];
        let mut edited = child("REQ-002", "ASK-001", Some(AcceptedBy::Automatic));
        edited.title = "Edited by hand".into();
        let reasons = readiness_reasons(&ask, &spec_of(vec![edited]));
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("reworded since it was accepted"))
        );
    }

    #[test]
    fn reconciling_the_same_wording_twice_adds_nothing() {
        let mut ask = new_ask("ASK-001".into(), "x", Mode::Automatic);
        let proposal = proposal_from(
            &child("REQ-001", "ASK-001", None),
            0,
            ProposalStatus::Pending,
        );
        assert_eq!(reconcile(&mut ask, vec![proposal.clone()]), vec![1]);
        assert!(reconcile(&mut ask, vec![proposal]).is_empty());
        assert_eq!(ask.proposals.len(), 1);
        assert_eq!(
            add_questions(
                &mut ask,
                vec![("Why?".into(), true), ("why?".into(), false)]
            ),
            1
        );
    }

    #[test]
    fn a_changed_request_reopens_the_decomposition_and_the_same_one_does_not() {
        let mut ask = complete_ask(Mode::Interactive);
        ask.decomposition.as_mut().unwrap().confirmed = true;
        assert!(!reopen(&mut ask, " sum numbers; empty is zero "));
        assert!(reopen(&mut ask, "sum numbers; empty is zero; newlines too"));
        let decomposition = ask.decomposition.unwrap();
        assert_eq!(decomposition.state, DecompositionState::Open);
        assert!(!decomposition.confirmed);
    }

    #[test]
    fn the_review_catches_duplicates_and_conflicts_and_advises_on_alignment() {
        let existing = child("REQ-001", "ASK-000", None);
        let spec = spec_of(vec![existing.clone()]);
        let request = "sum numbers from a string";
        let same_title = Proposal {
            number: 1,
            title: existing.title.to_uppercase(),
            story: "As a user, I want sums so that totals arrive.".into(),
            acceptance_criteria: vec![
                "Given \"1,2\", when add is called, then the result is 3".into(),
            ],
            fingerprint: "a".into(),
            status: ProposalStatus::Pending,
            requirement: None,
            reason: None,
            findings: vec![],
        };
        let review = review_proposal(&same_title, request, &spec, None, &[]);
        assert!(review.blocking[0].contains("duplicates REQ-001: the same title"));

        let mut shared = same_title.clone();
        shared.title = "Comma sums".into();
        shared.acceptance_criteria = existing.acceptance_criteria.clone();
        let review = review_proposal(&shared, request, &spec, None, &[]);
        assert!(review.blocking[0].contains("shares the criterion"));

        let mut sibling = same_title.clone();
        sibling.number = 2;
        sibling.title = "Comma sums".into();
        sibling.fingerprint = "b".into();
        let mut conflict = same_title.clone();
        conflict.title = "comma sums".into();
        conflict.fingerprint = "c".into();
        let review = review_proposal(&conflict, request, &spec, None, &[sibling.clone()]);
        assert!(
            review
                .blocking
                .iter()
                .any(|f| f.contains("conflicts with proposal 2"))
        );
        sibling.fingerprint = "c".into();
        let review = review_proposal(&conflict, request, &spec, None, &[sibling]);
        assert!(
            review
                .blocking
                .iter()
                .any(|f| f.contains("duplicates proposal 2"))
        );

        let mut aligned = same_title.clone();
        aligned.title = "Numbers are summed".into();
        aligned.acceptance_criteria =
            vec!["Given \"1,2\", when add is called, then the result is 3".into()];
        let review = review_proposal(&aligned, request, &spec, None, &[]);
        assert!(review.blocking.is_empty(), "{review:?}");
        assert!(review.advisory.is_empty(), "{review:?}");

        let mut elsewhere = aligned.clone();
        elsewhere.title = "Export to PDF".into();
        elsewhere.story = "As a user, I want export so that reports print.".into();
        elsewhere.acceptance_criteria =
            vec!["Given a report, when export is called, then a PDF exists".into()];
        let review = review_proposal(&elsewhere, request, &spec, None, &[]);
        assert!(review.advisory[0].starts_with("alignment:"));
        assert!(
            auto_acceptable(&[], &review),
            "alignment advises, it does not block"
        );
        assert!(!auto_acceptable(&["x: title is missing".into()], &review));
    }

    #[test]
    fn only_an_explicit_enumeration_splits_without_a_model() {
        assert!(split_enumerated("sum the numbers in a string").is_empty());
        let parts = split_enumerated("sum numbers; empty input is zero; newlines split too.");
        let titles: Vec<&str> = parts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Sum numbers", "Empty input is zero", "Newlines split too"]
        );
        assert!(parts[0].findings[0].contains("user story"));
        let listed = split_enumerated(
            "Please build:\n1. add numbers\n2) subtract numbers\n- divide numbers\nthanks",
        );
        let titles: Vec<&str> = listed.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Add numbers", "Subtract numbers", "Divide numbers"]
        );
    }
}

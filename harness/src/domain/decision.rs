//! The decision plane: bounded questions put to a local decision model,
//! the typed answers that come back, and the policy the harness applies
//! to them. Pure — the HTTP call lives in
//! [`crate::adapters::ollama_decision`].
//!
//! The division this module exists to keep: the model supplies a verdict
//! about one bounded question, and the harness owns what happens next.
//! A decision model gets to say the work looks finished; it does not get
//! to mark it implemented. Every consequence in here is deterministic
//! code reading a typed answer, never the model steering the workflow.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Ollama's decision route, added in 0.35. Named once so the adapter and
/// every "upgrade Ollama" hint agree on it.
pub const SYSTEMONE_PATH: &str = "/v1/systemone";

/// The `capabilities` entry `/api/show` reports for a model that can
/// answer [`SYSTEMONE_PATH`]. Discovery keys on the capability rather
/// than a model name, so a new decision model works the day it ships and
/// no name is baked into the harness.
pub const DECISION_CAPABILITY: &str = "decision";

/// The `capabilities` entry a model that can hold a chat conversation
/// reports. Generative work needs this one.
pub const COMPLETION_CAPABILITY: &str = "completion";

/// How long Ollama keeps a decision model resident. Decisions are single
/// forward passes, so the load cost dominates a one-off call; a short
/// residency makes a second judgment in the same session effectively
/// free without pinning 9GB for the rest of the day.
pub const KEEP_ALIVE: &str = "5m";

/// Hard ceiling on a request body without images, enforced by the server
/// with a 413. Checked before sending so an oversize brief is a local
/// error naming the budget rather than a wasted round trip.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// The documented bounds on one request's question set.
pub const MAX_QUESTIONS: usize = 64;
/// The documented bounds on `choice` options and `score` levels.
pub const MAX_CRITERIA: usize = 26;
/// Both `choice` and `score` need something to choose between.
pub const MIN_CRITERIA: usize = 2;

/// The default threshold a judgment must clear to count as a verdict.
///
/// Deliberately not borrowed from a benchmark. `0.80` is what the
/// threshold sweep in `tests/decision_live.rs` picked out against this
/// repository's own labeled criteria: it keeps 14 of the 15 vague ones
/// flagged and waves none through, while cutting confidently-wrong
/// flags from three to one by moving the near-misses into the dead band
/// where nothing reads them.
///
/// The two directions are not weighed alike, which is why the wider
/// band wins. Flagging wording that was fine teaches people to ignore
/// judgments; staying quiet costs nothing, because the deterministic
/// rules are still doing the job they did before. It is a policy input,
/// and a project is expected to re-measure it against its own wording.
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.80;

/// Whether judgments run at all, and whether they may refuse work.
///
/// `Advisory` is the default because a probability is not a gate until
/// somebody has measured it on their own data. `Enforce` exists so a
/// project that *has* measured can act on the answer, and its failure
/// behaviour is spelled out in [`Policy::judge`]: a request that did not
/// produce an answer refuses, it never approves.
///
/// Serialized lower case, unlike [`Verdict`] and [`Transition`] beside
/// it: a mode is a setting that round-trips with the `mode = "advisory"`
/// line in `.spec/config.toml`, while those two name states in the
/// workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// No questions are asked, nothing is reported.
    Off,
    /// Questions are asked and the answer is reported, and it changes no
    /// deterministic finding and blocks nothing.
    #[default]
    Advisory,
    /// The answer may refuse work. Still cannot approve it.
    Enforce,
}

impl Mode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "advisory" => Some(Self::Advisory),
            "enforce" => Some(Self::Enforce),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Advisory => "advisory",
            Self::Enforce => "enforce",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Optional wording for a `noul` question's two outcomes. Omitted
/// entries are `No` and `Yes` at the server; naming them is how a
/// question states its criteria explicitly instead of leaving the model
/// to guess what "yes" would mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcomes {
    #[serde(rename = "false")]
    pub when_false: String,
    #[serde(rename = "true")]
    pub when_true: String,
}

/// One bounded question. The three shapes Ollama documents, and nothing
/// else: a question the harness cannot name a type for is a question it
/// cannot read an answer to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// A probability of true. Note there is no confidence field on the
    /// answer — see [`Policy::verdict_for`].
    Noul {
        instructions: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<Outcomes>,
    },
    /// One option key out of a declared set, with a probability each.
    Choice {
        instructions: String,
        criteria: BTreeMap<String, Option<String>>,
    },
    /// A probability-weighted average over ordered levels.
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

impl Question {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }

    fn instructions(&self) -> &str {
        match self {
            Self::Noul { instructions, .. }
            | Self::Choice { instructions, .. }
            | Self::Score { instructions, .. } => instructions,
        }
    }

    /// Why this question would be rejected, in the server's own terms,
    /// checked here so a malformed question never costs a round trip.
    fn fault(&self, name: &str) -> Option<String> {
        if self.instructions().trim().is_empty() {
            return Some(format!(
                "question {name:?}: instructions must be a nonempty string"
            ));
        }
        match self {
            Self::Noul { .. } => None,
            Self::Choice { criteria, .. } => {
                if !(MIN_CRITERIA..=MAX_CRITERIA).contains(&criteria.len()) {
                    return Some(format!(
                        "question {name:?}: choice criteria must map {MIN_CRITERIA}\u{2013}{MAX_CRITERIA} option keys to descriptions"
                    ));
                }
                criteria
                    .keys()
                    .any(|key| key.trim().is_empty())
                    .then(|| format!("question {name:?}: choice option keys must not be blank"))
            }
            Self::Score { criteria, .. } => (!(MIN_CRITERIA..=MAX_CRITERIA)
                .contains(&criteria.len()))
            .then(|| {
                format!(
                    "question {name:?}: score criteria must be an array of {MIN_CRITERIA}\u{2013}{MAX_CRITERIA} descriptions"
                )
            }),
        }
    }
}

/// One typed answer. Deserialized from the server's reply, so the shape
/// is the server's: `noul` carries a probability and no confidence,
/// while `choice` and `score` carry a distribution and a concentration.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

impl Answer {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }

    /// The answer in one line, for a human reading a judgment.
    pub fn summary(&self) -> String {
        match self {
            Self::Noul { noul } => format!("probability of true {noul:.3}"),
            Self::Choice {
                choice, confidence, ..
            } => format!("{choice} (confidence {confidence:.3})"),
            Self::Score {
                score,
                legend,
                confidence,
                ..
            } => {
                let level = legend
                    .get(&format!("{}", score.round() as i64))
                    .map(String::as_str)
                    .unwrap_or("-");
                format!("{score:.3} nearest {level} (confidence {confidence:.3})")
            }
        }
    }

    /// Ollama's `confidence`, where the type has one.
    ///
    /// It is `1 - H(p)/ln(N)`: how concentrated the distribution is, and
    /// explicitly *not* calibrated correctness. `noul` has no such
    /// field, which is why [`Policy::verdict_for`] reads a dead band
    /// around the probability for booleans instead.
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Noul { .. } => None,
            Self::Choice { confidence, .. } | Self::Score { confidence, .. } => Some(*confidence),
        }
    }
}

/// The state a question is asked about, and the questions themselves.
///
/// `state` is a curated brief, not the repository: the whole rendered
/// prompt has to fit the model's loaded context (8,192 tokens on the
/// shipped decision models), and every question is scored against the
/// full state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub state: serde_json::Value,
    pub questions: BTreeMap<String, Question>,
}

impl Request {
    /// One question about one brief, the shape every judgment in the
    /// harness currently uses.
    pub fn single(name: &str, question: Question, state: serde_json::Value) -> Self {
        Self {
            state,
            questions: BTreeMap::from([(name.to_string(), question)]),
        }
    }

    /// Why the server would reject this request, checked locally first.
    ///
    /// Not belt-and-braces: a 400 from Ollama names the question but not
    /// the project's wording, and these faults are all things the caller
    /// built wrong rather than things the model decided.
    pub fn fault(&self) -> Option<String> {
        if !matches!(
            self.state,
            serde_json::Value::String(_)
                | serde_json::Value::Object(_)
                | serde_json::Value::Array(_)
        ) {
            return Some("state: must be a string, object, or array".into());
        }
        if matches!(&self.state, serde_json::Value::String(text) if text.trim().is_empty()) {
            return Some("state: must be a nonempty string".into());
        }
        if !(1..=MAX_QUESTIONS).contains(&self.questions.len()) {
            return Some(format!(
                "questions must contain 1\u{2013}{MAX_QUESTIONS} fields"
            ));
        }
        if let Some(name) = self.questions.keys().find(|name| name.trim().is_empty()) {
            return Some(format!("question name {name:?} must not be blank"));
        }
        self.questions
            .iter()
            .find_map(|(name, question)| question.fault(name))
    }

    /// A stable digest of the brief that was actually sent, so a recorded
    /// judgment can be tied back to its input without copying the whole
    /// brief into every log line.
    pub fn state_digest(&self) -> String {
        let rendered = self.state.to_string();
        let mut hasher = Sha256::new();
        hasher.update(rendered.as_bytes());
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("sha256:{digest}")
    }
}

/// Token accounting the server reports. `output_tokens` counts internal
/// scoring work, not the length of the JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// One decision round trip's reply: the model that answered, the answers
/// keyed by the names that were asked, and what it cost.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Outcome {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Usage,
}

/// What the model said about the bounded question.
///
/// Serialized in the same screaming register as the gate name and the
/// transition beside it, following `TddPhase`: a word naming a state
/// reads the same in a log line, a JSON reply, and the prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    /// The answer cleared the threshold in favour of the property.
    Holds,
    /// The answer cleared the threshold against the property.
    Fails,
    /// The answer landed in the dead band, or two answers disagreed.
    /// Not a verdict, and never silently read as either one.
    Inconclusive,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Holds => "HOLDS",
            Self::Fails => "FAILS",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the harness does about the verdict. The four transitions the
/// workflow owns; the model never picks one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Transition {
    /// Carry on. The judgment is reported and nothing is blocked.
    Continue,
    /// The work needs another pass before it moves on.
    Rework,
    /// A human has to look at this one.
    Escalate,
    /// Stop here.
    Stop,
}

impl Transition {
    /// The screaming-snake name the workflow document and the article
    /// both use, so a reply, a log line, and the prose agree.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "CONTINUE",
            Self::Rework => "REWORK",
            Self::Escalate => "ESCALATE",
            Self::Stop => "STOP",
        }
    }
}

impl fmt::Display for Transition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the brief came from, recorded alongside the answer so a
/// judgment can be audited without rerunning it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    /// What was summarized, in project terms, e.g.
    /// `REQ-007 acceptance criterion 2`.
    pub input: String,
    /// Digest of the exact brief that was sent.
    pub state: String,
    pub state_bytes: usize,
}

/// One recorded judgment: the question that was asked, what answered it,
/// what the answer was, and what the harness did about it.
///
/// This is the audit record the workflow promises. It is deliberately
/// verbose about provenance: a judgment nobody can tie back to a model
/// version, a question version, and an input is not evidence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Judgment {
    /// The gate this judgment informs, e.g. `CRITERION_MEASURABLE`.
    pub gate: &'static str,
    /// The question set and its version, e.g. `measurable/v1`. Bumped
    /// whenever the wording changes, because a threshold measured
    /// against one wording says nothing about another.
    pub question: &'static str,
    /// The exact model tag the server echoed back, not the tag that was
    /// requested: `nimble` and `nimble:latest` are the same request and
    /// a different record.
    pub model: String,
    pub verdict: Verdict,
    pub action: Transition,
    pub mode: Mode,
    /// The threshold this verdict was read against.
    pub threshold: f64,
    pub answer: Answer,
    pub provenance: Provenance,
    pub usage: Usage,
}

/// The deterministic rules that turn a typed answer into a verdict and a
/// transition. The whole point of the module: policy the owner can read,
/// test, and change, rather than prose from a model implicitly steering
/// the loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    pub mode: Mode,
    pub threshold: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            threshold: DEFAULT_MIN_CONFIDENCE,
        }
    }
}

impl Policy {
    pub fn new(mode: Mode, threshold: f64) -> Self {
        Self { mode, threshold }
    }

    /// Whether a question should be asked at all.
    pub fn asks(&self) -> bool {
        self.mode != Mode::Off
    }

    /// The verdict a single answer earns.
    ///
    /// `noul` answers have no confidence field — only a probability of
    /// true — so a boolean gets a symmetric dead band: true above the
    /// threshold, false below its mirror, and no verdict in between.
    /// `choice` and `score` carry Ollama's concentration figure, which
    /// has to clear the threshold before the top option counts as an
    /// answer at all.
    pub fn verdict_for(&self, answer: &Answer) -> Verdict {
        match answer {
            Answer::Noul { noul } => {
                if *noul >= self.threshold {
                    Verdict::Holds
                } else if *noul <= 1.0 - self.threshold {
                    Verdict::Fails
                } else {
                    Verdict::Inconclusive
                }
            }
            Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => {
                if *confidence >= self.threshold {
                    Verdict::Holds
                } else {
                    Verdict::Inconclusive
                }
            }
        }
    }

    /// The transition a verdict earns.
    ///
    /// In `Advisory` the answer is recorded and the workflow carries on,
    /// whatever it said. In `Enforce` a verdict against the property is
    /// rework and an inconclusive one is a human's problem — the two
    /// directions a judgment is allowed to push. Neither mode has a
    /// branch that turns a judgment into approval of anything: the
    /// deterministic gates upstream still have to pass on their own.
    pub fn transition_for(&self, verdict: Verdict) -> Transition {
        match (self.mode, verdict) {
            (Mode::Off | Mode::Advisory, _) => Transition::Continue,
            (Mode::Enforce, Verdict::Holds) => Transition::Continue,
            (Mode::Enforce, Verdict::Fails) => Transition::Rework,
            (Mode::Enforce, Verdict::Inconclusive) => Transition::Escalate,
        }
    }

    /// One answer read into a recorded judgment.
    pub fn judge(
        &self,
        gate: &'static str,
        question: &'static str,
        model: &str,
        answer: Answer,
        provenance: Provenance,
        usage: Usage,
    ) -> Judgment {
        let verdict = self.verdict_for(&answer);
        Judgment {
            gate,
            question,
            model: model.to_string(),
            verdict,
            action: self.transition_for(verdict),
            mode: self.mode,
            threshold: self.threshold,
            answer,
            provenance,
            usage,
        }
    }

    /// The verdict for a set of answers that all have to agree.
    ///
    /// Questions are scored independently — the server does not show one
    /// answer to the next — so an agreement check is the harness's job
    /// and nowhere else's. Any disagreement is inconclusive rather than
    /// a majority vote: two gauges reading differently means the part
    /// goes to a person.
    pub fn agreed(&self, answers: &[Answer]) -> Verdict {
        let mut verdicts = answers.iter().map(|answer| self.verdict_for(answer));
        match verdicts.next() {
            None => Verdict::Inconclusive,
            Some(first) => {
                if verdicts.all(|verdict| verdict == first) {
                    first
                } else {
                    Verdict::Inconclusive
                }
            }
        }
    }
}

/// The gate name for "is this acceptance criterion measurable".
pub const CRITERION_MEASURABLE: &str = "CRITERION_MEASURABLE";

/// The question set and version behind [`measurable_question`]. Any
/// change to the wording below is a new version, because a threshold
/// calibrated against one phrasing is not evidence about another.
pub const MEASURABLE_QUESTION: &str = "measurable/v1";

/// The name the answer comes back under.
pub const MEASURABLE_ANSWER: &str = "measurable";

/// One bounded question: can this acceptance criterion be checked by a
/// test with a single unambiguous result?
///
/// This is the gap the regex refiner cannot close.
/// `RequirementRefiner` asks whether the outcome clause *looks*
/// concrete: does it contain a number, a quoted literal, a named error,
/// or a known sentinel? That is satisfied by a number appearing
/// anywhere, so
///
/// > then code quality is improved by at least 20%
///
/// earns no finding at all, while being unmeasurable — nobody measured
/// code quality. The rule asks whether a number is present. This asks
/// whether the number *is* the assertion.
///
/// Both outcomes are described rather than left as Yes/No, so the model
/// is choosing between two stated criteria instead of guessing what the
/// question meant.
///
/// The wording is narrow for a measured reason. An earlier phrasing
/// asked the open question — "could a test check this criterion" — and
/// scored well on plain vagueness while passing every adversarial case
/// in `tests/decision_live.rs`: criteria with the right Gherkin shape
/// and technical vocabulary read as measurable at p > 0.9, including
/// "the system achieves 99.9% correctness across all code paths" and a
/// criterion that simply asserted it was measurable. Pointing the
/// question at the clause after `then`, and naming in the false branch
/// the specific dodges that clause uses, took those from 8 misses to
/// none. The lesson is about the question, not the model: an open
/// question invites a judgment of the sentence's style, and style is
/// exactly what convincing-looking wording gets right.
pub fn measurable_question() -> Question {
    Question::Noul {
        instructions: "Read only the text after \"then\" in this acceptance criterion. That \
                       clause is the assertion a test would make. Does it name a specific \
                       value, state, status, or error that could be written into an assert \
                       statement exactly as stated, without anyone first deciding what the \
                       words mean?"
            .to_string(),
        criteria: Some(Outcomes {
            when_false: "No - the clause after \"then\" uses a word whose meaning a reader \
                         has to settle first (correct, proper, relevant, acceptable, \
                         improved, timely, successful, intuitive, as specified, best \
                         practice), or it refers to a document, standard, threshold or test \
                         suite that is not quoted here, or it restates the goal instead of \
                         naming a value. A number in the sentence does not count unless the \
                         assertion itself is that number: a percentage of a quantity nobody \
                         measured, or a placeholder such as an SLO or a limit named but not \
                         given, is still vague."
                .to_string(),
            when_true: "Yes - the clause after \"then\" names a literal value, an exact \
                        status code, a named error type, or an exact relation between stated \
                        inputs, and two engineers reading it would write the same assertion."
                .to_string(),
        }),
    }
}

/// The brief for [`measurable_question`]: the criterion and nothing else.
///
/// The minimum relevant state, on purpose. The question is about this
/// sentence's own wording, so the story, the project, and the rest of
/// the spec are not evidence for it — they are tokens competing for a
/// context the server will not truncate.
pub fn measurable_state(criterion: &str) -> serde_json::Value {
    serde_json::json!({ "acceptance_criterion": criterion })
}

/// The finding wording for a criterion a judgment reads as unmeasurable.
/// Prefixed so a reader can always tell a probabilistic finding from the
/// deterministic ones beside it.
pub fn measurable_finding(criterion: &str, judgment: &Judgment) -> String {
    format!(
        "judgment ({}): criterion {criterion:?}: the outcome may not be measurable - {} says {}",
        judgment.question,
        judgment.model,
        judgment.answer.summary()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noul(probability: f64) -> Answer {
        Answer::Noul { noul: probability }
    }

    fn choice(confidence: f64) -> Answer {
        Answer::Choice {
            choice: "a".into(),
            probabilities: BTreeMap::from([("a".to_string(), 0.9), ("b".to_string(), 0.1)]),
            confidence,
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            input: "REQ-007 acceptance criterion 1".into(),
            state: "sha256:abc".into(),
            state_bytes: 12,
        }
    }

    #[test]
    fn a_mode_round_trips_through_its_configuration_spelling() {
        for mode in [Mode::Off, Mode::Advisory, Mode::Enforce] {
            assert_eq!(Mode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(Mode::parse("  ENFORCE "), Some(Mode::Enforce));
        assert_eq!(Mode::parse("sometimes"), None);
        assert_eq!(Mode::default(), Mode::Advisory);
        assert_eq!(Mode::Advisory.to_string(), "advisory");
    }

    /// A boolean answer has no confidence field, so the threshold has to
    /// be read off the probability itself from both directions.
    #[test]
    fn a_boolean_answer_has_a_dead_band_rather_than_a_confidence() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        assert_eq!(noul(0.5).confidence(), None);
        assert_eq!(policy.verdict_for(&noul(0.97)), Verdict::Holds);
        assert_eq!(policy.verdict_for(&noul(0.70)), Verdict::Holds);
        assert_eq!(policy.verdict_for(&noul(0.08)), Verdict::Fails);
        assert_eq!(policy.verdict_for(&noul(0.30)), Verdict::Fails);
        assert_eq!(policy.verdict_for(&noul(0.55)), Verdict::Inconclusive);
        assert_eq!(policy.verdict_for(&noul(0.45)), Verdict::Inconclusive);
    }

    #[test]
    fn a_choice_answer_must_clear_the_threshold_on_concentration() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        assert_eq!(choice(0.92).confidence(), Some(0.92));
        assert_eq!(policy.verdict_for(&choice(0.92)), Verdict::Holds);
        assert_eq!(policy.verdict_for(&choice(0.04)), Verdict::Inconclusive);
    }

    /// The whole promise of advisory mode: the answer is recorded and
    /// the workflow does not move because of it.
    #[test]
    fn advisory_mode_carries_on_whatever_the_answer_was() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        for verdict in [Verdict::Holds, Verdict::Fails, Verdict::Inconclusive] {
            assert_eq!(policy.transition_for(verdict), Transition::Continue);
        }
    }

    #[test]
    fn enforce_mode_reworks_a_failure_and_escalates_an_inconclusive_answer() {
        let policy = Policy::new(Mode::Enforce, 0.70);
        assert_eq!(policy.transition_for(Verdict::Holds), Transition::Continue);
        assert_eq!(policy.transition_for(Verdict::Fails), Transition::Rework);
        assert_eq!(
            policy.transition_for(Verdict::Inconclusive),
            Transition::Escalate
        );
    }

    #[test]
    fn off_mode_asks_nothing() {
        assert!(!Policy::new(Mode::Off, 0.70).asks());
        assert!(Policy::new(Mode::Advisory, 0.70).asks());
        assert!(Policy::new(Mode::Enforce, 0.70).asks());
    }

    /// Questions are scored independently, so agreement is the harness's
    /// job. Two gauges disagreeing sends the part to a person rather than
    /// to a majority vote.
    #[test]
    fn answers_that_disagree_are_inconclusive_rather_than_voted_on() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        assert_eq!(policy.agreed(&[noul(0.97), noul(0.95)]), Verdict::Holds);
        assert_eq!(policy.agreed(&[noul(0.02), noul(0.05)]), Verdict::Fails);
        assert_eq!(
            policy.agreed(&[noul(0.97), noul(0.02)]),
            Verdict::Inconclusive
        );
        assert_eq!(policy.agreed(&[]), Verdict::Inconclusive);
    }

    #[test]
    fn a_judgment_records_the_model_question_threshold_and_provenance() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        let judgment = policy.judge(
            CRITERION_MEASURABLE,
            MEASURABLE_QUESTION,
            "nimble:latest",
            noul(0.04),
            provenance(),
            Usage {
                input_tokens: 151,
                output_tokens: 1,
            },
        );
        assert_eq!(judgment.gate, "CRITERION_MEASURABLE");
        assert_eq!(judgment.question, "measurable/v1");
        assert_eq!(judgment.model, "nimble:latest");
        assert_eq!(judgment.verdict, Verdict::Fails);
        assert_eq!(judgment.action, Transition::Continue);
        assert_eq!(judgment.threshold, 0.70);
        assert_eq!(judgment.provenance.input, "REQ-007 acceptance criterion 1");
        assert_eq!(judgment.usage.input_tokens, 151);
    }

    #[test]
    fn a_transition_renders_as_the_name_the_workflow_document_uses() {
        assert_eq!(Transition::Continue.to_string(), "CONTINUE");
        assert_eq!(Transition::Rework.to_string(), "REWORK");
        assert_eq!(Transition::Escalate.to_string(), "ESCALATE");
        assert_eq!(Transition::Stop.to_string(), "STOP");
        assert_eq!(Verdict::Holds.to_string(), "HOLDS");
        assert_eq!(Verdict::Inconclusive.to_string(), "INCONCLUSIVE");
    }

    /// A printed judgment and its JSON have to use the same words, or
    /// the manual ends up documenting one of the two.
    #[test]
    fn a_judgment_serializes_the_same_words_it_prints() {
        let judgment = Policy::new(Mode::Enforce, 0.70).judge(
            "REQ-007",
            MEASURABLE_QUESTION,
            "nimble:latest",
            Answer::Noul { noul: 0.02 },
            Provenance {
                input: "REQ-007".into(),
                state: "sha256:0".into(),
                state_bytes: 1,
            },
            Usage::default(),
        );
        let json = serde_json::to_value(&judgment).unwrap();
        assert_eq!(json["verdict"], judgment.verdict.to_string());
        assert_eq!(json["action"], judgment.action.to_string());
        assert_eq!(json["verdict"], "FAILS");
        assert_eq!(json["action"], "REWORK");
    }

    #[test]
    fn the_measurable_question_serializes_as_a_noul_with_both_outcomes_described() {
        let request = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            measurable_state("Given \"1,2\", when add is called, then the result is 3"),
        );
        assert_eq!(request.fault(), None);
        let json = serde_json::to_value(&request.questions).unwrap();
        let question = &json["measurable"];
        assert_eq!(question["type"], "noul");
        // The question has to point at the clause after `then` rather
        // than ask about the sentence as a whole. The open phrasing
        // passed every adversarial case in the live evaluation, so this
        // asserts the narrowing is still there.
        let instructions = question["instructions"].as_str().unwrap();
        assert!(instructions.contains("after \"then\""), "{instructions}");
        assert!(instructions.contains("assert statement"), "{instructions}");
        assert!(
            question["criteria"]["true"]
                .as_str()
                .unwrap()
                .contains("literal value")
        );
        assert!(
            question["criteria"]["false"]
                .as_str()
                .unwrap()
                .contains("vague")
        );
    }

    #[test]
    fn the_measurable_brief_carries_the_criterion_and_nothing_else() {
        let state = measurable_state("Given a, when b, then 3");
        assert_eq!(
            state,
            serde_json::json!({"acceptance_criterion": "Given a, when b, then 3"})
        );
        assert_eq!(state.as_object().unwrap().len(), 1);
    }

    #[test]
    fn a_state_digest_is_stable_for_the_same_brief_and_differs_for_another() {
        let one = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            measurable_state("a"),
        );
        let same = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            measurable_state("a"),
        );
        let other = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            measurable_state("b"),
        );
        assert_eq!(one.state_digest(), same.state_digest());
        assert_ne!(one.state_digest(), other.state_digest());
        assert!(one.state_digest().starts_with("sha256:"));
    }

    #[test]
    fn an_empty_question_set_is_refused_before_it_is_sent() {
        let request = Request {
            state: serde_json::json!("x"),
            questions: BTreeMap::new(),
        };
        assert_eq!(
            request.fault(),
            Some("questions must contain 1\u{2013}64 fields".into())
        );
    }

    #[test]
    fn more_than_sixty_four_questions_is_refused() {
        let questions = (0..=MAX_QUESTIONS)
            .map(|n| (format!("q{n}"), measurable_question()))
            .collect();
        let request = Request {
            state: serde_json::json!("x"),
            questions,
        };
        assert!(request.fault().unwrap().contains("1\u{2013}64 fields"));
    }

    #[test]
    fn a_state_that_is_not_a_string_object_or_array_is_refused() {
        for value in [
            serde_json::json!(7),
            serde_json::json!(true),
            serde_json::json!(null),
        ] {
            let request = Request::single(MEASURABLE_ANSWER, measurable_question(), value);
            assert_eq!(
                request.fault(),
                Some("state: must be a string, object, or array".into())
            );
        }
    }

    #[test]
    fn a_blank_state_string_is_refused() {
        let request = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            serde_json::json!("  "),
        );
        assert_eq!(
            request.fault(),
            Some("state: must be a nonempty string".into())
        );
    }

    #[test]
    fn blank_instructions_are_refused_with_the_question_named() {
        let request = Request::single(
            "measurable",
            Question::Noul {
                instructions: "   ".into(),
                criteria: None,
            },
            serde_json::json!("x"),
        );
        assert_eq!(
            request.fault(),
            Some("question \"measurable\": instructions must be a nonempty string".into())
        );
    }

    #[test]
    fn a_blank_question_name_is_refused() {
        let request = Request {
            state: serde_json::json!("x"),
            questions: BTreeMap::from([(" ".to_string(), measurable_question())]),
        };
        assert_eq!(
            request.fault(),
            Some("question name \" \" must not be blank".into())
        );
    }

    #[test]
    fn a_choice_needs_between_two_and_twenty_six_options() {
        let one = Question::Choice {
            instructions: "pick".into(),
            criteria: BTreeMap::from([("only".to_string(), None)]),
        };
        assert!(
            one.fault("route")
                .unwrap()
                .contains("choice criteria must map 2\u{2013}26 option keys")
        );
        let many = Question::Choice {
            instructions: "pick".into(),
            criteria: (0..=MAX_CRITERIA)
                .map(|n| (format!("o{n}"), None))
                .collect(),
        };
        assert!(many.fault("route").is_some());
        let blank = Question::Choice {
            instructions: "pick".into(),
            criteria: BTreeMap::from([(" ".to_string(), None), ("b".to_string(), None)]),
        };
        assert_eq!(
            blank.fault("route"),
            Some("question \"route\": choice option keys must not be blank".into())
        );
        let fine = Question::Choice {
            instructions: "pick".into(),
            criteria: BTreeMap::from([("a".to_string(), None), ("b".to_string(), None)]),
        };
        assert_eq!(fine.fault("route"), None);
    }

    #[test]
    fn a_score_needs_between_two_and_twenty_six_levels() {
        let thin = Question::Score {
            instructions: "rate".into(),
            criteria: vec!["only".into()],
        };
        assert!(
            thin.fault("risk")
                .unwrap()
                .contains("score criteria must be an array of 2\u{2013}26 descriptions")
        );
        let fat = Question::Score {
            instructions: "rate".into(),
            criteria: (0..=MAX_CRITERIA).map(|n| format!("l{n}")).collect(),
        };
        assert!(fat.fault("risk").is_some());
        let fine = Question::Score {
            instructions: "rate".into(),
            criteria: vec!["low".into(), "high".into()],
        };
        assert_eq!(fine.fault("risk"), None);
    }

    #[test]
    fn every_question_type_reports_its_wire_name() {
        assert_eq!(measurable_question().type_name(), "noul");
        assert_eq!(
            Question::Choice {
                instructions: "x".into(),
                criteria: BTreeMap::from([("a".to_string(), None), ("b".to_string(), None)]),
            }
            .type_name(),
            "choice"
        );
        assert_eq!(
            Question::Score {
                instructions: "x".into(),
                criteria: vec!["a".into(), "b".into()],
            }
            .type_name(),
            "score"
        );
    }

    #[test]
    fn an_answer_summarizes_itself_for_a_human_reading_the_judgment() {
        assert_eq!(noul(0.9970).summary(), "probability of true 0.997");
        assert_eq!(choice(0.922).summary(), "a (confidence 0.922)");
        let score = Answer::Score {
            score: 0.815,
            legend: BTreeMap::from([
                ("0".to_string(), "Routine".to_string()),
                ("1".to_string(), "Soon".to_string()),
                ("2".to_string(), "Urgent".to_string()),
            ]),
            probabilities: BTreeMap::from([("0".to_string(), 0.38), ("1".to_string(), 0.43)]),
            confidence: 0.046,
        };
        assert_eq!(score.summary(), "0.815 nearest Soon (confidence 0.046)");
        assert_eq!(noul(0.5).type_name(), "noul");
        assert_eq!(score.type_name(), "score");
    }

    #[test]
    fn a_score_whose_legend_is_missing_the_level_still_summarizes() {
        let score = Answer::Score {
            score: 9.0,
            legend: BTreeMap::new(),
            probabilities: BTreeMap::new(),
            confidence: 0.5,
        };
        assert!(score.summary().contains("nearest -"));
    }

    #[test]
    fn a_measurable_finding_names_the_question_version_and_the_model() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        let judgment = policy.judge(
            CRITERION_MEASURABLE,
            MEASURABLE_QUESTION,
            "nimble:latest",
            noul(0.04),
            provenance(),
            Usage::default(),
        );
        let finding = measurable_finding("Given a, when b, then it works", &judgment);
        assert!(finding.starts_with("judgment (measurable/v1):"));
        assert!(finding.contains("Given a, when b, then it works"));
        assert!(finding.contains("nimble:latest"));
        assert!(finding.contains("probability of true 0.040"));
    }

    #[test]
    fn an_outcome_deserializes_the_documented_reply_for_every_answer_type() {
        let body = r#"{
            "model": "nimble",
            "answers": {
                "team": {"type":"choice","choice":"billing",
                         "probabilities":{"billing":0.985,"technical":0.012},
                         "confidence":0.922},
                "refund": {"type":"noul","noul":0.997},
                "urgency": {"type":"score","score":0.815,
                            "legend":{"0":"Routine","1":"Soon"},
                            "probabilities":{"0":0.378,"1":0.429},
                            "confidence":0.046}
            },
            "usage": {"input_tokens": 841, "output_tokens": 4}
        }"#;
        let outcome: Outcome = serde_json::from_str(body).unwrap();
        assert_eq!(outcome.model, "nimble");
        assert_eq!(outcome.usage.input_tokens, 841);
        assert_eq!(outcome.answers["refund"], Answer::Noul { noul: 0.997 });
        assert_eq!(outcome.answers["team"].confidence(), Some(0.922));
        assert_eq!(outcome.answers["urgency"].type_name(), "score");
    }

    #[test]
    fn a_reply_missing_the_value_for_its_type_is_not_an_answer() {
        let body = r#"{"model":"m","answers":{"measurable":{"type":"noul"}},
                       "usage":{"input_tokens":1,"output_tokens":1}}"#;
        assert!(serde_json::from_str::<Outcome>(body).is_err());
    }

    #[test]
    fn the_documented_limits_match_the_published_schema() {
        assert_eq!(MAX_QUESTIONS, 64);
        assert_eq!(MIN_CRITERIA, 2);
        assert_eq!(MAX_CRITERIA, 26);
        assert_eq!(MAX_REQUEST_BYTES, 65_536);
        assert_eq!(SYSTEMONE_PATH, "/v1/systemone");
        assert_eq!(DECISION_CAPABILITY, "decision");
        assert_eq!(COMPLETION_CAPABILITY, "completion");
    }
}

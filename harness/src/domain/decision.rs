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

/// How long Ollama keeps a decision model resident: not at all. It is
/// unloaded the moment it has answered.
///
/// The decision model is never asked alone. It is asked between two
/// turns of the generation model, and on the machine a harness is
/// typically run on, the generation model is sized to the machine:
/// 111 GB of a 128 GiB laptop here. A 9 GB decision model kept
/// resident beside it pushes part of the generation model out to
/// swap, and the next generation prompt pages it back in one layer at
/// a time. Measured: the same 9.7k-token prefill ran at 83-112
/// tokens/s with the generation model alone, 32 tokens/s with the
/// decision model resident for its old five-minute `keep_alive`, and
/// 89 tokens/s again after a decision call that carried `0`; inside
/// a delivery the step-fill prompt that followed the judgments fell
/// to 9 tokens/s and passed four minutes with the prefill not done.
///
/// What this costs: a reload per judgment, 1.3 s from the page cache
/// and 5.5 s cold, against 41 ms resident. A delivery asks a handful
/// of questions in a row, so that is seconds spent to keep minutes.
pub const KEEP_ALIVE: &str = "0";

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

/// What every surface that reaches the decision model tells the user.
///
/// One string rather than three, because the alternative is three
/// copies of two numbers that must never disagree. The numbers are
/// different in kind and the wording says so: the threshold is a dead
/// band around an answer, while the hit rate is the evaluation result.
/// Ollama's own documentation is explicit that its confidence figure is
/// not calibrated correctness, so presenting `0.80` as "80% accurate"
/// would be wrong.
///
/// The figures come from `tests/decision_live.rs`; re-run it after any
/// change to the wording in `prompts/prompts.toml` and update both
/// places together.
pub const DECISION_PLANE_HELP: &str = "\
Each acceptance criterion is put to the model as the question \
`measurable/v2`: could a test check this with one unambiguous result? \
The reply is a probability read against a decision band of 0.80 - at or \
above reads HOLDS, at or below 0.20 reads FAILS, and anything between \
is INCONCLUSIVE.

That band is a dead zone, not an accuracy score. Measured accuracy, \
against the 32-criterion labelled set in tests/decision_live.rs: 0 \
misses, 1 false alarm, 3 left unsure.

In the default `enforce` mode a judgment is a gate. FAILS asks for \
REWORK and INCONCLUSIVE asks to ESCALATE; either one appends its line \
to `findings`, makes `clean` false, and exits nonzero, so the loop \
iterates on it exactly as it does on a deterministic finding. The \
deterministic findings are never edited or dropped, and a judgment \
still cannot approve anything: it never changes a test result or \
whether a requirement is implemented. Set `[decision] mode` to \
`advisory` to report without gating, or `off` to ask nothing.";

/// Whether judgments run at all, and whether they may refuse work.
///
/// `Enforce` is the default because the question is pointed at wording
/// no deterministic rule reaches. A judgment that cannot refuse leaves
/// that gap unenforced: the regex rules pass, the command exits zero,
/// and the only thing standing between vague wording and the rest of
/// the workflow is a human reading a line they were not required to
/// read. A gate the loop can ignore is not a gate.
///
/// The failure behaviour is spelled out in [`Policy::judge`] and
/// [`crate::application::decision_service::apply_review`]: a request
/// that did not produce an answer refuses, it never approves.
///
/// `Advisory` remains for a project that has not measured the question
/// against its own wording yet, and `Off` for one that wants nothing
/// asked. Both are opt-in now, because the safe direction for a gate is
/// to fail closed.
///
/// Serialized lower case, unlike [`Verdict`] and [`Transition`] beside
/// it: a mode is a setting that round-trips with the `mode = "enforce"`
/// line in `.spec/config.toml`, while those two name states in the
/// workflow.
/// Ordered by how much a judgment may interrupt, so the weaker of two
/// modes is `min` of the two. [`Policies::policy_for`] reads a gate's
/// own ceiling that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// No questions are asked, nothing is reported.
    Off,
    /// Questions are asked and the answer is reported, and it changes no
    /// deterministic finding and blocks nothing.
    Advisory,
    /// The answer may refuse work. Still cannot approve it.
    #[default]
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
    /// The question set and its version, e.g. `measurable/v2`. Bumped
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
    ///
    /// A grade needs the gate's own [`GateKind::Graded`] floor as well:
    /// concentration says only how sure the model is, never which way
    /// it went. Reading concentration alone - which this did until the
    /// method was made total - returns `Holds` for a confident grade of
    /// *unusable*, so the gate most worth catching is the one it waves
    /// through. `kind` is a parameter rather than a second method
    /// because an overload that silently cannot fail its worst case is
    /// worse than no overload.
    pub fn verdict_for(&self, answer: &Answer, kind: GateKind) -> Verdict {
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
            Answer::Choice { confidence, .. } | Answer::Score { confidence, .. }
                if *confidence < self.threshold =>
            {
                Verdict::Inconclusive
            }
            // A score is the probability-weighted average over the
            // levels, so a model sure of the top level says 1.99 and
            // never 2.0. The nearest level is what it graded.
            Answer::Score { score, .. } => match kind {
                GateKind::Graded { floor, .. } if score.round() < floor as f64 => Verdict::Fails,
                // A gate that asked a boolean question and got a grade
                // has no floor to read it against, so it has no verdict
                // either. Saying so beats inventing one.
                GateKind::Did => Verdict::Inconclusive,
                GateKind::Graded { .. } => Verdict::Holds,
            },
            Answer::Choice { .. } => Verdict::Holds,
        }
    }

    /// Whether a question that could not be answered at all - the model
    /// unreachable, the reply unreadable - is allowed to pass as though
    /// it had been.
    ///
    /// The invariant the plane opens by stating: a question that did not
    /// answer is never an approval. Under `enforce` the failure is the
    /// caller's problem; under `advisory` or `off` nothing was gating
    /// anyway, so the work carries on with a note. One predicate rather
    /// than the rule written once per gate, so the places that read it
    /// cannot drift apart.
    pub fn tolerates_silence(&self) -> bool {
        self.mode != Mode::Enforce
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
    ///
    /// The gate arrives whole rather than as a name and a version, which
    /// were two chances to pass a mismatched pair - and a judgment
    /// filed under one question's version while reading another's
    /// wording is not evidence about either.
    pub fn judge(
        &self,
        gate: &'static TaskGate,
        model: &str,
        answer: Answer,
        provenance: Provenance,
        usage: Usage,
    ) -> Judgment {
        let verdict = self.verdict_for(&answer, gate.kind);
        Judgment {
            gate: gate.gate,
            question: gate.version(),
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
    pub fn agreed(&self, answers: &[Answer], kind: GateKind) -> Verdict {
        let mut verdicts = answers.iter().map(|answer| self.verdict_for(answer, kind));
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

/// A [`Policy`] per gate, over one plane-wide default.
///
/// Gates do not deserve the same policy. A question calibrated against a
/// labelled set with a measured miss rate has earned `enforce`; one
/// shipped this week has not, and the honest way to say so is per gate
/// rather than per plane. Without this, turning the plane up to
/// `enforce` for the gate that is ready turns it up for the gate that is
/// guessing, and the first false rework teaches the owner to turn the
/// whole plane off.
///
/// Absent an entry a gate takes `default`, so an owner configures only
/// what they want to differ.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Policies {
    pub default: Policy,
    by_gate: BTreeMap<String, Policy>,
}

impl Policies {
    /// Every gate on the plane-wide policy, which is the shipped state.
    pub fn uniform(default: Policy) -> Self {
        Self {
            default,
            by_gate: BTreeMap::new(),
        }
    }

    /// `gate` read against `policy` from here on.
    pub fn with(mut self, gate: &str, policy: Policy) -> Self {
        self.by_gate.insert(gate.to_string(), policy);
        self
    }

    /// What `gate`'s answers are read against.
    ///
    /// An explicit entry is taken at its word - a project that says a
    /// gate enforces has said so about that gate. Otherwise the gate
    /// gets the weaker of the plane's mode and its own
    /// [`TaskGate::ships_at`] ceiling, so an uncalibrated question
    /// cannot start reworking work because the plane was turned up for a
    /// calibrated one.
    pub fn policy_for(&self, gate: &TaskGate) -> Policy {
        match self.by_gate.get(gate.gate) {
            Some(configured) => *configured,
            None => Policy::new(self.default.mode.min(gate.ships_at), self.default.threshold),
        }
    }

    /// Whether anything on this plane asks a question. A plane whose
    /// default is off but which still enforces one gate is still a plane
    /// that talks to a model.
    pub fn asks(&self) -> bool {
        self.default.asks() || self.by_gate.values().any(Policy::asks)
    }

    /// The gates configured away from the default, named for the log so
    /// an owner can see what the file did without reading it back.
    pub fn overrides(&self) -> impl Iterator<Item = (&str, &Policy)> {
        self.by_gate
            .iter()
            .map(|(gate, policy)| (gate.as_str(), policy))
    }
}

/// How an answer to a gate is read.
///
/// The distinction is not cosmetic: a boolean has no confidence field
/// and so earns a symmetric dead band, while a grade has both a level
/// and a concentration and has to clear each. [`Policy::verdict_for`]
/// needs to be told which, because an answer alone cannot say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    /// Did it? A probability read against the policy's dead band.
    Did,
    /// How well? Ordered levels worst-first, and the index of the
    /// lowest one that still passes.
    Graded {
        levels: &'static [&'static str],
        floor: usize,
    },
}

/// One bounded question the workflow puts to the decision model about
/// work the writing model just produced.
///
/// Everything is `&'static`, matching [`Judgment::gate`], so a gate is a
/// plain `const` and the set of them is readable in one place. The brief
/// is deliberately *not* a field: a closure would cost the `const`, and
/// the briefs genuinely differ in input type - a `&str` criterion for
/// one gate, parsed file updates for another - so a uniform function
/// pointer would force an erasure that buys nothing. The brief stays a
/// plain function at the call site, where its types are already in
/// scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskGate {
    /// Recorded in the judgment, e.g. `CRITERION_MEASURABLE`.
    pub gate: &'static str,
    /// The `[decision.<name>]` table in `prompts.toml` holding the
    /// wording and the version. Also the name the answer comes back
    /// under - one name for one question, so a reply cannot be read
    /// against the wrong table.
    pub prompt: &'static str,
    /// What the question is about, as a finding names it: `criterion`,
    /// `unit test`, `step expression`.
    pub subject: &'static str,
    /// What one judgment counts as when a review is summed up, singular
    /// then plural: `judged 3 criteria`, `judged 1 scenario`.
    pub counted: [&'static str; 2],
    /// What is wrong when the answer goes against the property.
    pub complaint: &'static str,
    /// What to do about it, named when the answer cannot say which way
    /// it went - that gates under `enforce`, and a gate saying only
    /// "the model was unsure" leaves the loop nothing to act on.
    pub remedy: &'static str,
    pub kind: GateKind,
    /// The strictest mode this question's calibration justifies.
    ///
    /// A ceiling, not a setting: the plane's mode still applies, and the
    /// weaker of the two wins. A question shipped this week has a
    /// measured miss rate of nobody-knows, and turning the plane up to
    /// `enforce` for the gate that earned it must not turn it up for the
    /// gate that is guessing - the first false rework is what teaches an
    /// owner to switch the whole plane off.
    ///
    /// Raised only by a threshold sweep in `tests/decision_live.rs`
    /// against a labelled set, which is the precedent `measurable` set.
    /// A project that disagrees says so per gate in `.spec/config.toml`,
    /// and an explicit setting is taken at its word.
    pub ships_at: Mode,
}

impl TaskGate {
    /// The question set and version, e.g. `measurable/v2`.
    ///
    /// Read from the same table as the wording rather than declared
    /// here, so the two cannot drift: any change to the wording is a new
    /// version, because a threshold calibrated against one phrasing is
    /// not evidence about another.
    pub fn version(&self) -> &'static str {
        crate::domain::prompts::decision_prompt(self.prompt)
            .version
            .as_str()
    }

    /// The name the answer comes back under.
    pub fn answer_key(&self) -> &'static str {
        self.prompt
    }

    /// The question as the server reads it.
    ///
    /// Both outcomes of a boolean are described rather than left as
    /// Yes/No, so the model is choosing between two stated criteria
    /// instead of guessing what the question meant.
    pub fn question(&self) -> Question {
        let prompt = crate::domain::prompts::decision_prompt(self.prompt);
        match self.kind {
            GateKind::Did => {
                let (when_true, when_false) = prompt.outcomes(self.prompt);
                Question::Noul {
                    instructions: prompt.instructions.clone(),
                    criteria: Some(Outcomes {
                        when_false: when_false.to_string(),
                        when_true: when_true.to_string(),
                    }),
                }
            }
            GateKind::Graded { levels, .. } => Question::Score {
                instructions: prompt.instructions.clone(),
                criteria: levels.iter().map(|level| (*level).to_string()).collect(),
            },
        }
    }

    /// The finding for work a judgment did not read as satisfying this
    /// question. Prefixed so a reader can always tell a probabilistic
    /// finding from the deterministic ones beside it - they share a list
    /// once the judgment gates.
    ///
    /// [`Verdict::Inconclusive`] earns the remedy rather than borrowing
    /// the failure's line: it gates under `enforce` too, and the way out
    /// is the same either way, so the line names it.
    ///
    /// `None` for [`Verdict::Holds`]: there is nothing to report about
    /// work the question was satisfied by.
    pub fn finding(&self, input: &str, judgment: &Judgment) -> Option<String> {
        let (complaint, remedy) = match judgment.verdict {
            Verdict::Holds => return None,
            Verdict::Fails => (self.complaint, String::new()),
            Verdict::Inconclusive => (
                "the answer is too unclear to judge either way",
                format!("; {}", self.remedy),
            ),
        };
        Some(format!(
            "judgment ({}): {} {input:?}: {complaint} - {} says {}{remedy}",
            judgment.question,
            self.subject,
            judgment.model,
            judgment.answer.summary()
        ))
    }
}

/// Every judgment as a block per gate: which gate and question, how
/// many it judged, then one row each - what was judged, the verdict,
/// and the answer that decided it.
///
/// Every judgment, not only the ones that complained. Showing the
/// complaints alone leaves out the denominator: three judged and two
/// reported reads as two problems out of two, and the one that
/// satisfied the question disappears. Named as a second review so a
/// reader can hold it beside the deterministic verdict without the two
/// contradicting each other - they are different questions asked by
/// different models.
///
/// One renderer for every command that judges, so `spec refine`,
/// `spec implement` and `spec deliver` show a judgment the same way.
pub fn second_review(judgments: &[Judgment]) -> String {
    use crate::domain::human::{columns, counted, indent, sections, titled};
    let mut gates: Vec<&'static str> = Vec::new();
    for judgment in judgments {
        if !gates.contains(&judgment.gate) {
            gates.push(judgment.gate);
        }
    }
    let blocks: Vec<String> = gates
        .iter()
        .map(|gate| {
            let of_gate: Vec<&Judgment> = judgments.iter().filter(|j| j.gate == *gate).collect();
            let [one, many] = GATES
                .iter()
                .find(|known| known.gate == *gate)
                .map(|known| known.counted)
                .unwrap_or(["answer", "answers"]);
            let rows: Vec<Vec<String>> = of_gate
                .iter()
                .map(|judgment| {
                    vec![
                        judgment.provenance.input.clone(),
                        judgment.verdict.to_string(),
                        judgment.answer.summary(),
                    ]
                })
                .collect();
            titled(
                &format!(
                    "A second review ({gate}, {}) judged {}:",
                    of_gate[0].question,
                    counted(of_gate.len(), one, many)
                ),
                &indent(&columns(&rows)),
            )
        })
        .collect();
    sections(&blocks)
}

/// Can this acceptance criterion be checked by a test with a single
/// unambiguous result?
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
///
/// The wording itself lives in `prompts/prompts.toml` under
/// `[decision.measurable]`, with every other prompt the harness sends
/// and beside the version that names it.
pub const CRITERION_MEASURABLE: TaskGate = TaskGate {
    gate: "CRITERION_MEASURABLE",
    prompt: "measurable",
    subject: "criterion",
    counted: ["criterion", "criteria"],
    complaint: "the outcome may not be measurable",
    remedy: "reword it so the clause after \"then\" names the literal value a test \
             would assert",
    kind: GateKind::Did,
    ships_at: Mode::Enforce,
};

/// The brief for [`CRITERION_MEASURABLE`]: the criterion and nothing
/// else.
///
/// The minimum relevant state, on purpose. The question is about this
/// sentence's own wording, so the story, the project, and the rest of
/// the spec are not evidence for it — they are tokens competing for a
/// context the server will not truncate.
pub fn measurable_state(criterion: &str) -> serde_json::Value {
    serde_json::json!({ "acceptance_criterion": criterion })
}

/// Does this scenario put its criterion to the test?
///
/// The deterministic rule beside it checks that a scenario exists, is
/// valid Gherkin, and is tagged with the requirement. None of that can
/// see whether the Then step checks the outcome the criterion states or
/// merely that the call returned, which is the way a generated scenario
/// goes wrong.
pub const SCENARIO_EXERCISES_CRITERION: TaskGate = TaskGate {
    gate: "SCENARIO_EXERCISES_CRITERION",
    prompt: "scenario_exercises",
    subject: "scenario for",
    counted: ["scenario", "scenarios"],
    complaint: "it may not exercise what the criterion states",
    remedy: "make the Then step check the outcome the criterion names, for the \
             inputs the criterion is about",
    kind: GateKind::Did,
    ships_at: Mode::Advisory,
};

/// The brief for [`SCENARIO_EXERCISES_CRITERION`].
pub fn scenario_state(criterion: &str, scenario: &str) -> serde_json::Value {
    serde_json::json!({ "acceptance_criterion": criterion, "scenario": scenario })
}

/// Would this step expression match that step line?
///
/// The step runner answers this exactly, but only by running, which is
/// after the attempt has been spent. Asking first turns an undefined
/// step into a correction the writing model gets while it still has the
/// scenario in hand.
pub const STEPS_BIND_SCENARIO: TaskGate = TaskGate {
    gate: "STEPS_BIND_SCENARIO",
    prompt: "steps_bind",
    subject: "step expression for",
    counted: ["step", "steps"],
    complaint: "it may not match the step line it was written for",
    remedy: "make the expression match the line as written, with a parameter for \
             each literal the line carries",
    kind: GateKind::Did,
    ships_at: Mode::Advisory,
};

/// The brief for [`STEPS_BIND_SCENARIO`].
pub fn steps_state(line: &str, expression: &str) -> serde_json::Value {
    serde_json::json!({ "step_line": line, "step_expression": expression })
}

/// Does this test body assert the criterion, or pass regardless?
///
/// The gap [`crate::domain::generation::unasserted_criteria`] cannot
/// close. That rule catches a literal `TODO: assert` left standing,
/// cheaply and with certainty; it cannot tell `assertEquals(3, add())`
/// from `assertTrue(true)`, and the second is what a model writes when
/// it is told to replace a placeholder and has nothing to say.
///
/// Asked after an implement attempt and never after `unittest generate`:
/// at generation time the placeholders are supposed to be there, and
/// that contract is deliberately untouched.
pub const UNIT_TEST_ASSERTS: TaskGate = TaskGate {
    gate: "UNIT_TEST_ASSERTS",
    prompt: "test_asserts",
    subject: "unit test for",
    counted: ["unit test", "unit tests"],
    complaint: "it may pass whatever the production code returns",
    remedy: "assert the value the criterion names against the result of calling the \
             production code",
    kind: GateKind::Did,
    ships_at: Mode::Advisory,
};

/// The brief for [`UNIT_TEST_ASSERTS`]: the criterion and the one test
/// body, never the file it lives in.
pub fn test_asserts_state(criterion: &str, body: &str) -> serde_json::Value {
    serde_json::json!({ "acceptance_criterion": criterion, "unit_test_body": body })
}

/// How completely does this code implement its criterion?
///
/// The one graded gate, and the reason the kind exists. "Did it
/// implement the behaviour" has no honest boolean answer: the failure
/// this catches is a model that wrote the easy half and stopped, which
/// is neither yes nor no. A grade says which, and the floor says how
/// much is enough.
///
/// The floor is `complete` rather than `partial`. A test run is the
/// authority on whether code works, and it runs straight after; this
/// gate exists to catch the attempt that would waste that run, so
/// accepting a known-partial implementation would leave it with nothing
/// to catch.
pub const IMPLEMENTATION_COMPLETE: TaskGate = TaskGate {
    gate: "IMPLEMENTATION_COMPLETE",
    prompt: "implementation_complete",
    subject: "implementation of",
    counted: ["file", "files"],
    complaint: "it may not implement what the criterion describes",
    remedy: "implement the behaviour for every case the criterion names, rather than \
             returning a fixed value or leaving a case out",
    kind: GateKind::Graded {
        levels: &["stub", "partial", "complete"],
        floor: 2,
    },
    ships_at: Mode::Advisory,
};

/// The brief for [`IMPLEMENTATION_COMPLETE`]: the criterion and the
/// code this attempt changed, never the file. [`changed_code`] is what
/// cuts the file down to that.
pub fn implementation_state(criterion: &str, code: &str) -> serde_json::Value {
    serde_json::json!({ "acceptance_criterion": criterion, "production_code": code })
}

/// How much code one brief carries.
///
/// The shipped decision models load an 8,192-token context and refuse a
/// prompt that overflows it rather than truncating. Source code runs
/// three to four bytes a token, so this leaves the criterion and the
/// question room beside the code. A measured attempt against a
/// 1,200-line router sent 19,885 tokens and was refused, and because
/// the gate ships advisory the refusal was a line in the log and a
/// verdict nobody gave.
pub const MAX_BRIEF_CODE_BYTES: usize = 24 * 1024;

/// What stands between two changed regions in a brief.
pub const ELIDED: &str = "    // …\n";

/// How many unchanged lines each side of a change travel with it, so
/// a judge can see the signature a new body sits under.
const CONTEXT_LINES: usize = 3;

/// The lines of `after` that `before` did not have, each change with a
/// little of its surroundings, and nothing else.
///
/// A whole file is not an answer to "does this code satisfy the
/// criterion"; it is a haystack, and past a size it is not even that -
/// it is a refused request. A brand new file is all change, and so is
/// carried whole. A file handed back unchanged is no change at all,
/// and the empty brief is how a gate learns it has nothing to ask.
///
/// Lines are compared by content rather than aligned by a diff: a line
/// present anywhere in `before` is old, however far it moved. That is
/// deliberately cheap - the test file this is also pointed at runs to
/// thousands of lines - and the cost is that a moved block reads as
/// unchanged, which for "is the new code complete" is the right call.
pub fn changed_code(before: Option<&str>, after: &str) -> String {
    let Some(before) = before else {
        return crate::domain::diff::cap(after, MAX_BRIEF_CODE_BYTES).text;
    };
    let mut old: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in before.lines() {
        *old.entry(line).or_default() += 1;
    }
    let lines: Vec<&str> = after.lines().collect();
    let changed: Vec<bool> = lines
        .iter()
        .map(|line| match old.get_mut(line) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .collect();
    if !changed.iter().any(|c| *c) {
        return String::new();
    }
    let mut kept = vec![false; lines.len()];
    for (index, _) in changed.iter().enumerate().filter(|(_, c)| **c) {
        let from = index.saturating_sub(CONTEXT_LINES);
        let to = (index + CONTEXT_LINES).min(lines.len() - 1);
        kept[from..=to].iter_mut().for_each(|k| *k = true);
    }
    let mut out = String::new();
    let mut previous_kept = true;
    for (line, keep) in lines.iter().zip(&kept) {
        if *keep {
            if !previous_kept {
                out.push_str(ELIDED);
            }
            out.push_str(line);
            out.push('\n');
        }
        previous_kept = *keep;
    }
    crate::domain::diff::cap(&out, MAX_BRIEF_CODE_BYTES).text
}

/// Did this refactoring leave behaviour alone?
///
/// The test suite answers this too, and better - but only for behaviour
/// it covers. A refactor that quietly moves a boundary nothing asserts
/// goes green, and the next requirement is written against code that no
/// longer does what the last one agreed it would.
pub const REFACTOR_PRESERVES_BEHAVIOUR: TaskGate = TaskGate {
    gate: "REFACTOR_PRESERVES_BEHAVIOUR",
    prompt: "refactor_preserves",
    subject: "refactoring of",
    counted: ["file", "files"],
    complaint: "it may change what the code does, not just how it reads",
    remedy: "keep every branch, boundary and default as it was, and make the change \
             to the wording of the code alone",
    kind: GateKind::Did,
    ships_at: Mode::Advisory,
};

/// The brief for [`REFACTOR_PRESERVES_BEHAVIOUR`]: one function's before
/// and after, never the whole diff.
pub fn refactor_state(before: &str, after: &str) -> serde_json::Value {
    serde_json::json!({ "before": before, "after": after })
}

/// Every gate the harness can ask about, for the tests and reports that
/// have to cover all of them rather than the ones someone remembered.
pub const GATES: &[&TaskGate] = &[
    &CRITERION_MEASURABLE,
    &SCENARIO_EXERCISES_CRITERION,
    &STEPS_BIND_SCENARIO,
    &UNIT_TEST_ASSERTS,
    &IMPLEMENTATION_COMPLETE,
    &REFACTOR_PRESERVES_BEHAVIOUR,
];

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

    /// A gate that is not one of the shipped ones, for asking what a
    /// policy does about a name it has never heard.
    fn gate(name: &'static str) -> TaskGate {
        TaskGate {
            gate: name,
            ..CRITERION_MEASURABLE
        }
    }

    fn score(level: f64, confidence: f64) -> Answer {
        Answer::Score {
            score: level,
            legend: BTreeMap::from([
                ("0".to_string(), "unusable".to_string()),
                ("1".to_string(), "partial".to_string()),
                ("2".to_string(), "complete".to_string()),
            ]),
            probabilities: BTreeMap::new(),
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
        assert_eq!(
            Mode::default(),
            Mode::Enforce,
            "a gate fails closed: the question reaches wording no rule does"
        );
        assert_eq!(Mode::Advisory.to_string(), "advisory");
    }

    /// A boolean answer has no confidence field, so the threshold has to
    /// be read off the probability itself from both directions.
    #[test]
    fn a_boolean_answer_has_a_dead_band_rather_than_a_confidence() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        assert_eq!(noul(0.5).confidence(), None);
        assert_eq!(
            policy.verdict_for(&noul(0.97), GateKind::Did),
            Verdict::Holds
        );
        assert_eq!(
            policy.verdict_for(&noul(0.70), GateKind::Did),
            Verdict::Holds
        );
        assert_eq!(
            policy.verdict_for(&noul(0.08), GateKind::Did),
            Verdict::Fails
        );
        assert_eq!(
            policy.verdict_for(&noul(0.30), GateKind::Did),
            Verdict::Fails
        );
        assert_eq!(
            policy.verdict_for(&noul(0.55), GateKind::Did),
            Verdict::Inconclusive
        );
        assert_eq!(
            policy.verdict_for(&noul(0.45), GateKind::Did),
            Verdict::Inconclusive
        );
    }

    #[test]
    fn a_choice_answer_must_clear_the_threshold_on_concentration() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        assert_eq!(choice(0.92).confidence(), Some(0.92));
        assert_eq!(
            policy.verdict_for(&choice(0.92), GateKind::Did),
            Verdict::Holds
        );
        assert_eq!(
            policy.verdict_for(&choice(0.04), GateKind::Did),
            Verdict::Inconclusive
        );
    }

    /// Concentration says how sure the model is, never which way it
    /// went. Reading it alone - which this did until `verdict_for` was
    /// made total - returns `Holds` for a confident grade of *unusable*,
    /// so the very answer the gate exists to catch is the one it waved
    /// through. A grade has to clear the threshold *and* reach the
    /// Judgments from two gates render as two blocks, each counting its
    /// own rows in its own noun, and every row is there - the one that
    /// held as much as the one that did not.
    #[test]
    fn a_second_review_is_one_block_per_gate_with_every_row_in_it() {
        let policy = Policy::new(Mode::Advisory, DEFAULT_MIN_CONFIDENCE);
        let judge = |gate: &'static TaskGate, input: &str, answer: Answer| {
            policy.judge(
                gate,
                "nimble:test",
                answer,
                Provenance {
                    input: input.to_string(),
                    state: "sha256:0".into(),
                    state_bytes: 0,
                },
                Usage::default(),
            )
        };
        let judgments = vec![
            judge(
                &UNIT_TEST_ASSERTS,
                "REQ-001 criterion 1",
                Answer::Noul { noul: 0.91 },
            ),
            judge(
                &UNIT_TEST_ASSERTS,
                "REQ-001 criterion 2",
                Answer::Noul { noul: 0.12 },
            ),
            judge(
                &IMPLEMENTATION_COMPLETE,
                "src/main/java/Kata.java",
                score(2.0, 0.95),
            ),
        ];

        let rendered = second_review(&judgments);

        assert!(
            rendered.contains(&format!(
                "A second review (UNIT_TEST_ASSERTS, {}) judged 2 unit tests:",
                UNIT_TEST_ASSERTS.version()
            )),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!(
                "A second review (IMPLEMENTATION_COMPLETE, {}) judged 1 file:",
                IMPLEMENTATION_COMPLETE.version()
            )),
            "{rendered}"
        );
        assert!(
            rendered.contains("REQ-001 criterion 1  HOLDS"),
            "{rendered}"
        );
        assert!(
            rendered.contains("REQ-001 criterion 2  FAILS"),
            "{rendered}"
        );
        assert!(
            rendered.contains("src/main/java/Kata.java  HOLDS"),
            "{rendered}"
        );
        assert_eq!(second_review(&[]), "");
    }

    /// gate's floor.
    #[test]
    fn a_confident_grade_below_the_floor_fails_rather_than_holding() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        let graded = GateKind::Graded {
            levels: &["unusable", "partial", "complete"],
            floor: 2,
        };
        assert_eq!(
            policy.verdict_for(&score(0.0, 0.95), graded),
            Verdict::Fails
        );
        assert_eq!(
            policy.verdict_for(&score(1.4, 0.95), graded),
            Verdict::Fails
        );
        assert_eq!(
            policy.verdict_for(&score(2.0, 0.95), graded),
            Verdict::Holds
        );
        // A score is a probability-weighted average over the levels, so
        // a model that is 99% sure of the top level answers 1.99, not
        // 2.0. The floor is read against the nearest level, or the top
        // grade could never be reached at all.
        assert_eq!(
            policy.verdict_for(&score(1.996, 0.95), graded),
            Verdict::Holds,
            "nearest level is the top one"
        );
        assert_eq!(
            policy.verdict_for(&score(1.49, 0.95), graded),
            Verdict::Fails,
            "nearest level is still below the floor"
        );
        assert_eq!(
            policy.verdict_for(&score(0.0, 0.30), graded),
            Verdict::Inconclusive,
            "an unconcentrated answer is no answer, whatever level it points at"
        );
        assert_eq!(
            policy.verdict_for(&score(2.0, 0.95), GateKind::Did),
            Verdict::Inconclusive,
            "a boolean gate has no floor to read a grade against"
        );
    }

    /// A gate names the table its wording comes from, and the version
    /// it records comes from that same table. Asserted over every gate
    /// rather than the ones someone remembered, because the failure
    /// mode is a gate added without its table - which would record a
    /// judgment under a question nobody can read back.
    #[test]
    fn every_gate_reads_its_wording_and_version_from_its_own_table() {
        let mut seen = std::collections::BTreeSet::new();
        for gate in GATES {
            assert!(seen.insert(gate.gate), "{} is declared twice", gate.gate);
            assert!(
                gate.version().starts_with(&format!("{}/", gate.prompt)),
                "{} records {:?}, which does not name its own table {:?}",
                gate.gate,
                gate.version(),
                gate.prompt
            );
            // Asking builds the question from the table, so a boolean
            // gate whose table describes no outcomes panics here rather
            // than in front of a developer mid-delivery.
            match (gate.kind, gate.question()) {
                (GateKind::Did, Question::Noul { criteria, .. }) => {
                    assert!(criteria.is_some(), "{} states no outcomes", gate.gate);
                }
                (GateKind::Graded { levels, floor }, Question::Score { criteria, .. }) => {
                    assert_eq!(criteria, levels, "{} asks for other levels", gate.gate);
                    assert!(
                        floor < levels.len(),
                        "{}'s floor names a level it does not have",
                        gate.gate
                    );
                }
                (kind, question) => panic!("{} is {kind:?} but asks {question:?}", gate.gate),
            }
        }
    }

    /// A gate calibrated against a labelled set has earned `enforce`;
    /// one shipped this week has not. Per-gate policy is what lets both
    /// ship at once, instead of the plane being turned off the first
    /// time the unready gate reworks good work.
    #[test]
    fn a_gate_without_an_entry_of_its_own_takes_the_plane_wide_policy() {
        let plane = Policies::uniform(Policy::new(Mode::Advisory, 0.70));
        assert_eq!(plane.policy_for(&CRITERION_MEASURABLE).mode, Mode::Advisory);

        let mixed = plane.with("CRITERION_MEASURABLE", Policy::new(Mode::Enforce, 0.90));
        let measurable = mixed.policy_for(&CRITERION_MEASURABLE);
        assert_eq!(measurable.mode, Mode::Enforce);
        assert_eq!(measurable.threshold, 0.90);
        assert_eq!(
            mixed.policy_for(&gate("UNNAMED")).mode,
            Mode::Advisory,
            "an unnamed gate still takes the default"
        );
        assert_eq!(
            mixed.overrides().collect::<Vec<_>>().len(),
            1,
            "only what differs is recorded"
        );
    }

    /// A gate ships no stricter than its calibration justifies. Turning
    /// the plane up for the question that earned it must not turn it up
    /// for the one shipped this week - the first false rework is what
    /// teaches an owner to switch the whole plane off.
    #[test]
    fn an_uncalibrated_gate_stays_advisory_on_a_plane_that_enforces() {
        let plane = Policies::uniform(Policy::new(Mode::Enforce, 0.70));
        assert_eq!(
            plane.policy_for(&CRITERION_MEASURABLE).mode,
            Mode::Enforce,
            "the calibrated gate takes the plane's mode"
        );
        for gate in GATES.iter().filter(|gate| gate.ships_at != Mode::Enforce) {
            assert_eq!(
                plane.policy_for(gate).mode,
                Mode::Advisory,
                "{} has not earned enforce",
                gate.gate
            );
        }
        assert_eq!(
            Policies::uniform(Policy::new(Mode::Off, 0.70))
                .policy_for(&CRITERION_MEASURABLE)
                .mode,
            Mode::Off,
            "a ceiling never raises a mode"
        );
    }

    /// The ceiling is a default, not a veto: a project that measured a
    /// question against its own wording and wants it enforcing says so,
    /// and is taken at its word.
    #[test]
    fn a_gate_configured_by_hand_overrides_its_own_ceiling() {
        let plane = Policies::uniform(Policy::new(Mode::Advisory, 0.70))
            .with("UNIT_TEST_ASSERTS", Policy::new(Mode::Enforce, 0.95));
        assert_eq!(plane.policy_for(&UNIT_TEST_ASSERTS).mode, Mode::Enforce);
        assert_eq!(plane.policy_for(&UNIT_TEST_ASSERTS).threshold, 0.95);
    }

    /// A plane whose default is off but which still enforces one gate is
    /// still a plane that talks to a model, and has to be wired as one.
    #[test]
    fn a_plane_that_is_off_by_default_still_asks_for_a_gate_that_is_not() {
        let off = Policies::uniform(Policy::new(Mode::Off, 0.70));
        assert!(!off.asks());
        assert!(
            off.with("CRITERION_MEASURABLE", Policy::new(Mode::Advisory, 0.70))
                .asks()
        );
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
        assert_eq!(
            policy.agreed(&[noul(0.97), noul(0.95)], GateKind::Did),
            Verdict::Holds
        );
        assert_eq!(
            policy.agreed(&[noul(0.02), noul(0.05)], GateKind::Did),
            Verdict::Fails
        );
        assert_eq!(
            policy.agreed(&[noul(0.97), noul(0.02)], GateKind::Did),
            Verdict::Inconclusive
        );
        assert_eq!(policy.agreed(&[], GateKind::Did), Verdict::Inconclusive);
    }

    #[test]
    fn a_judgment_records_the_model_question_threshold_and_provenance() {
        let policy = Policy::new(Mode::Advisory, 0.70);
        let judgment = policy.judge(
            &CRITERION_MEASURABLE,
            "nimble:latest",
            noul(0.04),
            provenance(),
            Usage {
                input_tokens: 151,
                output_tokens: 1,
            },
        );
        assert_eq!(judgment.gate, "CRITERION_MEASURABLE");
        assert_eq!(judgment.question, "measurable/v2");
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
            &CRITERION_MEASURABLE,
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
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
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
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
            measurable_state("a"),
        );
        let same = Request::single(
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
            measurable_state("a"),
        );
        let other = Request::single(
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
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
            .map(|n| (format!("q{n}"), CRITERION_MEASURABLE.question()))
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
            let request = Request::single(
                CRITERION_MEASURABLE.answer_key(),
                CRITERION_MEASURABLE.question(),
                value,
            );
            assert_eq!(
                request.fault(),
                Some("state: must be a string, object, or array".into())
            );
        }
    }

    #[test]
    fn a_blank_state_string_is_refused() {
        let request = Request::single(
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
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
            questions: BTreeMap::from([(" ".to_string(), CRITERION_MEASURABLE.question())]),
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
        assert_eq!(CRITERION_MEASURABLE.question().type_name(), "noul");
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
            &CRITERION_MEASURABLE,
            "nimble:latest",
            noul(0.04),
            provenance(),
            Usage::default(),
        );
        let finding = CRITERION_MEASURABLE
            .finding("Given a, when b, then it works", &judgment)
            .expect("a failure");
        assert!(finding.starts_with("judgment (measurable/v2):"));
        assert!(finding.contains("Given a, when b, then it works"));
        assert!(finding.contains("nimble:latest"));
        assert!(finding.contains("probability of true 0.040"));
        assert!(finding.contains("may not be measurable"));
    }

    /// An inconclusive answer gates in `enforce`, so it needs a line of
    /// its own that names the reword rather than only reporting that the
    /// model was unsure.
    #[test]
    fn an_inconclusive_finding_names_the_reword_and_a_holding_one_says_nothing() {
        let policy = Policy::new(Mode::Enforce, 0.70);
        let judge = |probability| {
            policy.judge(
                &CRITERION_MEASURABLE,
                "nimble:latest",
                noul(probability),
                provenance(),
                Usage::default(),
            )
        };
        let unsure = CRITERION_MEASURABLE
            .finding("Given a, when b, then it is valid", &judge(0.55))
            .expect("an inconclusive answer gates, so it has to say something");
        assert!(unsure.starts_with("judgment (measurable/v2):"));
        assert!(unsure.contains("too unclear to judge either way"));
        assert!(unsure.contains("after \"then\""));
        assert!(unsure.contains("probability of true 0.550"));
        assert_eq!(
            CRITERION_MEASURABLE.finding("Given a, when b, then the result is 3", &judge(0.97)),
            None,
            "nothing to report about a criterion the question was satisfied by"
        );
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

    fn long_file(functions: usize) -> String {
        (0..functions)
            .map(|n| format!("fn existing_{n}() -> u32 {{\n    {n}\n}}\n"))
            .collect()
    }

    #[test]
    fn a_new_file_is_the_change_in_its_entirety() {
        let after = "fn add(a: u32, b: u32) -> u32 {\n    a + b\n}\n";
        assert_eq!(changed_code(None, after), after);
    }

    #[test]
    fn a_function_added_to_a_long_file_is_briefed_without_the_rest_of_the_file() {
        let before = long_file(400);
        let added = "fn criteria_coverage(id: &str) -> Coverage {\n    coverage_of(id)\n}\n";
        let after = format!(
            "{}{added}{}",
            &before[..before.len() / 2],
            &before[before.len() / 2..]
        );
        let brief = changed_code(Some(&before), &after);
        assert!(brief.contains(added), "{brief}");
        assert!(
            brief.len() < added.len() + 200,
            "the brief carries the file, not the change: {} bytes",
            brief.len()
        );
    }

    #[test]
    fn a_file_handed_back_unchanged_has_no_change_to_brief() {
        let file = long_file(3);
        assert_eq!(changed_code(Some(&file), &file), "");
    }

    #[test]
    fn two_separate_changes_are_both_briefed_with_a_marker_between() {
        let before = long_file(50);
        let mut lines: Vec<&str> = before.lines().collect();
        lines.insert(4, "    // first");
        lines.insert(120, "    // second");
        let after = lines.join("\n");
        let brief = changed_code(Some(&before), &after);
        assert!(
            brief.contains("// first") && brief.contains("// second"),
            "{brief}"
        );
        assert!(brief.contains(ELIDED), "{brief}");
    }

    #[test]
    fn a_change_larger_than_the_context_is_cut_on_a_line_boundary() {
        let after = long_file(3000);
        let brief = changed_code(None, &after);
        assert!(brief.len() <= MAX_BRIEF_CODE_BYTES, "{} bytes", brief.len());
        assert!(brief.ends_with('\n'), "cut mid-line");
    }
}

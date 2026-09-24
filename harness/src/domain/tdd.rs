//! The Red/Green/Refactor state machine. Transition rules and message
//! strings match the frozen seven-tool MCP contract
//! (`harness/tests/mcp_conformance.rs`).
//!
//! Transitions:
//! - any phase + failing test run -> RED
//! - any phase + passing test run -> GREEN
//! - GREEN + refactor started -> REFACTOR (never from RED: you never
//!   refactor on a red bar)
//!
//! Persistence is a chronological log of timestamped entries in
//! `.spec/state.json`. The file keeps the full history; an LLM brief
//! only receives the three latest entries plus the interpretation
//! instructions.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::domain::model::TestRunSummary;

/// How many dated state entries an LLM may be briefed with.
pub const LLM_STATE_ENTRIES: usize = 3;

/// How to read `.spec/state.json`. Written into the file so an agent
/// opening it knows the schema without a side document.
pub const STATE_INSTRUCTIONS: &str = "\
This file is the TDD phase log. `instructions` is this guide, not workflow state. \
`entries` is chronological, oldest first; each entry is the machine at `timestamp` (UTC RFC 3339). \
`phase` is START, RED, GREEN, or REFACTOR. Never refactor on RED; never mark a requirement implemented off GREEN. \
`lastRun` is test counts at that moment; failure details live on the test-run reply. \
`refactorLog` is every spec refactor --note, in order. \
`attemptLog` is model implementation attempts for the requirement in flight; a GREEN run clears it. \
Each attempt records what it wrote (`targets`), the failures it was briefed with (`failures`), and \
the output of the first run after it (`outcome`; empty means no run verified it). \
When briefing a model, include only the three most recent entries. Older entries stay on disk for humans.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TddPhase {
    #[default]
    Start,
    Red,
    Green,
    Refactor,
}

impl fmt::Display for TddPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            TddPhase::Start => "START",
            TddPhase::Red => "RED",
            TddPhase::Green => "GREEN",
            TddPhase::Refactor => "REFACTOR",
        };
        f.write_str(name)
    }
}

/// One recorded model implementation attempt: which requirement it was
/// for, what it wrote, the failures it was briefed with, and the output
/// of the first test run after it. The log lets every later attempt see
/// what was already tried and what each try actually caused, instead of
/// starting blind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplementAttempt {
    pub requirement: String,
    pub targets: Vec<String>,
    pub failures: Vec<String>,
    /// What the first build/test run after this attempt reported - the
    /// attempt's actual result, attached by [`TddStateMachine::record_test_run`].
    /// Empty means no run followed: the changes were never verified.
    /// Missing in 0.2.3 files; treated as empty so those logs still load.
    #[serde(default)]
    pub outcome: Vec<String>,
}

/// One dated snapshot of the machine, appended on every mutation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    pub timestamp: String,
    pub phase: TddPhase,
    #[serde(rename = "lastRun")]
    pub last_run: TestRunSummary,
    #[serde(rename = "refactorLog")]
    pub refactor_log: Vec<String>,
    #[serde(rename = "attemptLog")]
    pub attempt_log: Vec<ImplementAttempt>,
}

/// The machine's full state as a plain value, for persistence between
/// harness invocations. The file is a log: `entries` grows; `recent_entries`
/// is what an LLM is allowed to see. The envelope is strict
/// (`instructions` and `entries` are required). `attemptLog[].outcome`
/// defaults to empty so 0.2.3 files still load. A file that does not
/// match is a parse error (delete `.spec/state.json` to reset the machine).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TddSnapshot {
    pub instructions: String,
    pub entries: Vec<StateEntry>,
}

impl Default for TddSnapshot {
    fn default() -> Self {
        Self {
            instructions: STATE_INSTRUCTIONS.to_string(),
            entries: Vec::new(),
        }
    }
}

impl TddSnapshot {
    /// A one-entry log at `phase`, for tests and cucumber fixtures.
    pub fn at(phase: TddPhase) -> Self {
        Self::with(StateEntry {
            timestamp: "1970-01-01T00:00:00Z".into(),
            phase,
            ..Default::default()
        })
    }

    /// A one-entry log holding this snapshot of the machine.
    pub fn with(entry: StateEntry) -> Self {
        Self {
            instructions: STATE_INSTRUCTIONS.to_string(),
            entries: vec![entry],
        }
    }

    pub fn current(&self) -> Option<&StateEntry> {
        self.entries.last()
    }

    pub fn phase(&self) -> TddPhase {
        self.current().map(|e| e.phase).unwrap_or_default()
    }

    pub fn refactor_log(&self) -> &[String] {
        self.current()
            .map(|e| e.refactor_log.as_slice())
            .unwrap_or(&[])
    }

    pub fn attempt_log(&self) -> &[ImplementAttempt] {
        self.current()
            .map(|e| e.attempt_log.as_slice())
            .unwrap_or(&[])
    }

    /// The three latest dated entries — the only history an LLM brief
    /// may include. Older entries stay on disk.
    pub fn recent_entries(&self) -> &[StateEntry] {
        let start = self.entries.len().saturating_sub(LLM_STATE_ENTRIES);
        &self.entries[start..]
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Debug, Default)]
pub struct TddStateMachine {
    phase: TddPhase,
    last_run: TestRunSummary,
    refactor_log: Vec<String>,
    attempt_log: Vec<ImplementAttempt>,
    entries: Vec<StateEntry>,
}

impl TddStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restore(snapshot: TddSnapshot) -> Self {
        match snapshot.current() {
            Some(current) => Self {
                phase: current.phase,
                last_run: current.last_run.clone(),
                refactor_log: current.refactor_log.clone(),
                attempt_log: current.attempt_log.clone(),
                entries: snapshot.entries,
            },
            None => Self {
                entries: snapshot.entries,
                ..Default::default()
            },
        }
    }

    pub fn snapshot(&self) -> TddSnapshot {
        TddSnapshot {
            instructions: STATE_INSTRUCTIONS.to_string(),
            entries: self.entries.clone(),
        }
    }

    fn stamp(&mut self) {
        // Only the newest entry carries the attempt log - restore()
        // reads it from `current()` alone, so copies in older entries
        // are dead weight. Re-cloning the whole log into every dated
        // entry grew one live run's state file quadratically to 116 MB
        // (298 entries x up to 147 attempts of stored stack traces).
        if let Some(previous) = self.entries.last_mut() {
            previous.attempt_log = Vec::new();
        }
        self.entries.push(StateEntry {
            timestamp: now_rfc3339(),
            phase: self.phase,
            last_run: self.last_run.clone(),
            refactor_log: self.refactor_log.clone(),
            attempt_log: self.attempt_log.clone(),
        });
    }

    pub fn record_test_run(&mut self, summary: TestRunSummary) -> TddPhase {
        self.phase = if summary.passed() {
            TddPhase::Green
        } else if summary.no_tests() {
            // Success with zero reports is not RED: the runner missed
            // the suite. Keep the current phase so mark-implemented and
            // refactor are not blocked by a false red bar.
            self.phase
        } else {
            TddPhase::Red
        };
        if self.phase == TddPhase::Green {
            // The loop closed: past attempts are history the next
            // requirement should not inherit.
            self.attempt_log.clear();
        } else if let Some(attempt) = self
            .attempt_log
            .last_mut()
            .filter(|attempt| attempt.outcome.is_empty())
        {
            // The first run after the newest attempt is that attempt's
            // outcome: the next brief recounts what the changes actually
            // caused, not just what they were trying to fix. Older
            // attempts a run never followed stay unverified for good -
            // a later run reflects newer changes, not theirs.
            attempt.outcome = if summary.failure_details.is_empty() {
                vec![format!(
                    "tests={} failures={} errors={} skipped={}",
                    summary.tests, summary.failures, summary.errors, summary.skipped
                )]
            } else {
                summary.failure_details.clone()
            };
        }
        self.last_run = summary;
        self.stamp();
        self.phase
    }

    /// Log one model implementation attempt so later attempts can be
    /// briefed with what was already tried.
    pub fn record_attempt(&mut self, attempt: ImplementAttempt) {
        self.attempt_log.push(attempt);
        self.stamp();
    }

    /// Prior implementation attempts for one requirement, oldest first.
    pub fn attempts_for(&self, req_id: &str) -> Vec<ImplementAttempt> {
        self.attempt_log
            .iter()
            .filter(|attempt| attempt.requirement == req_id)
            .cloned()
            .collect()
    }

    pub fn start_refactor(&mut self, note: Option<&str>) -> Result<TddPhase, String> {
        if self.phase != TddPhase::Green {
            return Err(format!(
                "Refactoring is only allowed from GREEN (current phase: {}). {}",
                self.phase,
                refusal_advice(self.phase)
            ));
        }
        let entry = match note.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => "(no note)".to_string(),
        };
        self.refactor_log.push(entry);
        self.phase = TddPhase::Refactor;
        self.stamp();
        Ok(self.phase)
    }

    pub fn phase(&self) -> TddPhase {
        self.phase
    }

    pub fn last_run(&self) -> &TestRunSummary {
        &self.last_run
    }

    pub fn refactor_log(&self) -> &[String] {
        &self.refactor_log
    }

    /// A human/agent-readable hint about what to do next.
    ///
    /// Worded for the MCP agent, which calls tools. The shell rewrites
    /// it to name commands - see `cli_next_step` in `main.rs`.
    pub fn suggestion(&self) -> &'static str {
        match self.phase {
            TddPhase::Start => {
                "No tests have been run yet. Call run_tests to establish a baseline."
            }
            TddPhase::Red => {
                "Tests are failing. Write the simplest production code that makes them pass, \
                 then call run_tests again."
            }
            TddPhase::Green => {
                "All tests pass. Either call start_refactor to clean up, or call \
                 get_requirement for the next pending requirement and write a failing test \
                 for it."
            }
            TddPhase::Refactor => {
                "A refactor is in progress. Call run_tests to prove the refactor kept the \
                 bar green."
            }
        }
    }
}

/// Why refactoring is refused, said in terms of the phase the developer
/// is actually in. "Never refactor on a red bar" is the right sentence
/// on RED and nonsense anywhere else: at START there is no bar yet, and
/// in REFACTOR there is already one open.
fn refusal_advice(phase: TddPhase) -> &'static str {
    match phase {
        TddPhase::Red => "Never refactor on a red bar — make the tests pass first.",
        TddPhase::Start => "No tests have been run yet — run them to find out where you are.",
        TddPhase::Refactor => "A refactor is already in progress — run the tests to close it.",
        // The caller only asks once the phase is not GREEN.
        TddPhase::Green => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failing_run() -> TestRunSummary {
        TestRunSummary {
            tests: 8,
            failures: 2,
            errors: 1,
            ..Default::default()
        }
    }

    fn passing_run() -> TestRunSummary {
        TestRunSummary {
            tests: 8,
            ..Default::default()
        }
    }

    fn parseable_rfc3339(timestamp: &str) -> bool {
        chrono::DateTime::parse_from_rfc3339(timestamp).is_ok()
    }

    #[test]
    fn a_fresh_machine_starts_at_start_with_the_baseline_suggestion() {
        let machine = TddStateMachine::new();
        assert_eq!(machine.phase(), TddPhase::Start);
        assert_eq!(
            machine.suggestion(),
            "No tests have been run yet. Call run_tests to establish a baseline."
        );
        assert!(machine.snapshot().entries.is_empty());
    }

    #[test]
    fn a_failing_run_moves_to_red() {
        let mut machine = TddStateMachine::new();
        assert_eq!(machine.record_test_run(failing_run()), TddPhase::Red);
        assert_eq!(machine.last_run().failures, 2);
        assert!(machine.suggestion().starts_with("Tests are failing."));
    }

    #[test]
    fn a_passing_run_moves_to_green() {
        let mut machine = TddStateMachine::new();
        assert_eq!(machine.record_test_run(passing_run()), TddPhase::Green);
        assert!(machine.suggestion().starts_with("All tests pass."));
    }

    #[test]
    fn a_successful_run_with_no_tests_does_not_flip_to_red() {
        let mut machine = TddStateMachine::new();
        assert_eq!(
            machine.record_test_run(TestRunSummary::default()),
            TddPhase::Start
        );
        machine.record_test_run(passing_run());
        assert_eq!(
            machine.record_test_run(TestRunSummary::default()),
            TddPhase::Green
        );
    }

    #[test]
    fn refactor_is_allowed_from_green_and_logs_the_note() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(passing_run());
        let phase = machine.start_refactor(Some("  extract parser  ")).unwrap();
        assert_eq!(phase, TddPhase::Refactor);
        assert_eq!(machine.refactor_log(), ["extract parser"]);
        assert!(
            machine
                .suggestion()
                .starts_with("A refactor is in progress.")
        );
    }

    #[test]
    fn a_missing_or_blank_note_is_recorded_as_no_note() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(passing_run());
        machine.start_refactor(None).unwrap();
        machine.record_test_run(passing_run());
        machine.start_refactor(Some("   ")).unwrap();
        assert_eq!(machine.refactor_log(), ["(no note)", "(no note)"]);
    }

    #[test]
    fn refactor_is_refused_on_a_red_bar() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        let error = machine.start_refactor(Some("tidy up")).unwrap_err();
        assert_eq!(
            error,
            "Refactoring is only allowed from GREEN (current phase: RED). \
             Never refactor on a red bar — make the tests pass first."
        );
        assert_eq!(machine.phase(), TddPhase::Red);
        assert!(machine.refactor_log().is_empty());
    }

    /// The red-bar sentence belongs to RED. Before any run there is no
    /// bar to be on, and the refusal used to say there was.
    #[test]
    fn refactor_is_refused_before_any_run_without_inventing_a_red_bar() {
        let mut machine = TddStateMachine::new();
        let error = machine.start_refactor(None).unwrap_err();
        assert_eq!(
            error,
            "Refactoring is only allowed from GREEN (current phase: START). \
             No tests have been run yet — run them to find out where you are."
        );
        assert!(machine.snapshot().entries.is_empty());
    }

    #[test]
    fn a_second_refactor_is_refused_by_naming_the_one_already_open() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(passing_run());
        machine.start_refactor(Some("extract parser")).unwrap();
        assert_eq!(
            machine.start_refactor(Some("again")).unwrap_err(),
            "Refactoring is only allowed from GREEN (current phase: REFACTOR). \
             A refactor is already in progress — run the tests to close it."
        );
    }

    #[test]
    fn a_passing_run_after_refactor_returns_to_green() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(passing_run());
        machine.start_refactor(Some("cleanup")).unwrap();
        assert_eq!(machine.record_test_run(passing_run()), TddPhase::Green);
    }

    #[test]
    fn a_machine_round_trips_through_its_snapshot() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(passing_run());
        machine.start_refactor(Some("extract parser")).unwrap();
        let restored = TddStateMachine::restore(machine.snapshot());
        assert_eq!(restored.phase(), TddPhase::Refactor);
        assert_eq!(restored.last_run(), machine.last_run());
        assert_eq!(restored.refactor_log(), ["extract parser"]);
        assert_eq!(restored.snapshot().entries.len(), 2);
    }

    #[test]
    fn a_snapshot_serializes_phases_uppercase_and_fields_camel_case() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        let json = serde_json::to_string(&machine.snapshot()).unwrap();
        assert!(json.contains(r#""phase":"RED""#), "got: {json}");
        assert!(json.contains(r#""lastRun""#));
        assert!(json.contains(r#""refactorLog""#));
        assert!(json.contains(r#""instructions""#));
        assert!(json.contains(r#""entries""#));
        assert!(json.contains(r#""timestamp""#));
        let back: TddSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, machine.snapshot());
    }

    fn attempt(req: &str) -> ImplementAttempt {
        ImplementAttempt {
            requirement: req.into(),
            targets: vec!["src/main/java/Calc.java".into()],
            failures: vec!["CalcTest.adds: expected 3 but was 4".into()],
            ..Default::default()
        }
    }

    #[test]
    fn attempts_are_logged_and_filtered_by_requirement() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        machine.record_attempt(attempt("REQ-001"));
        machine.record_attempt(attempt("REQ-002"));
        machine.record_attempt(attempt("REQ-001"));
        assert_eq!(machine.attempts_for("REQ-001").len(), 2);
        assert_eq!(machine.attempts_for("REQ-002").len(), 1);
        assert!(machine.attempts_for("REQ-003").is_empty());
    }

    #[test]
    fn a_green_run_clears_the_attempt_log() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        machine.record_attempt(attempt("REQ-001"));
        machine.record_test_run(failing_run());
        assert_eq!(machine.attempts_for("REQ-001").len(), 1, "RED keeps it");
        machine.record_test_run(passing_run());
        assert!(machine.attempts_for("REQ-001").is_empty());
    }

    #[test]
    fn only_the_newest_entry_carries_the_attempt_log() {
        // One live run's state file grew quadratically to 116 MB because
        // every dated entry re-cloned the whole attempt log. restore()
        // only reads the newest entry's log, so older entries keep none.
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        for _ in 0..3 {
            machine.record_attempt(attempt("REQ-001"));
            machine.record_test_run(failing_run());
        }
        let snapshot = machine.snapshot();
        let (newest, older) = snapshot.entries.split_last().expect("entries were stamped");
        assert_eq!(newest.attempt_log.len(), 3, "the newest entry has it all");
        assert!(
            older.iter().all(|entry| entry.attempt_log.is_empty()),
            "older entries carry no attempt-log copies"
        );
        let restored = TddStateMachine::restore(snapshot);
        assert_eq!(
            restored.attempts_for("REQ-001").len(),
            3,
            "the log survives a round trip"
        );
    }

    fn failing_run_reporting(detail: &str) -> TestRunSummary {
        TestRunSummary {
            tests: 8,
            failures: 1,
            failure_details: vec![detail.to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn a_red_run_attaches_its_output_to_the_newest_attempt() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run_reporting("CalcTest.adds: TODO: assert"));
        machine.record_attempt(attempt("REQ-001"));
        machine.record_test_run(failing_run_reporting("CalcTest.adds: expected 3 but was 4"));
        assert_eq!(
            machine.attempts_for("REQ-001")[0].outcome,
            vec!["CalcTest.adds: expected 3 but was 4"],
            "the attempt's outcome is what the run after it reported"
        );
    }

    #[test]
    fn a_later_run_never_claims_an_older_untested_attempt() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run_reporting("first failure"));
        machine.record_attempt(attempt("REQ-001")); // never tested on its own
        machine.record_attempt(attempt("REQ-001"));
        machine.record_test_run(failing_run_reporting("second failure"));
        machine.record_test_run(failing_run_reporting("third failure"));
        let attempts = machine.attempts_for("REQ-001");
        assert!(
            attempts[0].outcome.is_empty(),
            "the superseded attempt stays unverified: {:?}",
            attempts[0].outcome
        );
        assert_eq!(
            attempts[1].outcome,
            vec!["second failure"],
            "only the first run after the newest attempt is its outcome"
        );
    }

    #[test]
    fn a_detail_less_red_run_records_its_counts_as_the_outcome() {
        let mut machine = TddStateMachine::new();
        machine.record_attempt(attempt("REQ-001"));
        machine.record_test_run(failing_run()); // counts only, no details
        assert_eq!(
            machine.attempts_for("REQ-001")[0].outcome,
            vec!["tests=8 failures=2 errors=1 skipped=0"]
        );
    }

    #[test]
    fn the_attempt_log_round_trips_through_the_snapshot() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        machine.record_attempt(attempt("REQ-001"));
        let json = serde_json::to_string(&machine.snapshot()).unwrap();
        assert!(json.contains(r#""attemptLog""#), "got: {json}");
        assert!(json.contains(r#""outcome""#), "got: {json}");
        let restored = TddStateMachine::restore(serde_json::from_str(&json).unwrap());
        assert_eq!(restored.attempts_for("REQ-001"), [attempt("REQ-001")]);
    }

    #[test]
    fn the_snapshot_schema_is_strict_with_no_compat_defaults() {
        // A fresh machine comes from a missing file (the adapter's
        // job), never from a lenient parse: partial JSON is an error.
        assert!(serde_json::from_str::<TddSnapshot>("{}").is_err());
        assert!(
            serde_json::from_str::<TddSnapshot>(r#"{"instructions": "x"}"#).is_err(),
            "entries is required"
        );
    }

    #[test]
    fn a_0_2_3_attempt_without_outcome_loads_as_unverified() {
        let json = r#"{
            "instructions": "x",
            "entries": [{
                "timestamp": "1970-01-01T00:00:00Z",
                "phase": "RED",
                "lastRun": {"tests": 1, "failures": 1, "errors": 0, "skipped": 0, "failureDetails": []},
                "refactorLog": [],
                "attemptLog": [{"requirement": "REQ-001", "targets": [], "failures": []}]
            }]
        }"#;
        let snapshot: TddSnapshot = serde_json::from_str(json).unwrap();
        assert!(
            snapshot.entries[0].attempt_log[0].outcome.is_empty(),
            "a missing outcome is an unverified attempt, not a parse error"
        );
    }

    #[test]
    fn every_mutation_appends_a_timestamped_entry() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        machine.record_attempt(attempt("REQ-001"));
        machine.record_test_run(passing_run());
        machine.start_refactor(Some("extract parser")).unwrap();
        let snapshot = machine.snapshot();
        assert_eq!(snapshot.entries.len(), 4);
        assert_eq!(
            snapshot.entries.iter().map(|e| e.phase).collect::<Vec<_>>(),
            [
                TddPhase::Red,
                TddPhase::Red,
                TddPhase::Green,
                TddPhase::Refactor
            ]
        );
        assert!(
            snapshot
                .entries
                .iter()
                .all(|e| parseable_rfc3339(&e.timestamp)),
            "timestamps: {:?}",
            snapshot
                .entries
                .iter()
                .map(|e| &e.timestamp)
                .collect::<Vec<_>>()
        );
        assert_eq!(snapshot.instructions, STATE_INSTRUCTIONS);
        assert!(snapshot.instructions.contains("three most recent entries"));
    }

    #[test]
    fn the_llm_brief_is_only_the_three_latest_entries() {
        let mut machine = TddStateMachine::new();
        for _ in 0..5 {
            machine.record_test_run(failing_run());
        }
        let snapshot = machine.snapshot();
        assert_eq!(snapshot.entries.len(), 5);
        let brief = snapshot.recent_entries();
        assert_eq!(brief.len(), LLM_STATE_ENTRIES);
        assert_eq!(brief[0].timestamp, snapshot.entries[2].timestamp);
        assert_eq!(brief[2].timestamp, snapshot.entries[4].timestamp);
        assert!(brief.iter().all(|e| e.phase == TddPhase::Red));
    }

    #[test]
    fn a_short_log_is_briefed_in_full() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(passing_run());
        let snapshot = machine.snapshot();
        assert_eq!(snapshot.recent_entries().len(), 1);
        assert_eq!(snapshot.recent_entries()[0].phase, TddPhase::Green);
    }

    #[test]
    fn a_refused_refactor_does_not_append_an_entry() {
        let mut machine = TddStateMachine::new();
        machine.record_test_run(failing_run());
        assert!(machine.start_refactor(Some("nope")).is_err());
        assert_eq!(machine.snapshot().entries.len(), 1);
    }
}

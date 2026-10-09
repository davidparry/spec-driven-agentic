//! The TDD loop use cases: run tests, show the phase, start a refactor.
//! Reply shapes for `run_tests` stay frozen (`harness/tests/mcp_conformance.rs`). `get_tdd_state`
//! adds interpretation instructions and at most the three latest dated
//! entries so an LLM is never briefed with the whole log.

use serde::Serialize;

use crate::domain::human::{Human, bullets, columns, counted, sections, titled};
use crate::domain::tdd::{ImplementAttempt, StateEntry, TddStateMachine};
use crate::ports::{RunnerError, StateStore, TestFilter, TestRunner};

/// The `run_tests` reply.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TestReport {
    pub phase: String,
    pub tests: u32,
    pub failures: u32,
    pub errors: u32,
    pub skipped: u32,
    #[serde(rename = "failureDetails")]
    pub failure_details: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

impl TestReport {
    /// See [`crate::domain::model::build_broken`]. The reply carries the
    /// counts the summary did, so it answers the same question.
    pub fn build_broken(&self) -> bool {
        crate::domain::model::build_broken(self.tests, self.errors)
    }
}

impl Human for TestReport {
    fn human(&self) -> String {
        sections(&[
            format!(
                "{}  {}",
                self.phase,
                counts(self.tests, self.failures, self.errors, self.skipped)
            ),
            bullets(&self.failure_details),
        ])
    }

    fn next_step(&self) -> Option<&str> {
        Some(&self.next_step)
    }
}

/// A test run as one line.
///
/// Zeroes are left out: `12 tests, 2 failures` is the whole story, and
/// printing `0 errors, 0 skipped` beside it buries the two numbers that
/// changed under two that did not.
fn counts(tests: u32, failures: u32, errors: u32, skipped: u32) -> String {
    let mut parts = vec![counted(tests as usize, "test", "tests")];
    for (count, singular, plural) in [
        (failures, "failure", "failures"),
        (errors, "error", "errors"),
        (skipped, "skipped", "skipped"),
    ] {
        if count > 0 {
            parts.push(counted(count as usize, singular, plural));
        }
    }
    parts.join(", ")
}

/// The `get_tdd_state` reply. `lastRun` intentionally omits the failure
/// details. `entries` is the LLM brief: at most the three latest dated
/// states, plus the instructions for reading them. The on-disk log may
/// be longer.
/// Field order is the reading order. `instructions` is ~900 characters
/// of unchanging guidance and it used to come first, so `spec state` -
/// the command the guide sends a stuck student to - answered "what
/// phase am I in?" with a page of prose before the one word they
/// wanted. Every field is still here, so the agent reading this over
/// MCP loses nothing.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StateReport {
    pub phase: String,
    #[serde(rename = "lastRun")]
    pub last_run: LastRun,
    #[serde(rename = "nextStep")]
    pub next_step: String,
    #[serde(rename = "refactorLog")]
    pub refactor_log: Vec<String>,
    /// At most the three latest dated entries. Older history stays on disk.
    pub entries: Vec<ReportedStateEntry>,
    pub instructions: String,
}

impl Human for StateReport {
    /// The phase, the last run, and the recent history.
    ///
    /// `instructions` is left out on purpose: it is ~900 characters of
    /// unchanging guidance written to brief a model, and `spec state`
    /// is the command a stuck student is sent to. Printing it would
    /// answer "what phase am I in?" with a page of prose. Anything
    /// reading the JSON still gets it.
    fn human(&self) -> String {
        let history: Vec<Vec<String>> = self
            .entries
            .iter()
            .map(|entry| {
                vec![
                    entry.timestamp.clone(),
                    entry.phase.clone(),
                    counts(
                        entry.last_run.tests,
                        entry.last_run.failures,
                        entry.last_run.errors,
                        entry.last_run.skipped,
                    ),
                ]
            })
            .collect();
        let header = columns(&[
            vec!["Phase".to_string(), self.phase.clone()],
            vec![
                "Last run".to_string(),
                counts(
                    self.last_run.tests,
                    self.last_run.failures,
                    self.last_run.errors,
                    self.last_run.skipped,
                ),
            ],
        ]);
        sections(&[
            header,
            titled("Refactor log", &bullets(&self.refactor_log)),
            titled("Recent states", &columns(&history)),
        ])
    }

    fn next_step(&self) -> Option<&str> {
        Some(&self.next_step)
    }
}

/// One dated state as an agent/LLM sees it: counts only, no stack traces.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ReportedStateEntry {
    pub timestamp: String,
    pub phase: String,
    #[serde(rename = "lastRun")]
    pub last_run: LastRun,
    #[serde(rename = "refactorLog")]
    pub refactor_log: Vec<String>,
    #[serde(rename = "attemptLog")]
    pub attempt_log: Vec<ImplementAttempt>,
}

/// The brief for a model implementation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationBrief {
    pub failures: Vec<String>,
    pub history: Vec<ImplementAttempt>,
    /// The three latest dated state entries, oldest first.
    pub states: Vec<StateEntry>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LastRun {
    pub tests: u32,
    pub failures: u32,
    pub errors: u32,
    pub skipped: u32,
}

fn last_run_counts(last: &crate::domain::model::TestRunSummary) -> LastRun {
    LastRun {
        tests: last.tests,
        failures: last.failures,
        errors: last.errors,
        skipped: last.skipped,
    }
}

fn test_next_step(summary: &crate::domain::model::TestRunSummary, suggestion: &str) -> String {
    if summary.no_tests() {
        return "No test reports were found after a successful build. The runner \
                missed Surefire output - check the project layout (for this workshop, \
                tests live under kata/)."
            .into();
    }
    if summary.build_broken()
        && summary
            .failure_details
            .iter()
            .any(|d| d.contains("parent POM") || d.contains("Fix the POM"))
    {
        return "The build failed before tests could run. Fix the POM named in the \
                failure details, then run spec test again."
            .into();
    }
    suggestion.to_string()
}

fn reported_entry(entry: &StateEntry) -> ReportedStateEntry {
    ReportedStateEntry {
        timestamp: entry.timestamp.clone(),
        phase: entry.phase.to_string(),
        last_run: last_run_counts(&entry.last_run),
        refactor_log: entry.refactor_log.clone(),
        attempt_log: entry.attempt_log.clone(),
    }
}

/// The `start_refactor` reply.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RefactorReport {
    pub phase: String,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

impl Human for RefactorReport {
    fn human(&self) -> String {
        format!("Phase  {}", self.phase)
    }

    fn next_step(&self) -> Option<&str> {
        Some(&self.next_step)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TddError {
    /// The project's runtime is missing; the harness reports, never installs.
    RuntimeMissing {
        runtime: String,
        hint: String,
    },
    Other(String),
}

pub struct TddService<S: StateStore> {
    state: S,
}

impl<S: StateStore> TddService<S> {
    pub fn new(state: S) -> Self {
        Self { state }
    }

    pub fn run_tests(
        &self,
        runner: &dyn TestRunner,
        filter: &TestFilter,
    ) -> Result<TestReport, TddError> {
        tracing::info!(filter = ?filter, "running tests");
        let mut machine = self.machine()?;
        let summary = runner.run(filter).map_err(|e| match e {
            RunnerError::RuntimeMissing { runtime, hint } => {
                TddError::RuntimeMissing { runtime, hint }
            }
            RunnerError::Failed(message) => TddError::Other(message),
        })?;
        let phase = machine.record_test_run(summary.clone());
        self.save(&machine)?;
        let (tests, failures, errors) = (summary.tests, summary.failures, summary.errors);
        tracing::info!(phase = %phase, tests, failures, errors, "test run recorded");
        Ok(TestReport {
            phase: phase.to_string(),
            tests: summary.tests,
            failures: summary.failures,
            errors: summary.errors,
            skipped: summary.skipped,
            failure_details: summary.failure_details.clone(),
            next_step: test_next_step(&summary, machine.suggestion()),
        })
    }

    pub fn state(&self) -> Result<StateReport, TddError> {
        let machine = self.machine()?;
        let snapshot = machine.snapshot();
        let last = machine.last_run();
        Ok(StateReport {
            instructions: snapshot.instructions.clone(),
            phase: machine.phase().to_string(),
            last_run: last_run_counts(last),
            refactor_log: machine.refactor_log().to_vec(),
            entries: snapshot
                .recent_entries()
                .iter()
                .map(reported_entry)
                .collect(),
            next_step: machine.suggestion().to_string(),
        })
    }

    pub fn refactor(&self, note: Option<&str>) -> Result<RefactorReport, TddError> {
        let mut machine = self.machine()?;
        let phase = machine.start_refactor(note).map_err(TddError::Other)?;
        self.save(&machine)?;
        Ok(RefactorReport {
            phase: phase.to_string(),
            next_step: machine.suggestion().to_string(),
        })
    }

    /// The brief for a model implementation attempt: the persisted
    /// failure details of the last run - stack traces and all - plus
    /// every prior attempt recorded for this requirement, and only the
    /// three latest dated state entries.
    pub fn implementation_brief(&self, req_id: &str) -> Result<ImplementationBrief, TddError> {
        let machine = self.machine()?;
        Ok(ImplementationBrief {
            failures: machine.last_run().failure_details.clone(),
            history: machine.attempts_for(req_id),
            states: machine.snapshot().recent_entries().to_vec(),
        })
    }

    /// Persist one model implementation attempt so the next attempt's
    /// brief includes it.
    pub fn record_attempt(&self, attempt: ImplementAttempt) -> Result<(), TddError> {
        let mut machine = self.machine()?;
        machine.record_attempt(attempt);
        self.save(&machine)
    }

    fn machine(&self) -> Result<TddStateMachine, TddError> {
        let snapshot = self.state.load().map_err(|e| TddError::Other(e.0))?;
        Ok(TddStateMachine::restore(snapshot))
    }

    fn save(&self, machine: &TddStateMachine) -> Result<(), TddError> {
        self.state
            .save(&machine.snapshot())
            .map_err(|e| TddError::Other(e.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::TestRunSummary;
    use crate::domain::tdd::{STATE_INSTRUCTIONS, StateEntry, TddPhase, TddSnapshot};
    use crate::test_support::FixedStateStore;

    struct ScriptedRunner(Result<TestRunSummary, RunnerError>);

    impl TestRunner for ScriptedRunner {
        fn run(&self, _: &TestFilter) -> Result<TestRunSummary, RunnerError> {
            self.0.clone()
        }
    }

    fn passing() -> ScriptedRunner {
        ScriptedRunner(Ok(TestRunSummary {
            tests: 8,
            ..Default::default()
        }))
    }

    fn failing() -> ScriptedRunner {
        ScriptedRunner(Ok(TestRunSummary {
            tests: 8,
            failures: 2,
            failure_details: vec!["CalcTest.adds: expected 3".into()],
            ..Default::default()
        }))
    }

    fn fresh() -> FixedStateStore {
        FixedStateStore::holding(TddSnapshot::default())
    }

    fn green_state() -> FixedStateStore {
        FixedStateStore::holding(TddSnapshot::with(StateEntry {
            timestamp: "1970-01-01T00:00:00Z".into(),
            phase: TddPhase::Green,
            last_run: TestRunSummary {
                tests: 8,
                ..Default::default()
            },
            ..Default::default()
        }))
    }

    #[test]
    fn a_failing_run_reports_red_with_details_and_persists() {
        let service = TddService::new(fresh());
        let report = service
            .run_tests(&failing(), &TestFilter::default())
            .unwrap();
        assert_eq!(report.phase, "RED");
        assert_eq!(report.failures, 2);
        assert_eq!(report.failure_details, vec!["CalcTest.adds: expected 3"]);
        assert!(report.next_step.starts_with("Tests are failing."));
        assert_eq!(service.state.saved.borrow()[0].phase(), TddPhase::Red);
    }

    #[test]
    fn a_passing_run_reports_green() {
        let service = TddService::new(fresh());
        let report = service
            .run_tests(&passing(), &TestFilter::default())
            .unwrap();
        assert_eq!(report.phase, "GREEN");
        assert!(report.next_step.starts_with("All tests pass."));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("failureDetails"));
        assert!(json.contains("nextStep"));
    }

    #[test]
    fn a_successful_run_with_no_tests_does_not_report_red() {
        let service = TddService::new(fresh());
        let runner = ScriptedRunner(Ok(TestRunSummary::default()));
        let report = service.run_tests(&runner, &TestFilter::default()).unwrap();
        assert_eq!(report.phase, "START");
        assert!(report.next_step.contains("missed Surefire"));
        assert_eq!(service.state.saved.borrow()[0].phase(), TddPhase::Start);
    }

    #[test]
    fn a_missing_runtime_passes_through_untouched_and_saves_nothing() {
        let service = TddService::new(fresh());
        let runner = ScriptedRunner(Err(RunnerError::RuntimeMissing {
            runtime: "mvn".into(),
            hint: "Install Maven.".into(),
        }));
        let error = service
            .run_tests(&runner, &TestFilter::default())
            .unwrap_err();
        assert_eq!(
            error,
            TddError::RuntimeMissing {
                runtime: "mvn".into(),
                hint: "Install Maven.".into(),
            }
        );
        assert!(service.state.saved.borrow().is_empty());
    }

    #[test]
    fn a_failed_runner_is_an_ordinary_error() {
        let service = TddService::new(fresh());
        let runner = ScriptedRunner(Err(RunnerError::Failed("boom".into())));
        assert_eq!(
            service
                .run_tests(&runner, &TestFilter::default())
                .unwrap_err(),
            TddError::Other("boom".into())
        );
    }

    #[test]
    fn state_reports_the_persisted_machine_without_failure_details() {
        let service = TddService::new(green_state());
        let report = service.state().unwrap();
        assert_eq!(report.phase, "GREEN");
        assert_eq!(report.last_run.tests, 8);
        assert_eq!(report.instructions, STATE_INSTRUCTIONS);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].timestamp, "1970-01-01T00:00:00Z");
        assert_eq!(report.entries[0].phase, "GREEN");
        assert!(report.next_step.starts_with("All tests pass."));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("lastRun"));
        assert!(json.contains("refactorLog"));
        assert!(json.contains("instructions"));
        assert!(json.contains("entries"));
        assert!(!json.contains("failureDetails"));
    }

    #[test]
    fn refactor_from_green_moves_to_refactor_and_persists_the_note() {
        let service = TddService::new(green_state());
        let report = service.refactor(Some("extract parser")).unwrap();
        assert_eq!(report.phase, "REFACTOR");
        assert!(report.next_step.starts_with("A refactor is in progress."));
        assert_eq!(
            service.state.saved.borrow()[0].refactor_log(),
            ["extract parser"]
        );
    }

    #[test]
    fn refactor_off_green_is_refused_with_the_java_message() {
        let service = TddService::new(fresh());
        assert_eq!(
            service.refactor(None).unwrap_err(),
            TddError::Other(
                "Refactoring is only allowed from GREEN (current phase: START). \
                 No tests have been run yet — run them to find out where you are."
                    .into()
            )
        );
    }

    #[test]
    fn the_implementation_brief_carries_failures_and_prior_attempts() {
        let service = TddService::new(fresh());
        service
            .run_tests(&failing(), &TestFilter::default())
            .unwrap();
        let store = FixedStateStore::holding(service.state.saved.borrow().last().unwrap().clone());
        let service = TddService::new(store);
        service
            .record_attempt(ImplementAttempt {
                requirement: "REQ-001".into(),
                targets: vec!["src/main/java/Calc.java".into()],
                failures: vec!["CalcTest.adds: expected 3".into()],
                ..Default::default()
            })
            .unwrap();
        // A follow-up RED run attaches its output as the attempt's outcome.
        let store = FixedStateStore::holding(service.state.saved.borrow().last().unwrap().clone());
        let service = TddService::new(store);
        service
            .run_tests(&failing(), &TestFilter::default())
            .unwrap();
        let store = FixedStateStore::holding(service.state.saved.borrow().last().unwrap().clone());
        let brief = TddService::new(store)
            .implementation_brief("REQ-001")
            .unwrap();
        assert_eq!(brief.failures, vec!["CalcTest.adds: expected 3"]);
        assert_eq!(brief.history.len(), 1);
        assert_eq!(brief.history[0].targets, vec!["src/main/java/Calc.java"]);
        assert_eq!(
            brief.history[0].outcome,
            vec!["CalcTest.adds: expected 3"],
            "the brief carries what the attempt's run actually reported"
        );
        assert_eq!(brief.states.len(), 3, "RED run, the attempt, its run");
        assert!(
            brief
                .states
                .iter()
                .all(|e| chrono::DateTime::parse_from_rfc3339(&e.timestamp).is_ok())
        );
    }

    #[test]
    fn the_brief_only_includes_attempts_for_the_requested_requirement() {
        let service = TddService::new(fresh());
        service
            .record_attempt(ImplementAttempt {
                requirement: "REQ-002".into(),
                ..Default::default()
            })
            .unwrap();
        let store = FixedStateStore::holding(service.state.saved.borrow().last().unwrap().clone());
        let brief = TddService::new(store)
            .implementation_brief("REQ-001")
            .unwrap();
        assert!(brief.history.is_empty());
    }

    #[test]
    fn the_state_reply_caps_entries_to_the_three_latest() {
        let mut entries = Vec::new();
        for i in 1..=5 {
            entries.push(StateEntry {
                timestamp: format!("2026-08-0{i}T00:00:00Z"),
                phase: TddPhase::Red,
                last_run: TestRunSummary {
                    tests: i,
                    failures: 1,
                    failure_details: vec!["secret stack".into()],
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        let service = TddService::new(FixedStateStore::holding(TddSnapshot {
            instructions: STATE_INSTRUCTIONS.into(),
            entries,
        }));
        let report = service.state().unwrap();
        assert_eq!(report.entries.len(), 3);
        assert_eq!(report.entries[0].timestamp, "2026-08-03T00:00:00Z");
        assert_eq!(report.entries[2].timestamp, "2026-08-05T00:00:00Z");
        assert_eq!(report.entries[2].last_run.tests, 5);
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains("secret stack"),
            "failure details stay off the LLM brief"
        );
        let brief = service.implementation_brief("REQ-001").unwrap();
        assert_eq!(brief.states.len(), 3);
        assert_eq!(brief.states[0].timestamp, "2026-08-03T00:00:00Z");
    }

    #[test]
    fn a_parent_pom_failure_names_the_pom_in_the_next_step() {
        let service = TddService::new(fresh());
        let runner = ScriptedRunner(Ok(TestRunSummary {
            tests: 0,
            errors: 1,
            failure_details: vec!["Non-resolvable parent POM for com.example:kata".into()],
            ..Default::default()
        }));
        let report = service.run_tests(&runner, &TestFilter::default()).unwrap();
        assert!(
            report.next_step.contains("Fix the POM named in the"),
            "next step: {}",
            report.next_step
        );
    }

    #[test]
    fn a_failing_state_store_propagates() {
        let service = TddService::new(FixedStateStore::failing("state boom"));
        assert_eq!(
            service.state().unwrap_err(),
            TddError::Other("state boom".into())
        );
        assert_eq!(
            service
                .run_tests(&passing(), &TestFilter::default())
                .unwrap_err(),
            TddError::Other("state boom".into())
        );
        assert_eq!(
            service.refactor(None).unwrap_err(),
            TddError::Other("state boom".into())
        );
        assert_eq!(
            service.implementation_brief("REQ-001").unwrap_err(),
            TddError::Other("state boom".into())
        );
        assert_eq!(
            service
                .record_attempt(ImplementAttempt::default())
                .unwrap_err(),
            TddError::Other("state boom".into())
        );
    }

    fn test_report(failures: u32, details: Vec<String>) -> TestReport {
        TestReport {
            phase: "RED".into(),
            tests: 12,
            failures,
            errors: 0,
            skipped: 0,
            failure_details: details,
            next_step: "Run spec implement REQ-001.".into(),
        }
    }

    /// Zeroes beside the numbers that changed bury them. A green run
    /// is "12 tests" and nothing else.
    #[test]
    fn a_green_run_reports_only_the_count_that_matters() {
        assert_eq!(test_report(0, Vec::new()).human(), "RED  12 tests");
    }

    #[test]
    fn a_failing_run_names_its_failures() {
        let rendered = test_report(2, vec!["adds: expected 3 but was 0".into()]).human();
        assert_eq!(
            rendered,
            "RED  12 tests, 2 failures\n\n  - adds: expected 3 but was 0"
        );
    }

    #[test]
    fn errors_and_skips_are_reported_when_there_are_any() {
        let mut report = test_report(0, Vec::new());
        report.errors = 1;
        report.skipped = 3;
        assert_eq!(report.human(), "RED  12 tests, 1 error, 3 skipped");
        assert_eq!(report.next_step(), Some("Run spec implement REQ-001."));
    }

    fn state_report(refactor_log: Vec<String>, entries: Vec<ReportedStateEntry>) -> StateReport {
        StateReport {
            phase: "GREEN".into(),
            last_run: LastRun {
                tests: 12,
                failures: 0,
                errors: 0,
                skipped: 0,
            },
            next_step: "Run spec refactor.".into(),
            refactor_log,
            entries,
            instructions: "A very long brief written for a model. ".repeat(30),
        }
    }

    /// `spec state` is where a stuck student is sent, and the brief is
    /// ~900 characters of guidance written for a model. Printing it
    /// would answer "what phase am I in?" with a page of prose.
    #[test]
    fn the_model_brief_never_reaches_the_terminal() {
        let rendered = state_report(Vec::new(), Vec::new()).human();
        assert!(!rendered.contains("written for a model"), "{rendered}");
        assert_eq!(rendered, "Phase     GREEN\nLast run  12 tests");
        assert_eq!(
            state_report(Vec::new(), Vec::new()).next_step(),
            Some("Run spec refactor.")
        );
    }

    /// `spec refactor --manual` marks the phase and stops, so the
    /// phase it moved to is the whole reply.
    #[test]
    fn a_marked_refactor_reports_the_phase_it_moved_to() {
        let report = RefactorReport {
            phase: "REFACTOR".into(),
            next_step: "Clean up, then run spec test.".into(),
        };
        assert_eq!(report.human(), "Phase  REFACTOR");
        assert_eq!(report.next_step(), Some("Clean up, then run spec test."));
    }

    #[test]
    fn the_refactor_log_and_recent_states_appear_when_there_are_any() {
        let entry = ReportedStateEntry {
            timestamp: "2026-10-05".into(),
            phase: "RED".into(),
            last_run: LastRun {
                tests: 12,
                failures: 2,
                errors: 0,
                skipped: 0,
            },
            refactor_log: Vec::new(),
            attempt_log: Vec::new(),
        };
        let rendered = state_report(vec!["extracted a helper".into()], vec![entry]).human();
        assert!(
            rendered.contains("Refactor log\n  - extracted a helper"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Recent states\n2026-10-05  RED  12 tests, 2 failures"),
            "{rendered}"
        );
    }
}

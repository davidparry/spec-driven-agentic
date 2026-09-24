//! The guarded command use case behind the `command_run` MCP tool.
//! Order of checks: the argv policy first (allowlist, path jail,
//! eval-escape flags), then the phase gate — commands only run on a RED
//! bar, the implementation phase — and only then does the executor
//! spawn anything, pinned to the project root with a hard timeout.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::application::spec_service::ServiceError;
use crate::domain::command_policy;
use crate::domain::tdd::{TddPhase, TddStateMachine};
use crate::ports::{CommandExecutor, StateStore};

/// The default and maximum timeout for one command, in seconds.
pub const MAX_TIMEOUT_SECS: u64 = 300;

/// The `command_run` reply: what ran and what it reported.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CommandReport {
    pub command: Vec<String>,
    /// `null` when the process was killed by the timeout.
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    #[serde(rename = "timedOut")]
    pub timed_out: bool,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct CommandService<S: StateStore, E: CommandExecutor> {
    state: S,
    executor: E,
    root: PathBuf,
}

impl<S: StateStore, E: CommandExecutor> CommandService<S, E> {
    pub fn new(state: S, executor: E, root: PathBuf) -> Self {
        Self {
            state,
            executor,
            root,
        }
    }

    pub fn run(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
    ) -> Result<CommandReport, ServiceError> {
        command_policy::validate(argv)?;
        let phase = self.phase()?;
        if phase != TddPhase::Red {
            return Err(ServiceError(format!(
                "Commands only run during the implementation phase — a RED bar \
                 (current phase: {phase}). Call run_tests first; failing tests \
                 are what an implementation command is for.",
            )));
        }
        let timeout = Duration::from_secs(
            timeout_secs
                .unwrap_or(MAX_TIMEOUT_SECS)
                .min(MAX_TIMEOUT_SECS),
        );
        let outcome = self.executor.run(argv, &self.root, timeout)?;
        let next_step = if outcome.timed_out {
            format!(
                "The command was killed after {} seconds. Try a narrower command, \
                 then call run_tests.",
                timeout.as_secs()
            )
        } else {
            "Call run_tests to see whether the bar moved.".to_string()
        };
        Ok(CommandReport {
            command: argv.to_vec(),
            exit_code: outcome.exit_code,
            stdout: outcome.stdout,
            stderr: outcome.stderr,
            timed_out: outcome.timed_out,
            duration_ms: outcome.duration_ms,
            next_step,
        })
    }

    fn phase(&self) -> Result<TddPhase, ServiceError> {
        let snapshot = self.state.load()?;
        Ok(TddStateMachine::restore(snapshot).phase())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;

    use super::*;
    use crate::domain::tdd::TddSnapshot;
    use crate::ports::{ExecError, ExecOutcome};
    use crate::test_support::FixedStateStore;

    /// Records what it was asked to run and replies with a script.
    struct FakeExecutor {
        reply: Result<ExecOutcome, ExecError>,
        calls: RefCell<Vec<(Vec<String>, PathBuf, Duration)>>,
    }

    impl FakeExecutor {
        fn replying(outcome: ExecOutcome) -> Self {
            Self {
                reply: Ok(outcome),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn failing(message: &str) -> Self {
            Self {
                reply: Err(ExecError(message.into())),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl CommandExecutor for FakeExecutor {
        fn run(
            &self,
            argv: &[String],
            dir: &Path,
            timeout: Duration,
        ) -> Result<ExecOutcome, ExecError> {
            self.calls
                .borrow_mut()
                .push((argv.to_vec(), dir.to_path_buf(), timeout));
            self.reply.clone()
        }
    }

    fn clean_outcome() -> ExecOutcome {
        ExecOutcome {
            exit_code: Some(0),
            stdout: "compiled".into(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 42,
        }
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    fn red_service(executor: FakeExecutor) -> CommandService<FixedStateStore, FakeExecutor> {
        CommandService::new(
            FixedStateStore::holding(TddSnapshot::at(TddPhase::Red)),
            executor,
            PathBuf::from("/project/root"),
        )
    }

    #[test]
    fn an_allowed_command_on_red_runs_in_the_root_and_reports() {
        let service = red_service(FakeExecutor::replying(clean_outcome()));
        let report = service.run(&argv(&["cargo", "build"]), None).unwrap();
        assert_eq!(report.command, argv(&["cargo", "build"]));
        assert_eq!(report.exit_code, Some(0));
        assert_eq!(report.stdout, "compiled");
        assert!(!report.timed_out);
        assert_eq!(report.duration_ms, 42);
        assert_eq!(
            report.next_step,
            "Call run_tests to see whether the bar moved."
        );
        let calls = service.executor.calls.borrow();
        assert_eq!(calls[0].1, PathBuf::from("/project/root"));
        assert_eq!(calls[0].2, Duration::from_secs(MAX_TIMEOUT_SECS));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("exitCode"));
        assert!(json.contains("timedOut"));
        assert!(json.contains("durationMs"));
        assert!(json.contains("nextStep"));
    }

    #[test]
    fn the_policy_refuses_before_anything_runs() {
        let service = red_service(FakeExecutor::replying(clean_outcome()));
        let error = service.run(&argv(&["rm", "-rf", "."]), None).unwrap_err();
        assert!(error.0.contains("not on the allowlist"), "got: {}", error.0);
        assert!(
            service.executor.calls.borrow().is_empty(),
            "a refused command never spawns"
        );
    }

    #[test]
    fn commands_off_a_red_bar_are_refused() {
        for phase in [TddPhase::Start, TddPhase::Green, TddPhase::Refactor] {
            let service = CommandService::new(
                FixedStateStore::holding(TddSnapshot::at(phase)),
                FakeExecutor::replying(clean_outcome()),
                PathBuf::from("/project/root"),
            );
            let error = service.run(&argv(&["cargo", "build"]), None).unwrap_err();
            assert!(
                error.0.contains(&format!("current phase: {phase}")),
                "got: {}",
                error.0
            );
            assert!(error.0.contains("run_tests"));
            assert!(service.executor.calls.borrow().is_empty());
        }
    }

    #[test]
    fn the_timeout_is_capped_at_the_maximum() {
        let service = red_service(FakeExecutor::replying(clean_outcome()));
        service.run(&argv(&["cargo", "build"]), Some(10)).unwrap();
        service.run(&argv(&["cargo", "build"]), Some(9999)).unwrap();
        let calls = service.executor.calls.borrow();
        assert_eq!(calls[0].2, Duration::from_secs(10));
        assert_eq!(calls[1].2, Duration::from_secs(MAX_TIMEOUT_SECS));
    }

    #[test]
    fn a_timed_out_command_reports_the_kill_in_the_next_step() {
        let service = red_service(FakeExecutor::replying(ExecOutcome {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
            duration_ms: 5_000,
        }));
        let report = service.run(&argv(&["cargo", "build"]), Some(5)).unwrap();
        assert!(report.timed_out);
        assert_eq!(report.exit_code, None);
        assert!(report.next_step.contains("killed after 5 seconds"));
    }

    #[test]
    fn executor_failures_surface_as_service_errors() {
        let service = red_service(FakeExecutor::failing("unable to launch cargo - gone"));
        let error = service.run(&argv(&["cargo", "build"]), None).unwrap_err();
        assert_eq!(error.0, "unable to launch cargo - gone");
    }

    #[test]
    fn a_failing_state_store_propagates() {
        let service = CommandService::new(
            FixedStateStore::failing("state boom"),
            FakeExecutor::replying(clean_outcome()),
            PathBuf::from("/project/root"),
        );
        let error = service.run(&argv(&["cargo", "build"]), None).unwrap_err();
        assert_eq!(error.0, "state boom");
    }
}

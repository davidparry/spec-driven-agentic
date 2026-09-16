//! Cargo test runner: runs `cargo test` and parses the `test result:`
//! summary lines from its output.

use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

use super::{build_failure, run_command, tail};
use crate::domain::model::TestRunSummary;
use crate::ports::{RunnerError, RuntimeProbe, TestFilter, TestRunner};

static RESULT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored")
        .expect("valid regex")
});
static FAILED_TEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^test (\S+) \.\.\. FAILED$").expect("valid regex"));

pub struct CargoRunner<R: RuntimeProbe> {
    root: PathBuf,
    probe: R,
    command: Vec<String>,
}

impl<R: RuntimeProbe> CargoRunner<R> {
    pub fn new(root: PathBuf, probe: R) -> Self {
        Self {
            root,
            probe,
            command: vec!["cargo".into(), "test".into()],
        }
    }

    /// Visible for tests: run an arbitrary command in place of cargo.
    pub fn with_command(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }
}

impl<R: RuntimeProbe> TestRunner for CargoRunner<R> {
    fn run(&self, filter: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        if self.probe.version("cargo").is_none() {
            return Err(RunnerError::RuntimeMissing {
                runtime: "cargo".into(),
                hint: "Install the Rust toolchain from https://rustup.rs, then rerun.".into(),
            });
        }
        let mut command = self.command.clone();
        if let Some(scenario) = &filter.scenario {
            command.push(scenario.clone());
        }
        let outcome = run_command(&command, &self.root)?;
        let combined = outcome.combined();
        match parse_cargo_output(&combined) {
            Some(summary) => Ok(summary),
            None if !outcome.success => Ok(build_failure(&combined)),
            None => Err(RunnerError::Failed(format!(
                "cargo test produced no test summary:\n{}",
                tail(&combined, 30)
            ))),
        }
    }
}

/// Sum every `test result:` line; `None` when there is none (the build
/// never reached the test phase).
pub fn parse_cargo_output(output: &str) -> Option<TestRunSummary> {
    let mut found = false;
    let mut summary = TestRunSummary::default();
    for captures in RESULT_LINE.captures_iter(output) {
        found = true;
        let passed: u32 = captures[1].parse().expect("digits only");
        let failed: u32 = captures[2].parse().expect("digits only");
        let ignored: u32 = captures[3].parse().expect("digits only");
        summary.tests += passed + failed;
        summary.failures += failed;
        summary.skipped += ignored;
    }
    if !found {
        return None;
    }
    for captures in FAILED_TEST.captures_iter(output) {
        summary
            .failure_details
            .push(format!("{}: FAILED", &captures[1]));
    }
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeRuntimeProbe;

    const PASSING: &str = "running 3 tests\n...\ntest result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n";
    const FAILING: &str = "running 2 tests\ntest domain::adds ... FAILED\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";

    #[test]
    fn passing_output_sums_the_result_lines() {
        let summary = parse_cargo_output(PASSING).unwrap();
        assert_eq!(summary.tests, 3);
        assert_eq!(summary.failures, 0);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn multiple_test_binaries_are_summed() {
        let output = format!("{PASSING}{FAILING}");
        let summary = parse_cargo_output(&output).unwrap();
        assert_eq!(summary.tests, 5);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.failure_details, vec!["domain::adds: FAILED"]);
    }

    #[test]
    fn output_without_a_result_line_is_none() {
        assert_eq!(parse_cargo_output("error[E0308]: mismatched types"), None);
    }

    fn runner(dir: &std::path::Path, script: &str) -> CargoRunner<FakeRuntimeProbe> {
        CargoRunner::new(dir.to_path_buf(), FakeRuntimeProbe::with(&["cargo"])).with_command(vec![
            "sh".into(),
            "-c".into(),
            script.into(),
        ])
    }

    #[test]
    fn a_run_parses_the_summary_lines() {
        let dir = tempfile::tempdir().unwrap();
        let summary = runner(
            dir.path(),
            "echo 'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured'",
        )
        .run(&TestFilter::default())
        .unwrap();
        assert_eq!(summary.tests, 4);
    }

    #[test]
    fn a_scenario_filter_is_passed_through() {
        let dir = tempfile::tempdir().unwrap();
        let filter = TestFilter {
            feature: None,
            scenario: Some("adds".into()),
        };
        let summary = runner(
            dir.path(),
            "echo 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured'",
        )
        .run(&filter)
        .unwrap();
        assert_eq!(summary.tests, 1);
    }

    #[test]
    fn a_compile_error_is_one_error_with_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let summary = runner(dir.path(), "echo 'error[E0308]'; exit 101")
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.errors, 1);
        assert!(summary.failure_details[0].contains("error[E0308]"));
    }

    #[test]
    fn a_successful_run_without_a_summary_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let error = runner(dir.path(), "echo nothing")
            .run(&TestFilter::default())
            .unwrap_err();
        assert!(
            matches!(&error, RunnerError::Failed(m) if m.starts_with("cargo test produced no test summary:"))
        );
    }

    #[test]
    fn a_missing_cargo_runtime_is_refused_with_a_hint() {
        let dir = tempfile::tempdir().unwrap();
        let runner = CargoRunner::new(dir.path().to_path_buf(), FakeRuntimeProbe::default());
        assert_eq!(
            runner.run(&TestFilter::default()).unwrap_err(),
            RunnerError::RuntimeMissing {
                runtime: "cargo".into(),
                hint: "Install the Rust toolchain from https://rustup.rs, then rerun.".into(),
            }
        );
    }
}

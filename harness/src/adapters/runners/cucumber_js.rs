//! cucumber-js test runner (JavaScript and TypeScript): runs
//! `npx cucumber-js --format json` and parses the JSON report.

use std::path::PathBuf;

use serde::Deserialize;

use super::{run_command, tail};
use crate::domain::model::TestRunSummary;
use crate::ports::{RunnerError, RuntimeProbe, TestFilter, TestRunner};

pub struct CucumberJsRunner<R: RuntimeProbe> {
    root: PathBuf,
    probe: R,
    command: Vec<String>,
}

impl<R: RuntimeProbe> CucumberJsRunner<R> {
    pub fn new(root: PathBuf, probe: R) -> Self {
        Self {
            root,
            probe,
            command: vec![
                "npx".into(),
                "cucumber-js".into(),
                "--format".into(),
                "json".into(),
            ],
        }
    }

    /// Visible for tests: run an arbitrary command in place of npx.
    pub fn with_command(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }
}

impl<R: RuntimeProbe> TestRunner for CucumberJsRunner<R> {
    fn run(&self, filter: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        if self.probe.version("node").is_none() {
            return Err(RunnerError::RuntimeMissing {
                runtime: "node".into(),
                hint: "Install Node.js from https://nodejs.org, then rerun.".into(),
            });
        }
        let mut command = self.command.clone();
        if let Some(scenario) = &filter.scenario {
            command.push("--name".into());
            command.push(scenario.clone());
        }
        if let Some(feature) = &filter.feature {
            command.push(feature.clone());
        }
        let outcome = run_command(&command, &self.root)?;
        parse_json_report(&outcome.stdout).map_err(|_| {
            RunnerError::Failed(format!(
                "cucumber-js produced no JSON report:\n{}",
                tail(&outcome.combined(), 30)
            ))
        })
    }
}

#[derive(Deserialize)]
struct JsFeature {
    #[serde(default)]
    name: String,
    #[serde(default)]
    elements: Vec<JsElement>,
}

#[derive(Deserialize)]
struct JsElement {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    steps: Vec<JsStep>,
}

#[derive(Deserialize)]
struct JsStep {
    #[serde(default)]
    name: String,
    result: Option<JsResult>,
}

#[derive(Deserialize)]
struct JsResult {
    #[serde(default)]
    status: String,
    error_message: Option<String>,
}

/// One test per scenario: a failed step fails it, an undefined, pending,
/// or ambiguous step is an error, an all-skipped scenario is skipped.
pub fn parse_json_report(json: &str) -> Result<TestRunSummary, String> {
    let features: Vec<JsFeature> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut summary = TestRunSummary::default();
    for feature in &features {
        for scenario in feature.elements.iter().filter(|e| e.kind != "background") {
            summary.tests += 1;
            let label = format!("{} > {}", feature.name, scenario.name);
            let status_of = |step: &JsStep| {
                step.result
                    .as_ref()
                    .map(|r| r.status.clone())
                    .unwrap_or_default()
            };
            if let Some(failed) = scenario.steps.iter().find(|s| status_of(s) == "failed") {
                summary.failures += 1;
                let message = failed
                    .result
                    .as_ref()
                    .and_then(|r| r.error_message.clone())
                    .unwrap_or_else(|| format!("step \"{}\" failed", failed.name));
                summary.failure_details.push(format!("{label}: {message}"));
            } else if let Some(broken) = scenario
                .steps
                .iter()
                .find(|s| matches!(status_of(s).as_str(), "undefined" | "pending" | "ambiguous"))
            {
                summary.errors += 1;
                summary.failure_details.push(format!(
                    "{label}: step \"{}\" is {}",
                    broken.name,
                    status_of(broken)
                ));
            } else if !scenario.steps.is_empty()
                && scenario.steps.iter().all(|s| status_of(s) == "skipped")
            {
                summary.skipped += 1;
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeRuntimeProbe;

    const REPORT: &str = r#"[
      {
        "name": "Calc",
        "elements": [
          {"type": "background", "name": "b", "steps": [{"name": "a calculator", "result": {"status": "passed"}}]},
          {"type": "scenario", "name": "adds", "steps": [{"name": "add", "result": {"status": "passed"}}]},
          {"type": "scenario", "name": "fails", "steps": [{"name": "boom", "result": {"status": "failed", "error_message": "expected 3"}}]},
          {"type": "scenario", "name": "new", "steps": [{"name": "later", "result": {"status": "undefined"}}]},
          {"type": "scenario", "name": "skipped", "steps": [{"name": "s", "result": {"status": "skipped"}}]}
        ]
      }
    ]"#;

    #[test]
    fn scenarios_are_counted_by_their_worst_step() {
        let summary = parse_json_report(REPORT).unwrap();
        assert_eq!(summary.tests, 4);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            summary.failure_details,
            vec![
                "Calc > fails: expected 3",
                "Calc > new: step \"later\" is undefined",
            ]
        );
    }

    #[test]
    fn a_failed_step_without_a_message_names_the_step() {
        let report = r#"[{"name": "F", "elements": [
            {"type": "scenario", "name": "s", "steps": [{"name": "boom", "result": {"status": "failed"}}]}
        ]}]"#;
        let summary = parse_json_report(report).unwrap();
        assert_eq!(summary.failure_details, vec!["F > s: step \"boom\" failed"]);
    }

    #[test]
    fn a_step_without_a_result_counts_as_nothing_special() {
        let report = r#"[{"name": "F", "elements": [
            {"type": "scenario", "name": "s", "steps": [{"name": "quiet"}]}
        ]}]"#;
        let summary = parse_json_report(report).unwrap();
        assert_eq!(summary.tests, 1);
        assert_eq!(summary.failures + summary.errors + summary.skipped, 0);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_json_report("not json").is_err());
    }

    fn runner(dir: &std::path::Path, script: &str) -> CucumberJsRunner<FakeRuntimeProbe> {
        CucumberJsRunner::new(dir.to_path_buf(), FakeRuntimeProbe::with(&["node"]))
            .with_command(vec!["sh".into(), "-c".into(), script.into()])
    }

    #[test]
    fn a_run_parses_the_json_from_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let summary = runner(dir.path(), "echo '[]'")
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 0);
    }

    #[test]
    fn filters_become_name_and_path_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let filter = TestFilter {
            feature: Some("features/calc.feature".into()),
            scenario: Some("adds".into()),
        };
        assert_eq!(
            runner(dir.path(), "echo '[]'").run(&filter).unwrap().tests,
            0
        );
    }

    #[test]
    fn missing_json_reports_the_output_tail() {
        let dir = tempfile::tempdir().unwrap();
        let error = runner(dir.path(), "echo 'npm ERR! not found' >&2; exit 1")
            .run(&TestFilter::default())
            .unwrap_err();
        assert!(
            matches!(&error, RunnerError::Failed(message)
                if message.starts_with("cucumber-js produced no JSON report:")
                    && message.contains("npm ERR! not found")),
            "unexpected: {error:?}"
        );
    }

    #[test]
    fn a_missing_node_runtime_is_refused_with_a_hint() {
        let dir = tempfile::tempdir().unwrap();
        let runner = CucumberJsRunner::new(dir.path().to_path_buf(), FakeRuntimeProbe::default());
        assert_eq!(
            runner.run(&TestFilter::default()).unwrap_err(),
            RunnerError::RuntimeMissing {
                runtime: "node".into(),
                hint: "Install Node.js from https://nodejs.org, then rerun.".into(),
            }
        );
    }
}

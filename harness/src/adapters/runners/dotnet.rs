//! .NET test runner: runs `dotnet test --logger trx` and parses the TRX
//! report(s) from the results directory.

use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{build_failure, run_command};
use crate::domain::model::TestRunSummary;
use crate::ports::{RunnerError, RuntimeProbe, TestFilter, TestRunner};

pub struct DotnetRunner<R: RuntimeProbe> {
    root: PathBuf,
    probe: R,
    command: Vec<String>,
}

impl<R: RuntimeProbe> DotnetRunner<R> {
    pub fn new(root: PathBuf, probe: R) -> Self {
        Self {
            root,
            probe,
            command: vec![
                "dotnet".into(),
                "test".into(),
                "--logger".into(),
                "trx".into(),
                "--results-directory".into(),
                "TestResults".into(),
            ],
        }
    }

    /// Visible for tests: run an arbitrary command in place of dotnet.
    pub fn with_command(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }
}

impl<R: RuntimeProbe> TestRunner for DotnetRunner<R> {
    fn run(&self, filter: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        if self.probe.version("dotnet").is_none() {
            return Err(RunnerError::RuntimeMissing {
                runtime: "dotnet".into(),
                hint: "Install the .NET SDK from https://dotnet.microsoft.com, then rerun.".into(),
            });
        }
        let mut command = self.command.clone();
        if let Some(scenario) = &filter.scenario {
            command.push("--filter".into());
            command.push(format!("Name~{scenario}"));
        }
        let results = self.root.join("TestResults");
        let _ = fs::remove_dir_all(&results);
        let outcome = run_command(&command, &self.root)?;
        let summary = parse_results_dir(&results).map_err(RunnerError::Failed)?;
        if summary.tests == 0 && !outcome.success {
            return Ok(build_failure(&outcome.combined()));
        }
        Ok(summary)
    }
}

/// Sum every `*.trx` report in the results directory. A missing
/// directory is an empty run.
pub fn parse_results_dir(dir: &Path) -> Result<TestRunSummary, String> {
    if !dir.is_dir() {
        return Ok(TestRunSummary::default());
    }
    let mut reports: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("Unable to read TRX reports in {} - {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "trx"))
        .collect();
    reports.sort();
    let mut total = TestRunSummary::default();
    for report in reports {
        let xml = fs::read_to_string(&report)
            .map_err(|e| format!("Unable to read {} - {e}", report.display()))?;
        let one =
            parse_trx(&xml).map_err(|e| format!("Unable to parse {} - {e}", report.display()))?;
        total.tests += one.tests;
        total.failures += one.failures;
        total.errors += one.errors;
        total.skipped += one.skipped;
        total.failure_details.extend(one.failure_details);
    }
    Ok(total)
}

/// Parse one TRX report: the `Counters` element carries the totals, the
/// failed `UnitTestResult` elements carry the details.
pub fn parse_trx(xml: &str) -> Result<TestRunSummary, String> {
    let mut reader = Reader::from_str(xml);
    let mut summary = TestRunSummary::default();
    let mut failed_test: Option<String> = None;
    let mut in_message = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"Counters" => {
                summary.tests += int_attr(&e, b"total");
                summary.failures += int_attr(&e, b"failed");
                summary.errors += int_attr(&e, b"error");
                summary.skipped += int_attr(&e, b"notExecuted");
            }
            // Self-closing failed results have no message to wait for.
            Ok(Event::Empty(e)) if e.name().as_ref() == b"UnitTestResult" => {
                if attr(&e, b"outcome").as_deref() == Some("Failed") {
                    let name = attr(&e, b"testName").unwrap_or_default();
                    summary.failure_details.push(format!("{name}: failed"));
                }
            }
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"UnitTestResult" => {
                    if attr(&e, b"outcome").as_deref() == Some("Failed") {
                        failed_test = Some(attr(&e, b"testName").unwrap_or_default());
                    }
                }
                b"Message" => in_message = failed_test.is_some(),
                _ => {}
            },
            // in_message is only ever set while failed_test holds a name,
            // so the take cannot come up empty here.
            Ok(Event::Text(text)) if in_message => {
                in_message = false;
                let name = failed_test.take().unwrap_or_default();
                let message = text.decode().map_err(|e| e.to_string())?;
                summary.failure_details.push(format!("{name}: {message}"));
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"Message" => in_message = false,
                b"UnitTestResult" => {
                    if let Some(name) = failed_test.take() {
                        summary.failure_details.push(format!("{name}: failed"));
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(summary)
}

fn attr(element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn int_attr(element: &BytesStart<'_>, name: &[u8]) -> u32 {
    attr(element, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeRuntimeProbe;

    pub const TRX: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
  <Results>
    <UnitTestResult testName="Calc.Adds" outcome="Passed"/>
    <UnitTestResult testName="Calc.Fails" outcome="Failed">
      <Output><ErrorInfo><Message>Expected 3 but was 4</Message></ErrorInfo></Output>
    </UnitTestResult>
    <UnitTestResult testName="Calc.Silent" outcome="Failed"/>
  </Results>
  <ResultSummary outcome="Failed">
    <Counters total="4" executed="3" passed="1" failed="2" error="0" notExecuted="1"/>
  </ResultSummary>
</TestRun>"#;

    #[test]
    fn counters_and_failed_results_are_extracted() {
        let summary = parse_trx(TRX).unwrap();
        assert_eq!(summary.tests, 4);
        assert_eq!(summary.failures, 2);
        assert_eq!(summary.errors, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            summary.failure_details,
            vec!["Calc.Fails: Expected 3 but was 4", "Calc.Silent: failed"]
        );
    }

    #[test]
    fn a_failed_result_with_a_body_but_no_message_reports_failed() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<TestRun>
  <Results>
    <UnitTestResult testName="Calc.NoMessage" outcome="Failed"><Output/></UnitTestResult>
  </Results>
  <ResultSummary outcome="Failed">
    <Counters total="1" executed="1" passed="0" failed="1" error="0" notExecuted="0"/>
  </ResultSummary>
</TestRun>"#;
        let summary = parse_trx(xml).unwrap();
        assert_eq!(summary.failure_details, vec!["Calc.NoMessage: failed"]);
    }

    #[test]
    fn invalid_xml_is_an_error() {
        assert!(parse_trx("<TestRun").is_err());
    }

    #[test]
    fn a_missing_results_directory_is_an_empty_run() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            parse_results_dir(&dir.path().join("nope")).unwrap(),
            TestRunSummary::default()
        );
    }

    #[test]
    fn the_results_directory_is_summed_across_trx_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.trx"), TRX).unwrap();
        fs::write(dir.path().join("b.trx"), TRX).unwrap();
        fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        let summary = parse_results_dir(dir.path()).unwrap();
        assert_eq!(summary.tests, 8);
        assert_eq!(summary.failure_details.len(), 4);
    }

    #[test]
    fn an_unparsable_report_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bad.trx"), "<TestRun").unwrap();
        let error = parse_results_dir(dir.path()).unwrap_err();
        assert!(error.starts_with("Unable to parse"), "got: {error}");
    }

    fn runner(dir: &std::path::Path, script: &str) -> DotnetRunner<FakeRuntimeProbe> {
        DotnetRunner::new(dir.to_path_buf(), FakeRuntimeProbe::with(&["dotnet"]))
            .with_command(vec!["sh".into(), "-c".into(), script.into()])
    }

    #[test]
    fn a_run_parses_the_trx_the_build_produced() {
        let dir = tempfile::tempdir().unwrap();
        let script =
            format!("mkdir -p TestResults && cat > TestResults/run.trx <<'EOF'\n{TRX}\nEOF");
        let summary = runner(dir.path(), &script)
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 4);
    }

    #[test]
    fn a_scenario_filter_becomes_a_name_filter() {
        let dir = tempfile::tempdir().unwrap();
        let script =
            format!("mkdir -p TestResults && cat > TestResults/run.trx <<'EOF'\n{TRX}\nEOF");
        let filter = TestFilter {
            feature: None,
            scenario: Some("Adds".into()),
        };
        assert_eq!(runner(dir.path(), &script).run(&filter).unwrap().tests, 4);
    }

    #[test]
    fn a_build_failure_without_reports_is_one_error_with_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let summary = runner(dir.path(), "echo 'CS1002: ; expected'; exit 1")
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.errors, 1);
        assert!(summary.failure_details[0].contains("CS1002"));
    }

    #[test]
    fn a_missing_dotnet_runtime_is_refused_with_a_hint() {
        let dir = tempfile::tempdir().unwrap();
        let runner = DotnetRunner::new(dir.path().to_path_buf(), FakeRuntimeProbe::default());
        assert_eq!(
            runner.run(&TestFilter::default()).unwrap_err(),
            RunnerError::RuntimeMissing {
                runtime: "dotnet".into(),
                hint: "Install the .NET SDK from https://dotnet.microsoft.com, then rerun.".into(),
            }
        );
    }

    #[test]
    fn stale_trx_files_are_removed_before_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let results = dir.path().join("TestResults");
        fs::create_dir_all(&results).unwrap();
        fs::write(results.join("stale.trx"), TRX).unwrap();
        let summary = runner(dir.path(), "true")
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 0);
    }
}

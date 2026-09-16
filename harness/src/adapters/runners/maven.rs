//! Maven test runner — the Rust port of the Java server's
//! `MavenTestRunner` + `SurefireReportParser`. Runs `mvn test` and sums
//! the Surefire `TEST-*.xml` reports; a compile error (no reports plus a
//! nonzero exit) becomes one error carrying the output tail.

use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};

use super::{build_failure, run_command};
use crate::domain::model::TestRunSummary;
use crate::ports::{RunnerError, RuntimeProbe, TestFilter, TestRunner};

pub struct MavenRunner<R: RuntimeProbe> {
    root: PathBuf,
    probe: R,
    command: Vec<String>,
    /// True when this workshop's sibling `kata/pom.xml` exists: run that
    /// project (parity with the Java server) instead of the aggregator.
    kata: bool,
}

impl<R: RuntimeProbe> MavenRunner<R> {
    pub fn new(root: PathBuf, probe: R) -> Self {
        let kata = root.join("kata/pom.xml").is_file();
        let command = if kata {
            vec![
                "mvn".into(),
                "-q".into(),
                "-B".into(),
                "-f".into(),
                "kata/pom.xml".into(),
                "test".into(),
            ]
        } else {
            vec!["mvn".into(), "-q".into(), "-B".into(), "test".into()]
        };
        Self {
            root,
            probe,
            command,
            kata,
        }
    }

    /// Visible for tests: run an arbitrary command in place of Maven.
    pub fn with_command(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }
}

impl<R: RuntimeProbe> TestRunner for MavenRunner<R> {
    fn run(&self, filter: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        if self.probe.version("mvn").is_none() {
            return Err(RunnerError::RuntimeMissing {
                runtime: "mvn".into(),
                hint: "Install a JDK and Apache Maven, then rerun.".into(),
            });
        }
        let mut command = self.command.clone();
        if let Some(feature) = &filter.feature {
            command.push(format!("-Dcucumber.features={feature}"));
        }
        if let Some(scenario) = &filter.scenario {
            command.push(format!("-Dcucumber.filter.name={scenario}"));
        }
        for dir in self.report_dirs() {
            let _ = fs::remove_dir_all(dir);
        }
        let outcome = run_command(&command, &self.root)?;
        let summary = parse_report_dirs(&self.report_dirs()).map_err(RunnerError::Failed)?;
        if summary.tests == 0 && !outcome.success {
            return Ok(build_failure_for(&outcome.combined()));
        }
        Ok(summary)
    }
}

impl<R: RuntimeProbe> MavenRunner<R> {
    fn report_dirs(&self) -> Vec<PathBuf> {
        if self.kata {
            vec![self.root.join("kata/target/surefire-reports")]
        } else {
            let mut dirs = collect_surefire_dirs(&self.root);
            let root = self.root.join("target/surefire-reports");
            if !dirs.contains(&root) {
                dirs.push(root);
            }
            dirs
        }
    }
}

/// Classify a Maven failure with no Surefire reports. A missing parent
/// POM is a build-config problem, not a prompt to write production code.
fn build_failure_for(output: &str) -> TestRunSummary {
    let mut summary = build_failure(output);
    if output.contains("Non-resolvable parent POM") || output.contains("ProjectBuildingException") {
        summary.failure_details.insert(
            0,
            "Build failed: Non-resolvable parent POM. Fix the POM (this \
             workshop's kata is a standalone project with no parent) before \
             writing production code."
                .into(),
        );
    }
    summary
}

/// Every `target/surefire-reports` directory under `root`, without
/// walking into `target` itself (nested build output).
pub fn collect_surefire_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_surefire(root, &mut dirs);
    dirs.sort();
    dirs
}

fn collect_surefire(dir: &Path, into: &mut Vec<PathBuf>) {
    let reports = dir.join("target/surefire-reports");
    if reports.is_dir() {
        into.push(reports);
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir()
            && name != "target"
            && name != "node_modules"
            && name != ".git"
            && !name.starts_with('.')
        {
            collect_surefire(&path, into);
        }
    }
}

/// Sum Surefire reports across every given directory.
pub fn parse_report_dirs(dirs: &[PathBuf]) -> Result<TestRunSummary, String> {
    let mut total = TestRunSummary::default();
    for dir in dirs {
        let one = parse_reports_dir(dir)?;
        total.tests += one.tests;
        total.failures += one.failures;
        total.errors += one.errors;
        total.skipped += one.skipped;
        total.failure_details.extend(one.failure_details);
    }
    Ok(total)
}

/// Sum every `TEST-*.xml` report in the Surefire directory. A missing
/// directory is an empty run, exactly like the Java parser.
pub fn parse_reports_dir(dir: &Path) -> Result<TestRunSummary, String> {
    if !dir.is_dir() {
        return Ok(TestRunSummary::default());
    }
    let mut reports: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("Unable to read surefire reports in {} - {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with("TEST-") && name.ends_with(".xml")
        })
        .collect();
    reports.sort();
    let mut total = TestRunSummary::default();
    for report in reports {
        let xml = fs::read_to_string(&report)
            .map_err(|e| format!("Unable to read {} - {e}", report.display()))?;
        let one = parse_surefire_xml(&xml)
            .map_err(|e| format!("Unable to parse {} - {e}", report.display()))?;
        total.tests += one.tests;
        total.failures += one.failures;
        total.errors += one.errors;
        total.skipped += one.skipped;
        total.failure_details.extend(one.failure_details);
    }
    Ok(total)
}

/// Parse one Surefire report. Detail strings match the Java parser
/// (`classname.name: message`, or the tag name when the message is
/// blank) - plus the element's body, which carries the stack trace:
/// the model's implementation attempts need the whole story, not just
/// the one-line message.
pub fn parse_surefire_xml(xml: &str) -> Result<TestRunSummary, String> {
    let mut reader = Reader::from_str(xml);
    let mut summary = TestRunSummary::default();
    let mut current_case: Option<String> = None;
    // True between <failure>/<error> open and close: the body text is
    // the stack trace, appended to the detail just pushed.
    let mut in_detail = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                in_detail = open_element(&e, &mut summary, &mut current_case);
            }
            Ok(Event::Empty(e)) => {
                open_element(&e, &mut summary, &mut current_case);
            }
            Ok(Event::Text(text)) if in_detail => {
                let body = text.decode().map_err(|e| e.to_string())?;
                append_stack_trace(&mut summary, &body);
            }
            Ok(Event::CData(text)) if in_detail => {
                let body = String::from_utf8_lossy(&text).into_owned();
                append_stack_trace(&mut summary, &body);
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"testcase" => current_case = None,
                b"failure" | b"error" => in_detail = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(summary)
}

/// Handle one opening element. Returns true when it opened a
/// failure/error detail whose body should be captured.
fn open_element(
    e: &BytesStart<'_>,
    summary: &mut TestRunSummary,
    current_case: &mut Option<String>,
) -> bool {
    match e.name().as_ref() {
        b"testsuite" => {
            summary.tests += int_attr(e, b"tests");
            summary.failures += int_attr(e, b"failures");
            summary.errors += int_attr(e, b"errors");
            summary.skipped += int_attr(e, b"skipped");
        }
        b"testcase" => {
            *current_case = Some(format!(
                "{}.{}",
                attr(e, b"classname").unwrap_or_default(),
                attr(e, b"name").unwrap_or_default()
            ));
        }
        tag @ (b"failure" | b"error") => {
            if let Some(case) = current_case {
                let message = attr(e, b"message").filter(|m| !m.trim().is_empty());
                let fallback = String::from_utf8_lossy(tag).into_owned();
                summary
                    .failure_details
                    .push(format!("{case}: {}", message.unwrap_or(fallback)));
                return true;
            }
        }
        _ => {}
    }
    false
}

fn append_stack_trace(summary: &mut TestRunSummary, body: &str) {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(detail) = summary.failure_details.last_mut() {
        detail.push('\n');
        detail.push_str(trimmed);
    }
}

fn attr(element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| {
            let raw = String::from_utf8_lossy(&a.value);
            unescape(&raw)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| raw.into_owned())
        })
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

    pub const PASSING_REPORT: &str = r#"<?xml version="1.0"?>
<testsuite name="Calc" tests="5" failures="0" errors="0" skipped="1">
  <testcase classname="CalcTest" name="adds"/>
</testsuite>"#;

    const FAILING_REPORT: &str = r#"<?xml version="1.0"?>
<testsuite name="Calc" tests="3" failures="1" errors="1" skipped="0">
  <testcase classname="CalcTest" name="adds">
    <failure message="expected 3 but was 4">stack</failure>
  </testcase>
  <testcase classname="CalcTest" name="blows">
    <error message="">stack</error>
  </testcase>
</testsuite>"#;

    #[test]
    fn a_passing_report_sums_the_suite_attributes() {
        let summary = parse_surefire_xml(PASSING_REPORT).unwrap();
        assert_eq!(summary.tests, 5);
        assert_eq!(summary.skipped, 1);
        assert!(summary.failure_details.is_empty());
    }

    #[test]
    fn unrelated_elements_inside_a_testcase_are_ignored() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="Calc" tests="1" failures="1" errors="0" skipped="0">
  <testcase classname="CalcTest" name="adds">
    <system-out>noise</system-out>
    <failure message="expected 3 but was 4">stack</failure>
  </testcase>
</testsuite>"#;
        let summary = parse_surefire_xml(xml).unwrap();
        assert_eq!(
            summary.failure_details,
            vec!["CalcTest.adds: expected 3 but was 4\nstack"]
        );
    }

    #[test]
    fn failures_and_errors_carry_classname_dot_name_details_with_the_stack() {
        let summary = parse_surefire_xml(FAILING_REPORT).unwrap();
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(
            summary.failure_details,
            vec![
                "CalcTest.adds: expected 3 but was 4\nstack",
                "CalcTest.blows: error\nstack",
            ]
        );
    }

    #[test]
    fn a_multi_line_stack_trace_in_cdata_is_kept_whole() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="Calc" tests="1" failures="1" errors="0" skipped="0">
  <testcase classname="CalcTest" name="adds">
    <failure message="expected 3 but was 4"><![CDATA[java.lang.AssertionError: expected 3 but was 4
	at CalcTest.adds(CalcTest.java:12)]]></failure>
  </testcase>
</testsuite>"#;
        let summary = parse_surefire_xml(xml).unwrap();
        assert_eq!(summary.failure_details.len(), 1);
        assert!(
            summary.failure_details[0].contains("at CalcTest.adds(CalcTest.java:12)"),
            "got: {}",
            summary.failure_details[0]
        );
    }

    #[test]
    fn xml_entities_in_a_failure_message_are_decoded() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="Calc" tests="1" failures="0" errors="1" skipped="0">
  <testcase classname="RunCucumberTest" name="case 1">
    <error message="The step &apos;the result is 0.&apos; is undefined.&#10;snippet">stack</error>
  </testcase>
</testsuite>"#;
        let summary = parse_surefire_xml(xml).unwrap();
        assert_eq!(
            summary.failure_details,
            vec![
                "RunCucumberTest.case 1: The step 'the result is 0.' is undefined.\nsnippet\nstack"
            ]
        );
    }

    #[test]
    fn a_whitespace_only_body_adds_nothing_to_the_detail() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="Calc" tests="1" failures="1" errors="0" skipped="0">
  <testcase classname="CalcTest" name="adds">
    <failure message="expected 3 but was 4">   </failure>
  </testcase>
</testsuite>"#;
        let summary = parse_surefire_xml(xml).unwrap();
        assert_eq!(
            summary.failure_details,
            vec!["CalcTest.adds: expected 3 but was 4"]
        );
    }

    #[test]
    fn invalid_xml_is_an_error() {
        assert!(parse_surefire_xml("<testsuite tests=\"1\"").is_err());
    }

    #[test]
    fn a_failure_outside_any_testcase_is_counted_but_carries_no_detail() {
        let report = r#"<testsuite tests="1" failures="1" errors="0" skipped="0">
            <failure message="orphan"/>
        </testsuite>"#;
        let summary = parse_surefire_xml(report).unwrap();
        assert_eq!(summary.failures, 1);
        assert!(summary.failure_details.is_empty());
    }

    #[test]
    fn a_missing_reports_directory_is_an_empty_run() {
        let dir = tempfile::tempdir().unwrap();
        let summary = parse_reports_dir(&dir.path().join("nope")).unwrap();
        assert_eq!(summary, TestRunSummary::default());
    }

    #[test]
    fn the_reports_directory_is_summed_across_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("TEST-a.xml"), PASSING_REPORT).unwrap();
        fs::write(dir.path().join("TEST-b.xml"), FAILING_REPORT).unwrap();
        fs::write(dir.path().join("not-a-report.txt"), "ignored").unwrap();
        let summary = parse_reports_dir(dir.path()).unwrap();
        assert_eq!(summary.tests, 8);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.failure_details.len(), 2);
    }

    #[test]
    fn an_unparsable_report_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("TEST-bad.xml"), "<testsuite").unwrap();
        let error = parse_reports_dir(dir.path()).unwrap_err();
        assert!(error.starts_with("Unable to parse"), "got: {error}");
    }

    fn runner(dir: &Path, script: &str) -> MavenRunner<FakeRuntimeProbe> {
        MavenRunner::new(dir.to_path_buf(), FakeRuntimeProbe::with(&["mvn"])).with_command(vec![
            "sh".into(),
            "-c".into(),
            script.into(),
        ])
    }

    #[test]
    fn a_run_parses_the_reports_the_build_produced() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "mkdir -p target/surefire-reports && cat > target/surefire-reports/TEST-a.xml <<'EOF'\n{PASSING_REPORT}\nEOF"
        );
        let summary = runner(dir.path(), &script)
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 5);
    }

    #[test]
    fn filters_are_passed_as_cucumber_properties() {
        // The fake build script ignores the extra args; this exercises
        // the argument construction paths.
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "mkdir -p target/surefire-reports && cat > target/surefire-reports/TEST-a.xml <<'EOF'\n{PASSING_REPORT}\nEOF"
        );
        let filter = TestFilter {
            feature: Some("features/calc.feature".into()),
            scenario: Some("Empty string".into()),
        };
        assert_eq!(runner(dir.path(), &script).run(&filter).unwrap().tests, 5);
    }

    #[test]
    fn a_build_failure_without_reports_is_one_error_with_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let summary = runner(dir.path(), "echo 'compile error'; exit 1")
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.errors, 1);
        assert!(summary.failure_details[0].contains("compile error"));
    }

    #[test]
    fn a_missing_maven_runtime_is_refused_with_a_hint() {
        let dir = tempfile::tempdir().unwrap();
        let runner = MavenRunner::new(dir.path().to_path_buf(), FakeRuntimeProbe::default());
        let error = runner.run(&TestFilter::default()).unwrap_err();
        assert_eq!(
            error,
            RunnerError::RuntimeMissing {
                runtime: "mvn".into(),
                hint: "Install a JDK and Apache Maven, then rerun.".into(),
            }
        );
    }

    #[test]
    fn stale_reports_are_removed_before_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join("target/surefire-reports");
        fs::create_dir_all(&reports).unwrap();
        fs::write(reports.join("TEST-stale.xml"), PASSING_REPORT).unwrap();
        // The build produces nothing and succeeds: an empty run, not the
        // stale five tests.
        let summary = runner(dir.path(), "true")
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 0);
        assert!(summary.no_tests());
    }

    #[test]
    fn a_kata_pom_runs_against_kata_surefire_not_the_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("kata")).unwrap();
        fs::write(dir.path().join("kata/pom.xml"), "<project/>").unwrap();
        let script = format!(
            "mkdir -p kata/target/surefire-reports && cat > kata/target/surefire-reports/TEST-a.xml <<'EOF'\n{PASSING_REPORT}\nEOF"
        );
        let summary = MavenRunner::new(dir.path().to_path_buf(), FakeRuntimeProbe::with(&["mvn"]))
            .with_command(vec!["sh".into(), "-c".into(), script])
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 5);
    }

    #[test]
    fn module_surefire_reports_are_summed_when_there_is_no_kata_pom() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "mkdir -p module/target/surefire-reports && cat > module/target/surefire-reports/TEST-a.xml <<'EOF'\n{PASSING_REPORT}\nEOF"
        );
        let summary = runner(dir.path(), &script)
            .run(&TestFilter::default())
            .unwrap();
        assert_eq!(summary.tests, 5);
    }

    #[test]
    fn a_non_resolvable_parent_pom_names_the_pom_not_production_code() {
        let dir = tempfile::tempdir().unwrap();
        let summary = runner(
            dir.path(),
            "echo 'Non-resolvable parent POM: com.davidparry.workshop:tdd-bdd-agentic'; exit 1",
        )
        .run(&TestFilter::default())
        .unwrap();
        assert_eq!(summary.errors, 1);
        assert!(
            summary.failure_details[0].contains("Fix the POM"),
            "got: {:?}",
            summary.failure_details
        );
    }
}

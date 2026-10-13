//! Concurrent writes, end to end.
//!
//! Six `spec scenario add` invocations against one feature file used to
//! produce five `"written": true` replies and three surviving
//! scenarios. Two callers were told their scenario had been added and
//! it had not.
//!
//! Threads alone cannot prove the fix, because every thread in one
//! process shares the in-process half of the claim. These tests use
//! real `spec` child processes for exactly that reason: separate
//! processes share nothing but the advisory lock on `.spec/`, which is
//! the thing under test.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use spec_harness::ports::WorkTree;

const SPEC: &str = env!("CARGO_BIN_EXE_spec");

const FEATURE: &str = "features/calc.feature";

const BASE: &str = "\
# The executable behavior spec for the kata.
Feature: String Calculator addition
  As a user of the calculator
  I want delimited number strings to be summed safely

  @REQ-001
  Scenario: An empty string returns zero
    Given a string calculator
    Then the result is 0

  # REQ-003+: scenarios are written live during the workshop.
";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("requirements")).unwrap();
    fs::write(
        dir.path().join("requirements/requirements.json"),
        r#"{"project":"Demo","requirements":[]}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("features")).unwrap();
    fs::write(dir.path().join(FEATURE), BASE).unwrap();
    dir
}

/// One `spec scenario add`, launched but not waited on.
fn add(root: &Path, req: &str, name: &str) -> std::process::Child {
    Command::new(SPEC)
        .args([
            "--root",
            &root.display().to_string(),
            "scenario",
            "add",
            "--feature",
            FEATURE,
            "--req",
            req,
            "--name",
            name,
            "--step",
            "Given a string calculator",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spec is built")
}

fn feature_on_disk(root: &Path) -> String {
    spec_harness::wiring::work_tree(root)
        .read(FEATURE)
        .expect("the feature file is readable")
        .expect("the feature file exists")
}

/// Every caller that was told its scenario was written has to be in
/// the file, and every caller that was not has to have said so.
/// Nothing may be reported as written and then quietly dropped.
#[test]
fn concurrent_processes_all_land_or_all_say_they_did_not() {
    let dir = project();
    let names: Vec<String> = (1..=6).map(|i| format!("Scenario {i}")).collect();
    let running: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, name)| add(dir.path(), &format!("REQ-{:03}", i + 10), name))
        .collect();

    let mut claimed = Vec::new();
    let mut refused = Vec::new();
    for (child, name) in running.into_iter().zip(&names) {
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("not valid Gherkin"),
            "{name} read a half-written feature file: {stderr}"
        );
        if out.status.success() && stdout.contains("\"written\": true") {
            claimed.push(name.clone());
        } else {
            refused.push((name.clone(), format!("{stdout}{stderr}")));
        }
    }
    assert!(
        refused.is_empty(),
        "an invocation failed outright: {refused:?}"
    );

    let written = feature_on_disk(dir.path());
    let missing: Vec<&String> = claimed
        .iter()
        .filter(|name| !written.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "these were told they were written and were then lost: {missing:?}\n{written}"
    );
    assert_eq!(claimed.len(), names.len());
}

/// The trailing note is the other half of the same promise: whichever
/// order the six land in, none of them may take it with them.
#[test]
fn concurrent_processes_do_not_destroy_the_note_that_closes_the_file() {
    let dir = project();
    let running: Vec<_> = (1..=4)
        .map(|i| add(dir.path(), &format!("REQ-{:03}", i + 20), &format!("S{i}")))
        .collect();
    for child in running {
        assert!(child.wait_with_output().unwrap().status.success());
    }
    let written = feature_on_disk(dir.path());
    assert_eq!(
        written.matches("# REQ-003+").count(),
        1,
        "the closing note was dropped or duplicated:\n{written}"
    );
    assert!(
        written.find("Scenario: S4").unwrap() < written.find("# REQ-003+").unwrap(),
        "a scenario landed below the closing note:\n{written}"
    );
}

/// A reader running alongside the writers must never see a torn file.
/// Each write lands by renaming a complete scratch file over the
/// target, so every read gets the old bytes or the new ones.
#[test]
fn a_reader_never_sees_a_half_written_file() {
    let dir = project();
    let mut running: Vec<_> = (1..=6)
        .map(|i| add(dir.path(), &format!("REQ-{:03}", i + 30), &format!("R{i}")))
        .collect();

    let store = spec_harness::wiring::work_tree(dir.path());
    let mut reads = 0u32;
    loop {
        let seen = store
            .read(FEATURE)
            .unwrap_or_else(|e| panic!("a reader failed after {reads} reads - {}", e.0))
            .expect("the feature file is always there");
        assert!(
            seen.starts_with("# The executable behavior spec"),
            "a reader caught the file mid-write after {reads} reads:\n{seen}"
        );
        reads += 1;
        let all_done = running
            .iter_mut()
            .all(|child| child.try_wait().expect("the child is waitable").is_some());
        if all_done {
            break;
        }
    }
    for mut child in running {
        assert!(child.wait().unwrap().success());
    }
    assert!(reads > 0, "the reader never got a turn");
}

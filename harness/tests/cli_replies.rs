//! What the shell actually prints and exits with.
//!
//! These run the real binary because that is where the defects were:
//! the reports themselves were right and the wrapper around them was
//! not. `spec validate` said `"valid": false` and exited 0, so a CI
//! gate scripted on it passed on a broken spec; the phase advice named
//! MCP tools that are not commands; and the non-TTY warning told
//! commands that do stage that they stage nothing.

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use spec_harness::adapters::spec_home::spec_file;
use spec_harness::domain::CONFIG_FILE;

const SPEC: &str = env!("CARGO_BIN_EXE_spec");

const VALID: &str = r#"{
  "project": "Demo",
  "requirements": [
    {
      "id": "REQ-001",
      "title": "Add two numbers",
      "story": "As a user I want to add numbers so that I get a sum",
      "acceptanceCriteria": [
        "Given a calculator When I add 1 and 2 Then the result is 3"
      ],
      "status": "pending"
    }
  ]
}"#;

/// Wording the refiner has nothing to say about - a concrete expected
/// value and an edge case - so the wizard asks its prompts once and
/// the answer count is fixed.
const CLEAN_WORDING: &str = r#"{
  "project": "Demo",
  "requirements": [
    {
      "id": "REQ-001",
      "title": "Add two numbers",
      "story": "As a user I want to add numbers so that I get a sum",
      "acceptanceCriteria": [
        "Given a calculator, when I add 1 and 2, then the result is 3",
        "Given an empty string \"\", when add is called, then the result is 0"
      ],
      "status": "pending"
    }
  ]
}"#;

/// Two requirements sharing an id - the duplicate-id case from the
/// dry run, which reported itself and exited 0.
const DUPLICATE_IDS: &str = r#"{
  "project": "Demo",
  "requirements": [
    {
      "id": "REQ-001",
      "title": "Add two numbers",
      "story": "As a user I want to add numbers so that I get a sum",
      "acceptanceCriteria": [
        "Given a calculator When I add 1 and 2 Then the result is 3"
      ],
      "status": "pending"
    },
    {
      "id": "REQ-001",
      "title": "Add three numbers",
      "story": "As a user I want to add numbers so that I get a sum",
      "acceptanceCriteria": [
        "Given a calculator When I add 1 and 2 Then the result is 3"
      ],
      "status": "pending"
    }
  ]
}"#;

fn project(spec: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("requirements")).unwrap();
    fs::write(dir.path().join("requirements/requirements.json"), spec).unwrap();
    // An endpoint that refuses instantly means no model resolves, so
    // every command here takes its template path. Leaving it out is
    // not the same thing: with no model configured `spec` picks an
    // installed Ollama model, which would make these tests call a
    // real model and depend on what the developer happens to have
    // pulled. The short discovery timeout keeps the commands that
    // survey MCP servers from spending the default one finding out
    // there are none.
    let config = spec_file(dir.path(), CONFIG_FILE);
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(
        &config,
        "[llm]\nendpoint = \"http://127.0.0.1:1\"\n\n\
         [tools]\ndiscovery_timeout_seconds = 1\ncall_timeout_seconds = 1\n",
    )
    .unwrap();
    dir
}

/// One `spec` run with answers piped in, as a script would.
fn spec_piped(root: &Path, args: &[&str], answers: &str) -> Output {
    let mut child = Command::new(SPEC)
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the spec binary runs");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .expect("a pipe")
        .write_all(answers.as_bytes())
        .expect("the answers are written");
    child.wait_with_output().expect("the run finishes")
}

fn spec_run(root: &Path, args: &[&str]) -> Output {
    Command::new(SPEC)
        .arg("--root")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("the spec binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn a_valid_spec_validates_and_exits_zero() {
    let dir = project(VALID);
    let output = spec_run(dir.path(), &["validate"]);
    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).contains("\"valid\": true"));
}

/// The defect: a gate scripted on `spec validate` passed on a spec the
/// command itself called invalid.
#[test]
fn an_invalid_spec_exits_nonzero_while_still_reporting_why() {
    let dir = project(DUPLICATE_IDS);
    let output = spec_run(dir.path(), &["validate"]);
    assert!(!output.status.success(), "should have failed the gate");
    let reply = stdout(&output);
    assert!(reply.contains("\"valid\": false"), "{reply}");
    assert!(reply.contains("duplicate id"), "{reply}");
}

/// A missing spec is a different failure from an invalid one, and it
/// was already non-zero.
#[test]
fn a_missing_spec_still_fails() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!spec_run(dir.path(), &["validate"]).status.success());
}

/// `spec state` used to open with ~900 characters of unchanging
/// guidance before saying which phase you were in.
#[test]
fn spec_state_answers_with_the_phase_before_the_guidance() {
    let dir = project(VALID);
    let reply = stdout(&spec_run(dir.path(), &["state"]));
    let phase = reply.find("\"phase\"").expect("a phase");
    let instructions = reply.find("\"instructions\"").expect("the guidance");
    assert!(phase < instructions, "{reply}");
}

/// The phase advice named `run_tests`, `start_refactor` and
/// `get_requirement` - none of which are commands.
#[test]
fn the_phase_advice_names_commands_the_reader_can_paste() {
    let dir = project(VALID);
    let reply = stdout(&spec_run(dir.path(), &["state"]));
    assert!(reply.contains("Run spec test"), "{reply}");
    assert!(!reply.contains("run_tests"), "{reply}");
}

/// The MCP server builds its own payloads, so translating the shell's
/// copy must not reach the tool descriptions the agent reads.
#[test]
fn the_tool_facing_wording_is_not_rewritten_for_the_agent() {
    let dir = project(VALID);
    let reply = stdout(&spec_run(dir.path(), &["state", "--help"]));
    assert!(reply.contains("get_tdd_state"), "{reply}");
}

/// `spec unittest generate` has no wizard and returned
/// `"staged": true`, yet on a pipe it warned that a wizard stages
/// nothing. It still warns that prompts come from the pipe, which is
/// true of every command that prompts.
#[test]
fn a_command_that_stages_is_not_warned_that_it_stages_nothing() {
    let dir = project(VALID);
    // The command surveys the project before it prompts.
    fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
    let output = spec_run(dir.path(), &["unittest", "generate", "REQ-001"]);
    let warning = String::from_utf8(output.stderr).unwrap();
    assert!(warning.contains("stdin is not a terminal"), "{warning}");
    assert!(!warning.contains("stages nothing"), "{warning}");
}

/// The wizard warning is still earned where it is true: `spec reword`
/// asks "Stage this?" last, and a spent pipe answers no - which is
/// exactly what happens here, so the reply confirms the warning.
#[test]
fn a_wizard_is_still_warned_that_it_will_stage_nothing() {
    let dir = project(VALID);
    let output = spec_run(dir.path(), &["reword", "REQ-001"]);
    let reply = stdout(&output);
    let warning = String::from_utf8(output.stderr).unwrap();
    assert!(warning.contains("stages nothing"), "{warning}");
    assert!(reply.contains("\"staged\": false"), "{reply}");
}

/// The hang: with a model configured, `spec reword` read the end of
/// the pipe as "r" at the "[r]eword again / [m]anual / [a]ccept"
/// prompt, reworded, found the same finding, and asked again forever.
/// The wording the guide quotes is pinned byte for byte, because
/// reaching it faster is the only change students should see.
#[test]
fn a_wizard_on_a_closed_stdin_declines_instead_of_asking_forever() {
    let dir = project(VALID);
    let output = spec_run(dir.path(), &["reword", "REQ-001"]);
    assert!(output.status.success(), "a declined wizard is not an error");
    let reply = stdout(&output);
    assert!(reply.contains("\"staged\": false"), "{reply}");
    assert!(
        reply.contains(
            "\"nextStep\": \"Nothing was staged. Run spec reword REQ-001 \
             again when the wording is ready.\""
        ),
        "{reply}"
    );
}

/// Proven at the prompter, not in one command: `spec draft` is a
/// different wizard with no prior answers to fall back on, and it
/// reaches its own declined outcome the same way.
#[test]
fn a_different_wizard_declines_on_a_closed_stdin_too() {
    let dir = project(VALID);
    let output = spec_run(dir.path(), &["draft"]);
    assert!(output.status.success(), "a declined wizard is not an error");
    let reply = stdout(&output);
    assert!(reply.contains("\"staged\": false"), "{reply}");
    assert!(
        reply.contains("Nothing was staged. Run spec draft again"),
        "{reply}"
    );
}

/// Why, not just what: a silent no-op is what sent the last run
/// looking for a crash.
#[test]
fn the_end_of_the_input_is_explained_on_stderr() {
    let dir = project(VALID);
    let output = spec_run(dir.path(), &["reword", "REQ-001"]);
    let explanation = String::from_utf8(output.stderr).unwrap();
    assert!(explanation.contains("end of input"), "{explanation}");
    assert!(explanation.contains("nothing is staged"), "{explanation}");
    // The non-TTY warning still leads, and still earns its wizard
    // sentence here.
    assert!(explanation.contains("stages nothing"), "{explanation}");
}

/// A pipe that answers some prompts and then runs out is the case the
/// guide warns about: it declines rather than guessing at the rest.
#[test]
fn a_pipe_that_runs_out_part_way_declines_and_stages_nothing() {
    let dir = project(VALID);
    let output = spec_piped(dir.path(), &["reword", "REQ-001"], "\n\n");
    assert!(output.status.success());
    let reply = stdout(&output);
    assert!(reply.contains("\"staged\": false"), "{reply}");
    assert!(reply.contains("Nothing was staged."), "{reply}");
}

/// And the documented recipe still works: answer every prompt,
/// including the confirmation, and the wizard stages.
#[test]
fn a_pipe_that_answers_every_prompt_still_stages() {
    let dir = project(CLEAN_WORDING);
    // title, story, criterion 1, criterion 2, the blank that ends the
    // list, then the confirmation.
    let output = spec_piped(dir.path(), &["reword", "REQ-001"], "\n\n\n\n\ny\n");
    assert!(output.status.success());
    let reply = stdout(&output);
    assert!(reply.contains("\"staged\": true"), "{reply}");
    let explanation = String::from_utf8(output.stderr).unwrap();
    assert!(
        !explanation.contains("end of input"),
        "the answers were enough: {explanation}"
    );
}

/// Naming a flag the user did not type sent them looking for a
/// mistake they had not made.
#[test]
fn a_half_given_draft_is_told_which_flags_are_missing() {
    let dir = project(VALID);
    let output = spec_run(dir.path(), &["draft", "--title", "Something"]);
    assert!(!output.status.success());
    let complaint = String::from_utf8(output.stderr).unwrap();
    assert!(complaint.contains("with --title"), "{complaint}");
    assert!(
        complaint.contains("needs --story and --criterion"),
        "{complaint}"
    );
}

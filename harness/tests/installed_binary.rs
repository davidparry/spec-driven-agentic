//! The `spec` on PATH and the `spec` this source builds, kept in step.
//!
//! `spec deliver` writes the spec with whichever `spec` the developer
//! typed, then judges the result by running this suite - which cargo
//! compiles from the source sitting beside it. While those two are the
//! same build, nobody has to think about it. When they are not, the
//! loop writes a spec its own tests cannot read, and the stop it prints
//! blames unrelated failures on a red bar instead.
//!
//! Measured here: an installed release binary from before
//! `ask_requirement` replaced `ask` as the type of an ask, carrying the
//! same `--version` as the crate, so neither the tool nor the developer
//! had anything to go on. One delivery run spent its whole budget on it.
//!
//! So the two halves are made to meet the way the loop makes them meet:
//! the PATH binary writes an ask, and the binary built from this source
//! validates what it wrote. Nothing is asserted about versions - they
//! matched throughout the failure this guards.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The binary cargo just built from the source under test.
const COMPILED: &str = env!("CARGO_BIN_EXE_spec");

const REQUEST: &str = "explain one spec command by name - what it does";

/// A `spec` that writes asks the compiled validator rejects is an older
/// build of this tool, and every delivery run made with it is spent
/// before it starts.
///
/// Skipped, not failed, when there is no `spec` on PATH: that is how CI
/// runs, and a developer who has not installed one cannot be out of
/// step with anything.
#[test]
fn the_spec_on_path_writes_asks_this_source_can_read() {
    let Some(installed) = spec_on_path() else {
        eprintln!("no spec on PATH - nothing to be out of step with");
        return;
    };

    let project = tempfile::tempdir().expect("a temporary project");
    let root = project.path();
    std::fs::create_dir_all(root.join("requirements")).expect("a requirements directory");
    std::fs::write(
        root.join("requirements/requirements.json"),
        "{\n  \"project\": \"Probe\",\n  \"requirements\": []\n}\n",
    )
    .expect("an empty spec to draft into");

    // `--request-only` preserves the words without breaking them down,
    // so no model is reached and the probe costs a process start.
    // `--no-branch` keeps it away from git, which a temporary directory
    // has none of.
    let drafted = spec(
        &installed,
        root,
        &["draft", "--request-only", REQUEST, "--no-branch"],
    );
    assert!(
        drafted.status.success(),
        "`spec draft` failed under {}:\n{}",
        installed.display(),
        String::from_utf8_lossy(&drafted.stderr)
    );

    let judged = spec(Path::new(COMPILED), root, &["validate"]);
    let verdict = String::from_utf8_lossy(&judged.stdout);
    assert!(
        verdict.contains("\"valid\": true"),
        "the spec on your PATH ({}) writes an ask this source cannot read, so a delivery \
         run with it would generate a spec these tests reject. Reinstall it with \
         `cargo install --path harness`, then run this again.\n\nwhat it wrote:\n{}\n\nwhat \
         this source says of it:\n{verdict}",
        installed.display(),
        std::fs::read_to_string(root.join("requirements/requirements.json"))
            .unwrap_or_else(|e| e.to_string())
    );
}

/// `spec`, run against `root` so neither binary walks up out of the
/// temporary project looking for one.
fn spec(binary: &Path, root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary)
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("{} runs: {e}", binary.display()))
}

/// The first `spec` on PATH, skipping this crate's own build directory:
/// a developer with `harness/target/debug` on PATH is testing the
/// compiled binary against itself, which proves nothing.
fn spec_on_path() -> Option<PathBuf> {
    let ours = Path::new(COMPILED).parent()?.to_path_buf();
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|dir| dir != &ours)
        .map(|dir| dir.join("spec"))
        .find(|candidate| candidate.is_file())
}

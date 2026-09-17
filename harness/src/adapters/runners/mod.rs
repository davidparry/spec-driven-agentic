//! [`TestRunner`](crate::ports::TestRunner) implementations, one per
//! supported build tool. Each is a thin process shell around a pure,
//! fixture-tested report parser, and each refuses with the structured
//! `runtime_missing` error when its runtime is absent — the harness never
//! installs anything.

pub mod cargo;
pub mod cucumber_js;
pub mod dotnet;
pub mod maven;

use std::path::Path;
use std::process::Command;

use crate::adapters::fs_project::FsProjectFiles;
use crate::adapters::process_runtime::ProcessRuntimeProbe;
use crate::domain::language::{Language, detect_languages};
use crate::ports::{RunnerError, TestRunner};

/// The one language→runner dispatch table, shared by every composition
/// root so adding a language means touching exactly one match.
pub fn runner_for_language(root: &Path, language: Language) -> Box<dyn TestRunner> {
    match language {
        Language::Java => Box::new(maven::MavenRunner::new(
            root.to_path_buf(),
            ProcessRuntimeProbe,
        )),
        Language::JavaScript | Language::TypeScript => Box::new(
            cucumber_js::CucumberJsRunner::new(root.to_path_buf(), ProcessRuntimeProbe),
        ),
        Language::DotNet => Box::new(dotnet::DotnetRunner::new(
            root.to_path_buf(),
            ProcessRuntimeProbe,
        )),
        Language::Rust => Box::new(cargo::CargoRunner::new(
            root.to_path_buf(),
            ProcessRuntimeProbe,
        )),
    }
}

/// Picks the test runner for the project's first detected language. The
/// error string is what `run_tests` reports when no project is detected.
pub fn detect_runner(root: &Path) -> Result<Box<dyn TestRunner>, String> {
    let files = FsProjectFiles::new(root.to_path_buf());
    let Some(language) = detect_languages(&files).first().copied() else {
        tracing::warn!(root = %root.display(), "no supported project detected for a test runner");
        return Err(
            "No supported project detected (pom.xml, build.gradle, package.json, \
             *.csproj, Cargo.toml). Run spec inspect."
                .to_string(),
        );
    };
    tracing::info!(language = language.display(), "test runner detected");
    Ok(runner_for_language(root, language))
}

/// The last `lines` lines of `text` — what the Java `MavenTestRunner`
/// reports when a build fails before tests could run.
pub(crate) fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let from = all.len().saturating_sub(lines);
    all[from..].join("\n")
}

#[derive(Debug)]
pub(crate) struct CommandOutcome {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl CommandOutcome {
    pub fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

pub(crate) fn run_command(command: &[String], dir: &Path) -> Result<CommandOutcome, RunnerError> {
    tracing::info!(command = %command.join(" "), dir = %dir.display(), "running test command");
    let output = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(dir)
        .output()
        .map_err(|e| {
            tracing::warn!(program = %command[0], error = %e, "test command failed to launch");
            RunnerError::Failed(format!("unable to launch {} - {e}", command[0]))
        })?;
    tracing::info!(success = output.status.success(), "test command finished");
    Ok(CommandOutcome {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    })
}

/// The shared compile-error reply: no reports plus a nonzero exit.
pub(crate) fn build_failure(output: &str) -> crate::domain::model::TestRunSummary {
    crate::domain::model::TestRunSummary {
        tests: 0,
        failures: 0,
        errors: 1,
        skipped: 0,
        failure_details: vec![format!(
            "Build failed before tests could run:\n{}",
            // Generous: the whole compiler error / stack trace is the
            // model's brief on the next implementation attempt.
            tail(output, 100)
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_only_the_last_lines() {
        let text = (1..=40)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let tailed = tail(&text, 30);
        assert!(tailed.starts_with("11\n"));
        assert!(tailed.ends_with("\n40"));
        assert_eq!(tail("short", 30), "short");
    }

    #[test]
    fn run_command_captures_output_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let ok = run_command(
            &["sh".into(), "-c".into(), "echo out; echo err >&2".into()],
            dir.path(),
        )
        .unwrap();
        assert_eq!(ok.stdout, "out\n");
        assert_eq!(ok.stderr, "err\n");
        assert!(ok.success);
        assert_eq!(ok.combined(), "out\nerr\n");
        let failed = run_command(&["sh".into(), "-c".into(), "exit 3".into()], dir.path()).unwrap();
        assert!(!failed.success);
    }

    #[test]
    fn a_missing_program_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = run_command(&["definitely-not-a-command-xyz".into()], dir.path()).unwrap_err();
        assert!(
            matches!(&error, RunnerError::Failed(m) if m.starts_with("unable to launch definitely-not-a-command-xyz -"))
        );
    }

    #[test]
    fn a_build_failure_is_one_error_with_the_output_tail() {
        let summary = build_failure("compile error: expected ;\n");
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.tests, 0);
        assert_eq!(
            summary.failure_details,
            vec!["Build failed before tests could run:\ncompile error: expected ;"]
        );
    }
}

//! Process implementation of the [`RuntimeProbe`] port: asks each
//! runtime command for `--version` and reports its first output line.
//! Probing only — this adapter never installs or modifies anything.

use std::process::Command;

use crate::ports::RuntimeProbe;

pub struct ProcessRuntimeProbe;

impl RuntimeProbe for ProcessRuntimeProbe {
    fn version(&self, command: &str) -> Option<String> {
        let output = Command::new(command).arg("--version").output().ok()?;
        if !output.status.success() {
            return None;
        }
        // Some runtimes (older JDKs) print the version on stderr.
        let text = if output.stdout.is_empty() {
            output.stderr
        } else {
            output.stdout
        };
        first_line(&String::from_utf8_lossy(&text))
    }
}

fn first_line(text: &str) -> Option<String> {
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_trims_and_drops_the_rest() {
        assert_eq!(
            first_line("  cargo 1.97.0 \nrelease: x\n"),
            Some("cargo 1.97.0".into())
        );
        assert_eq!(first_line(""), None);
        assert_eq!(first_line("   \n"), None);
    }

    #[test]
    fn a_present_command_reports_its_version_line() {
        // `cargo` is guaranteed present: it is running this test.
        let version = ProcessRuntimeProbe.version("cargo").expect("cargo exists");
        assert!(version.starts_with("cargo "), "got: {version}");
    }

    #[test]
    fn a_missing_command_reports_none() {
        assert_eq!(
            ProcessRuntimeProbe.version("definitely-not-a-real-runtime-xyz"),
            None
        );
    }

    #[test]
    fn a_command_that_exits_nonzero_reports_none() {
        // `false` exists everywhere, ignores its arguments, and exits 1.
        assert_eq!(ProcessRuntimeProbe.version("false"), None);
    }

    #[test]
    fn a_version_printed_on_stderr_is_still_reported() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-jdk");
        std::fs::write(&script, "#!/bin/sh\necho 'fake 1.0' 1>&2\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            ProcessRuntimeProbe.version(script.to_str().unwrap()),
            Some("fake 1.0".into())
        );
    }
}

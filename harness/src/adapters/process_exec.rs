//! The real [`CommandExecutor`]: spawn the argv with the project root as
//! its working directory, enforce a hard timeout (the process is killed
//! on expiry), and cap each output stream to its last lines so a noisy
//! build cannot flood the agent's context.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::adapters::runners::tail;
use crate::ports::{CommandExecutor, ExecError, ExecOutcome};

/// How many lines of each stream survive truncation.
const OUTPUT_LINES: usize = 200;

/// How often the executor checks whether the child finished.
const POLL: Duration = Duration::from_millis(50);

fn isolate_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // SAFETY: `isolate_group` put this child in its own process group
        // whose pgid equals the child's pid; killpg only signals that group.
        unsafe {
            libc::killpg(pid, libc::SIGKILL);
        }
        let _ = child.wait();
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.wait();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub struct ProcessCommandExecutor;

/// Drain one child stream on its own thread so a full pipe buffer can
/// never deadlock the wait loop.
fn drain(stream: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stream) = stream {
            let mut bytes = Vec::new();
            let _ = stream.read_to_end(&mut bytes);
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        text
    })
}

impl CommandExecutor for ProcessCommandExecutor {
    fn run(
        &self,
        argv: &[String],
        dir: &Path,
        timeout: Duration,
    ) -> Result<ExecOutcome, ExecError> {
        let started = Instant::now();
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        isolate_group(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| ExecError(format!("unable to launch {} - {e}", argv[0])))?;
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());

        let (exit_code, timed_out) = loop {
            match child.try_wait() {
                Err(e) => return Err(ExecError(format!("waiting on {} failed - {e}", argv[0]))),
                Ok(Some(status)) => break (status.code(), false),
                Ok(None) if started.elapsed() >= timeout => {
                    kill_tree(&mut child);
                    break (None, true);
                }
                Ok(None) => std::thread::sleep(POLL),
            }
        };

        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        Ok(ExecOutcome {
            exit_code,
            stdout: tail(&stdout, OUTPUT_LINES),
            stderr: tail(&stderr, OUTPUT_LINES),
            timed_out,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    fn run(parts: &[&str], timeout: Duration) -> ExecOutcome {
        let dir = tempfile::tempdir().unwrap();
        ProcessCommandExecutor
            .run(&argv(parts), dir.path(), timeout)
            .unwrap()
    }

    #[test]
    fn output_and_exit_code_are_captured() {
        let outcome = run(
            &["sh", "-c", "echo out; echo err >&2; exit 3"],
            Duration::from_secs(10),
        );
        assert_eq!(outcome.exit_code, Some(3));
        assert_eq!(outcome.stdout, "out");
        assert_eq!(outcome.stderr, "err");
        assert!(!outcome.timed_out);
    }

    #[test]
    fn the_command_runs_in_the_given_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("marker.txt"), "here").unwrap();
        let outcome = ProcessCommandExecutor
            .run(
                &argv(&["sh", "-c", "cat marker.txt"]),
                dir.path(),
                Duration::from_secs(10),
            )
            .unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.stdout, "here");
    }

    #[test]
    fn a_hung_command_is_killed_at_the_timeout() {
        let started = Instant::now();
        let outcome = run(&["sleep", "30"], Duration::from_millis(200));
        assert!(outcome.timed_out);
        assert_eq!(outcome.exit_code, None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the kill happened at the timeout, not after the sleep"
        );
    }

    #[test]
    fn noisy_output_is_truncated_to_its_tail() {
        let outcome = run(&["sh", "-c", "seq 1 500"], Duration::from_secs(10));
        assert!(
            outcome.stdout.starts_with("301\n"),
            "kept the last 200 lines"
        );
        assert!(outcome.stdout.ends_with("\n500"));
    }

    #[test]
    fn a_missing_program_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = ProcessCommandExecutor
            .run(
                &argv(&["definitely-not-a-command-xyz"]),
                dir.path(),
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(
            error
                .0
                .starts_with("unable to launch definitely-not-a-command-xyz -")
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_the_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("alive");
        let script = format!(
            "(sleep 1; echo still > '{}') & exec sleep 30",
            marker.display()
        );
        let outcome = ProcessCommandExecutor
            .run(
                &argv(&["sh", "-c", &script]),
                dir.path(),
                Duration::from_millis(200),
            )
            .unwrap();
        assert!(outcome.timed_out);
        std::thread::sleep(Duration::from_millis(1200));
        assert!(
            !marker.exists(),
            "a grandchild must not outlive the timed-out command"
        );
    }
}

//! The real [`Vcs`]: shells out to `git` in the project directory.
//!
//! Shelling out rather than linking a git library because the harness
//! needs exactly three answers, and because the `git` the developer has
//! on their PATH is the one whose config, worktrees, and submodules
//! match what they see in their own terminal.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::ports::{GitState, Vcs, VcsError};

/// A git that has not answered in this long is a git the harness stops
/// waiting on: a probe at startup must never hang the loop. Reached in
/// practice by a repository on an unreachable network mount.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct GitCli {
    root: PathBuf,
}

impl GitCli {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// One `git` run, as `(success, trimmed stdout)`. `None` when git
    /// is absent, failed to start, or outran [`PROBE_TIMEOUT`].
    fn run(&self, args: &[&str]) -> Option<(bool, String)> {
        run_git(&self.root, args, PROBE_TIMEOUT)
    }
}

/// `git` with its working directory set to `root`, killed if it outlasts
/// `timeout`.
///
/// `-c core.fsmonitor=false` because a repository with a file-system
/// monitor configured can have `git status` block on a daemon that is
/// not running.
fn run_git(root: &Path, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut child = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

impl Vcs for GitCli {
    fn state(&self) -> GitState {
        let inside = matches!(
            self.run(&["rev-parse", "--is-inside-work-tree"]),
            Some((true, answer)) if answer == "true"
        );
        if !inside {
            return GitState::default();
        }
        // `--show-current` is empty on a detached HEAD and on a
        // repository with no commits yet; both mean "no branch to name".
        let branch = self
            .run(&["branch", "--show-current"])
            .and_then(|(ok, name)| (ok && !name.is_empty()).then_some(name));
        // `--porcelain` lists tracked edits and untracked files alike.
        // A probe that cannot tell reports clean rather than inventing
        // work the developer does not have.
        let dirty = matches!(
            self.run(&["status", "--porcelain"]),
            Some((true, out)) if !out.is_empty()
        );
        GitState {
            repository: true,
            branch,
            dirty,
        }
    }

    fn create_branch(&self, name: &str) -> Result<(), VcsError> {
        // `--` so a name that looks like a flag is still a branch name.
        match run_git(
            &self.root,
            &["switch", "--create", name, "--"],
            PROBE_TIMEOUT,
        ) {
            Some((true, _)) => Ok(()),
            Some((false, _)) => Err(VcsError(format!(
                "git could not create the branch {name} - it may already exist, \
                 or the name may not be a valid ref."
            ))),
            None => Err(VcsError(
                "git did not answer - it may not be installed, or not on PATH.".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "--initial-branch=main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            run_git(dir.path(), &args, PROBE_TIMEOUT).expect("git is on PATH for these tests");
        }
        dir
    }

    fn commit(dir: &Path) {
        std::fs::write(dir.join("README.md"), "hi\n").unwrap();
        run_git(dir, &["add", "."], PROBE_TIMEOUT).unwrap();
        run_git(dir, &["commit", "-m", "first"], PROBE_TIMEOUT).unwrap();
    }

    #[test]
    fn a_directory_outside_any_repository_reads_as_no_repository() {
        let dir = tempfile::tempdir().unwrap();
        let state = GitCli::new(dir.path().to_path_buf()).state();
        assert_eq!(state, GitState::default());
        assert!(!state.repository);
    }

    #[test]
    fn a_clean_repository_names_its_branch() {
        let dir = repo();
        commit(dir.path());
        let state = GitCli::new(dir.path().to_path_buf()).state();
        assert!(state.repository);
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert!(!state.dirty, "nothing was edited");
    }

    /// Untracked files count: a branch made over them carries them.
    #[test]
    fn an_untracked_file_makes_the_tree_dirty() {
        let dir = repo();
        commit(dir.path());
        std::fs::write(dir.path().join("scratch.txt"), "wip").unwrap();
        assert!(GitCli::new(dir.path().to_path_buf()).state().dirty);
    }

    #[test]
    fn a_created_branch_is_the_one_checked_out() {
        let dir = repo();
        commit(dir.path());
        let git = GitCli::new(dir.path().to_path_buf());
        git.create_branch("spec/kata").unwrap();
        assert_eq!(git.state().branch.as_deref(), Some("spec/kata"));
    }

    #[test]
    fn creating_a_branch_that_exists_is_a_structured_error() {
        let dir = repo();
        commit(dir.path());
        let git = GitCli::new(dir.path().to_path_buf());
        git.create_branch("spec/kata").unwrap();
        let error = git.create_branch("spec/kata").unwrap_err();
        assert!(error.0.contains("spec/kata"), "{}", error.0);
    }

    /// A repository with no commits is still a repository, and `git
    /// branch --show-current` has nothing to print - the harness must
    /// not read that as "not a repository".
    #[test]
    fn a_repository_with_no_commits_is_still_a_repository() {
        let dir = repo();
        let state = GitCli::new(dir.path().to_path_buf()).state();
        assert!(state.repository);
    }
}

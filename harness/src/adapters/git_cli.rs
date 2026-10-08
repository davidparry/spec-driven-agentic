//! The real [`Vcs`]: shells out to `git` in the project directory.
//!
//! Shelling out rather than linking a git library because the harness
//! needs exactly three answers, and because the `git` the developer has
//! on their PATH is the one whose config, worktrees, and submodules
//! match what they see in their own terminal.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::ports::{DiffOutput, GitState, Vcs, VcsDiff, VcsError};

/// A git that has not answered in this long is a git the harness stops
/// waiting on: a probe at startup must never hang the loop. Reached in
/// practice by a repository on an unreachable network mount.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Reading a diff is not a probe. It walks the whole working tree, and
/// on a large repository that takes longer than a question the loop is
/// waiting on at startup.
const DIFF_TIMEOUT: Duration = Duration::from_secs(30);

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

/// A finished `git` run, both streams kept.
struct GitOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

/// One `git` run as `(success, trimmed stdout)` — the shape the probes
/// want, where an answer is a single word and a failure needs no
/// explanation beyond "it failed".
fn run_git(root: &Path, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    run_git_output(root, args, timeout)
        .map(|output| (output.success, output.stdout.trim().to_string()))
}

/// `git` with its working directory set to `root`, killed if it outlasts
/// `timeout`.
///
/// `-c core.fsmonitor=false` because a repository with a file-system
/// monitor configured can have `git status` block on a daemon that is
/// not running.
///
/// stdout is returned verbatim: leading and trailing space is content
/// in a unified diff, so only the callers that want a single-word
/// answer trim it.
fn run_git_output(root: &Path, args: &[&str], timeout: Duration) -> Option<GitOutput> {
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
    Some(GitOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
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

impl VcsDiff for GitCli {
    fn diff(&self, path: Option<&str>) -> Result<DiffOutput, VcsError> {
        // Three different failures the caller would otherwise have to
        // guess between: git is not on this machine, git is here but
        // this directory is not a repository, and git ran and refused.
        let inside = self.ask("rev-parse", &["rev-parse", "--is-inside-work-tree"])?;
        if !inside.success || inside.stdout.trim() != "true" {
            return Err(VcsError(format!(
                "{} is not inside a git work tree, so there is nothing to diff.",
                self.root.display()
            )));
        }
        // `git diff HEAD` in a repository with no commits fails with a
        // bare `fatal:`. The first commit is the thing to say instead.
        if !self
            .ask("rev-parse", &["rev-parse", "--verify", "HEAD"])?
            .success
        {
            return Err(VcsError(
                "this repository has no commits yet, so there is no HEAD to diff against.".into(),
            ));
        }

        // `--no-color` is explicit because `color.diff = always` in a
        // developer's own git config would otherwise put ANSI escapes
        // into text that goes on to a model.
        let diff = self.ask_for("git diff", &["diff", "--no-color", "HEAD"], path)?;
        let untracked = self.ask_for(
            "git ls-files",
            &["ls-files", "--others", "--exclude-standard"],
            path,
        )?;

        Ok(DiffOutput {
            diff: diff.stdout,
            untracked: untracked
                .stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect(),
        })
    }
}

impl GitCli {
    /// One `git` run on the diff budget. A git that never answers is
    /// reported as the missing tool it probably is, named by the
    /// subcommand that was waiting on it.
    fn ask(&self, label: &str, args: &[&str]) -> Result<GitOutput, VcsError> {
        run_git_output(&self.root, args, DIFF_TIMEOUT).ok_or_else(|| {
            VcsError(format!(
                "git did not answer `{label}` - it may not be installed, or not on PATH."
            ))
        })
    }

    /// [`GitCli::ask`] narrowed to one pathspec, refusing on git's own
    /// words rather than returning its partial output as an answer.
    fn ask_for(
        &self,
        label: &str,
        args: &[&str],
        path: Option<&str>,
    ) -> Result<GitOutput, VcsError> {
        // `--` so a path that looks like a flag or a ref is still read
        // as a path.
        let mut args: Vec<&str> = args.to_vec();
        if let Some(path) = path {
            args.push("--");
            args.push(path);
        }
        let output = self.ask(label, &args)?;
        if output.success {
            return Ok(output);
        }
        let reason = output.stderr.trim();
        Err(VcsError(if reason.is_empty() {
            format!("{label} failed without saying why.")
        } else {
            format!("{label} failed - {reason}")
        }))
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

    #[test]
    fn an_edit_to_a_tracked_file_is_in_the_diff() {
        let dir = repo();
        commit(dir.path());
        std::fs::write(dir.path().join("README.md"), "hi there\n").unwrap();
        let output = GitCli::new(dir.path().to_path_buf()).diff(None).unwrap();
        assert!(output.diff.contains("README.md"), "got: {}", output.diff);
        assert!(output.diff.contains("+hi there"), "got: {}", output.diff);
        assert!(output.untracked.is_empty());
    }

    /// A staged edit is still uncommitted work, and a summary that only
    /// read the unstaged half would be missing most of a reviewed change.
    #[test]
    fn a_staged_edit_is_in_the_diff_against_head() {
        let dir = repo();
        commit(dir.path());
        std::fs::write(dir.path().join("README.md"), "staged\n").unwrap();
        run_git(dir.path(), &["add", "."], PROBE_TIMEOUT).unwrap();
        let output = GitCli::new(dir.path().to_path_buf()).diff(None).unwrap();
        assert!(output.diff.contains("+staged"), "got: {}", output.diff);
    }

    /// `git diff HEAD` cannot see a file git has never been told about,
    /// so the names come from a second question.
    #[test]
    fn an_untracked_file_is_named_rather_than_diffed() {
        let dir = repo();
        commit(dir.path());
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(dir.path().join("requirements/new.json"), "{}\n").unwrap();
        let output = GitCli::new(dir.path().to_path_buf()).diff(None).unwrap();
        assert_eq!(output.untracked, vec!["requirements/new.json".to_string()]);
        assert!(output.diff.is_empty(), "got: {}", output.diff);
    }

    #[test]
    fn a_path_narrows_both_the_diff_and_the_untracked_list() {
        let dir = repo();
        commit(dir.path());
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(dir.path().join("requirements/new.json"), "{}\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "edited\n").unwrap();
        let git = GitCli::new(dir.path().to_path_buf());

        let narrowed = git.diff(Some("requirements")).unwrap();
        assert!(
            !narrowed.diff.contains("README.md"),
            "the path did not narrow the diff: {}",
            narrowed.diff
        );
        assert_eq!(
            narrowed.untracked,
            vec!["requirements/new.json".to_string()]
        );

        let whole = git.diff(None).unwrap();
        assert!(whole.diff.contains("README.md"), "got: {}", whole.diff);
    }

    #[test]
    fn a_directory_outside_any_repository_has_no_diff() {
        let dir = tempfile::tempdir().unwrap();
        let error = GitCli::new(dir.path().to_path_buf())
            .diff(None)
            .unwrap_err();
        assert!(
            error.0.contains("not inside a git work tree"),
            "{}",
            error.0
        );
    }

    /// There is no HEAD to compare against before the first commit, and
    /// git's own `fatal:` does not say which commit it wanted.
    #[test]
    fn a_repository_with_no_commits_names_the_missing_first_commit() {
        let dir = repo();
        let error = GitCli::new(dir.path().to_path_buf())
            .diff(None)
            .unwrap_err();
        assert!(error.0.contains("no commits yet"), "{}", error.0);
    }

    /// A pathspec git will not accept is git's refusal, not an empty
    /// diff that reads as "nothing changed".
    #[test]
    fn a_pathspec_git_refuses_is_reported_as_a_refusal() {
        let dir = repo();
        commit(dir.path());
        let error = GitCli::new(dir.path().to_path_buf())
            .diff(Some("../outside"))
            .unwrap_err();
        assert!(error.0.starts_with("git diff failed"), "{}", error.0);
    }

    /// A diff is content: trimming it would eat the blank context line
    /// that ends most hunks.
    #[test]
    fn the_diff_keeps_the_whitespace_git_printed() {
        let dir = repo();
        commit(dir.path());
        std::fs::write(dir.path().join("README.md"), "hi\n\n").unwrap();
        let output = GitCli::new(dir.path().to_path_buf()).diff(None).unwrap();
        assert!(output.diff.ends_with('\n'), "got: {:?}", output.diff);
    }
}

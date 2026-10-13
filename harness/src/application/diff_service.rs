//! The use case behind the `git_diff` tool and `spec diff`: ask version
//! control what changed under a path, and shape the answer into
//! something a model can read.
//!
//! Order of checks: the path jail first, so a pathspec that could reach
//! outside the project root never reaches git, then the diff itself,
//! then the budget. There is no phase gate — reading what changed is
//! safe on any colour of bar.

use serde::Serialize;

use crate::application::spec_service::ServiceError;
use crate::domain::diff::{MAX_DIFF_BYTES, cap, cap_untracked, requested_path};
use crate::ports::VcsDiff;

/// What the working tree is compared against. Uncommitted work is
/// staged and unstaged alike, which is what someone asking "what
/// changed?" means, and neither half alone would answer it.
pub const AGAINST: &str = "HEAD";

/// The `git_diff` reply: what changed under a path, and what to do with it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DiffReport {
    /// The pathspec that was diffed, `null` for the whole project.
    pub path: Option<String>,
    pub against: &'static str,
    pub diff: String,
    /// Whether the diff was cut at [`MAX_DIFF_BYTES`].
    pub truncated: bool,
    /// Files git is not tracking: named, never read.
    pub untracked: Vec<String>,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct DiffService<V: VcsDiff> {
    vcs: V,
}

impl<V: VcsDiff> DiffService<V> {
    pub fn new(vcs: V) -> Self {
        Self { vcs }
    }

    pub fn diff(&self, path: Option<&str>) -> Result<DiffReport, ServiceError> {
        let path = requested_path(path)
            .map_err(|reason| ServiceError(format!("that path is refused: {reason}")))?;
        let output = self
            .vcs
            .diff(path.as_deref())
            .map_err(|e| ServiceError(e.0))?;

        let diff = cap(&output.diff, MAX_DIFF_BYTES);
        let (untracked, dropped) = cap_untracked(output.untracked);
        let next_step = next_step(
            path.as_deref(),
            &diff.text,
            &untracked,
            diff.truncated,
            dropped,
        );

        Ok(DiffReport {
            path,
            against: AGAINST,
            diff: diff.text,
            truncated: diff.truncated,
            untracked,
            next_step,
        })
    }
}

/// What to do with this reply, in the caller's own situation.
///
/// An empty diff is the case worth wording carefully: it is the same
/// answer for "nothing changed here" and for "that path is spelled
/// wrong", and git reports neither.
fn next_step(
    path: Option<&str>,
    diff: &str,
    untracked: &[String],
    truncated: bool,
    dropped: usize,
) -> String {
    let location = match path {
        Some(path) => format!("under {path}"),
        None => "in the project".to_string(),
    };
    if diff.is_empty() && untracked.is_empty() {
        return format!(
            "Nothing {location} has changed since the last commit. If you expected \
             changes, check the path is spelled the way it is on disk."
        );
    }

    let mut step = format!(
        "Summarize this for the developer: name what behavior changed {location} and \
         why it matters, and call out anything that looks unintended."
    );
    if !untracked.is_empty() {
        step.push_str(
            " The untracked files are new and have no diff yet - say they were added \
             rather than describing contents you were not shown.",
        );
    }
    if truncated {
        step.push_str(&format!(
            " The diff was cut at {MAX_DIFF_BYTES} bytes, so say the summary is \
             partial; a narrower path returns the rest."
        ));
    }
    if dropped > 0 {
        step.push_str(&format!(
            " {dropped} further untracked files are not listed."
        ));
    }
    step
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{DiffOutput, VcsError};

    /// Records the pathspec it was asked for and replies with a script.
    struct FakeGit {
        reply: Result<DiffOutput, VcsError>,
        asked: std::cell::RefCell<Vec<Option<String>>>,
    }

    impl FakeGit {
        fn replying(diff: &str, untracked: &[&str]) -> Self {
            Self {
                reply: Ok(DiffOutput {
                    diff: diff.to_string(),
                    untracked: untracked.iter().map(|n| n.to_string()).collect(),
                }),
                asked: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn failing(message: &str) -> Self {
            Self {
                reply: Err(VcsError(message.into())),
                asked: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl VcsDiff for FakeGit {
        fn diff(&self, path: Option<&str>) -> Result<DiffOutput, VcsError> {
            self.asked.borrow_mut().push(path.map(String::from));
            self.reply.clone()
        }
    }

    #[test]
    fn a_diff_under_a_path_is_reported_with_what_to_do_about_it() {
        let service = DiffService::new(FakeGit::replying("diff --git a/x b/x\n+one\n", &[]));
        let report = service.diff(Some("requirements")).unwrap();
        assert_eq!(report.path.as_deref(), Some("requirements"));
        assert_eq!(report.against, "HEAD");
        assert_eq!(report.diff, "diff --git a/x b/x\n+one\n");
        assert!(!report.truncated);
        assert!(report.next_step.contains("under requirements"));

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("nextStep"), "{json}");
    }

    /// The jail runs before git does: a pathspec that could reach
    /// outside the root is never handed to a subprocess.
    #[test]
    fn a_path_outside_the_root_is_refused_before_git_runs() {
        let service = DiffService::new(FakeGit::replying("", &[]));
        let error = service.diff(Some("../../etc")).unwrap_err();
        assert!(error.0.contains("refused"), "got: {}", error.0);
        assert!(
            service.vcs.asked.borrow().is_empty(),
            "a refused path still reached git"
        );
    }

    #[test]
    fn the_normalized_path_is_the_one_git_is_asked_for() {
        let service = DiffService::new(FakeGit::replying("", &[]));
        service.diff(Some("./requirements/")).unwrap();
        assert_eq!(
            service.vcs.asked.borrow().as_slice(),
            [Some("requirements".to_string())]
        );
    }

    #[test]
    fn no_path_diffs_the_whole_project() {
        let service = DiffService::new(FakeGit::replying("+one\n", &[]));
        let report = service.diff(None).unwrap();
        assert_eq!(report.path, None);
        assert_eq!(service.vcs.asked.borrow().as_slice(), [None]);
        assert!(report.next_step.contains("in the project"));
    }

    /// The one reply a reader can misread: git says the same nothing
    /// for a clean path and for a path that does not exist.
    #[test]
    fn an_empty_diff_says_so_and_suggests_checking_the_path() {
        let service = DiffService::new(FakeGit::replying("", &[]));
        let report = service.diff(Some("requirements")).unwrap();
        assert!(report.next_step.contains("Nothing under requirements"));
        assert!(report.next_step.contains("check the path"));
    }

    /// An untracked file is a change with no diff, and a model told to
    /// summarize a diff would otherwise invent its contents.
    #[test]
    fn untracked_files_are_named_and_the_reply_says_they_have_no_diff() {
        let service = DiffService::new(FakeGit::replying("", &["requirements/new.json"]));
        let report = service.diff(None).unwrap();
        assert_eq!(report.untracked, vec!["requirements/new.json".to_string()]);
        assert!(
            report.next_step.contains("no diff yet"),
            "{}",
            report.next_step
        );
        assert!(!report.next_step.contains("Nothing"));
    }

    #[test]
    fn an_oversized_diff_is_cut_and_the_reply_admits_it() {
        let huge = "x".repeat(MAX_DIFF_BYTES + 1) + "\n";
        let service = DiffService::new(FakeGit::replying(&huge, &[]));
        let report = service.diff(None).unwrap();
        assert!(report.truncated);
        assert!(report.diff.len() <= MAX_DIFF_BYTES);
        assert!(report.next_step.contains("partial"), "{}", report.next_step);
    }

    #[test]
    fn a_refusal_from_git_is_the_service_error() {
        let service = DiffService::new(FakeGit::failing(
            "git did not answer - it may not be installed, or not on PATH.",
        ));
        let error = service.diff(None).unwrap_err();
        assert_eq!(
            error.0,
            "git did not answer - it may not be installed, or not on PATH."
        );
    }
}

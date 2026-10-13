//! The branch gate: the one stop a generating run makes before it
//! starts writing.
//!
//! The harness writes the project's real files. That is only safe
//! because the developer can throw the result away, and git is what
//! makes it throwable. So the two orchestrators that generate at
//! scale, `spec greenfield` and `spec deliver`, ask once, up front,
//! whether to put the run on a branch of its own.
//!
//! It is one question with three answers and no wrong one: a name, an
//! empty line (take a generated name), or a decline (write where we
//! stand). Outside a repository, or with `--no-branch`, nothing is
//! asked and nothing is created.

use crate::domain::branch::{clean_branch_name, generated_branch_name};
use crate::ports::{Prompter, Vcs};

/// What the gate did, for the caller to narrate or assert on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Branched {
    /// A branch was created and checked out.
    Created(String),
    /// The project is not a git repository, so there was nothing to
    /// branch from.
    NoRepository,
    /// `--no-branch` was passed: git was not consulted at all.
    Skipped,
    /// The developer was asked and said no.
    Declined,
    /// The developer asked for a branch and git refused. Not fatal -
    /// the run goes on where it stands, having said so.
    Failed(String),
}

/// Offer the run a branch of its own.
///
/// `no_branch` short-circuits before `vcs` is touched, which is the
/// point of the flag: a project that is deliberately not under git, or
/// a CI job that manages its own refs, should not pay for a git probe
/// or be asked a question nobody will answer.
pub fn offer_branch(
    prompter: &mut dyn Prompter,
    vcs: &dyn Vcs,
    no_branch: bool,
    today: &str,
    seed: u64,
) -> Branched {
    if no_branch {
        return Branched::Skipped;
    }
    let state = vcs.state();
    if !state.repository {
        prompter.warn(
            "This project is not a git repository, so there is no branch to work on \
             and no undo for what this run writes. Continuing here.",
        );
        return Branched::NoRepository;
    }
    let on = state.branch.as_deref().unwrap_or("a detached HEAD");
    prompter.tell(&format!(
        "This run writes the project's files directly. You are on {on}."
    ));
    if state.dirty {
        // Said before the question, because it changes the answer: a
        // branch made now carries the uncommitted work onto it.
        prompter.warn(
            "There is uncommitted work here. A new branch carries it along, so it \
             will be mixed in with what this run writes.",
        );
    }
    let generated = generated_branch_name(today, seed);
    let typed = match prompter.ask(&format!(
        "Branch name for this run (Enter for {generated}, or n to stay on {on})"
    )) {
        Ok(answer) => answer,
        // No one is there to answer. Staying put is the choice that
        // changes nothing about where the run writes.
        Err(_) => return Branched::Declined,
    };
    let answer = typed.trim();
    if answer.eq_ignore_ascii_case("n") || answer.eq_ignore_ascii_case("no") {
        return Branched::Declined;
    }
    let name = if answer.is_empty() {
        generated
    } else {
        match clean_branch_name(answer) {
            Ok(name) => name,
            Err(reason) => {
                prompter.warn(&format!(
                    "{answer} is not a usable branch name ({reason}). Continuing on {on}."
                ));
                return Branched::Failed(reason);
            }
        }
    };
    match vcs.create_branch(&name) {
        Ok(()) => {
            prompter.tell(&format!(
                "Working on {name}. Keep it, merge it, or throw the whole run away with \
                 git switch {on} && git branch -D {name}."
            ));
            Branched::Created(name)
        }
        Err(error) => {
            prompter.warn(&format!("{}. Continuing on {on}.", error.0));
            Branched::Failed(error.0)
        }
    }
}

/// Today, as the generated branch name wants it.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// A seed for the generated name. Nothing depends on it being
/// unguessable; it only has to differ between two runs on one day.
pub fn seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{GitState, PromptError, VcsError, Working};
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeVcs {
        state: GitState,
        created: RefCell<Vec<String>>,
        refuse: Option<String>,
    }

    impl FakeVcs {
        fn repository() -> Self {
            Self {
                state: GitState {
                    repository: true,
                    branch: Some("main".into()),
                    dirty: false,
                },
                ..Default::default()
            }
        }
    }

    impl Vcs for FakeVcs {
        fn state(&self) -> GitState {
            self.state.clone()
        }

        fn create_branch(&self, name: &str) -> Result<(), VcsError> {
            if let Some(reason) = &self.refuse {
                return Err(VcsError(reason.clone()));
            }
            self.created.borrow_mut().push(name.to_string());
            Ok(())
        }
    }

    struct Nothing;
    impl Working for Nothing {}

    #[derive(Default)]
    struct Scripted {
        answers: RefCell<Vec<String>>,
        told: RefCell<Vec<String>>,
        warned: RefCell<Vec<String>>,
        /// No one is attached: every question fails.
        exhausted: bool,
    }

    impl Scripted {
        fn answering(answer: &str) -> Self {
            Self {
                answers: RefCell::new(vec![answer.to_string()]),
                ..Default::default()
            }
        }

        fn said(&self) -> String {
            let mut all = self.told.borrow().join(" ");
            all.push(' ');
            all.push_str(&self.warned.borrow().join(" "));
            all
        }
    }

    impl Prompter for Scripted {
        fn ask(&mut self, _: &str) -> Result<String, PromptError> {
            if self.exhausted {
                return Err(PromptError("no one is there".into()));
            }
            let mut answers = self.answers.borrow_mut();
            Ok(if answers.is_empty() {
                String::new()
            } else {
                answers.remove(0)
            })
        }

        fn confirm(&mut self, _: &str) -> Result<bool, PromptError> {
            Ok(false)
        }

        fn tell(&mut self, message: &str) {
            self.told.borrow_mut().push(message.to_string());
        }

        fn warn(&mut self, message: &str) {
            self.warned.borrow_mut().push(message.to_string());
        }

        fn working(&mut self, _: &str) -> Box<dyn Working> {
            Box::new(Nothing)
        }
    }

    #[test]
    fn a_typed_name_is_the_branch_that_gets_created() {
        let vcs = FakeVcs::repository();
        let mut prompter = Scripted::answering("newline support");
        let outcome = offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7);
        assert_eq!(outcome, Branched::Created("spec/newline-support".into()));
        assert_eq!(*vcs.created.borrow(), ["spec/newline-support"]);
    }

    /// Enter is the path most people take, so it has to land somewhere
    /// rather than ask again.
    #[test]
    fn an_empty_answer_takes_the_generated_name() {
        let vcs = FakeVcs::repository();
        let mut prompter = Scripted::answering("");
        let outcome = offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7);
        assert_eq!(
            outcome,
            Branched::Created(generated_branch_name("2026-10-05", 7))
        );
    }

    #[test]
    fn declining_leaves_the_run_where_it_stands() {
        let vcs = FakeVcs::repository();
        let mut prompter = Scripted::answering("n");
        assert_eq!(
            offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7),
            Branched::Declined
        );
        assert!(vcs.created.borrow().is_empty());
    }

    /// The whole point of the flag: git is never consulted, so a
    /// project that is deliberately not under version control is not
    /// asked about one.
    #[test]
    fn the_no_branch_flag_asks_nothing_and_touches_no_git() {
        let vcs = FakeVcs::repository();
        let mut prompter = Scripted::answering("newlines");
        assert_eq!(
            offer_branch(&mut prompter, &vcs, true, "2026-10-05", 7),
            Branched::Skipped
        );
        assert!(vcs.created.borrow().is_empty());
        assert_eq!(prompter.said().trim(), "");
    }

    #[test]
    fn a_project_outside_a_repository_is_told_there_is_no_undo() {
        let vcs = FakeVcs::default();
        let mut prompter = Scripted::answering("newlines");
        assert_eq!(
            offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7),
            Branched::NoRepository
        );
        assert!(prompter.said().contains("not a git repository"));
        assert!(prompter.said().contains("no undo"));
    }

    /// A branch made over uncommitted work carries it, which decides
    /// whether a developer wants one - so it is said before the ask.
    #[test]
    fn uncommitted_work_is_called_out_before_the_question() {
        let vcs = FakeVcs {
            state: GitState {
                repository: true,
                branch: Some("main".into()),
                dirty: true,
            },
            ..Default::default()
        };
        let mut prompter = Scripted::answering("");
        offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7);
        assert!(prompter.said().contains("uncommitted work"));
    }

    /// git refusing is not the end of the run - it is a warning and a
    /// run that writes where it already was.
    #[test]
    fn a_git_failure_warns_and_the_run_continues() {
        let vcs = FakeVcs {
            refuse: Some("branch spec/x already exists".into()),
            ..FakeVcs::repository()
        };
        let mut prompter = Scripted::answering("x");
        let outcome = offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7);
        assert_eq!(
            outcome,
            Branched::Failed("branch spec/x already exists".into())
        );
        assert!(prompter.said().contains("Continuing on main"));
    }

    #[test]
    fn an_unusable_name_warns_rather_than_handing_git_a_bad_ref() {
        let vcs = FakeVcs::repository();
        let mut prompter = Scripted::answering("a..b");
        assert!(matches!(
            offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7),
            Branched::Failed(_)
        ));
        assert!(vcs.created.borrow().is_empty());
        assert!(prompter.said().contains("not a usable branch name"));
    }

    /// Piped into a script with nothing left to read: staying put is
    /// the answer that changes nothing.
    #[test]
    fn no_one_to_answer_means_no_branch() {
        let vcs = FakeVcs::repository();
        let mut prompter = Scripted {
            exhausted: true,
            ..Default::default()
        };
        assert_eq!(
            offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7),
            Branched::Declined
        );
        assert!(vcs.created.borrow().is_empty());
    }

    #[test]
    fn a_detached_head_is_named_rather_than_left_blank() {
        let vcs = FakeVcs {
            state: GitState {
                repository: true,
                branch: None,
                dirty: false,
            },
            ..Default::default()
        };
        let mut prompter = Scripted::answering("n");
        offer_branch(&mut prompter, &vcs, false, "2026-10-05", 7);
        assert!(prompter.said().contains("detached HEAD"));
    }
}

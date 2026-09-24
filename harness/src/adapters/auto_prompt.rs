//! What a run with nobody at the keyboard answers, so `spec deliver` can
//! walk the whole loop without ever stopping at a prompt.
//!
//! Every wizard prompt in this harness already treats Enter as "accept
//! what is in front of you": the drafting wizard shows each proposed
//! field in brackets and keeps it, `Accept [Enter for all, ...]` takes
//! every proposal, and `[1-n, Enter for 1]` takes the first. So a
//! prompter that answers the empty string is not a special unattended
//! dialect - it is the accept-the-proposal path the interactive session
//! takes when the developer holds down Enter.

use crate::ports::{PromptError, Prompter, Working};

/// A [`Prompter`] that never reads: questions take their default and
/// confirmations say yes.
///
/// Every question is echoed with the answer given on the developer's
/// behalf. The proposal lives in the question text - `title [Adds two
/// numbers] (Enter keeps it):` - so echoing is the only way the transcript
/// says what wording the run actually took, and a decision made for
/// someone should be one they can read afterwards.
pub struct AutoPrompter<P: Prompter> {
    inner: P,
    /// What the echo credits the answer to, e.g. `spec deliver never
    /// stops to ask`.
    because: String,
}

impl<P: Prompter> AutoPrompter<P> {
    /// Narration goes to `inner`; `because` says why nothing was asked.
    pub fn new(inner: P, because: &str) -> Self {
        Self {
            inner,
            because: because.to_string(),
        }
    }

    /// The wrapped prompter back, for a caller that needs what it
    /// recorded once the run is over.
    pub fn into_inner(self) -> P {
        self.inner
    }
}

impl<P: Prompter> Prompter for AutoPrompter<P> {
    fn tell(&mut self, message: &str) {
        self.inner.tell(message);
    }

    fn warn(&mut self, message: &str) {
        self.inner.warn(message);
    }

    fn working(&mut self, message: &str) -> Box<dyn Working> {
        self.inner.working(message)
    }

    /// The empty answer, which every prompt reads as "keep the
    /// proposal". Never fails, so no loop that re-asks until it likes
    /// the answer can spin here.
    fn ask(&mut self, question: &str) -> Result<String, PromptError> {
        let because = self.because.clone();
        self.inner.tell(&format!("{question} kept ({because})"));
        Ok(String::new())
    }

    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        let because = self.because.clone();
        self.inner.tell(&format!("{question} yes ({because})"));
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Narration is recorded; a read that arrives here is a bug, because
    /// reaching the inner prompter's input is reaching the terminal.
    #[derive(Default)]
    struct Recorder {
        told: Vec<String>,
        warned: Vec<String>,
    }

    impl Prompter for Recorder {
        fn tell(&mut self, message: &str) {
            self.told.push(message.to_string());
        }

        fn warn(&mut self, message: &str) {
            self.warned.push(message.to_string());
        }

        fn ask(&mut self, question: &str) -> Result<String, PromptError> {
            panic!("an unattended run must not read input, but {question:?} was asked");
        }

        fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
            panic!("an unattended run must not read input, but {question:?} was confirmed");
        }
    }

    fn auto() -> AutoPrompter<Recorder> {
        AutoPrompter::new(Recorder::default(), "spec deliver")
    }

    #[test]
    fn every_question_takes_the_proposal_and_no_question_reaches_the_input() {
        let mut prompter = auto();
        assert_eq!(prompter.ask("REQ-001 title [Adds two]:").unwrap(), "");
        assert_eq!(prompter.ask("Accept [Enter for all]:").unwrap(), "");
    }

    /// The proposal is in the question text, so echoing it is what tells
    /// the developer which wording the run accepted for them.
    #[test]
    fn every_question_is_echoed_with_the_proposal_it_kept() {
        let mut prompter = auto();
        let _ = prompter.ask("REQ-001 story [As a user ...]:");
        assert_eq!(
            prompter.inner.told,
            vec!["REQ-001 story [As a user ...]: kept (spec deliver)"]
        );
    }

    #[test]
    fn every_confirmation_is_approved_and_echoed_with_its_reason() {
        let mut prompter = auto();
        assert!(prompter.confirm("Commit the generated tests?").unwrap());
        assert_eq!(
            prompter.inner.told,
            vec!["Commit the generated tests? yes (spec deliver)"]
        );
    }

    #[test]
    fn narration_still_reaches_the_developer() {
        let mut prompter = auto();
        prompter.tell("REQ-001 committed to the spec.");
        prompter.warn("Runtime missing (JDK).");
        drop(prompter.working("Running the tests - working"));
        assert_eq!(
            prompter.inner.told,
            vec![
                "REQ-001 committed to the spec.",
                "Running the tests - working ...",
            ]
        );
        assert_eq!(prompter.inner.warned, vec!["Runtime missing (JDK)."]);
    }

    /// The reason is whatever the composition root names, so a second
    /// entry point with nobody at the keyboard can say why in its own
    /// words.
    #[test]
    fn the_echoed_reason_is_the_one_the_caller_named() {
        let mut prompter = AutoPrompter::new(Recorder::default(), "no terminal");
        assert!(prompter.confirm("Start a refactor?").unwrap());
        assert_eq!(
            prompter.inner.told,
            vec!["Start a refactor? yes (no terminal)"]
        );
    }

    #[test]
    fn the_wrapped_prompter_comes_back_with_what_it_recorded() {
        let mut prompter = auto();
        prompter.tell("narrated");
        assert_eq!(prompter.into_inner().told, vec!["narrated"]);
    }
}

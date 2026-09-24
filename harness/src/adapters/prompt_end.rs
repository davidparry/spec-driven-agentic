//! What a wizard does when the answers run out.
//!
//! An empty line is an answer; end of input is the absence of one.
//! Conflating them is what made `spec reword` hang on a spent pipe: the
//! wording review asks "[r]eword again, [m]anual, [a]ccept [Enter for
//! r]", read the end of the pipe as "r", reworded, found the same
//! finding, and asked again - forever.
//!
//! [`PromptError::is_end_of_input`] is the distinction. This decorator
//! is what the CLI does with it: stop asking, decline, and say why.

use std::io::Write;

use crate::ports::{PromptError, Prompter, Working};

/// A [`Prompter`] that treats the end of the input as a decline.
///
/// Once the input has run out, no further question is put to it - the
/// answers cannot arrive now, and asking again is how a loop becomes
/// an infinite one. Confirmations answer no, which is the safe
/// terminal action: a wizard that ends in "Stage this?" reaches its
/// own declined outcome and reports it, rather than erroring out with
/// a report the caller never sees.
///
/// Wrapped at the composition root alongside [`HushingPrompter`] for
/// the same reason: the wizard several layers down should not have to
/// know where its answers come from, and a wizard written later
/// inherits this without asking for it.
///
/// [`HushingPrompter`]: crate::adapters::spinner::HushingPrompter
pub struct AbortOnEndOfInput<P: Prompter> {
    inner: P,
    /// Where the one-line explanation goes. Stderr by default, so a
    /// caller parsing the JSON on stdout still can.
    explain_to: Box<dyn Write + Send>,
    ended: bool,
}

impl<P: Prompter> AbortOnEndOfInput<P> {
    pub fn new(inner: P) -> Self {
        Self::explaining_to(inner, Box::new(std::io::stderr()))
    }

    pub fn explaining_to(inner: P, explain_to: Box<dyn Write + Send>) -> Self {
        Self {
            inner,
            explain_to,
            ended: false,
        }
    }

    /// Whether the input has run out. Once true it stays true.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Latch, and say once what happened. Silence here is what a piped
    /// caller experiences as the command mysteriously doing nothing.
    fn note(&mut self, error: &PromptError) {
        if self.ended {
            return;
        }
        self.ended = true;
        let _ = writeln!(
            self.explain_to,
            "{error}: nothing more will be asked, the confirmation is declined, \
             and nothing is staged. Supply an answer for every prompt, including \
             the final confirmation, to stage from a pipe."
        );
        let _ = self.explain_to.flush();
    }

    /// The error to give a caller that asks after the input ended,
    /// without putting the question to anyone.
    fn no_answer_coming() -> PromptError {
        PromptError::ended("the input already ended")
    }
}

impl<P: Prompter> Prompter for AbortOnEndOfInput<P> {
    fn tell(&mut self, message: &str) {
        self.inner.tell(message);
    }

    fn warn(&mut self, message: &str) {
        self.inner.warn(message);
    }

    fn working(&mut self, message: &str) -> Box<dyn Working> {
        self.inner.working(message)
    }

    fn ask(&mut self, question: &str) -> Result<String, PromptError> {
        if self.ended {
            return Err(Self::no_answer_coming());
        }
        match self.inner.ask(question) {
            Err(error) if error.is_end_of_input() => {
                self.note(&error);
                Err(error)
            }
            other => other,
        }
    }

    /// No is the safe answer, and the one the wizard already knows how
    /// to act on.
    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        if self.ended {
            return Ok(false);
        }
        match self.inner.confirm(question) {
            Err(error) if error.is_end_of_input() => {
                self.note(&error);
                Ok(false)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A prompter with a script of answers that, like a pipe, reports
    /// the end once the script runs out.
    struct Pipe {
        answers: Vec<String>,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl Pipe {
        fn of(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().rev().map(|a| a.to_string()).collect(),
                asked: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl Prompter for Pipe {
        fn tell(&mut self, _message: &str) {}

        fn ask(&mut self, question: &str) -> Result<String, PromptError> {
            self.asked.lock().unwrap().push(question.to_string());
            self.answers
                .pop()
                .ok_or_else(|| PromptError::ended("the pipe ran out"))
        }

        fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
            let answer = self.ask(&format!("{question} [y/N]"))?;
            Ok(answer.eq_ignore_ascii_case("y"))
        }
    }

    fn wrapped(answers: &[&str]) -> (AbortOnEndOfInput<Pipe>, Arc<Mutex<Vec<String>>>, Sink) {
        let pipe = Pipe::of(answers);
        let asked = pipe.asked.clone();
        let sink = Sink::default();
        let prompter = AbortOnEndOfInput::explaining_to(pipe, Box::new(sink.clone()));
        (prompter, asked, sink)
    }

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn answers_that_are_there_are_passed_straight_through() {
        let (mut prompter, _, sink) = wrapped(&["a title", "", "y"]);
        assert_eq!(prompter.ask("title:").unwrap(), "a title");
        // An empty line is still an answer - the developer pressed
        // Enter and meant the default.
        assert_eq!(prompter.ask("story:").unwrap(), "");
        assert!(prompter.confirm("Stage this?").unwrap());
        assert!(!prompter.ended());
        assert_eq!(sink.text(), "");
    }

    #[test]
    fn the_end_of_the_input_declines_a_confirmation() {
        let (mut prompter, _, _) = wrapped(&[]);
        assert!(!prompter.confirm("Stage this?").unwrap());
        assert!(prompter.ended());
    }

    #[test]
    fn the_end_of_the_input_is_an_error_for_a_question_that_has_no_safe_default() {
        let (mut prompter, _, _) = wrapped(&[]);
        let error = prompter.ask("title:").unwrap_err();
        assert!(error.is_end_of_input(), "{error}");
    }

    /// The hang, at the seam: a loop that re-asks until it likes the
    /// answer used to spin forever on a spent pipe.
    #[test]
    fn nothing_is_asked_once_the_input_has_run_out() {
        let (mut prompter, asked, _) = wrapped(&["only one"]);
        assert_eq!(prompter.ask("first:").unwrap(), "only one");
        for _ in 0..5 {
            assert!(
                prompter
                    .ask("choose [r/m/a]:")
                    .unwrap_err()
                    .is_end_of_input()
            );
        }
        // One question reached the pipe before it ran out, one found
        // the end; the other four were never put to anyone.
        assert_eq!(asked.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_confirmation_after_the_end_declines_without_asking() {
        let (mut prompter, asked, _) = wrapped(&[]);
        assert!(prompter.ask("title:").unwrap_err().is_end_of_input());
        assert!(!prompter.confirm("Stage this?").unwrap());
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_reason_is_explained_once_and_not_on_every_prompt() {
        let (mut prompter, _, sink) = wrapped(&[]);
        let _ = prompter.ask("title:");
        let _ = prompter.ask("story:");
        let _ = prompter.confirm("Stage this?");
        assert_eq!(sink.text().lines().count(), 1, "{}", sink.text());
        assert!(sink.text().contains("nothing is staged"), "{}", sink.text());
    }

    /// A genuine read failure is not the end of the input, and must
    /// not be quietly turned into "no".
    #[test]
    fn a_broken_input_still_fails_rather_than_declining() {
        struct Broken;
        impl Prompter for Broken {
            fn tell(&mut self, _message: &str) {}
            fn ask(&mut self, _question: &str) -> Result<String, PromptError> {
                Err(PromptError("input is not readable - disk on fire".into()))
            }
            fn confirm(&mut self, _question: &str) -> Result<bool, PromptError> {
                self.ask("").map(|_| true)
            }
        }
        let mut prompter = AbortOnEndOfInput::new(Broken);
        assert!(prompter.confirm("Stage this?").is_err());
        assert!(!prompter.ended());
    }
}

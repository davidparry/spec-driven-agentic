//! Rustyline-backed [`Prompter`] for interactive terminals: the wizard
//! questions (drafting, greenfield) get real line editing - arrow keys
//! move the cursor anywhere in the typed text, Home/End jump, and the
//! up arrow recalls earlier answers from this session. The question
//! prints on its own line (bracketed Enter-default in green); the
//! answer is edited on a `> ` line below it, because rustyline needs a
//! plain prompt to keep its cursor arithmetic honest.

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::adapters::console_prompt::{RED, RESET, highlight_suggestion, paint_narration};
use crate::adapters::spinner::Spinner;
use crate::ports::{PromptError, Prompter, Working};

const INPUT_PROMPT: &str = "> ";

pub struct ReadlinePrompter {
    editor: DefaultEditor,
}

impl ReadlinePrompter {
    pub fn new() -> Result<Self, PromptError> {
        let editor = DefaultEditor::new()
            .map_err(|e| PromptError(format!("the terminal is not usable - {e}")))?;
        Ok(Self { editor })
    }
}

impl Prompter for ReadlinePrompter {
    fn tell(&mut self, message: &str) {
        println!("{}", paint_narration(message));
    }

    fn warn(&mut self, message: &str) {
        println!("{RED}{message}{RESET}");
    }

    /// The trailing dots animate while the guard lives - a spinner for
    /// the long model calls and test runs.
    fn working(&mut self, message: &str) -> Box<dyn Working> {
        Box::new(Spinner::start(message))
    }

    fn ask(&mut self, question: &str) -> Result<String, PromptError> {
        println!("{}", highlight_suggestion(question));
        let answer = map_answer(self.editor.readline(INPUT_PROMPT))?;
        if !answer.is_empty() {
            // Arrow-up recall within the session; nothing is persisted.
            let _ = self.editor.add_history_entry(&answer);
        }
        Ok(answer)
    }

    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        let answer = self.ask(&format!("{question} [y/N]"))?;
        Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
    }
}

/// Rustyline's endings, mapped to the port's vocabulary. Ctrl+C and
/// Ctrl+D abort the wizard - a structured error the flows report, never
/// a silently empty answer.
fn map_answer(result: Result<String, ReadlineError>) -> Result<String, PromptError> {
    match result {
        Ok(line) => Ok(line.trim().to_string()),
        Err(ReadlineError::Interrupted) => Err(PromptError(
            "input is not readable - interrupted (Ctrl+C)".into(),
        )),
        Err(ReadlineError::Eof) => Err(PromptError(
            "input is not readable - end of input (Ctrl+D)".into(),
        )),
        Err(error) => Err(PromptError(format!("input is not readable - {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_trimmed_like_the_console_prompter() {
        assert_eq!(
            map_answer(Ok("  a fine title  ".into())),
            Ok("a fine title".into())
        );
    }

    #[test]
    fn ctrl_c_aborts_with_a_structured_error() {
        assert_eq!(
            map_answer(Err(ReadlineError::Interrupted)),
            Err(PromptError(
                "input is not readable - interrupted (Ctrl+C)".into()
            ))
        );
    }

    #[test]
    fn ctrl_d_aborts_with_a_structured_error() {
        assert_eq!(
            map_answer(Err(ReadlineError::Eof)),
            Err(PromptError(
                "input is not readable - end of input (Ctrl+D)".into()
            ))
        );
    }

    #[test]
    fn any_other_failure_names_the_cause() {
        let error = ReadlineError::Io(std::io::Error::other("boom"));
        assert_eq!(
            map_answer(Err(error)),
            Err(PromptError("input is not readable - boom".into()))
        );
    }

    #[test]
    fn a_prompter_opens_on_the_test_terminal() {
        assert!(ReadlinePrompter::new().is_ok());
    }

    #[test]
    fn tell_and_warn_write_without_panicking() {
        // The narration goes to stdout (captured by the harness); the
        // interesting rendering rules live in highlight_suggestion and
        // the shared color constants, tested with the console prompter.
        let mut prompter = ReadlinePrompter::new().unwrap();
        prompter.tell("Scenarios committed to features/kata.feature");
        prompter.warn("the model call failed - Implement by hand instead.");
        // Piped (CI) runs take the told-once path; on a real terminal
        // this animates for an instant before the guard drops. Both
        // paths are tested deterministically in the spinner module.
        let guard = prompter.working("Running the tests - working");
        drop(guard);
    }
}

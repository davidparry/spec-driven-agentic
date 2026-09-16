//! Rustyline-backed [`InteractiveShell`]: line editing, arrow-key
//! history recall, and a session history persisted to `.bdd-history`
//! in the project root so the next shell picks up where this one left
//! off. Ctrl+C and Ctrl+D surface as structured session endings.

use std::path::PathBuf;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::ports::{InteractiveShell, ShellError, ShellLine};

pub struct ReadlineShell {
    editor: DefaultEditor,
    history: PathBuf,
}

impl ReadlineShell {
    /// Open a shell whose session history lives at `history`. A missing
    /// history file just means this is the first session.
    pub fn open(history: PathBuf) -> Result<Self, ShellError> {
        let mut editor = DefaultEditor::new()
            .map_err(|e| ShellError(format!("the terminal is not usable - {e}")))?;
        let _ = editor.load_history(&history);
        Ok(Self { editor, history })
    }

    /// Remember a line for arrow-key recall and the saved session.
    fn remember(&mut self, line: &str) {
        let _ = self.editor.add_history_entry(line);
    }

    #[cfg(test)]
    fn recalled(&self) -> Vec<String> {
        self.editor.history().iter().map(String::from).collect()
    }
}

impl InteractiveShell for ReadlineShell {
    fn read_line(&mut self, prompt: &str) -> Result<ShellLine, ShellError> {
        let result = self.editor.readline(prompt);
        if let Ok(line) = &result {
            self.remember(line);
        }
        map_readline(result)
    }

    fn tell(&mut self, message: &str) {
        println!("{message}");
    }

    fn save_session(&mut self) -> Result<(), ShellError> {
        self.editor
            .save_history(&self.history)
            .map_err(|e| ShellError(format!("{} is not writable - {e}", self.history.display())))
    }
}

/// Rustyline's endings, mapped to the port's vocabulary: Ctrl+C and
/// Ctrl+D end the session; anything else is a real error.
fn map_readline(result: Result<String, ReadlineError>) -> Result<ShellLine, ShellError> {
    match result {
        Ok(line) => Ok(ShellLine::Line(line)),
        Err(ReadlineError::Interrupted) => Ok(ShellLine::Interrupted),
        Err(ReadlineError::Eof) => Ok(ShellLine::End),
        Err(error) => Err(ShellError(format!("input is not readable - {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_a_line() {
        assert_eq!(
            map_readline(Ok("spec list".into())),
            Ok(ShellLine::Line("spec list".into()))
        );
    }

    #[test]
    fn ctrl_c_is_the_interrupted_ending() {
        assert_eq!(
            map_readline(Err(ReadlineError::Interrupted)),
            Ok(ShellLine::Interrupted)
        );
    }

    #[test]
    fn ctrl_d_is_the_end_of_input() {
        assert_eq!(map_readline(Err(ReadlineError::Eof)), Ok(ShellLine::End));
    }

    #[test]
    fn any_other_failure_is_a_shell_error() {
        let error = ReadlineError::Io(std::io::Error::other("boom"));
        assert_eq!(
            map_readline(Err(error)),
            Err(ShellError("input is not readable - boom".into()))
        );
    }

    #[test]
    fn the_session_history_persists_between_shells() {
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join(".bdd-history");
        let mut first = ReadlineShell::open(history.clone()).unwrap();
        first.remember("spec list");
        first.remember("state");
        first.save_session().unwrap();
        let second = ReadlineShell::open(history).unwrap();
        assert_eq!(second.recalled(), vec!["spec list", "state"]);
    }

    #[test]
    fn a_missing_history_file_is_a_fresh_session() {
        let dir = tempfile::tempdir().unwrap();
        let shell = ReadlineShell::open(dir.path().join(".bdd-history")).unwrap();
        assert!(shell.recalled().is_empty());
    }

    #[test]
    fn an_unwritable_history_path_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut shell = ReadlineShell::open(dir.path().join("no-such-dir/.bdd-history")).unwrap();
        shell.remember("spec list");
        let error = shell.save_session().unwrap_err();
        assert!(error.0.contains("is not writable -"), "error: {}", error.0);
    }
}

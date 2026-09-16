//! The interactive shell loop behind bare `bdd`: read a line, split it
//! shell-style, hand the tokens to the command dispatcher, repeat.
//! `exit`, `quit`, Ctrl+C, or Ctrl+D end the session; the history is
//! saved on the way out so the next shell can resume it. The dispatcher
//! is injected, so the loop knows nothing about clap or the services.

use crate::ports::{InteractiveShell, ShellLine};

pub const SHELL_PROMPT: &str = "bdd> ";

/// Why the session ended and how much happened - enough for the caller
/// to say goodbye accurately and for tests to pin the loop's behavior.
#[derive(Debug, PartialEq, Eq)]
pub struct ShellSummary {
    pub commands: usize,
    pub ending: Ending,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Ending {
    /// `exit` or `quit` was typed.
    Exit,
    /// Ctrl+C.
    Interrupted,
    /// Ctrl+D or the input ran out.
    EndOfInput,
    /// The shell itself failed to read input.
    Failed(String),
}

/// A brand-new project deserves a nudge: first shell session here (no
/// history file yet), a model ready to generate, and no requirements
/// spec in the root.
pub fn is_greenfield_start(first_session: bool, model_ready: bool, spec_exists: bool) -> bool {
    first_session && model_ready && !spec_exists
}

/// Offer to start the greenfield loop. `y` dispatches `greenfield`;
/// anything else declines quietly and the shell carries on.
pub fn offer_greenfield(shell: &mut dyn InteractiveShell, dispatch: &mut dyn FnMut(Vec<String>)) {
    shell.tell(
        "It appears you are in a greenfield - this project has no \
         requirements/requirements.json yet.",
    );
    match shell.read_line("Start with the greenfield command now? [y/N] ") {
        Ok(ShellLine::Line(answer)) if answer.trim().eq_ignore_ascii_case("y") => {
            dispatch(vec!["greenfield".into()]);
        }
        _ => shell.tell(
            "No problem - type greenfield any time, or spec draft to begin \
             with the spec.",
        ),
    }
}

/// Run the shell until the session ends. Every non-empty line that is
/// not `exit`/`quit` is tokenized and dispatched; a leading `bdd` token
/// is forgiven so pasted one-shot commands still work.
pub fn run_shell(
    shell: &mut dyn InteractiveShell,
    dispatch: &mut dyn FnMut(Vec<String>),
) -> ShellSummary {
    let mut commands = 0;
    let ending = loop {
        match shell.read_line(SHELL_PROMPT) {
            Err(error) => break Ending::Failed(error.0),
            Ok(ShellLine::Interrupted) => break Ending::Interrupted,
            Ok(ShellLine::End) => break Ending::EndOfInput,
            Ok(ShellLine::Line(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line == "exit" || line == "quit" {
                    break Ending::Exit;
                }
                match shell_words::split(line) {
                    Err(error) => shell.tell(&format!("unreadable input - {error}")),
                    Ok(mut tokens) => {
                        if tokens.first().map(String::as_str) == Some("bdd") {
                            tokens.remove(0);
                        }
                        if tokens.is_empty() {
                            continue;
                        }
                        commands += 1;
                        dispatch(tokens);
                    }
                }
            }
        }
    };
    if let Err(error) = shell.save_session() {
        shell.tell(&format!("the session history was not saved - {}", error.0));
    }
    ShellSummary { commands, ending }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ShellError;
    use std::collections::VecDeque;

    /// Scripted shell: reads come from the script, everything told is
    /// captured, and session saves are counted (or made to fail).
    struct FakeShell {
        script: VecDeque<Result<ShellLine, ShellError>>,
        prompts: Vec<String>,
        told: Vec<String>,
        saves: usize,
        save_fails: bool,
    }

    impl FakeShell {
        fn reading(script: Vec<Result<ShellLine, ShellError>>) -> Self {
            Self {
                script: script.into(),
                prompts: Vec::new(),
                told: Vec::new(),
                saves: 0,
                save_fails: false,
            }
        }

        fn line(text: &str) -> Result<ShellLine, ShellError> {
            Ok(ShellLine::Line(text.into()))
        }
    }

    impl InteractiveShell for FakeShell {
        fn read_line(&mut self, prompt: &str) -> Result<ShellLine, ShellError> {
            self.prompts.push(prompt.into());
            self.script.pop_front().unwrap_or(Ok(ShellLine::End))
        }

        fn tell(&mut self, message: &str) {
            self.told.push(message.into());
        }

        fn save_session(&mut self) -> Result<(), ShellError> {
            self.saves += 1;
            if self.save_fails {
                Err(ShellError(".bdd-history is not writable - boom".into()))
            } else {
                Ok(())
            }
        }
    }

    fn run(shell: &mut FakeShell) -> (ShellSummary, Vec<Vec<String>>) {
        let mut dispatched = Vec::new();
        let summary = run_shell(shell, &mut |tokens| dispatched.push(tokens));
        (summary, dispatched)
    }

    #[test]
    fn commands_run_without_the_bdd_prefix_until_exit() {
        let mut shell = FakeShell::reading(vec![
            FakeShell::line("spec list"),
            FakeShell::line("state"),
            FakeShell::line("exit"),
        ]);
        let (summary, dispatched) = run(&mut shell);
        assert_eq!(
            summary,
            ShellSummary {
                commands: 2,
                ending: Ending::Exit
            }
        );
        assert_eq!(dispatched, vec![vec!["spec", "list"], vec!["state"]]);
        assert_eq!(shell.saves, 1, "the session is saved on the way out");
        assert!(
            shell.prompts.iter().all(|p| p == SHELL_PROMPT),
            "prompts: {:?}",
            shell.prompts
        );
    }

    #[test]
    fn a_greenfield_start_needs_all_three_signs() {
        assert!(is_greenfield_start(true, true, false));
        assert!(
            !is_greenfield_start(false, true, false),
            "not the first session"
        );
        assert!(!is_greenfield_start(true, false, false), "no model ready");
        assert!(
            !is_greenfield_start(true, true, true),
            "a spec already exists"
        );
    }

    #[test]
    fn accepting_the_greenfield_offer_dispatches_the_command() {
        let mut shell = FakeShell::reading(vec![FakeShell::line("y")]);
        let mut dispatched = Vec::new();
        offer_greenfield(&mut shell, &mut |tokens| dispatched.push(tokens));
        assert_eq!(dispatched, vec![vec!["greenfield"]]);
        assert_eq!(
            shell.prompts,
            vec!["Start with the greenfield command now? [y/N] "]
        );
        assert!(
            shell
                .told
                .iter()
                .any(|m| m.contains("It appears you are in a greenfield")),
            "told: {:?}",
            shell.told
        );
    }

    #[test]
    fn a_capital_y_also_accepts_the_offer() {
        let mut shell = FakeShell::reading(vec![FakeShell::line(" Y ")]);
        let mut dispatched = Vec::new();
        offer_greenfield(&mut shell, &mut |tokens| dispatched.push(tokens));
        assert_eq!(dispatched, vec![vec!["greenfield"]]);
    }

    #[test]
    fn declining_the_offer_points_at_the_commands_instead() {
        let mut shell = FakeShell::reading(vec![FakeShell::line("n")]);
        let mut dispatched = Vec::new();
        offer_greenfield(&mut shell, &mut |tokens| dispatched.push(tokens));
        assert!(dispatched.is_empty());
        assert!(
            shell
                .told
                .iter()
                .any(|m| m.contains("type greenfield any time")),
            "told: {:?}",
            shell.told
        );
    }

    #[test]
    fn a_ctrl_c_on_the_offer_declines_quietly() {
        let mut shell = FakeShell::reading(vec![Ok(ShellLine::Interrupted)]);
        let mut dispatched = Vec::new();
        offer_greenfield(&mut shell, &mut |tokens| dispatched.push(tokens));
        assert!(dispatched.is_empty());
    }

    #[test]
    fn a_leading_bdd_token_is_forgiven() {
        let mut shell = FakeShell::reading(vec![
            FakeShell::line("bdd spec list"),
            FakeShell::line("quit"),
        ]);
        let (summary, dispatched) = run(&mut shell);
        assert_eq!(summary.ending, Ending::Exit);
        assert_eq!(dispatched, vec![vec!["spec", "list"]]);
    }

    #[test]
    fn quoted_arguments_stay_together() {
        let mut shell = FakeShell::reading(vec![
            FakeShell::line(
                r#"scenario add --feature features/calc.feature --step "Given a calculator""#,
            ),
            FakeShell::line("exit"),
        ]);
        let (_, dispatched) = run(&mut shell);
        assert_eq!(
            dispatched,
            vec![vec![
                "scenario",
                "add",
                "--feature",
                "features/calc.feature",
                "--step",
                "Given a calculator",
            ]]
        );
    }

    #[test]
    fn blank_lines_and_a_lone_bdd_are_skipped() {
        let mut shell = FakeShell::reading(vec![
            FakeShell::line(""),
            FakeShell::line("   "),
            FakeShell::line("bdd"),
            FakeShell::line("exit"),
        ]);
        let (summary, dispatched) = run(&mut shell);
        assert_eq!(summary.commands, 0);
        assert!(dispatched.is_empty());
    }

    #[test]
    fn an_unbalanced_quote_is_reported_and_the_shell_stays_open() {
        let mut shell = FakeShell::reading(vec![
            FakeShell::line(r#"feature create --name "half quoted"#),
            FakeShell::line("state"),
            FakeShell::line("exit"),
        ]);
        let (summary, dispatched) = run(&mut shell);
        assert_eq!(summary.commands, 1);
        assert_eq!(dispatched, vec![vec!["state"]]);
        assert!(
            shell
                .told
                .iter()
                .any(|m| m.starts_with("unreadable input -")),
            "told: {:?}",
            shell.told
        );
    }

    #[test]
    fn ctrl_c_ends_the_session_and_still_saves_the_history() {
        let mut shell = FakeShell::reading(vec![
            FakeShell::line("spec list"),
            Ok(ShellLine::Interrupted),
        ]);
        let (summary, _) = run(&mut shell);
        assert_eq!(summary.ending, Ending::Interrupted);
        assert_eq!(shell.saves, 1);
    }

    #[test]
    fn end_of_input_ends_the_session() {
        let mut shell = FakeShell::reading(vec![Ok(ShellLine::End)]);
        let (summary, _) = run(&mut shell);
        assert_eq!(
            summary,
            ShellSummary {
                commands: 0,
                ending: Ending::EndOfInput
            }
        );
    }

    #[test]
    fn a_read_failure_ends_the_session_with_the_reason() {
        let mut shell = FakeShell::reading(vec![Err(ShellError("the terminal vanished".into()))]);
        let (summary, _) = run(&mut shell);
        assert_eq!(
            summary.ending,
            Ending::Failed("the terminal vanished".into())
        );
    }

    #[test]
    fn a_failed_session_save_is_told_not_fatal() {
        let mut shell = FakeShell::reading(vec![FakeShell::line("exit")]);
        shell.save_fails = true;
        let (summary, _) = run(&mut shell);
        assert_eq!(summary.ending, Ending::Exit);
        assert!(
            shell
                .told
                .iter()
                .any(|m| m.contains("the session history was not saved")),
            "told: {:?}",
            shell.told
        );
    }
}

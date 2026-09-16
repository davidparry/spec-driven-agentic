//! Console implementation of the [`Prompter`] port. Generic over the
//! reader and writer so tests drive it with in-memory buffers; `main.rs`
//! instantiates it over stdin/stdout.

use std::io::{BufRead, Write};

use crate::ports::{PromptError, Prompter};

pub struct ConsolePrompter<R: BufRead, W: Write> {
    input: R,
    output: W,
}

impl<R: BufRead, W: Write> ConsolePrompter<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self { input, output }
    }

    fn read_line(&mut self) -> Result<String, PromptError> {
        let mut line = String::new();
        self.input
            .read_line(&mut line)
            .map_err(|e| PromptError(format!("input is not readable - {e}")))?;
        Ok(line.trim().to_string())
    }
}

/// ANSI colors, matching the shell banner's color handling. Shared with
/// the readline prompter so both render identically.
pub(crate) const RED: &str = "\x1b[31m";
pub(crate) const GREEN: &str = "\x1b[32m";
/// Bright yellow - the spinner's animated dots.
pub(crate) const YELLOW: &str = "\x1b[93m";
pub(crate) const RESET: &str = "\x1b[0m";

/// The criterion prompts' drop convention - typing `-` removes the
/// criterion. Destructive, so the dash and the words render red; the
/// quotes around the dash stay plain.
const DROP_HINT: &str = "'-' drops it";

fn paint_drop_hint(text: &str) -> String {
    text.replace(DROP_HINT, &format!("'{RED}-{RESET}' {RED}drops it{RESET}"))
}

/// Parenthetical hints and pick lists - `(java, javascript, ...)`,
/// `(As a ..., I want ..., so that ...)` - painted green like the
/// bracketed Enter-default. Nested parens do not appear in these
/// prompts, so the first `)` closes the span.
fn paint_parens(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open + 1..].find(')').map(|i| open + 1 + i) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..open]);
        out.push_str(GREEN);
        out.push_str(&rest[open..=close]);
        out.push_str(RESET);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Render the question's suggestions in green and the destructive
/// `'-' drops it` hint in red. Bracketed spans (`[prior answer]`,
/// `[y/N]`, `[1-3, Enter for 1]`) are the Enter-default; parenthetical
/// spans are the pick list or wording shape. Both are what the
/// developer is being asked to take.
pub(crate) fn highlight_suggestion(question: &str) -> String {
    let (Some(open), Some(close)) = (question.find('['), question.rfind(']')) else {
        return paint_drop_hint(&paint_parens(question));
    };
    if close < open {
        return paint_drop_hint(&paint_parens(question));
    }
    format!(
        "{}{GREEN}{}{RESET}{}",
        paint_parens(&question[..open]),
        &question[open..=close],
        paint_drop_hint(&paint_parens(&question[close + 1..]))
    )
}

/// Color the chosen language and numbered proposals on narration
/// lines. Green is this harness's marker for a value the developer picked
/// or is being asked to pick.
pub(crate) fn paint_narration(message: &str) -> String {
    if let Some(value) = message.strip_prefix("Project language: ") {
        let value = value.strip_suffix('.').unwrap_or(value);
        return format!("Project language: {GREEN}{value}{RESET}.");
    }
    if let Some(rest) = message.strip_prefix("Scaffolded ")
        && let Some((head, language)) = rest.split_once(" files for ")
    {
        let language = language.strip_suffix('.').unwrap_or(language);
        return format!("Scaffolded {head} files for {GREEN}{language}{RESET}.");
    }
    paint_numbered_choice(message)
}

fn paint_numbered_choice(message: &str) -> String {
    let Some(rest) = message.strip_prefix("  ") else {
        return message.to_string();
    };
    let Some((index, title)) = rest.split_once(". ") else {
        return message.to_string();
    };
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return message.to_string();
    }
    format!("  {index}. {GREEN}{title}{RESET}")
}

impl<R: BufRead, W: Write> Prompter for ConsolePrompter<R, W> {
    fn tell(&mut self, message: &str) {
        // Best effort: a broken pipe on an interactive console is fatal
        // anyway and the answer would be lost regardless.
        let _ = writeln!(self.output, "{}", paint_narration(message));
    }

    fn warn(&mut self, message: &str) {
        let _ = writeln!(self.output, "{RED}{message}{RESET}");
    }

    fn ask(&mut self, question: &str) -> Result<String, PromptError> {
        let _ = write!(self.output, "{} ", highlight_suggestion(question));
        let _ = self.output.flush();
        self.read_line()
    }

    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        let answer = self.ask(&format!("{question} [y/N]"))?;
        Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompter(input: &str) -> ConsolePrompter<std::io::Cursor<Vec<u8>>, Vec<u8>> {
        ConsolePrompter::new(std::io::Cursor::new(input.as_bytes().to_vec()), Vec::new())
    }

    #[test]
    fn ask_prints_the_question_and_returns_the_trimmed_answer() {
        let mut p = prompter("  a fine title  \n");
        assert_eq!(p.ask("Title?").unwrap(), "a fine title");
        assert_eq!(String::from_utf8(p.output).unwrap(), "Title? ");
    }

    #[test]
    fn tell_writes_a_line() {
        let mut p = prompter("");
        p.tell("finding: vague wording");
        assert_eq!(
            String::from_utf8(p.output).unwrap(),
            "finding: vague wording\n"
        );
    }

    #[test]
    fn tell_paints_the_chosen_language() {
        let mut p = prompter("");
        p.tell("Project language: Java (Cucumber-JVM).");
        assert_eq!(
            String::from_utf8(p.output).unwrap(),
            "Project language: \x1b[32mJava (Cucumber-JVM)\x1b[0m.\n"
        );
    }

    #[test]
    fn the_bracketed_enter_default_is_rendered_green() {
        let mut p = prompter("\n");
        p.ask("REQ-001 title [Convert string to number] (Enter keeps it):")
            .unwrap();
        assert_eq!(
            String::from_utf8(p.output).unwrap(),
            "REQ-001 title \x1b[32m[Convert string to number]\x1b[0m \x1b[32m(Enter keeps it)\x1b[0m: "
        );
    }

    #[test]
    fn a_suggestion_with_inner_brackets_is_colored_as_one_span() {
        assert_eq!(
            highlight_suggestion("criterion 1 [Given \"x[0]\", then 3] (Enter keeps it):"),
            "criterion 1 \x1b[32m[Given \"x[0]\", then 3]\x1b[0m \x1b[32m(Enter keeps it)\x1b[0m:"
        );
    }

    #[test]
    fn the_drop_hint_dash_and_words_are_red_but_the_quotes_stay_plain() {
        assert_eq!(
            highlight_suggestion(
                "REQ-004 criterion 2 [Given \"\", then 0] (Enter keeps it, '-' drops it):"
            ),
            "REQ-004 criterion 2 \x1b[32m[Given \"\", then 0]\x1b[0m \
             \x1b[32m(Enter keeps it, '\x1b[31m-\x1b[0m' \x1b[31mdrops it\x1b[0m)\x1b[0m:"
        );
    }

    #[test]
    fn a_drop_hint_without_a_bracketed_default_is_still_painted() {
        assert_eq!(
            highlight_suggestion("criterion 2 (Enter keeps it, '-' drops it):"),
            "criterion 2 \x1b[32m(Enter keeps it, '\x1b[31m-\x1b[0m' \x1b[31mdrops it\x1b[0m)\x1b[0m:"
        );
    }

    #[test]
    fn a_parenthetical_pick_list_is_rendered_green() {
        assert_eq!(
            highlight_suggestion(
                "Language for the new project (java, javascript, typescript, dotnet, rust):"
            ),
            "Language for the new project \x1b[32m(java, javascript, typescript, dotnet, rust)\x1b[0m:"
        );
        assert_eq!(
            highlight_suggestion("REQ-001 story (As a ..., I want ..., so that ...):"),
            "REQ-001 story \x1b[32m(As a ..., I want ..., so that ...)\x1b[0m:"
        );
        assert_eq!(
            highlight_suggestion("REQ-001 criterion 1 (leave blank to finish the criteria):"),
            "REQ-001 criterion 1 \x1b[32m(leave blank to finish the criteria)\x1b[0m:"
        );
    }

    #[test]
    fn a_story_shape_stays_green_beside_the_bracketed_enter_default() {
        assert_eq!(
            highlight_suggestion(
                "REQ-002 story (As a ..., I want ..., so that ...) [As a user, I want 0] (Enter keeps it):"
            ),
            "REQ-002 story \x1b[32m(As a ..., I want ..., so that ...)\x1b[0m \
             \x1b[32m[As a user, I want 0]\x1b[0m \x1b[32m(Enter keeps it)\x1b[0m:"
        );
    }

    #[test]
    fn questions_without_a_default_are_left_plain() {
        assert_eq!(highlight_suggestion("Project name:"), "Project name:");
        assert_eq!(
            highlight_suggestion("a stray ] before ["),
            "a stray ] before ["
        );
    }

    #[test]
    fn the_chosen_language_is_rendered_green() {
        assert_eq!(
            paint_narration("Project language: Java (Cucumber-JVM)."),
            "Project language: \x1b[32mJava (Cucumber-JVM)\x1b[0m."
        );
        assert_eq!(
            paint_narration("Scaffolded 6 files for Java (Cucumber-JVM)."),
            "Scaffolded 6 files for \x1b[32mJava (Cucumber-JVM)\x1b[0m."
        );
    }

    #[test]
    fn a_numbered_proposal_title_is_rendered_green() {
        assert_eq!(
            paint_narration("  1. Comma separated numbers are summed"),
            "  1. \x1b[32mComma separated numbers are summed\x1b[0m"
        );
        assert_eq!(
            paint_narration("  REQ-001 Comma sums"),
            "  REQ-001 Comma sums",
            "accepted-id lines are not a pick list"
        );
        assert_eq!(
            paint_narration("finding: vague wording"),
            "finding: vague wording"
        );
        assert_eq!(
            paint_narration("  x. Not a numbered pick"),
            "  x. Not a numbered pick"
        );
    }

    #[test]
    fn an_unclosed_parenthetical_is_left_plain() {
        assert_eq!(
            highlight_suggestion("Language (java, javascript"),
            "Language (java, javascript"
        );
    }

    #[test]
    fn warn_writes_the_line_in_red() {
        let mut p = prompter("");
        p.warn("the model call failed - Implement by hand instead.");
        assert_eq!(
            String::from_utf8(p.output).unwrap(),
            "\x1b[31mthe model call failed - Implement by hand instead.\x1b[0m\n"
        );
    }

    #[test]
    fn confirm_accepts_y_and_yes_in_any_case() {
        for answer in ["y\n", "Y\n", "yes\n", "YES\n"] {
            assert!(prompter(answer).confirm("Stage it?").unwrap());
        }
        for answer in ["n\n", "\n", "nope\n"] {
            assert!(!prompter(answer).confirm("Stage it?").unwrap());
        }
    }

    #[test]
    fn a_failing_reader_is_a_structured_error() {
        struct BrokenReader;
        impl std::io::Read for BrokenReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("broken"))
            }
        }
        let mut p = ConsolePrompter::new(std::io::BufReader::new(BrokenReader), Vec::new());
        let error = p.ask("Title?").unwrap_err();
        assert!(
            error.0.starts_with("input is not readable -"),
            "got: {}",
            error.0
        );
    }
}

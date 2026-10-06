//! Whether a model's replacement for a file would destroy what is there.
//!
//! Observed live: `spec implement` was asked to add one tool to a
//! 1124-line module and replied with the single word `placeholder`. It
//! was written to disk and the next `spec test`
//! reported a build failure - the whole server gone, in a step whose
//! whole point is that the test run decides. The run is the validator
//! for whether code is *right*; it never gets the chance when the reply
//! is not code at all.
//!
//! Two questions are asked of a replacement, and only where a file
//! already exists - a new file has nothing to lose:
//!
//! 1. Does it still declare every name the file declares today? A reply
//!    that drops names other files call is truncated or junk, whatever
//!    else it may be.
//! 2. Do its braces close? That is what a reply cut off mid-generation
//!    looks like when the cut lands inside the last body and so takes no
//!    declaration with it.
//!
//! Neither asks whether the code is any good. Both describe damage that
//! no correct attempt has ever done.

use super::language::Language;
use super::neighborhood::declared_symbols;

/// How many dropped names to name before the message stops being one.
const NAMED_LOSSES: usize = 5;

/// Why a replacement may not be written over the file it replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// Names the file declares today that the replacement does not, in
    /// the order the file introduces them.
    Drops(Vec<String>),
    /// Braces that never close.
    Truncated,
}

impl Damage {
    /// What to tell someone who has not seen the diff, and what to do.
    pub fn describe(&self, path: &str) -> String {
        match self {
            Self::Drops(names) => format!(
                "The reply for {path} was not written: it drops {}, which {path} \
                 declares today. A reply that deletes what it was not asked to \
                 touch is a bad reply, not a refactor. Run spec implement again.",
                naming(names),
            ),
            Self::Truncated => format!(
                "The reply for {path} was not written: its braces never close, which \
                 is what a reply cut off part-way through looks like. Run spec \
                 implement again, or raise timeout_seconds under [llm] if the model \
                 is running out of time."
            ),
        }
    }
}

/// What replacing `before` with `after` would destroy, if anything.
///
/// `None` is not approval - it only means this reply does not do one of
/// the two things that are never right.
pub fn damage(language: Language, before: &str, after: &str) -> Option<Damage> {
    let kept = declared_symbols(language, after);
    let dropped: Vec<String> = declared_symbols(language, before)
        .into_iter()
        .filter(|name| !kept.contains(name))
        .collect();
    if !dropped.is_empty() {
        return Some(Damage::Drops(dropped));
    }
    // The file on disk is part of a project that builds, so its braces
    // close. When the scanner disagrees it cannot read this file's
    // syntax - a raw string or a regex literal it took for a quote - and
    // it has nothing worth saying about the replacement either.
    (brace_balance(before) == 0 && brace_balance(after) != 0).then_some(Damage::Truncated)
}

fn naming(names: &[String]) -> String {
    let shown = names
        .iter()
        .take(NAMED_LOSSES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().checked_sub(NAMED_LOSSES) {
        Some(rest) if rest > 0 => format!("{shown} and {rest} more"),
        _ => shown,
    }
}

/// How far `source`'s braces are from closing, with comments and string
/// literals skipped.
///
/// Not a parser, and it does not need to be: [`damage`] only trusts it
/// about a replacement once it has shown it can read the original.
fn brace_balance(source: &str) -> i32 {
    let chars: Vec<char> = source.chars().collect();
    let mut depth = 0;
    let mut at = 0;
    while at < chars.len() {
        at = match chars[at] {
            '/' if follows(&chars, at, '/') => past_line_comment(&chars, at),
            '/' if follows(&chars, at, '*') => past_block_comment(&chars, at),
            'r' if starts_raw_string(&chars, at) => past_raw_string(&chars, at),
            quote @ ('"' | '`') => past_quoted(&chars, at, quote),
            '\'' => past_single_quote(&chars, at),
            '{' => {
                depth += 1;
                at + 1
            }
            '}' => {
                depth -= 1;
                at + 1
            }
            _ => at + 1,
        };
    }
    depth
}

fn follows(chars: &[char], at: usize, next: char) -> bool {
    chars.get(at + 1) == Some(&next)
}

fn past_line_comment(chars: &[char], at: usize) -> usize {
    chars[at..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(chars.len(), |end| at + end)
}

/// Rust nests block comments, and the other four tolerate the extra
/// bookkeeping for an unnested one.
fn past_block_comment(chars: &[char], at: usize) -> usize {
    let mut depth = 1;
    let mut at = at + 2;
    while at < chars.len() && depth > 0 {
        if chars[at] == '/' && follows(chars, at, '*') {
            depth += 1;
            at += 2;
        } else if chars[at] == '*' && follows(chars, at, '/') {
            depth -= 1;
            at += 2;
        } else {
            at += 1;
        }
    }
    at
}

/// `r"..."` and `r#"..."#`, but not the `r` of `for` or a name.
fn starts_raw_string(chars: &[char], at: usize) -> bool {
    if at > 0 && (chars[at - 1].is_alphanumeric() || chars[at - 1] == '_') {
        return false;
    }
    let hashes = chars[at + 1..].iter().take_while(|c| **c == '#').count();
    chars.get(at + 1 + hashes) == Some(&'"')
}

fn past_raw_string(chars: &[char], at: usize) -> usize {
    let hashes = chars[at + 1..].iter().take_while(|c| **c == '#').count();
    let mut at = at + hashes + 2;
    while at < chars.len() {
        if chars[at] == '"' && chars[at + 1..].iter().take_while(|c| **c == '#').count() >= hashes {
            return at + hashes + 1;
        }
        at += 1;
    }
    at
}

fn past_quoted(chars: &[char], at: usize, quote: char) -> usize {
    let mut at = at + 1;
    while at < chars.len() {
        match chars[at] {
            '\\' => at += 2,
            c if c == quote => return at + 1,
            _ => at += 1,
        }
    }
    at
}

/// A single quote is a string in JavaScript, a character literal in the
/// other four, and a lifetime in Rust. A lifetime has no partner on its
/// line, which is the cheapest way to tell them apart - and the text
/// between two lifetimes on one line holds no brace to miscount.
fn past_single_quote(chars: &[char], at: usize) -> usize {
    let mut scan = at + 1;
    while scan < chars.len() && chars[scan] != '\n' {
        match chars[scan] {
            '\\' => scan += 2,
            '\'' => return scan + 1,
            _ => scan += 1,
        }
    }
    at + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reply that started this: a module came back as one word.
    #[test]
    fn a_reply_that_throws_the_file_away_is_damage() {
        let before = "pub struct WorkflowServer {}\npub fn advertise() {}\n";
        assert_eq!(
            damage(Language::Rust, before, "placeholder"),
            Some(Damage::Drops(vec![
                "WorkflowServer".to_string(),
                "advertise".to_string(),
            ]))
        );
    }

    #[test]
    fn a_reply_that_adds_behaviour_and_keeps_the_rest_is_allowed() {
        let before = "pub struct WorkflowServer {}\npub fn advertise() {}\n";
        let after = "pub struct WorkflowServer {}\npub fn advertise() {}\npub fn coverage() {}\n";
        assert_eq!(damage(Language::Rust, before, after), None);
    }

    /// Renaming is how a refactor looks, and it is not this guard's
    /// call to make - but it is indistinguishable from deletion from
    /// here, and the file it would delete is the one being implemented.
    #[test]
    fn a_reply_that_renames_what_the_file_declares_is_damage() {
        let before = "pub fn advertise() {}\n";
        let after = "pub fn advertise_tools() {}\n";
        assert_eq!(
            damage(Language::Rust, before, after),
            Some(Damage::Drops(vec!["advertise".to_string()]))
        );
    }

    #[test]
    fn a_file_that_declared_nothing_can_be_replaced_freely() {
        assert_eq!(damage(Language::Rust, "// notes\n", "placeholder"), None);
    }

    #[test]
    fn every_language_is_read_for_what_it_declares() {
        assert_eq!(
            damage(Language::Java, "public class Kata {}", "placeholder"),
            Some(Damage::Drops(vec!["Kata".to_string()]))
        );
        assert_eq!(
            damage(Language::DotNet, "internal class Kata {}", "placeholder"),
            Some(Damage::Drops(vec!["Kata".to_string()]))
        );
        assert_eq!(
            damage(
                Language::TypeScript,
                "export function addNumbers() {}",
                "placeholder"
            ),
            Some(Damage::Drops(vec!["addNumbers".to_string()]))
        );
    }

    #[test]
    fn a_reply_cut_off_mid_body_is_damage() {
        let before = "pub fn advertise() {\n    ok();\n}\n";
        let after = "pub fn advertise() {\n    ok();\n    let x = compute(";
        assert_eq!(
            damage(Language::Rust, before, after),
            Some(Damage::Truncated)
        );
    }

    /// Truncation that takes a declaration with it is reported as the
    /// loss, which says more than "braces".
    #[test]
    fn truncation_that_loses_a_name_is_reported_as_the_loss() {
        let before = "pub fn one() {}\npub fn two() {}\n";
        let after = "pub fn one() {\n";
        assert_eq!(
            damage(Language::Rust, before, after),
            Some(Damage::Drops(vec!["two".to_string()]))
        );
    }

    #[test]
    fn braces_inside_comments_and_strings_are_not_structure() {
        let source = "fn f() {\n  // }\n  /* } */\n  let s = \"}\";\n  let c = '}';\n}\n";
        assert_eq!(brace_balance(source), 0);
    }

    #[test]
    fn a_raw_string_full_of_braces_is_still_one_token() {
        let source = "fn f() {\n  let re = r#\"^\\{(?<x>[a-z]+)\\}$\"#;\n}\n";
        assert_eq!(brace_balance(source), 0);
    }

    #[test]
    fn a_lifetime_is_not_an_unterminated_literal() {
        let source = "fn render<'a>(items: &'a [T]) -> Cow<'a, str> {\n    go()\n}\n";
        assert_eq!(brace_balance(source), 0);
    }

    #[test]
    fn a_nested_block_comment_closes_once() {
        assert_eq!(brace_balance("fn f() { /* a /* b */ */ }"), 0);
    }

    #[test]
    fn a_template_literal_hides_its_braces() {
        assert_eq!(brace_balance("function f() {\n  return `a}b`;\n}\n"), 0);
    }

    /// When the scanner cannot read the original it has nothing to say
    /// about the replacement - whatever it misreads in one it misreads
    /// in the other, and a false alarm here throws away real work.
    #[test]
    fn a_file_the_scanner_cannot_read_is_left_alone() {
        // An unterminated-looking quote the scanner mis-handles: the
        // original does not balance by its count, so it stays quiet.
        let before = "fn f() { let s = \"unclosed;\n";
        let after = "fn f() { let s = \"unclosed;\n  more();\n";
        assert_eq!(damage(Language::Rust, before, after), None);
    }

    #[test]
    fn the_message_names_what_would_be_lost_and_what_to_do() {
        let told = Damage::Drops(vec!["WorkflowServer".to_string()]).describe("src/mcp.rs");
        assert!(told.contains("src/mcp.rs"), "{told}");
        assert!(told.contains("WorkflowServer"), "{told}");
        assert!(told.contains("not written"), "{told}");
        assert!(told.contains("spec implement"), "{told}");
    }

    #[test]
    fn a_long_list_of_losses_is_summarised() {
        let names: Vec<String> = (0..9).map(|i| format!("thing{i}")).collect();
        let told = Damage::Drops(names).describe("src/mcp.rs");
        assert!(
            told.contains("thing0, thing1, thing2, thing3, thing4"),
            "{told}"
        );
        assert!(told.contains("and 4 more"), "{told}");
    }

    #[test]
    fn the_truncation_message_points_at_the_timeout() {
        let told = Damage::Truncated.describe("src/mcp.rs");
        assert!(told.contains("braces never close"), "{told}");
        assert!(told.contains("timeout_seconds"), "{told}");
    }
}

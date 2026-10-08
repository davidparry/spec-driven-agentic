//! The pure half of reading a diff: which path may be asked for, and
//! how much of git's answer one reply is willing to carry.

use crate::domain::paths::{PathJail, confine};

/// How much diff text one reply carries before it is cut.
///
/// A diff goes somewhere with a context window, and `spec diff` with no
/// path on a long-running branch can run to megabytes. The cut is
/// announced rather than silent, and the recovery is a narrower path.
pub const MAX_DIFF_BYTES: usize = 200_000;

/// How many untracked files one reply names. A project with no
/// `.gitignore` can have tens of thousands of them, and the point of
/// the list is to notice new work, not to inventory a build directory.
pub const MAX_UNTRACKED: usize = 50;

/// The pathspec a diff may be narrowed to.
///
/// `None` stays `None` — the whole project is a legitimate request.
/// Anything else goes through the same jail every write uses, so a
/// pathspec can never reach outside the project root.
pub fn requested_path(path: Option<&str>) -> Result<Option<String>, PathJail> {
    path.map(confine).transpose()
}

/// Diff text cut to a budget, and whether cutting happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capped {
    pub text: String,
    pub truncated: bool,
}

/// Cut `diff` to at most `limit` bytes, on a line boundary.
///
/// Mid-line is the one place a cut must not land: half of `-    return
/// a + b;` reads as a change that was never made. The head is kept
/// rather than the tail because a unified diff names its files there.
pub fn cap(diff: &str, limit: usize) -> Capped {
    if diff.len() <= limit {
        return Capped {
            text: diff.to_string(),
            truncated: false,
        };
    }
    let mut cut = 0;
    for (index, _) in diff.match_indices('\n') {
        if index + 1 > limit {
            break;
        }
        cut = index + 1;
    }
    // One line longer than the whole budget - a minified file, or a
    // diff of something that is not really text. There is no line
    // boundary to use, so fall back to the last char boundary: cutting
    // mid-codepoint would not be a string at all.
    if cut == 0 {
        cut = (0..=limit)
            .rev()
            .find(|index| diff.is_char_boundary(*index))
            .unwrap_or(0);
    }
    Capped {
        text: diff[..cut].to_string(),
        truncated: true,
    }
}

/// The first [`MAX_UNTRACKED`] names, and how many were dropped.
pub fn cap_untracked(untracked: Vec<String>) -> (Vec<String>, usize) {
    let dropped = untracked.len().saturating_sub(MAX_UNTRACKED);
    let mut kept = untracked;
    kept.truncate(MAX_UNTRACKED);
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_project_is_a_pathspec_of_its_own() {
        assert_eq!(requested_path(None).unwrap(), None);
    }

    #[test]
    fn a_relative_path_normalizes() {
        assert_eq!(
            requested_path(Some("./requirements")).unwrap(),
            Some("requirements".to_string())
        );
    }

    #[test]
    fn a_path_reaching_outside_the_root_is_refused() {
        for path in ["../etc", "/etc/passwd", "~/secrets", "C:\\Windows"] {
            assert!(requested_path(Some(path)).is_err(), "allowed: {path}");
        }
    }

    #[test]
    fn a_diff_inside_the_budget_is_carried_whole() {
        let diff = "diff --git a/a b/a\n+one\n";
        let capped = cap(diff, MAX_DIFF_BYTES);
        assert_eq!(capped.text, diff);
        assert!(!capped.truncated);
    }

    /// Half a hunk line reads as a change nobody made, so the cut lands
    /// on a newline and the text it keeps is whole lines only.
    #[test]
    fn an_oversized_diff_is_cut_on_a_line_boundary() {
        let diff = "aaaa\nbbbb\ncccc\ndddd\n";
        let capped = cap(diff, 12);
        assert_eq!(capped.text, "aaaa\nbbbb\n");
        assert!(capped.truncated);
    }

    /// A minified file is one line longer than any budget. Keeping
    /// nothing would be worse than keeping a prefix of it.
    #[test]
    fn a_single_line_longer_than_the_budget_is_still_cut() {
        let diff = "+".repeat(100);
        let capped = cap(&diff, 10);
        assert_eq!(capped.text.len(), 10);
        assert!(capped.truncated);
    }

    /// Cutting mid-codepoint would not produce a string at all.
    #[test]
    fn a_cut_never_lands_inside_a_character() {
        let diff = "+".to_string() + &"é".repeat(50);
        let capped = cap(&diff, 10);
        assert!(capped.truncated);
        assert!(diff.starts_with(&capped.text));
    }

    #[test]
    fn an_untracked_list_is_capped_and_says_how_many_it_dropped() {
        let names: Vec<String> = (0..MAX_UNTRACKED + 7).map(|i| format!("f{i}")).collect();
        let (kept, dropped) = cap_untracked(names);
        assert_eq!(kept.len(), MAX_UNTRACKED);
        assert_eq!(dropped, 7);

        let (kept, dropped) = cap_untracked(vec!["one".to_string()]);
        assert_eq!(kept, vec!["one".to_string()]);
        assert_eq!(dropped, 0);
    }
}

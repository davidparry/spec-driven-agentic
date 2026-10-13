//! How a reply reads when a person is the one reading it.
//!
//! The CLI answers two audiences with one reply. Something parsing the
//! output needs the JSON shape it has always had; someone watching a
//! terminal needs to find the answer without reading punctuation. The
//! rendering lives here, pure and testable, and the choice of which to
//! print is made at the edge in `main.rs`.

/// A reply in the words a person reads.
///
/// A trait of our own rather than [`std::fmt::Display`]: `spec list`
/// replies with a `Vec<ListedRequirement>`, and the orphan rule forbids
/// implementing a foreign trait for a foreign container.
pub trait Human {
    /// The reply itself, without the advice that closes it.
    fn human(&self) -> String;

    /// The advice, worded as the service wrote it.
    ///
    /// Handed back rather than rendered, because the CLI rewrites tool
    /// names into commands at the one place it prints, and a reply that
    /// rendered its own advice would escape that.
    fn next_step(&self) -> Option<&str> {
        None
    }
}

/// How far apart two columns sit.
const GAP: &str = "  ";

/// Rows laid out so their columns line up.
///
/// Lining up is the whole readability win for a list: a status column
/// that starts in a different place on every row has to be read, where
/// one that starts in the same place can be scanned. The last cell is
/// never padded, so no line carries trailing whitespace.
pub fn columns(rows: &[Vec<String>]) -> String {
    let widths = widths(rows);
    rows.iter()
        .map(|row| padded(row, &widths))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The widest cell in each column, counted in characters.
///
/// Characters rather than bytes: a title with an accent in it is no
/// wider on screen for being wider in memory.
fn widths(rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            let width = cell.chars().count();
            match widths.get_mut(column) {
                Some(widest) => *widest = (*widest).max(width),
                None => widths.push(width),
            }
        }
    }
    widths
}

/// One row, every cell but the last padded to its column.
fn padded(row: &[String], widths: &[usize]) -> String {
    let last = row.len().saturating_sub(1);
    row.iter()
        .enumerate()
        .map(|(column, cell)| {
            if column == last {
                return cell.clone();
            }
            let pad = widths[column].saturating_sub(cell.chars().count());
            format!("{cell}{}", " ".repeat(pad))
        })
        .collect::<Vec<_>>()
        .join(GAP)
}

/// Items as an indented bulleted list.
pub fn bullets<S: AsRef<str>>(items: &[S]) -> String {
    items
        .iter()
        .map(|item| format!("  - {}", item.as_ref()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `1 issue`, `2 issues` — a count and its noun agreeing.
pub fn counted(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// A block moved in under the heading above it.
///
/// The same two spaces [`bullets`] uses, so an indented table and an
/// indented list sit at the same depth.
pub fn indent(body: &str) -> String {
    body.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A heading glued to the body beneath it, or nothing at all when the
/// body is empty.
///
/// The empty case is the reason this exists: a `Changed` heading over
/// no files reads as a bug in the heading.
pub fn titled(heading: &str, body: &str) -> String {
    if body.is_empty() {
        return String::new();
    }
    format!("{heading}\n{body}")
}

/// The sections of a reply, blank-line separated, skipping any that
/// had nothing to say.
///
/// Here so no rendering has to track whether it already emitted
/// something before deciding to write a separator.
pub fn sections<S: AsRef<str>>(parts: &[S]) -> String {
    parts
        .iter()
        .map(|part| part.as_ref())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply that is all answer and no advice, which is what the
    /// default on the trait is for.
    struct Bare;

    impl Human for Bare {
        fn human(&self) -> String {
            "the answer".to_string()
        }
    }

    /// Not every reply closes with advice - `spec show` and `spec
    /// list` are answers to a question, not steps in a loop - so the
    /// trait defaults to having none rather than making each of them
    /// say so.
    #[test]
    fn a_reply_with_no_advice_does_not_have_to_say_so() {
        assert_eq!(Bare.human(), "the answer");
        assert_eq!(Bare.next_step(), None);
    }

    fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect()
    }

    /// The point of the helper: a column starts in the same place on
    /// every row, so it can be scanned instead of read.
    #[test]
    fn columns_line_up_under_the_widest_cell() {
        let laid_out = columns(&rows(&[
            &["REQ-1", "pending", "Add two numbers"],
            &["HARNESS-012", "implemented", "Serve the spec"],
        ]));
        assert_eq!(
            laid_out,
            "REQ-1        pending      Add two numbers\n\
             HARNESS-012  implemented  Serve the spec"
        );
    }

    /// Trailing whitespace is invisible and still ends up in diffs and
    /// copied text, so the last cell is never padded.
    #[test]
    fn the_last_cell_is_never_padded() {
        let laid_out = columns(&rows(&[&["a", "long value"], &["bbbb", "x"]]));
        for line in laid_out.lines() {
            assert_eq!(line.trim_end(), line, "trailing space in: {line:?}");
        }
    }

    /// A short row must not index past the widths it helped compute.
    #[test]
    fn rows_of_different_lengths_still_lay_out() {
        let laid_out = columns(&rows(&[&["a", "b", "c"], &["dd"]]));
        assert_eq!(laid_out, "a   b  c\ndd");
    }

    #[test]
    fn nothing_to_lay_out_renders_as_nothing() {
        assert_eq!(columns(&[]), "");
    }

    /// Width is what the reader sees, not what the string weighs.
    #[test]
    fn an_accented_title_is_measured_in_characters() {
        let laid_out = columns(&rows(&[&["éé", "x"], &["ab", "y"]]));
        assert_eq!(laid_out, "éé  x\nab  y");
    }

    #[test]
    fn bullets_indent_each_item() {
        assert_eq!(bullets(&["one", "two"]), "  - one\n  - two");
        assert_eq!(bullets::<&str>(&[]), "");
    }

    #[test]
    fn a_count_agrees_with_its_noun() {
        assert_eq!(counted(0, "issue", "issues"), "0 issues");
        assert_eq!(counted(1, "issue", "issues"), "1 issue");
        assert_eq!(counted(2, "issue", "issues"), "2 issues");
    }

    #[test]
    fn an_indented_block_sits_where_a_bulleted_one_does() {
        assert_eq!(indent("a\nb"), "  a\n  b");
        assert_eq!(indent(""), "");
    }

    /// A heading over nothing reads as a bug in the heading.
    #[test]
    fn a_heading_over_an_empty_body_is_dropped_whole() {
        assert_eq!(titled("Changed", "  - a.rs"), "Changed\n  - a.rs");
        assert_eq!(titled("Changed", ""), "");
    }

    #[test]
    fn empty_sections_leave_no_gap_behind_them() {
        assert_eq!(sections(&["head", "", "tail"]), "head\n\ntail");
        assert_eq!(sections(&["only"]), "only");
        assert_eq!(sections::<&str>(&[]), "");
    }
}

//! Whether a chunk of code already covers an acceptance criterion.
//!
//! A deliberately literal heuristic, ported from the workshop's own
//! grader (`scripts/verify-workshop-run.sh`): code covers a criterion
//! when it feeds in the same quoted input literals and names the same
//! expected number. Everything else — scenario names, method names,
//! assertion style — is the author's.
//!
//! This is evidence for a warning, never a gate. A criterion worded
//! without literals is invisible to it, and a literal two requirements
//! share can make them look alike. `spec implement` reports what it
//! noticed and leaves the judgment with the developer.
//!
//! Pure: strings in, verdict out.

use std::sync::LazyLock;

use regex::Regex;

/// Does `code` feed in every input literal of `criterion` and land on
/// its expected value?
///
/// Unlike the shell grader, a criterion naming neither a literal nor a
/// number is *not* covered: with nothing to match on, "covered" would
/// be true of any code at all.
pub fn covers(code: &str, criterion: &str) -> bool {
    let literals = input_literals(criterion);
    let expected = expected_number(criterion);
    if literals.is_empty() && expected.is_none() {
        return false;
    }
    if !literals
        .iter()
        .all(|literal| code.contains(&format!("\"{literal}\"")))
    {
        return false;
    }
    match expected {
        None => true,
        Some(value) => numbers_in(code).any(|found| found == value),
    }
}

/// Does `code` cover every criterion of a requirement? An empty list is
/// not coverage — a requirement with nothing to satisfy is not evidence
/// that anything satisfied it.
pub fn covers_all(code: &str, criteria: &[String]) -> bool {
    !criteria.is_empty() && criteria.iter().all(|criterion| covers(code, criterion))
}

/// Every double-quoted literal the criterion names — the inputs the
/// behavior is exercised with.
fn input_literals(criterion: &str) -> Vec<&str> {
    static QUOTED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""([^"]*)""#).expect("valid regex"));
    QUOTED
        .captures_iter(criterion)
        .filter_map(|capture| capture.get(1))
        .map(|literal| literal.as_str())
        .collect()
}

/// The value the criterion expects: the last number after its final
/// `then`, which is where Given/When/Then puts the outcome.
fn expected_number(criterion: &str) -> Option<&str> {
    static THEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bthen\b").expect("valid"));
    let last_then = THEN.find_iter(criterion).last()?;
    numbers_in(&criterion[last_then.end()..]).last()
}

fn numbers_in(text: &str) -> impl Iterator<Item = &str> {
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-?\d+").expect("valid"));
    NUMBER.find_iter(text).map(|found| found.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADD_TWO: &str = "Given the input \"1,2\", when add is called, then the result is 3";

    #[test]
    fn code_feeding_the_literal_and_naming_the_value_covers_the_criterion() {
        assert!(covers(
            "@Test void adds() { assertEquals(3, calc.add(\"1,2\")); }",
            ADD_TWO
        ));
        // The same criterion as a Gherkin scenario.
        assert!(covers(
            "Given the input \"1,2\"\nWhen add is called\nThen the result is 3",
            ADD_TWO
        ));
    }

    #[test]
    fn a_different_input_or_a_different_value_is_not_coverage() {
        assert!(!covers("assertEquals(3, calc.add(\"4,-1\"));", ADD_TWO));
        assert!(!covers("assertEquals(7, calc.add(\"1,2\"));", ADD_TWO));
    }

    #[test]
    fn the_expected_value_is_not_satisfied_by_a_longer_number() {
        // The bug a bare `contains` would have: "13" holding "3".
        assert!(!covers("assertEquals(13, calc.add(\"1,2\"));", ADD_TWO));
        assert!(!covers("calc.add(\"1,2\"); // line 23", ADD_TWO));
        // Nor by the negative of it.
        assert!(!covers("assertEquals(-3, calc.add(\"1,2\"));", ADD_TWO));
    }

    #[test]
    fn a_negative_outcome_is_matched_with_its_sign() {
        let criterion = "Given \"-1,-2\", when add is called, then the result is -3";
        assert!(covers("assertEquals(-3, calc.add(\"-1,-2\"));", criterion));
        assert!(!covers("assertEquals(3, calc.add(\"-1,-2\"));", criterion));
    }

    #[test]
    fn the_outcome_is_read_after_the_last_then() {
        // "then" appears twice; the number belongs to the second.
        let criterion = "Given \"2\", when the total is 1 then doubled, then the result is 4";
        assert!(covers("calc.doubled(\"2\") == 4", criterion));
        assert!(!covers("calc.doubled(\"2\") == 1", criterion));
    }

    #[test]
    fn an_empty_literal_is_still_a_literal_to_match() {
        let criterion = "Given an empty string \"\", when add is called, then the result is 0";
        assert!(covers("assertEquals(0, calc.add(\"\"));", criterion));
        // 0 alone is not enough: the empty-string input is the point.
        assert!(!covers("assertEquals(0, calc.add(SOMETHING));", criterion));
    }

    #[test]
    fn a_criterion_with_no_literal_and_no_number_is_never_covered() {
        // Nothing to match on, so "covered" would be true of any code -
        // a warning nobody could act on.
        let vague = "Given a calculator, when nothing is entered, then an error is raised";
        assert!(!covers("throw new IllegalArgumentException();", vague));
        assert!(!covers("", vague));
    }

    #[test]
    fn a_requirement_is_covered_only_when_every_criterion_is() {
        let criteria = vec![
            ADD_TWO.to_string(),
            "Given \"1,2,3\", when add is called, then the result is 6".to_string(),
        ];
        let both = "calc.add(\"1,2\") == 3; calc.add(\"1,2,3\") == 6;";
        assert!(covers_all(both, &criteria));
        assert!(!covers_all("calc.add(\"1,2\") == 3;", &criteria));
        // A requirement with no criteria is not evidence of anything.
        assert!(!covers_all(both, &[]));
    }
}

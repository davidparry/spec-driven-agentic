//! Which failures on a red bar belong to the requirement being delivered.
//!
//! `spec deliver` reads the bar once after authoring and then spends its
//! attempts turning it green. That only works when the red is this
//! requirement's own: its unit test's placeholders, its scenarios'
//! pending steps. A suite that was already failing for some other reason
//! (a test about something else, a drifted assertion in a file the
//! requirement never touches) cannot be turned green by implementing
//! this requirement, so every attempt is spent proving that. Measured on
//! this crate: one unrelated failing test, three implementation attempts
//! at several minutes each, and the bar exactly where it started.
//!
//! So the failures are attributed first. A failure that names nothing of
//! the requirement - not its id, not its title, not a line of its
//! acceptance criteria, not a file it owns - is *foreign*, and a bar with
//! a foreign failure on it is reported rather than worked.

use super::model::Requirement;
use super::steps::criterion_to_steps;

/// A marker shorter than this, once normalized, matches by accident:
/// `add`, `sum`, `then`.
const MIN_MARKER_LEN: usize = 4;

/// The failures nothing about `requirement` accounts for, in the order
/// they were reported.
///
/// Matching is lenient on purpose, and lenient in the safe direction. A
/// failure wrongly read as the requirement's costs what the loop costs
/// today; a failure wrongly read as foreign stops a run that could have
/// gone green. So every spelling the runners use is accepted at once:
/// `REQ-001`, `Req001Test`, `req_001_test.rs`, and a scenario or unit
/// test named after the title or a criterion, however the language
/// cased it. Punctuation and whitespace are dropped from both sides
/// before comparing.
pub fn foreign_failures<'a>(failures: &'a [String], requirement: &Requirement) -> Vec<&'a str> {
    let markers = markers_of(requirement);
    failures
        .iter()
        .map(String::as_str)
        .filter(|failure| {
            let text = normalize(failure);
            !markers.iter().any(|marker| text.contains(marker.as_str()))
        })
        .collect()
}

/// Everything a failure might be named by when it is this
/// requirement's, normalized the same way the failure text is.
fn markers_of(requirement: &Requirement) -> Vec<String> {
    let mut raw: Vec<String> = vec![requirement.id.clone(), requirement.title.clone()];
    for criterion in &requirement.acceptance_criteria {
        raw.push(criterion.clone());
        if let Some(steps) = criterion_to_steps(criterion) {
            raw.extend(steps);
        }
    }
    raw.extend(
        requirement
            .feature_file
            .iter()
            .filter_map(|path| stem(path)),
    );
    raw.extend(
        requirement
            .production_files
            .iter()
            .filter_map(|path| stem(path)),
    );
    raw.iter()
        .map(|marker| normalize(marker))
        .filter(|marker| marker.len() >= MIN_MARKER_LEN)
        .collect()
}

/// The file name without its extension: the part a test class, a
/// module, or a stack frame would carry.
fn stem(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
}

/// Lowercase ASCII letters and digits only, so `Req001Test`,
/// `req_001_test` and `REQ-001` are one string.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement() -> Requirement {
        Requirement {
            id: "REQ-002".into(),
            title: "Newlines separate numbers".into(),
            status: "pending".into(),
            story: "As a user, I want newlines to count so that pasted lists add.".into(),
            acceptance_criteria: vec![
                "Given the input \"1\\n2,3\", when add is called, then the result is 6".into(),
            ],
            feature_file: Some("features/newlines.feature".into()),
            production_files: vec!["src/main/java/StringCalculator.java".into()],
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// The failures a delivery is supposed to be red on, in every
    /// shape the runners report them.
    #[test]
    fn the_requirements_own_failures_are_not_foreign() {
        let failures = strings(&[
            // Maven: the generated unit test, class and method.
            "Req002Test.givenTheInput: TODO: assert\n\tat Req002Test.givenTheInput(Req002Test.java:12)",
            // Cargo: the generated module and test function.
            "req_002_newlines_separate_numbers::given_the_input_when_add_is_called: FAILED",
            // cucumber-rs: the failing step line, no scenario name.
            "Given the input \"1\\n2,3\"",
            // Cucumber-JVM / cucumber-js: the scenario named after the title.
            "Calculator > Newlines separate numbers case 1: step \"Then the result is 6\" is undefined",
            // A stack frame in the production file the requirement declares.
            "NullPointerException\n\tat StringCalculator.add(StringCalculator.java:9)",
            // The feature file itself.
            "features/newlines.feature:7 failed",
        ]);
        assert!(
            foreign_failures(&failures, &requirement()).is_empty(),
            "{:?}",
            foreign_failures(&failures, &requirement())
        );
    }

    /// A failure about something else entirely is what the loop cannot
    /// fix, and it comes back verbatim so the stop can name it.
    #[test]
    fn a_failure_naming_nothing_of_the_requirement_is_foreign() {
        let failures = strings(&[
            "Req002Test.givenTheInput: TODO: assert",
            "every_documented_version_floor_is_the_version_this_crate_ships: FAILED",
            "Req001Test.emptyString: expected 0 but was 1",
        ]);
        assert_eq!(
            foreign_failures(&failures, &requirement()),
            vec![
                "every_documented_version_floor_is_the_version_this_crate_ships: FAILED",
                "Req001Test.emptyString: expected 0 but was 1",
            ]
        );
    }

    /// Case and punctuation differ by language and runner; none of it
    /// decides ownership.
    #[test]
    fn spelling_differences_do_not_make_a_failure_foreign() {
        let failures = strings(&["REQ_002 :: something", "req-002-test failed", "Req002"]);
        assert!(foreign_failures(&failures, &requirement()).is_empty());
    }

    /// A short title would match almost anything, so it is not a
    /// marker at all - the id and the criteria still are.
    #[test]
    fn a_short_title_is_not_a_marker() {
        let mut short = requirement();
        short.title = "Add".into();
        let failures = strings(&["Adder.total: expected 1"]);
        assert_eq!(
            foreign_failures(&failures, &short),
            vec!["Adder.total: expected 1"]
        );
    }

    #[test]
    fn a_green_bar_has_nothing_foreign_on_it() {
        assert!(foreign_failures(&[], &requirement()).is_empty());
    }
}

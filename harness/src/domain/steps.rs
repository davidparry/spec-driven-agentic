//! Step-definition discovery: extract the patterns a project's step
//! definition sources declare (per framework), match them against the
//! steps its feature files use, and report what is missing. Pure logic —
//! file access stays behind ports.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::domain::feature::FeatureDoc;
use crate::domain::language::Language;

/// One feature step with no matching step definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingStep {
    pub feature: String,
    pub scenario: String,
    /// The Gherkin keyword the definition needs (And/But resolved to the
    /// preceding real keyword).
    pub keyword: String,
    /// The step text without its keyword.
    pub text: String,
}

static JAVA_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@(?:Given|When|Then|And|But)\s*\(\s*"((?:[^"\\]|\\.)*)""#).expect("valid regex")
});
static JS_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(?:Given|When|Then)\s*\(\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)")"#)
        .expect("valid regex")
});
static CSHARP_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\[(?:Given|When|Then|StepDefinition)\s*\(\s*@?"((?:[^"\\]|\\.)*)"\s*\)\]"#)
        .expect("valid regex")
});
static RUST_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r##"#\[(?:given|when|then)\s*\(\s*(?:(?:regex|expr)\s*=\s*)?r?#?"((?:[^"\\]|\\.)*)"#?\s*\)\]"##,
    )
    .expect("valid regex")
});

/// The file extension a language's step definitions live in.
pub fn source_extension(language: Language) -> &'static str {
    match language {
        Language::Java => "java",
        Language::JavaScript => "js",
        Language::TypeScript => "ts",
        Language::DotNet => "cs",
        Language::Rust => "rs",
    }
}

/// Extract every step-definition pattern declared in one source file.
pub fn extract_patterns(language: Language, source: &str) -> Vec<String> {
    let regex = match language {
        Language::Java => &JAVA_DEF,
        Language::JavaScript | Language::TypeScript => &JS_DEF,
        Language::DotNet => &CSHARP_DEF,
        Language::Rust => &RUST_DEF,
    };
    regex
        .captures_iter(source)
        .map(|c| {
            c.iter()
                .skip(1)
                .flatten()
                .next()
                .expect("one capture group matches")
                .as_str()
                .replace("\\\"", "\"")
                .replace("\\'", "'")
        })
        .collect()
}

/// Does `text` match `pattern`? Patterns are either anchored regexes
/// (starting with `^`) or Cucumber expressions (`{int}`, `{string}`, ...).
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    let regex_source = if pattern.starts_with('^') {
        pattern.to_string()
    } else {
        cucumber_expression_to_regex(pattern)
    };
    Regex::new(&regex_source).is_ok_and(|r| r.is_match(text))
}

/// Translate a Cucumber expression into an anchored regex.
fn cucumber_expression_to_regex(expression: &str) -> String {
    let mut out = String::from("^");
    let mut rest = expression;
    while let Some(open) = rest.find('{') {
        let (literal, tail) = rest.split_at(open);
        out.push_str(&regex::escape(literal));
        let Some(close) = tail.find('}') else {
            out.push_str(&regex::escape(tail));
            rest = "";
            break;
        };
        out.push_str(match &tail[..=close] {
            "{int}" => r"-?\d+",
            "{float}" => r"-?\d+(?:\.\d+)?",
            "{word}" => r"\S+",
            "{string}" => r#""[^"]*"|'[^']*'"#,
            _ => ".*",
        });
        rest = &tail[close + 1..];
    }
    out.push_str(&regex::escape(rest));
    out.push('$');
    out
}

/// Every step in the given features that no pattern matches, deduplicated
/// by resolved keyword + text (the definition would be shared anyway).
pub fn find_missing(features: &[FeatureDoc], patterns: &[String]) -> Vec<MissingStep> {
    let mut missing: Vec<MissingStep> = Vec::new();
    for feature in features {
        for scenario in &feature.scenarios {
            let mut last_keyword = "Given".to_string();
            for step in &scenario.steps {
                let (keyword, text) = split_step(step);
                let keyword = if keyword == "And" || keyword == "But" {
                    last_keyword.clone()
                } else {
                    last_keyword = keyword.to_string();
                    last_keyword.clone()
                };
                if patterns.iter().any(|p| pattern_matches(p, text)) {
                    continue;
                }
                if missing
                    .iter()
                    .any(|m| m.keyword == keyword && m.text == text)
                {
                    continue;
                }
                missing.push(MissingStep {
                    feature: feature.path.clone(),
                    scenario: scenario.name.clone(),
                    keyword,
                    text: text.to_string(),
                });
            }
        }
    }
    missing
}

fn split_step(step: &str) -> (&str, &str) {
    match step.split_once(' ') {
        Some((keyword, text)) => (keyword, text),
        None => (step, ""),
    }
}

/// Turn one well-formed acceptance criterion ("Given X, when Y, then Z")
/// into the three Gherkin steps of a scenario. Returns `None` when the
/// criterion does not follow the Given/when/then shape the spec
/// validator enforces.
pub fn criterion_to_steps(criterion: &str) -> Option<Vec<String>> {
    static SHAPE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^given\s+(.+?),\s*when\s+(.+?),\s*then\s+(.+)$").expect("valid regex")
    });
    let captures = SHAPE.captures(criterion.trim())?;
    Some(vec![
        format!("Given {}", &captures[1]),
        format!("When {}", &captures[2]),
        format!("Then {}", &captures[3]),
    ])
}

/// Turn a concrete step text into a Cucumber expression: numbers become
/// `{int}`, quoted strings become `{string}`.
pub fn step_to_expression(text: &str) -> String {
    static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""[^"]*""#).expect("valid"));
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d+\b").expect("valid"));
    let with_strings = QUOTED.replace_all(text, "{string}");
    NUMBER.replace_all(&with_strings, "{int}").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::feature::{FeatureDoc, ScenarioDoc};

    #[test]
    fn java_annotations_are_extracted() {
        let source = r#"
            @Given("a calculator")
            public void aCalculator() {}
            @When("add is called with {string}")
            public void add(String input) {}
            @And("the \"quoted\" case")
            public void quoted() {}
        "#;
        assert_eq!(
            extract_patterns(Language::Java, source),
            vec![
                "a calculator",
                "add is called with {string}",
                "the \"quoted\" case",
            ]
        );
    }

    #[test]
    fn javascript_and_typescript_calls_are_extracted() {
        let source = r#"
            Given('a calculator', function () {});
            When("add is called with {string}", (input) => {});
        "#;
        for language in [Language::JavaScript, Language::TypeScript] {
            assert_eq!(
                extract_patterns(language, source),
                vec!["a calculator", "add is called with {string}"]
            );
        }
    }

    #[test]
    fn csharp_attributes_are_extracted() {
        let source = r#"
            [Given(@"a calculator")]
            public void GivenACalculator() {}
            [Then("the result is {int}")]
            public void TheResultIs(int value) {}
        "#;
        assert_eq!(
            extract_patterns(Language::DotNet, source),
            vec!["a calculator", "the result is {int}"]
        );
    }

    #[test]
    fn rust_attribute_macros_are_extracted() {
        let source = r##"
            #[given("a calculator")]
            fn a_calculator(w: &mut W) {}
            #[then(regex = r"^the result is (\d+)$")]
            fn result(w: &mut W, n: u32) {}
        "##;
        assert_eq!(
            extract_patterns(Language::Rust, source),
            vec!["a calculator", "^the result is (\\d+)$"]
        );
    }

    #[test]
    fn cucumber_expressions_match_their_placeholders() {
        assert!(pattern_matches("the result is {int}", "the result is 42"));
        assert!(pattern_matches("the result is {int}", "the result is -1"));
        assert!(!pattern_matches(
            "the result is {int}",
            "the result is many"
        ));
        assert!(pattern_matches(
            "add is called with {string}",
            "add is called with \"1,2\""
        ));
        assert!(pattern_matches(
            "a {word} calculator",
            "a scientific calculator"
        ));
        assert!(pattern_matches("the rate is {float}", "the rate is 1.25"));
        assert!(pattern_matches("anything {} goes", "anything at all goes"));
        assert!(pattern_matches("a calculator", "a calculator"));
        assert!(!pattern_matches(
            "a calculator",
            "a calculator with history"
        ));
    }

    #[test]
    fn an_unclosed_placeholder_is_treated_literally() {
        assert!(pattern_matches("a {broken", "a {broken"));
    }

    #[test]
    fn anchored_regex_patterns_are_used_directly() {
        assert!(pattern_matches(r"^the result is (\d+)$", "the result is 7"));
        assert!(!pattern_matches(
            r"^the result is (\d+)$",
            "the result is x"
        ));
        assert!(!pattern_matches(r"^bro][ken$", "anything"));
    }

    fn feature_with_steps(steps: Vec<&str>) -> FeatureDoc {
        FeatureDoc {
            path: "features/calc.feature".into(),
            name: "Calc".into(),
            tags: vec![],
            scenarios: vec![ScenarioDoc {
                name: "Adds".into(),
                tags: vec![],
                steps: steps.into_iter().map(String::from).collect(),
            }],
        }
    }

    #[test]
    fn missing_steps_resolve_and_but_to_the_preceding_keyword() {
        let feature = feature_with_steps(vec![
            "Given a calculator",
            "And a memory bank",
            "When add is called",
            "But subtract is not",
            "Then the result is 3",
        ]);
        let patterns = vec!["a calculator".to_string()];
        let missing = find_missing(&[feature], &patterns);
        let resolved: Vec<(&str, &str)> = missing
            .iter()
            .map(|m| (m.keyword.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            resolved,
            vec![
                ("Given", "a memory bank"),
                ("When", "add is called"),
                ("When", "subtract is not"),
                ("Then", "the result is 3"),
            ]
        );
    }

    #[test]
    fn duplicate_missing_steps_are_reported_once() {
        let one = feature_with_steps(vec!["Given a calculator"]);
        let two = FeatureDoc {
            path: "features/other.feature".into(),
            ..feature_with_steps(vec!["Given a calculator"])
        };
        assert_eq!(find_missing(&[one, two], &[]).len(), 1);
    }

    #[test]
    fn a_fully_defined_feature_has_no_missing_steps() {
        let feature = feature_with_steps(vec!["Given a calculator", "Then the result is 3"]);
        let patterns = vec!["a calculator".into(), "the result is {int}".into()];
        assert_eq!(find_missing(&[feature], &patterns), vec![]);
    }

    #[test]
    fn a_bare_keyword_step_is_handled() {
        let feature = feature_with_steps(vec!["Given"]);
        let missing = find_missing(&[feature], &[]);
        assert_eq!(missing[0].text, "");
    }

    #[test]
    fn a_well_formed_criterion_becomes_three_steps() {
        let steps = criterion_to_steps(
            "Given an empty string \"\", when add is called, then the result is 0",
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                "Given an empty string \"\"",
                "When add is called",
                "Then the result is 0",
            ]
        );
    }

    #[test]
    fn criterion_parsing_is_case_insensitive_and_trims() {
        let steps = criterion_to_steps("  given a, WHEN b, Then 3  ").unwrap();
        assert_eq!(steps, vec!["Given a", "When b", "Then 3"]);
    }

    #[test]
    fn a_malformed_criterion_is_none() {
        assert_eq!(criterion_to_steps("the result should be 6"), None);
        assert_eq!(criterion_to_steps("Given a, then 3"), None);
    }

    #[test]
    fn concrete_steps_become_cucumber_expressions() {
        assert_eq!(
            step_to_expression("add is called with \"1,2\""),
            "add is called with {string}"
        );
        assert_eq!(
            step_to_expression("the result is 42"),
            "the result is {int}"
        );
        assert_eq!(
            step_to_expression("the result is 0."),
            "the result is {int}.",
            "a trailing period on the Gherkin line must stay on the expression or Cucumber treats the step as undefined"
        );
        assert_eq!(step_to_expression("a calculator"), "a calculator");
    }

    #[test]
    fn every_language_names_its_source_extension() {
        assert_eq!(source_extension(Language::Java), "java");
        assert_eq!(source_extension(Language::JavaScript), "js");
        assert_eq!(source_extension(Language::TypeScript), "ts");
        assert_eq!(source_extension(Language::DotNet), "cs");
        assert_eq!(source_extension(Language::Rust), "rs");
    }
}

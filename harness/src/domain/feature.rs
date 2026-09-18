//! The feature-file model the harness reports and mutates: a plain,
//! framework-neutral view of Gherkin, with pure `parse` and `render`
//! functions. The `gherkin` crate is a pure parser (no IO), so it lives
//! here the way `serde` does; file discovery and reading stay in the
//! adapter ring.

use serde::Serialize;

/// One feature file, summarized for listings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureSummary {
    pub path: String,
    pub name: String,
    #[serde(rename = "scenarioCount")]
    pub scenario_count: usize,
}

/// One feature file in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeatureDoc {
    pub path: String,
    pub name: String,
    pub tags: Vec<String>,
    /// The comment lines above the feature keyword, `#` included - the
    /// file's header. The Gherkin parser discards comments, so these are
    /// read off the raw text; without them a staged edit would strip the
    /// header off every file it touches. Comments *between* or after
    /// scenarios are still lost on a round trip: keeping those would mean
    /// anchoring each one to its neighbouring scenario, and the header is
    /// the part that documents the file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    /// The narrative under the feature keyword ("As a ... I want ... So
    /// that ..."), one line per entry, indentation normalized away.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub description: Vec<String>,
    pub scenarios: Vec<ScenarioDoc>,
}

/// One scenario: its tags and its steps rendered as written
/// ("Given ...", "When ...", "Then ...").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScenarioDoc {
    pub name: String,
    pub tags: Vec<String>,
    pub steps: Vec<String>,
}

/// Parse Gherkin text into the plain model. The parser strips the
/// leading `@` from tags; this restores it so tags read as authored.
pub fn parse(path: &str, content: &str) -> Result<FeatureDoc, String> {
    let feature = gherkin::Feature::parse(content, gherkin::GherkinEnv::default())
        .map_err(|e| format!("{path}: not valid Gherkin - {e}"))?;
    let tags_as_written = |tags: &[String]| tags.iter().map(|t| format!("@{t}")).collect();
    Ok(FeatureDoc {
        path: path.to_string(),
        name: feature.name.clone(),
        tags: tags_as_written(&feature.tags),
        comments: leading_comments(content, feature.position.line),
        description: feature
            .description
            .as_deref()
            .map(description_lines)
            .unwrap_or_default(),
        scenarios: feature
            .scenarios
            .iter()
            .map(|scenario| ScenarioDoc {
                name: scenario.name.clone(),
                tags: tags_as_written(&scenario.tags),
                steps: scenario
                    .steps
                    .iter()
                    .map(|step| format!("{} {}", step.keyword.trim(), step.value))
                    .collect(),
            })
            .collect(),
    })
}

/// The comment lines above the feature keyword, from the first one to the
/// last, blank lines between them kept. `feature_line` is 1-based.
fn leading_comments(content: &str, feature_line: usize) -> Vec<String> {
    let preamble: Vec<&str> = content
        .lines()
        .take(feature_line.saturating_sub(1))
        .collect();
    let is_comment = |line: &&str| line.trim_start().starts_with('#');
    let first = preamble.iter().position(is_comment);
    let last = preamble.iter().rposition(is_comment);
    match (first, last) {
        (Some(first), Some(last)) => preamble[first..=last]
            .iter()
            .map(|line| line.trim_end().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Narrative lines with their indentation dropped, so that rendering can
/// re-indent them and the round trip stays stable.
fn description_lines(description: &str) -> Vec<String> {
    description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// Render the model back to canonical Gherkin text.
pub fn render(doc: &FeatureDoc) -> String {
    let mut out = String::new();
    for comment in &doc.comments {
        out.push_str(comment);
        out.push('\n');
    }
    if !doc.tags.is_empty() {
        out.push_str(&doc.tags.join(" "));
        out.push('\n');
    }
    out.push_str(&format!("Feature: {}\n", doc.name));
    for line in &doc.description {
        out.push_str(&format!("  {line}\n"));
    }
    for scenario in &doc.scenarios {
        out.push('\n');
        if !scenario.tags.is_empty() {
            out.push_str(&format!("  {}\n", scenario.tags.join(" ")));
        }
        out.push_str(&format!("  Scenario: {}\n", scenario.name));
        for step in &scenario.steps {
            out.push_str(&format!("    {step}\n"));
        }
    }
    out
}

impl FeatureDoc {
    pub fn summary(&self) -> FeatureSummary {
        FeatureSummary {
            path: self.path.clone(),
            name: self.name.clone(),
            scenario_count: self.scenarios.len(),
        }
    }

    /// Every tag carried by the feature or any scenario in it.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags = self.tags.clone();
        for scenario in &self.scenarios {
            for tag in &scenario.tags {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
        }
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> FeatureDoc {
        FeatureDoc {
            path: "features/x.feature".into(),
            name: "String calculator".into(),
            tags: vec!["@kata".into()],
            comments: Vec::new(),
            description: Vec::new(),
            scenarios: vec![
                ScenarioDoc {
                    name: "Empty string".into(),
                    tags: vec!["@REQ-001".into()],
                    steps: vec![
                        "Given a calculator".into(),
                        "When add is called with \"\"".into(),
                        "Then the result is 0".into(),
                    ],
                },
                ScenarioDoc {
                    name: "Single number".into(),
                    tags: vec!["@REQ-002".into(), "@kata".into()],
                    steps: vec!["Given a calculator".into()],
                },
            ],
        }
    }

    #[test]
    fn a_summary_carries_path_name_and_scenario_count() {
        assert_eq!(
            doc().summary(),
            FeatureSummary {
                path: "features/x.feature".into(),
                name: "String calculator".into(),
                scenario_count: 2,
            }
        );
    }

    #[test]
    fn all_tags_deduplicates_across_feature_and_scenarios() {
        assert_eq!(doc().all_tags(), vec!["@kata", "@REQ-001", "@REQ-002"]);
    }

    #[test]
    fn the_summary_serializes_scenario_count_in_camel_case() {
        let json = serde_json::to_string(&doc().summary()).unwrap();
        assert!(json.contains("scenarioCount"));
    }

    #[test]
    fn a_document_round_trips_through_render_and_parse() {
        let original = doc();
        let text = render(&original);
        assert_eq!(parse(&original.path, &text).unwrap(), original);
    }

    #[test]
    fn render_of_an_untagged_scenarioless_feature_is_just_the_header() {
        let bare = FeatureDoc {
            path: "features/new.feature".into(),
            name: "Fresh feature".into(),
            tags: vec![],
            comments: Vec::new(),
            description: Vec::new(),
            scenarios: vec![],
        };
        assert_eq!(render(&bare), "Feature: Fresh feature\n");
    }

    #[test]
    fn render_of_an_untagged_scenario_omits_the_tag_line() {
        let doc = FeatureDoc {
            path: "features/new.feature".into(),
            name: "F".into(),
            tags: vec![],
            comments: Vec::new(),
            description: Vec::new(),
            scenarios: vec![ScenarioDoc {
                name: "S".into(),
                tags: vec![],
                steps: vec!["Given a".into()],
            }],
        };
        assert_eq!(render(&doc), "Feature: F\n\n  Scenario: S\n    Given a\n");
    }

    /// The shape the kata ships: a comment block, then the feature
    /// keyword, then an "As a / I want / So that" narrative.
    const WITH_HEADER: &str = "\
# The executable behavior spec (BDD level) for the kata.
#
# Each scenario is tagged with the requirement it verifies.
Feature: String Calculator addition
  As a user of the calculator
  I want delimited number strings to be summed safely
  So that any input produces a predictable result

  @REQ-001
  Scenario: An empty string returns zero
    Given a string calculator
    Then the result is 0
";

    #[test]
    fn a_staged_edit_keeps_the_comment_block_and_the_narrative() {
        let doc = parse("features/calc.feature", WITH_HEADER).unwrap();
        assert_eq!(doc.comments.len(), 3);
        assert_eq!(
            doc.comments[0],
            "# The executable behavior spec (BDD level) for the kata."
        );
        assert_eq!(doc.comments[1], "#");
        assert_eq!(
            doc.description,
            vec![
                "As a user of the calculator",
                "I want delimited number strings to be summed safely",
                "So that any input produces a predictable result",
            ]
        );
        // Appending a scenario is a parse/render round trip; the header has
        // to survive it or the file loses its documentation.
        let mut edited = doc.clone();
        edited.scenarios.push(ScenarioDoc {
            name: "A single number returns its value".into(),
            tags: vec!["@REQ-002".into()],
            steps: vec!["Given a string calculator".into()],
        });
        let text = render(&edited);
        assert!(
            text.starts_with("# The executable behavior spec"),
            "got: {text}"
        );
        assert!(
            text.contains("  As a user of the calculator\n"),
            "got: {text}"
        );
        assert_eq!(parse("features/calc.feature", &text).unwrap(), edited);
    }

    #[test]
    fn a_feature_with_a_header_round_trips_byte_for_byte() {
        let doc = parse("features/calc.feature", WITH_HEADER).unwrap();
        assert_eq!(render(&doc), WITH_HEADER);
    }

    #[test]
    fn tags_above_the_feature_keyword_are_not_mistaken_for_comments() {
        let tagged = "# a note\n@kata\nFeature: F\n\n  Scenario: S\n    Given a\n";
        let doc = parse("features/calc.feature", tagged).unwrap();
        assert_eq!(doc.comments, vec!["# a note"]);
        assert_eq!(doc.tags, vec!["@kata"]);
        assert_eq!(render(&doc), tagged);
    }

    #[test]
    fn a_feature_without_a_header_renders_exactly_as_before() {
        let plain = "Feature: F\n\n  Scenario: S\n    Given a\n";
        let doc = parse("features/calc.feature", plain).unwrap();
        assert!(doc.comments.is_empty());
        assert!(doc.description.is_empty());
        assert_eq!(render(&doc), plain);
    }

    #[test]
    fn an_empty_header_is_left_out_of_the_tool_reply() {
        let plain = parse("features/calc.feature", "Feature: F\n").unwrap();
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("comments"), "got: {json}");
        assert!(!json.contains("description"), "got: {json}");
    }

    #[test]
    fn parse_of_invalid_gherkin_names_the_file() {
        let error = parse("features/broken.feature", "not gherkin").unwrap_err();
        assert!(
            error.starts_with("features/broken.feature: not valid Gherkin -"),
            "got: {error}"
        );
    }
}

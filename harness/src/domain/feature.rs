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

/// Render the model back to canonical Gherkin text.
pub fn render(doc: &FeatureDoc) -> String {
    let mut out = String::new();
    if !doc.tags.is_empty() {
        out.push_str(&doc.tags.join(" "));
        out.push('\n');
    }
    out.push_str(&format!("Feature: {}\n", doc.name));
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
            scenarios: vec![ScenarioDoc {
                name: "S".into(),
                tags: vec![],
                steps: vec!["Given a".into()],
            }],
        };
        assert_eq!(render(&doc), "Feature: F\n\n  Scenario: S\n    Given a\n");
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

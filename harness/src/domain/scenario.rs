//! Turning one requirement's acceptance criteria into Gherkin scenarios.
//!
//! The deterministic template reads each criterion literally, which is
//! always correct and rarely idiomatic: it cannot know that the feature
//! file it is joining opens every scenario on "Given a string calculator".
//! A model is asked to say the same thing in the file's own vocabulary,
//! and its reply is only used when it still covers every criterion.

use serde::Deserialize;

use crate::domain::feature::{FeatureDoc, ScenarioDoc};
use crate::domain::generation::{decode_json, strip_code_fences};
use crate::domain::model::Requirement;
use crate::domain::prompts::{RenderedPrompt, render};
use crate::domain::steps::criterion_to_steps;

/// The Gherkin keywords a step may open with, as [`ScenarioService`]
/// checks them before staging.
///
/// [`ScenarioService`]: crate::application::scenario_service::ScenarioService
const KEYWORDS: [&str; 5] = ["Given ", "When ", "Then ", "And ", "But "];

/// One scenario, before it is tagged and staged.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProposedScenario {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub steps: Vec<String>,
}

/// The literal reading of a requirement: one scenario per Given/When/Then
/// criterion, named after the requirement it came from.
///
/// A criterion that is not Given/When/Then shaped yields nothing - there is
/// no honest way to guess its steps - so the result can be shorter than the
/// criteria list, and empty when none of them parse.
pub fn scenario_template(requirement: &Requirement) -> Vec<ProposedScenario> {
    requirement
        .acceptance_criteria
        .iter()
        .enumerate()
        .filter_map(|(index, criterion)| {
            criterion_to_steps(criterion).map(|steps| ProposedScenario {
                name: format!("{} case {}", requirement.title, index + 1),
                steps,
            })
        })
        .collect()
}

/// The scenarios as Gherkin, for the `<template>` block of the prompt and
/// for anything else that wants to show them the way they will be written.
pub fn as_gherkin(scenarios: &[ProposedScenario]) -> String {
    scenarios
        .iter()
        .map(|scenario| {
            let steps = scenario
                .steps
                .iter()
                .map(|step| format!("    {step}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("  Scenario: {}\n{steps}\n", scenario.name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The instructions for writing one requirement's scenarios in the
/// vocabulary its feature file already uses, rendered from the
/// `[scenario]` templates.
pub fn scenario_prompt(
    requirement: &Requirement,
    feature_path: &str,
    feature: &str,
    known_steps: &[String],
    template: &[ProposedScenario],
) -> RenderedPrompt {
    render(
        "scenario",
        minijinja::context! {
            req_id => requirement.id,
            title => requirement.title,
            story => requirement.story,
            criteria => requirement.acceptance_criteria,
            feature_path,
            feature,
            known_steps,
            template => as_gherkin(template),
        },
    )
}

/// Parse the model's reply, with a reason when it cannot be used so the
/// retry can tell the model what was wrong.
///
/// `expected` is the criteria count: one scenario per criterion is the
/// contract the coverage check downstream is graded on, so a reply that
/// drops or invents one is refused rather than quietly accepted.
/// `taken` are the scenario names already in the feature file.
pub fn parse_scenarios_checked(
    reply: &str,
    expected: usize,
    taken: &[String],
) -> Result<Vec<ProposedScenario>, String> {
    let body = strip_code_fences(reply);
    let parsed: Option<Vec<ProposedScenario>> = decode_json(&body, '[')
        .or_else(|| decode_json::<ProposedScenario>(&body, '{').map(|one| vec![one]));
    let Some(scenarios) = parsed else {
        return Err("the reply was not a JSON array of scenarios (name, steps)".into());
    };
    if scenarios.len() != expected {
        return Err(format!(
            "the reply held {} scenario(s) for {expected} acceptance criterion(s) - write exactly one scenario per criterion, in order",
            scenarios.len()
        ));
    }
    for scenario in &scenarios {
        check(scenario, taken, &scenarios)?;
    }
    Ok(scenarios)
}

fn check(
    scenario: &ProposedScenario,
    taken: &[String],
    all: &[ProposedScenario],
) -> Result<(), String> {
    let name = scenario.name.trim();
    if name.is_empty() {
        return Err("a scenario arrived with no name".into());
    }
    if taken.iter().any(|existing| existing == name) {
        return Err(format!(
            "scenario \"{name}\" is already in the feature file - every name must be new"
        ));
    }
    if all.iter().filter(|s| s.name.trim() == name).count() > 1 {
        return Err(format!("scenario \"{name}\" is named twice in the reply"));
    }
    for step in &scenario.steps {
        if !KEYWORDS.iter().any(|keyword| step.starts_with(keyword)) {
            return Err(format!(
                "step \"{step}\" must start with Given, When, Then, And, or But"
            ));
        }
    }
    let opens = |keyword: &str| {
        scenario
            .steps
            .iter()
            .any(|step| step.starts_with(keyword) || step.starts_with("And "))
    };
    if !scenario.steps.iter().any(|s| s.starts_with("When ")) || !opens("Then ") {
        return Err(format!(
            "scenario \"{name}\" needs at least one When and one Then"
        ));
    }
    Ok(())
}

/// The scenario names already in the document, so a proposal cannot
/// collide with one. Staging refuses a duplicate name, and finding that
/// out after three model calls is a poor way to learn it.
pub fn taken_names(doc: Option<&FeatureDoc>) -> Vec<String> {
    doc.map(|doc| doc.scenarios.iter().map(|s| s.name.clone()).collect())
        .unwrap_or_default()
}

/// The scenarios a document already carries for one requirement, by tag.
pub fn tagged(doc: &FeatureDoc, req_id: &str) -> Vec<String> {
    let tag = format!("@{req_id}");
    doc.scenarios
        .iter()
        .filter(|s: &&ScenarioDoc| s.tags.iter().any(|t| t == &tag))
        .map(|s| s.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement() -> Requirement {
        Requirement {
            id: "REQ-003".into(),
            title: "Two numbers separated by a comma are summed".into(),
            status: "pending".into(),
            story: "As a user, I want comma-separated numbers to be summed so that I can add multiple values at once.".into(),
            acceptance_criteria: vec![
                r#"Given "1,2", when add is called, then the result is 3"#.into(),
                r#"Given "10,20", when add is called, then the result is 30"#.into(),
            ],
            feature_file: Some("features/calc.feature".into()),
        }
    }

    #[test]
    fn the_template_reads_each_criterion_literally() {
        let template = scenario_template(&requirement());
        assert_eq!(template.len(), 2);
        assert_eq!(
            template[0].steps,
            vec![
                r#"Given "1,2""#,
                "When add is called",
                "Then the result is 3"
            ]
        );
        assert_eq!(
            template[1].name,
            "Two numbers separated by a comma are summed case 2"
        );
    }

    #[test]
    fn a_criterion_that_is_not_given_when_then_shaped_yields_no_scenario() {
        let mut requirement = requirement();
        requirement.acceptance_criteria = vec!["the calculator is fast".into()];
        assert!(scenario_template(&requirement).is_empty());
    }

    #[test]
    fn the_prompt_carries_the_criteria_the_file_and_the_template() {
        let prompt = scenario_prompt(
            &requirement(),
            "features/calc.feature",
            "Feature: Calc\n\n  @REQ-001\n  Scenario: Empty\n    Given a string calculator\n",
            &["a string calculator".to_string()],
            &scenario_template(&requirement()),
        );
        assert!(prompt.user.contains("id: REQ-003"));
        assert!(
            prompt
                .user
                .contains(r#"1. Given "1,2", when add is called"#)
        );
        assert!(prompt.user.contains("Scenario: Empty"));
        assert!(prompt.user.contains("- a string calculator"));
        assert!(prompt.user.contains("Scenario: Two numbers separated"));
        assert!(prompt.system.contains("ONLY a JSON array"));
        assert!(
            prompt
                .system
                .contains("one scenario per acceptance criterion")
        );
    }

    #[test]
    fn a_reply_in_the_files_vocabulary_parses() {
        let reply = r#"[
          {"name": "Two numbers are summed", "steps": ["Given a string calculator", "When I add \"1,2\"", "Then the result is 3"]},
          {"name": "Two larger numbers are summed", "steps": ["Given a string calculator", "When I add \"10,20\"", "Then the result is 30"]}
        ]"#;
        let scenarios = parse_scenarios_checked(reply, 2, &[]).unwrap();
        assert_eq!(scenarios.len(), 2);
        assert_eq!(scenarios[0].name, "Two numbers are summed");
        assert_eq!(scenarios[1].steps[1], r#"When I add "10,20""#);
    }

    #[test]
    fn a_fenced_reply_still_parses() {
        let reply =
            "```json\n[{\"name\": \"S\", \"steps\": [\"Given a\", \"When b\", \"Then c\"]}]\n```";
        assert_eq!(parse_scenarios_checked(reply, 1, &[]).unwrap().len(), 1);
    }

    #[test]
    fn the_wrong_number_of_scenarios_is_refused_by_count() {
        let reply = r#"[{"name": "Only one", "steps": ["Given a", "When b", "Then c"]}]"#;
        let reason = parse_scenarios_checked(reply, 2, &[]).unwrap_err();
        assert!(reason.contains("held 1 scenario(s) for 2"), "{reason}");
    }

    #[test]
    fn a_name_already_in_the_file_is_refused() {
        let reply = r#"[{"name": "Taken", "steps": ["Given a", "When b", "Then c"]}]"#;
        let reason = parse_scenarios_checked(reply, 1, &["Taken".into()]).unwrap_err();
        assert!(reason.contains("already in the feature file"), "{reason}");
    }

    #[test]
    fn a_name_used_twice_in_one_reply_is_refused() {
        let reply = r#"[
          {"name": "Same", "steps": ["Given a", "When b", "Then c"]},
          {"name": "Same", "steps": ["Given a", "When d", "Then e"]}
        ]"#;
        let reason = parse_scenarios_checked(reply, 2, &[]).unwrap_err();
        assert!(reason.contains("named twice"), "{reason}");
    }

    #[test]
    fn a_step_without_a_keyword_is_refused() {
        let reply = r#"[{"name": "S", "steps": ["the calculator adds", "Then c"]}]"#;
        let reason = parse_scenarios_checked(reply, 1, &[]).unwrap_err();
        assert!(reason.contains("must start with Given"), "{reason}");
    }

    #[test]
    fn a_scenario_with_no_when_or_no_then_is_refused() {
        let no_then = r#"[{"name": "S", "steps": ["Given a", "When b"]}]"#;
        assert!(
            parse_scenarios_checked(no_then, 1, &[])
                .unwrap_err()
                .contains("needs at least one When and one Then")
        );
        let no_when = r#"[{"name": "S", "steps": ["Given a", "Then c"]}]"#;
        assert!(parse_scenarios_checked(no_when, 1, &[]).is_err());
    }

    #[test]
    fn prose_is_refused_with_a_reason_the_model_can_act_on() {
        let reason = parse_scenarios_checked("Sure! Here are your scenarios:", 1, &[]).unwrap_err();
        assert!(reason.contains("was not a JSON array"), "{reason}");
    }

    #[test]
    fn a_bare_object_is_read_as_one_scenario() {
        let reply = r#"{"name": "S", "steps": ["Given a", "When b", "Then c"]}"#;
        assert_eq!(parse_scenarios_checked(reply, 1, &[]).unwrap().len(), 1);
    }
}

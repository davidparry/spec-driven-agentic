//! Splitting a plain-words description into proposed requirements.
//! The model decides how many distinct requirements the description
//! contains; a proposal only qualifies when it arrives complete -
//! title, story, and at least one acceptance criterion.

use serde::{Deserialize, Serialize};

use crate::domain::generation::{decode_json, strip_code_fences};
use crate::domain::model::Requirement;
use crate::domain::prompts::{RenderedPrompt, render};
use crate::domain::refiner::{covers_edge_case, suggestion_for};

/// One requirement the model proposes from the description.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProposedRequirement {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub story: String,
    #[serde(default, alias = "acceptanceCriteria", alias = "criteria")]
    pub acceptance_criteria: Vec<String>,
}

/// The strict instructions for turning a description into a JSON array
/// of complete requirements, rendered from the `[proposal]` templates.
pub fn proposal_prompt(description: &str) -> RenderedPrompt {
    render("proposal", minijinja::context! { description })
}

/// Parse the model's reply into qualified proposals. Incomplete
/// elements are dropped; an unparseable reply is an empty list - the
/// caller falls back to manual drafting either way.
pub fn parse_proposals(reply: &str) -> Vec<ProposedRequirement> {
    parse_proposals_checked(reply).unwrap_or_default()
}

/// Parse with a reason when the reply cannot be used, so a retry can
/// tell the model what was wrong.
pub fn parse_proposals_checked(reply: &str) -> Result<Vec<ProposedRequirement>, String> {
    let body = strip_code_fences(reply);
    let parsed =
        decode_json_array(&body).or_else(|| decode_json_object(&body).map(|one| vec![one]));
    let Some(proposals) = parsed else {
        return Err(
            "the reply was not a JSON array of requirements (title, story, acceptanceCriteria)"
                .into(),
        );
    };
    let complete: Vec<ProposedRequirement> = proposals
        .into_iter()
        .filter(is_complete)
        .map(ProposedRequirement::normalized)
        .collect();
    if complete.is_empty() {
        return Err(
            "the JSON held no complete requirement (each needs a title, a story, and at least one Given/When/Then criterion)"
                .into(),
        );
    }
    Ok(complete)
}

/// Why a batch of proposals is not worth staging yet, or `None` once
/// every one of them covers an edge case.
///
/// The wording review applies this same rule to every draft, so a
/// proposal that only passes clean input earns a finding the moment the
/// author has finished reading it. Spending a retry here instead trades
/// a round the author never sees for a second walk through wording they
/// have already approved.
pub fn missing_edge_case(proposals: &[ProposedRequirement]) -> Option<String> {
    let happy: Vec<String> = proposals
        .iter()
        .filter(|proposal| {
            !proposal
                .acceptance_criteria
                .iter()
                .any(|criterion| covers_edge_case(criterion))
        })
        .map(|proposal| format!("{:?}", proposal.title))
        .collect();
    let (subject, verb) = match happy.len() {
        0 => return None,
        1 => ("requirement", "covers"),
        _ => ("requirements", "cover"),
    };
    Some(format!(
        "{subject} {} {verb} only happy paths - add at least one edge case \
         to each (empty, invalid, or error input)",
        happy.join(", ")
    ))
}

impl ProposedRequirement {
    /// A requirement's title, story, and criteria are single-line fields,
    /// and the spec writes an input newline as the two characters `\n` (see
    /// REQ-005's `"1\n2,3"`). A model handed that sometimes answers with a
    /// real newline in its place - even in a criterion it was told to copy
    /// verbatim - which leaves the criterion wrapping across two lines in
    /// every prompt and report and no longer matching the rest of the spec.
    /// Put the escape back.
    fn normalized(mut self) -> Self {
        self.title = escape_controls(&self.title);
        self.story = escape_controls(&self.story);
        self.acceptance_criteria = self
            .acceptance_criteria
            .iter()
            .map(|criterion| escape_controls(criterion))
            .collect();
        self
    }
}

/// One spec field folded back onto the single line it is authored on,
/// with a real control character written the way the spec writes it.
///
/// Also what the `// criterion` comment above a generated test is built
/// from: a real newline there would leave the rest of the criterion
/// outside the comment, which is a compile error rather than a cosmetic
/// one. Unlike [`crate::domain::generation::escape_literal`] this leaves
/// backslashes and quotes alone - a comment needs no delimiter escaped,
/// and doubling them would stop it reading as the spec wrote it.
pub(crate) fn escape_controls(text: &str) -> String {
    text.replace('\r', "")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn is_complete(proposal: &ProposedRequirement) -> bool {
    !proposal.title.trim().is_empty()
        && !proposal.story.trim().is_empty()
        && !proposal.acceptance_criteria.is_empty()
        && proposal
            .acceptance_criteria
            .iter()
            .all(|criterion| !criterion.trim().is_empty())
}

/// The first JSON array in `body`.
fn decode_json_array(body: &str) -> Option<Vec<ProposedRequirement>> {
    decode_json(body, '[')
}

/// A bare object is treated as a one-element array - the same courtesy
/// `parse_file_updates` already gives implementation replies.
fn decode_json_object(body: &str) -> Option<ProposedRequirement> {
    decode_json(body, '{')
}

/// One earlier wording of the draft and the findings it produced, as
/// the `[rewording]` user template consumes it.
#[derive(Serialize)]
struct WordingContext<'a> {
    title: &'a str,
    story: &'a str,
    criteria: &'a [String],
    findings: &'a [String],
}

/// The strict instructions for rewording one draft from a single
/// validate + refine finding: same requirement, same meaning, exactly
/// this one finding addressed. The caller chains one call per finding,
/// each briefed with the draft the previous call produced. `history`
/// recounts every earlier wording of this draft and the findings it
/// produced, so the model never circles back to a wording the review
/// already rejected. Rendered from the `[rewording]` templates.
pub fn rewording_prompt(
    candidate: &Requirement,
    finding: &str,
    history: &[(Requirement, Vec<String>)],
) -> RenderedPrompt {
    let history: Vec<WordingContext> = history
        .iter()
        .map(|(wording, findings)| WordingContext {
            title: &wording.title,
            story: &wording.story,
            criteria: &wording.acceptance_criteria,
            findings,
        })
        .collect();
    render(
        "rewording",
        minijinja::context! {
            title => candidate.title,
            story => candidate.story,
            criteria => candidate.acceptance_criteria,
            finding,
            hint => suggestion_for(finding),
            history,
        },
    )
}

/// Parse the model's rewording reply. `None` unless the object arrives
/// complete - the caller falls back to hand rewording.
pub fn parse_rewording(reply: &str) -> Option<ProposedRequirement> {
    parse_rewording_checked(reply).ok()
}

/// Parse with a reason when the rewording cannot be used.
pub fn parse_rewording_checked(reply: &str) -> Result<ProposedRequirement, String> {
    let body = strip_code_fences(reply);
    let Some(proposal) = decode_json_object(&body) else {
        return Err(
            "the reply was not a JSON object with title, story, and acceptanceCriteria".into(),
        );
    };
    if is_complete(&proposal) {
        Ok(proposal.normalized())
    } else {
        Err(
            "the JSON object was incomplete (needs a title, a story, and at least one Given/When/Then criterion)"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_newline_in_a_reworded_criterion_becomes_the_escape_again() {
        // What the model actually answered on a reword of REQ-007: the spec
        // writes the input newline as the two characters `\n`, and the reply
        // came back with JSON's `\n` escape instead - valid JSON that
        // decodes to a real newline, in the story and in a criterion it was
        // told to keep verbatim.
        let reply = r#"{"title": "Custom delimiter",
            "story": "As a user, I want \"//+\n1+2\" to sum so that I can pick a delimiter.",
            "acceptanceCriteria": ["Given \"//+\n1+2\", when add is called, then the result is 3"]}"#;
        let proposal = parse_rewording_checked(reply).unwrap();
        assert!(
            !proposal.story.contains('\n'),
            "story kept a real newline: {:?}",
            proposal.story
        );
        assert_eq!(
            proposal.acceptance_criteria,
            vec![r#"Given "//+\n1+2", when add is called, then the result is 3"#]
        );
    }

    #[test]
    fn an_escape_the_model_wrote_correctly_is_left_alone() {
        let reply = r#"{"title": "T", "story": "As a user, I want x so that y.",
                        "acceptanceCriteria": ["Given \"1\\n2\", when add is called, then the result is 3"]}"#;
        let proposal = parse_rewording_checked(reply).unwrap();
        assert_eq!(
            proposal.acceptance_criteria,
            vec![r#"Given "1\n2", when add is called, then the result is 3"#]
        );
    }

    #[test]
    fn a_tab_in_a_proposed_criterion_is_escaped_too() {
        let reply = r#"[{"title": "T", "story": "As a user, I want x so that y.",
            "acceptanceCriteria": ["Given \"1\t2\", when add is called, then the result is 3"]}]"#;
        let proposals = parse_proposals_checked(reply).unwrap();
        assert_eq!(
            proposals[0].acceptance_criteria,
            vec![r#"Given "1\t2", when add is called, then the result is 3"#]
        );
    }

    fn candidate() -> Requirement {
        Requirement {
            id: "REQ-001".into(),
            title: "Convert string to number".into(),
            status: "pending".into(),
            story: "As a user, I want conversion so that input becomes numbers.".into(),
            acceptance_criteria: vec![
                "Given \"1,2\", when add is called, then the result is 3".into(),
            ],
            feature_file: None,
        }
    }

    #[test]
    fn the_rewording_prompt_carries_the_draft_one_finding_its_hint_and_rules() {
        let prompt = rewording_prompt(
            &candidate(),
            "criteria: only happy paths - add at least one edge case",
            &[],
        );
        assert!(prompt.user.contains("title: Convert string to number"));
        assert!(
            prompt
                .user
                .contains("- Given \"1,2\", when add is called, then the result is 3")
        );
        assert!(prompt.user.contains("The one review finding to address:"));
        assert!(prompt.user.contains("- criteria: only happy paths"));
        assert!(prompt.user.contains("hint: add an edge case"));
        assert!(prompt.system.contains("ONLY one JSON object"));
        assert!(prompt.system.contains("Address only this one finding"));
        assert!(
            !prompt.user.contains("Earlier wordings"),
            "a first pass has no history section"
        );
    }

    #[test]
    fn a_finding_without_a_hint_is_listed_plain() {
        let prompt = rewording_prompt(&candidate(), "something entirely novel", &[]);
        assert!(prompt.user.contains("- something entirely novel"));
        assert!(!prompt.user.contains("hint:"));
    }

    #[test]
    fn the_rewording_prompt_recounts_every_earlier_wording_and_its_findings() {
        let mut first = candidate();
        first.title = "Numbers".into();
        let history = vec![(
            first,
            vec!["title: too vague to name the behavior".to_string()],
        )];
        let prompt = rewording_prompt(&candidate(), "criteria: only happy paths", &history);
        assert!(prompt.user.contains("Earlier wordings of this draft"));
        assert!(prompt.user.contains("Wording 1:\ntitle: Numbers"));
        assert!(
            prompt
                .user
                .contains("Wording 1 findings:\n- title: too vague to name the behavior")
        );
        assert!(prompt.user.contains("do not return to any of them"));
    }

    #[test]
    fn a_complete_rewording_object_parses_with_or_without_fences() {
        let reply =
            r#"{"title": "T", "story": "S", "acceptanceCriteria": ["Given x, when y, then z"]}"#;
        assert_eq!(parse_rewording(reply).unwrap().title, "T");
        let fenced = format!("```json\n{reply}\n```");
        assert!(parse_rewording(&fenced).is_some());
    }

    #[test]
    fn incomplete_or_unparseable_rewordings_are_none() {
        assert!(parse_rewording("Sure! Reworded below:").is_none());
        assert!(
            parse_rewording(r#"{"title": "", "story": "S", "acceptanceCriteria": ["x"]}"#)
                .is_none()
        );
        assert!(
            parse_rewording(r#"{"title": "T", "story": "S", "acceptanceCriteria": []}"#).is_none()
        );
        assert!(
            parse_rewording(r#"{"title": "T", "story": "S", "acceptanceCriteria": [" "]}"#)
                .is_none()
        );
    }

    #[test]
    fn the_prompt_carries_the_description_and_the_strict_rules() {
        let prompt = proposal_prompt("sum numbers from a string");
        assert!(prompt.user.contains("sum numbers from a string"));
        assert!(prompt.system.contains("ONLY a JSON array"));
        assert!(prompt.system.contains("acceptanceCriteria"));
        assert!(prompt.system.contains("Do not invent capabilities"));
        // The edge case earns a rule of its own: buried as the fourth
        // clause of the criteria rule, models dropped it.
        assert!(
            prompt
                .system
                .contains("at least one criterion covering an edge case")
        );
        // A broad description is unpacked into its conventional parts,
        // never refused with an empty array.
        assert!(prompt.system.contains("unpack"));
        assert!(
            prompt
                .system
                .contains("rather than replying with an empty array")
        );
    }

    #[test]
    fn a_bare_json_object_parses_as_one_proposal() {
        let reply =
            r#"{"title": "T", "story": "S", "acceptanceCriteria": ["Given x, when y, then z"]}"#;
        assert_eq!(parse_proposals(reply).len(), 1);
        assert!(parse_proposals_checked("Sure! Here are the requirements:").is_err());
        assert!(parse_proposals_checked("[]").is_err());
    }

    #[test]
    fn a_json_array_parses_into_proposals() {
        let reply = r#"[
            {"title": "Empty string returns zero",
             "story": "As a user, I want empty input to be 0 so that no input is safe.",
             "acceptanceCriteria": ["Given \"\", when add is called, then the result is 0"]},
            {"title": "Comma separated numbers are summed",
             "story": "As a user, I want comma sums so that totals come from one input.",
             "acceptanceCriteria": ["Given \"1,2\", when add is called, then the result is 3"]}
        ]"#;
        let proposals = parse_proposals(reply);
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].title, "Empty string returns zero");
        assert_eq!(proposals[1].acceptance_criteria.len(), 1);
    }

    #[test]
    fn a_happy_path_only_proposal_is_named_in_the_retry_reason() {
        let reply = r#"[
            {"title": "Comma separated numbers are summed",
             "story": "As a user, I want comma sums so that totals come from one input.",
             "acceptanceCriteria": ["Given \"1,2\", when add is called, then the result is 3"]},
            {"title": "Empty string returns zero",
             "story": "As a user, I want empty input to be 0 so that no input is safe.",
             "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]}
        ]"#;
        assert_eq!(
            missing_edge_case(&parse_proposals(reply)),
            Some(
                "requirement \"Comma separated numbers are summed\" covers only happy \
                 paths - add at least one edge case to each (empty, invalid, or error input)"
                    .to_string()
            )
        );
    }

    #[test]
    fn several_happy_path_proposals_are_all_named_and_read_as_plural() {
        let reply = r#"[
            {"title": "Comma sums", "story": "As a user, I want sums so that totals arrive.",
             "acceptanceCriteria": ["Given \"1,2\", when add is called, then the result is 3"]},
            {"title": "Newline sums", "story": "As a user, I want sums so that totals arrive.",
             "acceptanceCriteria": ["Given \"1\\n2\", when add is called, then the result is 3"]}
        ]"#;
        let reason = missing_edge_case(&parse_proposals(reply)).unwrap();
        assert!(
            reason.starts_with(
                "requirements \"Comma sums\", \"Newline sums\" cover only happy paths"
            ),
            "reason: {reason}"
        );
    }

    /// The canonical edge case is written `""`, with no keyword to match -
    /// the same courtesy the wording review extends.
    #[test]
    fn an_empty_input_criterion_satisfies_the_check_without_the_word_empty() {
        let reply = r#"[{"title": "T", "story": "As a user, I want x so that y.",
            "acceptanceCriteria": ["Given \"1,2\", when add is called, then the result is 3",
                                   "Given \"\", when add is called, then the result is 0"]}]"#;
        assert_eq!(missing_edge_case(&parse_proposals(reply)), None);
    }

    #[test]
    fn a_code_fenced_reply_still_parses() {
        let reply = "```json\n[{\"title\": \"T\", \"story\": \"S\", \
                     \"acceptanceCriteria\": [\"Given x, when y, then z\"]}]\n```";
        assert_eq!(parse_proposals(reply).len(), 1);
    }

    #[test]
    fn prose_or_broken_json_is_an_empty_list() {
        assert!(parse_proposals("Sure! Here are the requirements:").is_empty());
        assert!(parse_proposals("[{\"title\": unclosed").is_empty());
    }

    #[test]
    fn a_complete_array_followed_by_commentary_still_parses() {
        // Seen live: qwen3-coder-next emitted six calculator
        // requirements, then a Note and a second copy of the array.
        // from_str rejected the trailing prose and greenfield fell
        // through to a single manual draft - nothing pending after
        // REQ-001 closed.
        let reply = r#"[{"title": "Add two numbers from a comma-separated string", "story": "As a user, I want to add.", "acceptanceCriteria": ["Given \"1,2\", when add is called, then the result is 3"]}, {"title": "Handle multiple numbers", "story": "As a user, I want many numbers.", "acceptanceCriteria": ["Given \"1,2,3\", when add is called, then the result is 6"]}]

Note: commentary after the array.

Final JSON:
[{"title": "ignored copy", "story": "S", "acceptanceCriteria": ["Given x, when y, then z"]}]"#;
        let proposals = parse_proposals(reply);
        assert_eq!(proposals.len(), 2);
        assert_eq!(
            proposals[0].title,
            "Add two numbers from a comma-separated string"
        );
        assert_eq!(proposals[1].title, "Handle multiple numbers");
    }

    #[test]
    fn incomplete_proposals_are_dropped() {
        let reply = r#"[
            {"title": "", "story": "S", "acceptanceCriteria": ["Given x, when y, then z"]},
            {"title": "T", "story": " ", "acceptanceCriteria": ["Given x, when y, then z"]},
            {"title": "T", "story": "S", "acceptanceCriteria": []},
            {"title": "T", "story": "S", "acceptanceCriteria": ["  "]},
            {"title": "Kept", "story": "S", "acceptanceCriteria": ["Given x, when y, then z"]}
        ]"#;
        let proposals = parse_proposals(reply);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].title, "Kept");
    }
}

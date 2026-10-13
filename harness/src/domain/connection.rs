//! Which production file one requirement's work belongs in, when the
//! code cannot tell.
//!
//! [`crate::application::assets::production_path`] answers this from
//! evidence: the bodies of the step definitions the requirement's
//! scenarios run through, read for the production symbols they name.
//! That is the right answer whenever the scenarios reach the behavior,
//! and it refuses to guess when they reach nothing — a freshly
//! generated scenario whose steps are still placeholders names no code
//! at all, and every project past a kata has several files the work
//! could land in.
//!
//! The refusal used to end the matter: `spec implement` asked for
//! `--into` and an unattended run took the conventional path. Neither
//! connects the requirement to anything, so the next run re-derives the
//! same nothing. Here the question is put to the model instead, over
//! the requirement, its scenarios, and whatever code they do reach.
//!
//! The answer is a choice, never an invention: the caller lists the
//! files it is willing to write, and a reply naming anything else is
//! refused with the list. Resolution is pure — the prompt and the
//! validator both read values, never the filesystem — so every reply
//! worth handling is a unit test.

use crate::domain::generation::{decode_json, strip_code_fences};
use crate::domain::paths::confine;
use crate::domain::prompts::{RenderedPrompt, render};

/// How much of the scenarios' own code the question carries. Enough for
/// a step definition and the helper it delegates to; past that the code
/// crowds out the requirement it is evidence for.
const EVIDENCE_BUDGET: usize = 4_000;

/// How many existing files the answer may choose between. A project with
/// more production files than this has long since stopped being one
/// where any of them is obviously the answer, and a list that fills the
/// prompt buys nothing.
const MAX_CANDIDATES: usize = 60;

/// Everything the question needs. The requirement is what the work is
/// for; the scenarios and the code they reach are the evidence that was
/// not enough on its own.
pub struct ConnectionInput<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub story: &'a str,
    pub criteria: &'a [String],
    /// The requirement's own scenarios as Gherkin text.
    pub scenarios: &'a str,
    /// The bodies of the step definitions those scenarios run through.
    /// Empty when they are all still placeholders, which is the case
    /// this question exists for.
    pub evidence: &'a str,
}

/// The files the answer may name: every existing one the caller would
/// write, and the one it would create if none of them fits.
///
/// Kept together because the two are only meaningful as a pair — the
/// prompt has to mark which candidate does not exist yet, and the
/// validator has to accept it alongside the rest.
pub struct Candidates {
    /// Existing production files, capped and in a stable order, with
    /// [`Self::new_file`] last.
    offered: Vec<String>,
    new_file: String,
}

impl Candidates {
    /// `existing` is every production file the caller would let an
    /// attempt write; `new_file` is the path it would create instead.
    pub fn new(existing: &[String], new_file: &str) -> Self {
        let mut offered: Vec<String> = existing
            .iter()
            .filter(|path| *path != new_file)
            .cloned()
            .collect();
        offered.sort();
        offered.truncate(MAX_CANDIDATES);
        offered.push(new_file.to_string());
        Self {
            offered,
            new_file: new_file.to_string(),
        }
    }

    /// Every path the answer may name, in the order the prompt lists
    /// them.
    pub fn offered(&self) -> &[String] {
        &self.offered
    }

    /// The candidate that does not exist yet.
    pub fn new_file(&self) -> &str {
        &self.new_file
    }
}

/// Put the question to the model: this requirement, its scenarios, the
/// code they reach, and the files the answer may choose between.
pub fn connection_prompt(input: &ConnectionInput<'_>, candidates: &Candidates) -> RenderedPrompt {
    render(
        "connection",
        minijinja::context! {
            id => input.id,
            title => input.title,
            story => input.story,
            criteria => input.criteria,
            scenarios => input.scenarios.trim(),
            evidence => budgeted(input.evidence),
            candidates => candidates.offered(),
            new_file => candidates.new_file(),
        },
    )
}

/// `evidence` cut to [`EVIDENCE_BUDGET`] on a character boundary.
fn budgeted(evidence: &str) -> String {
    let evidence = evidence.trim();
    match evidence.char_indices().nth(EVIDENCE_BUDGET) {
        Some((at, _)) => evidence[..at].to_string(),
        None => evidence.to_string(),
    }
}

/// Parse the model's choice of production file, with a reason when the
/// reply cannot be used so the retry can tell the model what was wrong.
///
/// The answer has to be one of `candidates` verbatim. The caller already
/// decided which files an attempt is allowed to write, so the model is
/// choosing between them — never naming a test file, a path outside the
/// project, or a file no build compiles.
pub fn parse_connection_checked(reply: &str, candidates: &Candidates) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Reply {
        #[serde(
            default,
            alias = "productionFile",
            alias = "file",
            alias = "path",
            alias = "production"
        )]
        production_file: String,
    }

    let body = strip_code_fences(reply);
    let Some(Reply { production_file }) = decode_json::<Reply>(&body, '{') else {
        return Err(
            "the reply was not a JSON object of the form {\"productionFile\": \"...\"}".into(),
        );
    };
    if production_file.trim().is_empty() {
        return Err("the JSON object held no productionFile".into());
    }
    let confined = confine(&production_file)
        .map_err(|jail| format!("productionFile {production_file:?} is not usable - {jail}"))?;
    if candidates.offered().contains(&confined) {
        return Ok(confined);
    }
    Err(format!(
        "productionFile {confined:?} is not one of the candidates - answer with exactly one \
         of: {}",
        candidates.offered().join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Candidates {
        Candidates::new(
            &[
                "src/domain/tdd.rs".to_string(),
                "src/deliver.rs".to_string(),
            ],
            "src/explain.rs",
        )
    }

    fn input<'a>(scenarios: &'a str, evidence: &'a str) -> ConnectionInput<'a> {
        ConnectionInput {
            id: "REQ-001",
            title: "Explain command by name",
            story: "As a user, I want an explanation so that I know what a command does.",
            criteria: &[],
            scenarios,
            evidence,
        }
    }

    /// The new file goes last and is marked as such, so a model reading
    /// the list can tell the one choice that creates a file from the
    /// ones that extend something already there.
    #[test]
    fn the_prompt_carries_the_requirement_the_evidence_and_the_marked_new_file() {
        let criteria = ["Given a, when b, then 3".to_string()];
        let prompt = connection_prompt(
            &ConnectionInput {
                criteria: &criteria,
                ..input(
                    "  @REQ-001\n  Scenario: Explained\n    When asked\n",
                    "fn explain(name: &str) {}",
                )
            },
            &candidates(),
        );
        assert_eq!(prompt.section, "connection");
        assert!(prompt.user.contains("REQ-001: Explain command by name"));
        assert!(prompt.user.contains("- Given a, when b, then 3"));
        assert!(prompt.user.contains("Scenario: Explained"));
        assert!(prompt.user.contains("fn explain(name: &str) {}"));
        assert!(
            prompt.user.contains("- src/explain.rs (new file)"),
            "{}",
            prompt.user
        );
        assert!(
            prompt.user.contains("- src/deliver.rs\n"),
            "an existing candidate is not marked: {}",
            prompt.user
        );
        assert!(prompt.system.contains("ONLY a JSON object"));
        assert!(prompt.system.contains("Never invent a path"));
        assert!(
            prompt.system.contains("src/explain.rs does not exist yet"),
            "{}",
            prompt.system
        );
    }

    /// Scenarios whose steps are all placeholders reach no code, which
    /// is the case the question exists for. The prompt drops the empty
    /// sections rather than heading nothing.
    #[test]
    fn a_requirement_whose_scenarios_reach_no_code_asks_without_an_evidence_section() {
        let prompt = connection_prompt(&input("", ""), &candidates());
        assert!(!prompt.user.contains("Its scenarios:"), "{}", prompt.user);
        assert!(
            !prompt
                .user
                .contains("The code those scenarios run through:"),
            "{}",
            prompt.user
        );
        assert!(prompt.user.contains("Candidate production files:"));
    }

    /// A step definition that reaches half the crate would push the
    /// requirement out of the prompt it is evidence for.
    #[test]
    fn evidence_past_the_budget_is_cut_and_the_question_survives() {
        let huge = "fn reached() {}\n".repeat(2_000);
        let prompt = connection_prompt(&input("", &huge), &candidates());
        assert!(prompt.user.len() < huge.len(), "{}", prompt.user.len());
        assert!(prompt.user.contains("fn reached() {}"));
        assert!(prompt.user.contains("Candidate production files:"));
    }

    /// Multibyte evidence is cut on a character boundary, not a byte -
    /// a comment in any language is code to the budget.
    #[test]
    fn a_budget_that_lands_mid_character_does_not_split_it() {
        let wide = "é".repeat(EVIDENCE_BUDGET * 2);
        assert_eq!(budgeted(&wide).chars().count(), EVIDENCE_BUDGET);
    }

    /// Every shape a local model has actually replied in: bare, fenced,
    /// under an alias, and followed by commentary nobody asked for.
    #[test]
    fn a_candidate_parses_however_the_reply_is_dressed() {
        let candidates = candidates();
        for reply in [
            r#"{"productionFile": "src/deliver.rs"}"#,
            "```json\n{\"file\": \"src/deliver.rs\"}\n```",
            r#"{"path": "./src/deliver.rs"}"#,
            "{\"productionFile\": \"src/deliver.rs\"}\n\nNote: deliver.rs drives the loop.",
        ] {
            assert_eq!(
                parse_connection_checked(reply, &candidates).unwrap(),
                "src/deliver.rs",
                "{reply}"
            );
        }
    }

    /// The new file is a real choice, not decoration.
    #[test]
    fn the_new_file_is_a_choice_the_model_may_make() {
        assert_eq!(
            parse_connection_checked(r#"{"productionFile": "src/explain.rs"}"#, &candidates())
                .unwrap(),
            "src/explain.rs"
        );
    }

    /// The caller listed the files it is willing to write. Anything else
    /// is an implementation written into a module nobody chose, so the
    /// refusal quotes the list back for the retry.
    #[test]
    fn a_file_outside_the_candidates_is_refused_with_the_list() {
        let candidates = candidates();
        let reason = parse_connection_checked(r#"{"productionFile": "src/mcp.rs"}"#, &candidates)
            .unwrap_err();
        assert!(reason.contains("src/deliver.rs"), "{reason}");
        assert!(reason.contains("src/explain.rs"), "{reason}");
        assert!(
            parse_connection_checked(r#"{"productionFile": "/etc/passwd"}"#, &candidates)
                .unwrap_err()
                .contains("absolute paths are not allowed")
        );
        assert!(
            parse_connection_checked(r#"{"productionFile": "../elsewhere.rs"}"#, &candidates)
                .unwrap_err()
                .contains("could reach outside")
        );
    }

    #[test]
    fn prose_or_an_empty_choice_is_refused_with_the_shape() {
        let candidates = candidates();
        assert!(
            parse_connection_checked("src/deliver.rs, I think.", &candidates)
                .unwrap_err()
                .contains("productionFile")
        );
        assert!(
            parse_connection_checked(r#"{"productionFile": "  "}"#, &candidates)
                .unwrap_err()
                .contains("held no productionFile")
        );
    }

    /// The list is capped, stably ordered, and never drops the new file:
    /// truncation that lost it would leave a project with a hundred
    /// modules no way to answer "none of these".
    #[test]
    fn the_candidate_list_is_capped_and_keeps_the_new_file_last() {
        let many: Vec<String> = (0..MAX_CANDIDATES + 20)
            .map(|n| format!("src/m{n:03}.rs"))
            .collect();
        let candidates = Candidates::new(&many, "src/fresh.rs");
        assert_eq!(candidates.offered().len(), MAX_CANDIDATES + 1);
        assert_eq!(candidates.offered().first().unwrap(), "src/m000.rs");
        assert_eq!(candidates.offered().last().unwrap(), "src/fresh.rs");
    }

    /// The conventional path can already exist - a kata's `Kata.java`
    /// is both the existing production file and the one convention
    /// names. Offering it twice would read as two choices.
    #[test]
    fn a_new_file_that_already_exists_is_offered_once() {
        let candidates = Candidates::new(
            &["src/lib.rs".to_string(), "src/explain.rs".to_string()],
            "src/explain.rs",
        );
        assert_eq!(candidates.offered(), ["src/lib.rs", "src/explain.rs"]);
    }
}

//! The spec-driven workflow, written down: the states, the commands,
//! the loop, and the invariants, embedded from `prompts/workflow.md`.
//! Every advice prompt carries it as the known process, so the model
//! reasons from the real workflow instead of guessing it.

use serde::Serialize;

use crate::domain::prompts::{RenderedPrompt, render};

/// The textual process document - the single source of workflow wording
/// for prompts and advice calls.
pub const WORKFLOW_PROCESS: &str = include_str!("../../prompts/workflow.md");

/// The `bdd status` advice call: the workflow process plus the full
/// project state - phase, last run counts, staged changes, and every
/// requirement's position - so the model names the one next command.
pub fn next_step_prompt(
    phase: &str,
    last_run: impl Serialize,
    staged: impl Serialize,
    requirements: impl Serialize,
) -> RenderedPrompt {
    render(
        "next_step",
        minijinja::context! {
            workflow => WORKFLOW_PROCESS,
            phase,
            last_run,
            staged,
            requirements,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Run {
        tests: u32,
        failures: u32,
        errors: u32,
        skipped: u32,
    }

    #[derive(Serialize)]
    struct Change {
        path: String,
        action: String,
        summary: String,
    }

    #[derive(Serialize)]
    struct Position {
        id: String,
        title: String,
        status: String,
        findings: Vec<String>,
    }

    #[test]
    fn the_workflow_document_names_the_loop_and_the_invariants() {
        assert!(WORKFLOW_PROCESS.contains("THE LOOP FOR ONE REQUIREMENT"));
        assert!(WORKFLOW_PROCESS.contains("Never refactor on RED"));
        assert!(WORKFLOW_PROCESS.contains("bdd spec mark-implemented"));
        assert!(WORKFLOW_PROCESS.contains("outside-in double loop"));
    }

    #[test]
    fn the_next_step_prompt_carries_the_process_and_the_whole_state() {
        let prompt = next_step_prompt(
            "GREEN",
            Run {
                tests: 6,
                failures: 0,
                errors: 0,
                skipped: 0,
            },
            vec![Change {
                path: "requirements/requirements.json".into(),
                action: "modify".into(),
                summary: "mark REQ-001 implemented".into(),
            }],
            vec![Position {
                id: "REQ-001".into(),
                title: "Adds two numbers".into(),
                status: "pending".into(),
                findings: vec!["No scenario is tagged @REQ-001".into()],
            }],
        );
        assert!(prompt.system.contains("THE LOOP FOR ONE REQUIREMENT"));
        assert!(prompt.system.contains("exact next command"));
        assert!(prompt.user.contains("The TDD phase: GREEN"));
        assert!(
            prompt
                .user
                .contains("tests=6 failures=0 errors=0 skipped=0")
        );
        assert!(
            prompt
                .user
                .contains("- requirements/requirements.json (modify): mark REQ-001 implemented")
        );
        assert!(
            prompt
                .user
                .contains("- REQ-001 \"Adds two numbers\" status=pending")
        );
        assert!(prompt.user.contains("gaps: No scenario is tagged @REQ-001"));
    }

    #[test]
    fn an_empty_state_reads_as_nothing_staged() {
        let prompt = next_step_prompt(
            "START",
            Run {
                tests: 0,
                failures: 0,
                errors: 0,
                skipped: 0,
            },
            Vec::<Change>::new(),
            Vec::<Position>::new(),
        );
        assert!(prompt.user.contains("Nothing is staged."));
        assert!(!prompt.user.contains("Staged files awaiting review"));
    }
}

//! One artefact at a time.
//!
//! A deterministic examiner accepts the unit or sends that unit back.
//! The decision model is asked only when the examiner cannot decide.
//! A rework is the reason [`crate::application::agent_service::Agent::ask`]
//! already retries, so the correction covers this unit and nothing else.

use crate::application::decision_service::{Brief, TaskJudge};
use crate::domain::decision::{Judgment, Policy, TaskGate, Transition, steps_state};
use crate::domain::steps::compile_pattern;
use crate::ports::DecisionError;

/// What an examiner made of one unit.
pub(crate) enum Exam {
    /// The deterministic check holds, or the decision model did not rework.
    Accept,
    /// The writing model gets this reason back, for this unit only.
    Retry(String),
    /// The deterministic check cannot settle it. One brief, one gate.
    Ask(Brief),
}

/// Fold an exam into the `Result` the writing-model retry already understands.
///
/// `Accept` keeps the unit. `Retry` is the correction. `Ask` calls `judge`
/// when one is attached; a `REWORK` becomes `Retry` with the gate's finding,
/// and `Continue` — the advisory default — keeps the unit and the judgment.
/// With no judge, an undecided exam is accepted: there is nothing to ask.
pub(crate) fn settle(
    exam: Exam,
    recorded: &mut Vec<Judgment>,
    judge: Option<&dyn TaskJudge>,
    policy: Policy,
    gate: &'static TaskGate,
) -> Result<(), String> {
    match exam {
        Exam::Accept => Ok(()),
        Exam::Retry(reason) => Err(reason),
        Exam::Ask(brief) => settle_ask(brief, recorded, judge, policy, gate),
    }
}

fn settle_ask(
    brief: Brief,
    recorded: &mut Vec<Judgment>,
    judge: Option<&dyn TaskJudge>,
    policy: Policy,
    gate: &'static TaskGate,
) -> Result<(), String> {
    let Some(judge) = judge else {
        return Ok(());
    };
    match judge.judge_task(gate, &brief.input, brief.state) {
        Ok(judgment) => {
            let reworks = judgment.action == Transition::Rework;
            let finding = gate.finding(&brief.input, &judgment);
            recorded.push(judgment);
            if reworks {
                Err(finding.expect("a reworking judgment has a finding"))
            } else {
                Ok(())
            }
        }
        Err(error) if policy.tolerates_silence() => {
            tracing::warn!(
                gate = gate.gate,
                error = %error,
                "no judgment: accepting the deterministic verdict"
            );
            Ok(())
        }
        Err(error) => Err(silence_is_a_refusal(gate, &error)),
    }
}

fn silence_is_a_refusal(gate: &TaskGate, error: &DecisionError) -> String {
    format!(
        "{} could not be judged ({error}) and this gate is enforcing - \
         set [decision.gates.{}] mode = \"advisory\" to carry on without it",
        gate.gate, gate.gate
    )
}

/// Whether `expression` binds `step_text` — the step with its keyword removed.
///
/// A match is accepted and the writing model is not asked. A pattern that
/// compiles and does not match is sent back. A pattern that will not compile
/// is the one case the matcher has nothing to say about, so it becomes the
/// `steps_bind` brief.
pub(crate) fn expression_exam(step_text: &str, expression: &str, line: &str) -> Exam {
    match compile_pattern(expression) {
        Some(matcher) if matcher.is_match(step_text) => Exam::Accept,
        Some(_) => Exam::Retry(format!(
            "the expression {expression:?} does not match the step line {line:?}"
        )),
        None => Exam::Ask(Brief::new(
            format!("step {line:?}"),
            steps_state(line, expression),
        )),
    }
}

/// A filled step body either replaced its placeholder or it did not.
/// The decision model is not asked: a placeholder is visible in the text.
pub(crate) fn placeholder_exam(pattern: &str, still_pending: bool) -> Exam {
    if still_pending {
        Exam::Retry(format!(
            "the definition for {pattern:?} is still a placeholder"
        ))
    } else {
        Exam::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::decision::{Answer, Mode, Provenance, STEPS_BIND_SCENARIO, Usage};
    use crate::domain::steps::step_to_expression;
    use crate::ports::DecisionError;

    struct ScriptedJudge {
        noul: f64,
        mode: Mode,
    }

    impl TaskJudge for ScriptedJudge {
        fn judge_task(
            &self,
            gate: &'static TaskGate,
            input: &str,
            state: serde_json::Value,
        ) -> Result<Judgment, DecisionError> {
            Ok(Policy::new(self.mode, 0.7).judge(
                gate,
                "test",
                Answer::Noul { noul: self.noul },
                Provenance {
                    input: input.to_string(),
                    state: state.to_string(),
                    state_bytes: state.to_string().len(),
                },
                Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
            ))
        }
    }

    #[test]
    fn a_matching_expression_is_accepted_without_a_question() {
        let text = "a requirement with zero acceptance criteria (an empty list)";
        let expression = step_to_expression(text);
        let line = format!("Given {text}");
        assert!(matches!(
            expression_exam(text, &expression, &line),
            Exam::Accept
        ));
    }

    #[test]
    fn an_expression_that_misses_the_line_is_sent_back_alone() {
        let exam = expression_exam(
            "the result is ready",
            "the result is {int}",
            "Then the result is ready",
        );
        match exam {
            Exam::Retry(reason) => {
                assert!(reason.contains("the result is {int}"), "{reason}");
                assert!(reason.contains("the result is ready"), "{reason}");
            }
            Exam::Accept => panic!("expected a retry, got an acceptance"),
            Exam::Ask(_) => panic!("expected a retry, got a question"),
        }
    }

    #[test]
    fn an_expression_that_will_not_compile_is_a_single_brief() {
        let exam = expression_exam("anything", r"^bro][ken$", "Then anything");
        assert!(matches!(exam, Exam::Ask(_)));
    }

    #[test]
    fn an_enforcing_refusal_retries_and_an_advisory_one_keeps_the_unit() {
        let mut recorded = Vec::new();
        let error = settle(
            Exam::Ask(Brief::new("step", serde_json::json!({}))),
            &mut recorded,
            Some(&ScriptedJudge {
                noul: 0.05,
                mode: Mode::Enforce,
            }),
            Policy::new(Mode::Enforce, 0.7),
            &STEPS_BIND_SCENARIO,
        )
        .unwrap_err();
        assert!(error.contains("step expression"), "{error}");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].action, Transition::Rework);

        let mut recorded = Vec::new();
        settle(
            Exam::Ask(Brief::new("step", serde_json::json!({}))),
            &mut recorded,
            Some(&ScriptedJudge {
                noul: 0.05,
                mode: Mode::Advisory,
            }),
            Policy::new(Mode::Advisory, 0.7),
            &STEPS_BIND_SCENARIO,
        )
        .unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].action, Transition::Continue);
    }

    #[test]
    fn a_placeholder_that_is_still_there_is_sent_back() {
        assert!(matches!(
            placeholder_exam("the result is {int}", true),
            Exam::Retry(_)
        ));
        assert!(matches!(
            placeholder_exam("the result is {int}", false),
            Exam::Accept
        ));
    }
}

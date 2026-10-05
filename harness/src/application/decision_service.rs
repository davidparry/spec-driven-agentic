//! The judgment use case: put a bounded question to the configured
//! decision model and record what came back.
//!
//! The service owns one rule above all others, and it is the reason the
//! failure paths are as long as the success path: **a request that did
//! not produce an answer is never an approval.** Advisory mode degrades
//! to no judgment at all and leaves every deterministic verdict exactly
//! as it found it. Enforcing mode refuses. Neither has a branch where a
//! timeout, a missing model, or a malformed reply reads as "looks fine".

use serde::Serialize;

use crate::application::spec_service::RefinementReport;
use crate::domain::decision::{
    CRITERION_MEASURABLE, Judgment, MEASURABLE_ANSWER, MEASURABLE_QUESTION, Mode, Policy, Request,
    Transition, Verdict, measurable_finding, measurable_question, measurable_state,
};
use crate::ports::{DecisionError, DecisionModel};

/// The configured decision model and the policy its answers are read
/// against.
///
/// `Clone` so an async caller can hand a copy to a blocking thread.
/// Cloning an [`crate::adapters::ollama_decision::OllamaDecision`]
/// shares one connection pool, so this is cheap and keeps keep-alive
/// working across judgments.
#[derive(Clone)]
pub struct DecisionService<D: DecisionModel> {
    model: String,
    client: D,
    policy: Policy,
}

/// What a refinement pass learned from the decision model, in the shape
/// a caller renders.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CriteriaReview {
    pub judgments: Vec<Judgment>,
    /// The strongest transition any one judgment asked for. Always
    /// `CONTINUE` in advisory mode.
    pub action: Transition,
    /// The advisory lines a human reads, one per criterion the model
    /// read as unmeasurable. Deliberately not merged into the
    /// refinement's own findings — see [`DecisionService::review_criteria`].
    pub advisories: Vec<String>,
}

impl<D: DecisionModel> DecisionService<D> {
    pub fn new(model: String, client: D, policy: Policy) -> Self {
        Self {
            model,
            client,
            policy,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// The same service with its mode weakened to advisory.
    ///
    /// For surfaces that report a judgment but must never gate on one,
    /// whatever the project configured. Weakening only: a mode of `off`
    /// stays off, because a project that turned judgment off did not ask
    /// for it anywhere.
    pub fn into_advisory(self) -> Self {
        let mode = match self.policy.mode {
            Mode::Off => Mode::Off,
            Mode::Advisory | Mode::Enforce => Mode::Advisory,
        };
        Self {
            policy: Policy::new(mode, self.policy.threshold),
            ..self
        }
    }

    /// One bounded judgment: can this acceptance criterion be checked by
    /// a test with a single unambiguous result?
    ///
    /// `input` names what was summarized, for the provenance record.
    pub fn judge_criterion(&self, input: &str, criterion: &str) -> Result<Judgment, DecisionError> {
        let request = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            measurable_state(criterion),
        );
        let provenance = crate::domain::decision::Provenance {
            input: input.to_string(),
            state: request.state_digest(),
            state_bytes: request.state.to_string().len(),
        };
        let outcome = self.client.decide(&self.model, &request)?;
        let answer = outcome
            .answers
            .get(MEASURABLE_ANSWER)
            .cloned()
            // The adapter already refused a mismatched answer set, so
            // reaching here would be a bug rather than a bad reply.
            .ok_or_else(|| DecisionError::UnexpectedAnswers {
                missing: vec![MEASURABLE_ANSWER.to_string()],
                unexpected: outcome.answers.keys().cloned().collect(),
            })?;
        let judgment = self.policy.judge(
            CRITERION_MEASURABLE,
            MEASURABLE_QUESTION,
            // The tag the server echoed, not the one that was asked
            // for: `nimble` and `nimble:latest` are one request and two
            // different records.
            &outcome.model,
            answer,
            provenance,
            outcome.usage,
        );
        tracing::info!(
            gate = judgment.gate,
            question = judgment.question,
            model = %judgment.model,
            verdict = %judgment.verdict,
            action = %judgment.action,
            mode = %judgment.mode,
            threshold = judgment.threshold,
            answer = %judgment.answer.summary(),
            input = %judgment.provenance.input,
            state = %judgment.provenance.state,
            "decision judgment recorded"
        );
        Ok(judgment)
    }

    /// Every criterion of one requirement, judged.
    ///
    /// The findings this produces are kept out of the refinement's own
    /// `findings` list on purpose. `clean` is a deterministic verdict on
    /// deterministic rules, and quietly folding a probability into it
    /// would make the one field a reader trusts mean something new
    /// without saying so. The judgments travel beside it, labelled, and
    /// the deterministic answer is still whatever it was.
    pub fn review_criteria(
        &self,
        id: &str,
        criteria: &[String],
    ) -> Result<CriteriaReview, DecisionError> {
        let mut judgments = Vec::new();
        let mut advisories = Vec::new();
        let mut action = Transition::Continue;
        for (index, criterion) in criteria.iter().enumerate() {
            // Numbered only when there is more than one, so provenance
            // for a single criterion reads as what it is.
            let input = if criteria.len() == 1 {
                id.to_string()
            } else {
                format!("{id} acceptance criterion {}", index + 1)
            };
            let judgment = self.judge_criterion(&input, criterion)?;
            if judgment.verdict == Verdict::Fails {
                advisories.push(measurable_finding(criterion, &judgment));
            }
            action = stronger(action, judgment.action);
            judgments.push(judgment);
        }
        Ok(CriteriaReview {
            judgments,
            action,
            advisories,
        })
    }

    /// Attach the judgments for `criteria` to a refinement report.
    ///
    /// Returns the transition the harness should act on. In advisory
    /// mode a failed decision request is recorded as a note and the
    /// report is otherwise untouched; in enforcing mode it is an error,
    /// because the alternative is letting an unreachable model read as
    /// agreement.
    pub fn judge_refinement(
        &self,
        report: &mut RefinementReport,
        criteria: &[String],
    ) -> Result<Transition, DecisionError> {
        // `off` is the one mode that asks nothing. Checked here rather
        // than at each call site so no future caller can forget it.
        if !self.policy.asks() {
            return Ok(Transition::Continue);
        }
        let review = self.review_criteria(&report.id, criteria);
        apply_review(self.policy, report, review)
    }
}

/// Records a review on a refinement report, deciding what a failure to
/// get one means.
///
/// A free function taking the policy rather than a method, because the
/// decision model plays no part: a caller that did the asking on another
/// thread has a review and a policy but no client. There is exactly one
/// place that decides what an unanswered question does, and it is here.
pub fn apply_review(
    policy: Policy,
    report: &mut RefinementReport,
    review: Result<CriteriaReview, DecisionError>,
) -> Result<Transition, DecisionError> {
    let before = report.findings.clone();
    match review {
        Ok(review) => {
            report.judgment_advisories = review.advisories;
            report.judgments = review.judgments;
            report.judgment_action = Some(review.action);
            debug_assert_eq!(before, report.findings, "a judgment never edits a finding");
            Ok(review.action)
        }
        Err(error) if policy.mode == Mode::Enforce => Err(error),
        Err(error) => {
            tracing::warn!(error = %error, "no judgment: carrying on with the deterministic verdict");
            report.judgment_note = Some(format!("no judgment - {error}"));
            Ok(Transition::Continue)
        }
    }
}

/// The transition that wins when several judgments disagree about what
/// should happen. Ordered by how much it interrupts: carrying on loses
/// to everything, stopping beats everything.
fn stronger(left: Transition, right: Transition) -> Transition {
    fn rank(transition: Transition) -> u8 {
        match transition {
            Transition::Continue => 0,
            Transition::Rework => 1,
            Transition::Escalate => 2,
            Transition::Stop => 3,
        }
    }
    if rank(right) > rank(left) {
        right
    } else {
        left
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::decision::{Answer, Outcome, Usage};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A scripted decision model. Records every request so a test can
    /// assert what was actually asked, which is the only way to tell a
    /// narrow question from a broad one.
    struct ScriptedDecision {
        replies: RefCell<Vec<Result<Outcome, DecisionError>>>,
        asked: RefCell<Vec<Request>>,
    }

    impl ScriptedDecision {
        fn answering(probabilities: &[f64]) -> Self {
            Self {
                replies: RefCell::new(
                    probabilities
                        .iter()
                        .map(|p| {
                            Ok(Outcome {
                                model: "nimble:test".into(),
                                answers: BTreeMap::from([(
                                    MEASURABLE_ANSWER.to_string(),
                                    Answer::Noul { noul: *p },
                                )]),
                                usage: Usage {
                                    input_tokens: 151,
                                    output_tokens: 1,
                                },
                            })
                        })
                        .collect(),
                ),
                asked: RefCell::new(Vec::new()),
            }
        }

        fn failing(error: DecisionError) -> Self {
            Self {
                replies: RefCell::new(vec![Err(error)]),
                asked: RefCell::new(Vec::new()),
            }
        }
    }

    impl DecisionModel for ScriptedDecision {
        fn decide(&self, _model: &str, request: &Request) -> Result<Outcome, DecisionError> {
            self.asked.borrow_mut().push(request.clone());
            let mut replies = self.replies.borrow_mut();
            if replies.is_empty() {
                return Err(DecisionError::Unavailable("script exhausted".into()));
            }
            replies.remove(0)
        }
    }

    fn service(client: ScriptedDecision, mode: Mode) -> DecisionService<ScriptedDecision> {
        DecisionService::new("nimble:test".into(), client, Policy::new(mode, 0.70))
    }

    fn report() -> RefinementReport {
        RefinementReport {
            id: "REQ-007".into(),
            clean: true,
            findings: Vec::new(),
            source: crate::application::spec_service::WORKING_TREE,
            next_step: "unchanged".into(),
            judgments: Vec::new(),
            judgment_advisories: Vec::new(),
            judgment_action: None,
            judgment_note: None,
        }
    }

    #[test]
    fn a_confident_yes_is_a_recorded_judgment_that_holds() {
        let service = service(ScriptedDecision::answering(&[0.97]), Mode::Advisory);
        let judgment = service
            .judge_criterion(
                "REQ-007 acceptance criterion 1",
                "Given \"1,2\", when add is called, then the result is 3",
            )
            .unwrap();
        assert_eq!(judgment.verdict, Verdict::Holds);
        assert_eq!(judgment.action, Transition::Continue);
        assert_eq!(judgment.model, "nimble:test");
        assert_eq!(judgment.question, "measurable/v1");
        assert_eq!(judgment.gate, "CRITERION_MEASURABLE");
        assert_eq!(judgment.usage.input_tokens, 151);
        assert_eq!(judgment.provenance.input, "REQ-007 acceptance criterion 1");
        assert!(judgment.provenance.state.starts_with("sha256:"));
        assert!(judgment.provenance.state_bytes > 0);
    }

    /// The brief is the criterion and nothing else: the question is
    /// about this sentence's wording, so the rest of the spec is tokens
    /// competing for a context the server will not truncate.
    #[test]
    fn the_question_carries_only_the_criterion_being_judged() {
        let service = service(ScriptedDecision::answering(&[0.9]), Mode::Advisory);
        service
            .judge_criterion("REQ-007 criterion 1", "then the result is 3")
            .unwrap();
        let asked = service.client.asked.borrow();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].questions.len(), 1, "one bounded question");
        assert_eq!(
            asked[0].state,
            serde_json::json!({"acceptance_criterion": "then the result is 3"})
        );
    }

    #[test]
    fn a_confident_no_is_a_recorded_judgment_that_fails() {
        let service = service(ScriptedDecision::answering(&[0.04]), Mode::Advisory);
        let judgment = service
            .judge_criterion("REQ-007 criterion 1", "then it works")
            .unwrap();
        assert_eq!(judgment.verdict, Verdict::Fails);
        assert_eq!(
            judgment.action,
            Transition::Continue,
            "advisory mode records the answer and carries on"
        );
    }

    #[test]
    fn an_answer_in_the_dead_band_is_inconclusive() {
        let service = service(ScriptedDecision::answering(&[0.55]), Mode::Advisory);
        let judgment = service
            .judge_criterion("REQ-007 criterion 1", "then it is valid")
            .unwrap();
        assert_eq!(judgment.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn every_criterion_of_a_requirement_is_judged_and_the_failures_are_advised() {
        let service = service(ScriptedDecision::answering(&[0.97, 0.03]), Mode::Advisory);
        let review = service
            .review_criteria(
                "REQ-007",
                &[
                    "Given \"1,2\", when add is called, then the result is 3".into(),
                    "Given a request, when it is served, then the response completes before the user notices".into(),
                ],
            )
            .unwrap();
        assert_eq!(review.judgments.len(), 2);
        assert_eq!(review.judgments[0].verdict, Verdict::Holds);
        assert_eq!(review.judgments[1].verdict, Verdict::Fails);
        assert_eq!(review.advisories.len(), 1, "only the failure is advised");
        assert!(review.advisories[0].contains("completes before the user notices"));
        assert!(review.advisories[0].starts_with("judgment (measurable/v1):"));
        assert_eq!(review.action, Transition::Continue);
        assert_eq!(
            review.judgments[1].provenance.input, "REQ-007 acceptance criterion 2",
            "provenance numbers the criteria as a human would"
        );
    }

    #[test]
    fn a_requirement_with_no_criteria_asks_nothing() {
        let service = service(ScriptedDecision::answering(&[]), Mode::Advisory);
        let review = service.review_criteria("REQ-007", &[]).unwrap();
        assert!(review.judgments.is_empty());
        assert!(review.advisories.is_empty());
        assert_eq!(review.action, Transition::Continue);
        assert!(service.client.asked.borrow().is_empty());
    }

    /// The property advisory mode exists to guarantee.
    #[test]
    fn an_advisory_judgment_leaves_every_deterministic_field_alone() {
        let service = service(ScriptedDecision::answering(&[0.02]), Mode::Advisory);
        let mut report = report();
        report.findings = vec!["criteria: only happy paths".into()];
        report.clean = false;
        let action = service
            .judge_refinement(&mut report, &["then it works".into()])
            .unwrap();
        assert_eq!(action, Transition::Continue);
        assert_eq!(report.findings, vec!["criteria: only happy paths"]);
        assert!(!report.clean);
        assert_eq!(report.next_step, "unchanged");
        assert_eq!(report.judgments.len(), 1);
        assert_eq!(report.judgment_advisories.len(), 1);
        assert_eq!(report.judgment_action, Some(Transition::Continue));
        assert_eq!(report.judgment_note, None);
    }

    /// A clean deterministic verdict stays clean. The judgment is
    /// reported next to it, and `clean` does not quietly change meaning.
    #[test]
    fn a_failing_judgment_does_not_make_a_clean_requirement_unclean() {
        let service = service(ScriptedDecision::answering(&[0.01]), Mode::Advisory);
        let mut report = report();
        service
            .judge_refinement(&mut report, &["then it works".into()])
            .unwrap();
        assert!(report.clean, "the deterministic verdict is untouched");
        assert!(report.findings.is_empty());
        assert_eq!(report.judgments[0].verdict, Verdict::Fails);
    }

    #[test]
    fn an_unavailable_model_in_advisory_mode_is_a_note_and_nothing_else() {
        let service = service(
            ScriptedDecision::failing(DecisionError::Unavailable("connection refused".into())),
            Mode::Advisory,
        );
        let mut report = report();
        let action = service
            .judge_refinement(&mut report, &["then the result is 3".into()])
            .unwrap();
        assert_eq!(action, Transition::Continue);
        assert!(report.judgments.is_empty());
        assert!(report.judgment_advisories.is_empty());
        assert_eq!(report.judgment_action, None);
        let note = report.judgment_note.unwrap();
        assert!(note.starts_with("no judgment - "), "got: {note}");
        assert!(note.contains("connection refused"));
        assert!(report.clean, "the deterministic verdict still stands");
    }

    /// The failure mode that matters most: an enforcing gate whose model
    /// is down must refuse, not wave the work through.
    #[test]
    fn an_unavailable_model_in_enforce_mode_refuses_rather_than_approving() {
        for error in [
            DecisionError::Unavailable("connection refused".into()),
            DecisionError::Timeout { seconds: 60 },
            DecisionError::Malformed("not json".into()),
            DecisionError::ModelMissing {
                model: "absent".into(),
            },
            DecisionError::NotADecisionModel {
                model: "chatter".into(),
                detail: "does not support decision".into(),
            },
            DecisionError::Unsupported {
                endpoint: "http://localhost:11434".into(),
            },
        ] {
            let service = service(ScriptedDecision::failing(error.clone()), Mode::Enforce);
            let mut report = report();
            let refused = service
                .judge_refinement(&mut report, &["then the result is 3".into()])
                .unwrap_err();
            assert_eq!(refused, error);
            assert!(
                report.judgments.is_empty() && report.judgment_action.is_none(),
                "no verdict is recorded for a request that did not answer"
            );
        }
    }

    #[test]
    fn enforce_mode_asks_for_rework_on_a_confident_failure() {
        let service = service(ScriptedDecision::answering(&[0.02]), Mode::Enforce);
        let mut report = report();
        let action = service
            .judge_refinement(&mut report, &["then it works".into()])
            .unwrap();
        assert_eq!(action, Transition::Rework);
        assert_eq!(report.judgment_action, Some(Transition::Rework));
        assert!(
            report.clean,
            "even enforcing, the gate is not the clean flag"
        );
    }

    #[test]
    fn enforce_mode_escalates_an_inconclusive_answer_to_a_human() {
        let service = service(ScriptedDecision::answering(&[0.5]), Mode::Enforce);
        let mut report = report();
        let action = service
            .judge_refinement(&mut report, &["then it is valid".into()])
            .unwrap();
        assert_eq!(action, Transition::Escalate);
    }

    /// Several criteria, several answers: the most interrupting
    /// transition wins rather than the last one read.
    #[test]
    fn the_strongest_transition_across_criteria_is_the_one_reported() {
        let service = service(
            ScriptedDecision::answering(&[0.97, 0.02, 0.5]),
            Mode::Enforce,
        );
        let review = service
            .review_criteria("REQ-007", &["a".into(), "b".into(), "c".into()])
            .unwrap();
        assert_eq!(review.action, Transition::Escalate);
    }

    #[test]
    fn transition_strength_is_ordered_by_how_much_it_interrupts() {
        assert_eq!(
            stronger(Transition::Continue, Transition::Rework),
            Transition::Rework
        );
        assert_eq!(
            stronger(Transition::Escalate, Transition::Rework),
            Transition::Escalate
        );
        assert_eq!(
            stronger(Transition::Stop, Transition::Escalate),
            Transition::Stop
        );
        assert_eq!(
            stronger(Transition::Continue, Transition::Continue),
            Transition::Continue
        );
    }

    #[test]
    fn a_reply_answering_a_different_question_is_refused_by_the_service_too() {
        let client = ScriptedDecision {
            replies: RefCell::new(vec![Ok(Outcome {
                model: "nimble:test".into(),
                answers: BTreeMap::from([("urgency".to_string(), Answer::Noul { noul: 0.9 })]),
                usage: Usage::default(),
            })]),
            asked: RefCell::new(Vec::new()),
        };
        let error = service(client, Mode::Advisory)
            .judge_criterion("REQ-007 criterion 1", "then the result is 3")
            .unwrap_err();
        assert!(
            matches!(&error, DecisionError::UnexpectedAnswers { missing, .. }
                if missing == &["measurable".to_string()]),
            "got {error:?}"
        );
    }

    #[test]
    fn mode_off_asks_nothing_even_when_a_model_is_configured() {
        let service = service(ScriptedDecision::answering(&[0.01]), Mode::Off);
        let mut report = report();
        let action = service
            .judge_refinement(&mut report, &["then it works".into()])
            .unwrap();
        assert_eq!(action, Transition::Continue);
        assert!(report.judgments.is_empty());
        assert_eq!(report.judgment_note, None);
        assert!(
            service.client.asked.borrow().is_empty(),
            "no request is made at all"
        );
    }

    #[test]
    fn the_service_reports_the_model_and_policy_it_was_built_with() {
        let service = service(ScriptedDecision::answering(&[]), Mode::Enforce);
        assert_eq!(service.model(), "nimble:test");
        assert_eq!(service.policy().mode, Mode::Enforce);
        assert_eq!(service.policy().threshold, 0.70);
    }

    /// The MCP surface reports judgments but must never gate on one, so
    /// it weakens an enforcing project to advisory. Weakening only: a
    /// project that turned judgment off asked for it nowhere.
    #[test]
    fn weakening_to_advisory_lowers_enforce_and_leaves_off_alone() {
        for (configured, expected) in [
            (Mode::Enforce, Mode::Advisory),
            (Mode::Advisory, Mode::Advisory),
            (Mode::Off, Mode::Off),
        ] {
            let weakened = service(ScriptedDecision::answering(&[]), configured).into_advisory();
            assert_eq!(weakened.policy().mode, expected, "from {configured}");
            assert_eq!(weakened.policy().threshold, 0.70, "the threshold is kept");
            assert_eq!(weakened.model(), "nimble:test");
        }
    }

    /// When several criteria disagree about what should happen, the most
    /// interrupting answer wins. Carrying on can never overrule a
    /// request for rework just because it was judged later.
    #[test]
    fn the_most_interrupting_transition_wins_a_disagreement() {
        use Transition::{Continue, Escalate, Rework, Stop};
        assert_eq!(stronger(Continue, Rework), Rework);
        assert_eq!(stronger(Rework, Continue), Rework);
        assert_eq!(stronger(Rework, Escalate), Escalate);
        assert_eq!(stronger(Escalate, Rework), Escalate);
        assert_eq!(stronger(Escalate, Stop), Stop);
        assert_eq!(stronger(Stop, Continue), Stop);
        assert_eq!(stronger(Continue, Continue), Continue);
    }
}

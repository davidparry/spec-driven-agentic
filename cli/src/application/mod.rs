//! Use-case services. Each service is composed from domain logic and
//! [`crate::ports`] traits through constructor injection; no service names
//! a concrete adapter.

pub(crate) mod assets;
pub mod change_service;
pub mod command_service;
pub mod generation_service;
pub mod implement_service;
pub mod init_service;
pub mod inspect_service;
pub mod memory_service;
pub mod model_service;
pub mod scenario_service;
pub mod spec_mutation_service;
pub mod spec_service;
pub mod status_service;
pub mod tdd_service;

use crate::domain::prompts::{RenderedPrompt, correction_user};
use crate::ports::{LlmError, LlmGenerator};

/// How many times a model call is tried when the reply fails
/// validation. Overridden by `--retry` or `[llm] retry` in
/// `.bdd-mcp.toml`.
pub const DEFAULT_LLM_ATTEMPTS: u32 = 3;

/// A model round trip that either never reached a reply, or whose
/// reply failed validation after every attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LlmReplyError {
    Call(LlmError),
    Invalid { reason: String },
}

/// One fully logged LLM round trip, used by every service that calls a
/// model. The complete system and user prompts go to the debug log
/// before the call and the entire reply (or the failure) after it, each
/// line tagged with the prompt-catalog section that rendered the message
/// (e.g. `[proposal]`, `[rewording]`), so the log names the template
/// behind every exchange.
pub(crate) fn generate_logged<L: LlmGenerator + ?Sized>(
    llm: &L,
    model: &str,
    prompt: &RenderedPrompt,
) -> Result<String, LlmError> {
    tracing::debug!(
        model,
        system = %prompt.system,
        user = %prompt.user,
        "[{}] LLM request",
        prompt.section
    );
    let reply = llm.generate(model, &prompt.system, &prompt.user);
    match &reply {
        Ok(text) if text.trim().is_empty() => {
            tracing::warn!("[{}] LLM returned an empty reply", prompt.section);
        }
        Ok(text) => tracing::debug!(
            bytes = text.len(),
            reply = %text,
            "[{}] LLM response",
            prompt.section
        ),
        Err(error) => tracing::debug!(
            error = %error.0,
            "[{}] LLM call failed",
            prompt.section
        ),
    }
    reply
}

/// Generate until the reply passes `parse`, up to `attempts` times.
/// Each retry keeps the original user prompt and appends the invalid
/// reply plus the reason, so the model can correct itself. Transport
/// failures are not retried. `on_retry` is told before attempts 2..N
/// (`attempt` is 1-based, `of` is the configured maximum).
pub(crate) fn generate_valid<T, L: LlmGenerator + ?Sized>(
    llm: &L,
    model: &str,
    prompt: &RenderedPrompt,
    attempts: u32,
    parse: impl Fn(&str) -> Result<T, String>,
    mut on_retry: impl FnMut(u32, u32, &str),
) -> Result<T, LlmReplyError> {
    let of = attempts.max(1);
    let mut user = prompt.user.clone();
    let mut last_reason = String::from("the reply was invalid");
    for attempt in 1..=of {
        let current = RenderedPrompt {
            section: prompt.section.clone(),
            system: prompt.system.clone(),
            user: user.clone(),
        };
        let reply = generate_logged(llm, model, &current).map_err(LlmReplyError::Call)?;
        match parse(&reply) {
            Ok(value) => return Ok(value),
            Err(reason) => {
                last_reason = reason;
                if attempt < of {
                    tracing::warn!(
                        attempt,
                        of,
                        reason = %last_reason,
                        "[{}] invalid LLM reply, retrying",
                        prompt.section
                    );
                    on_retry(attempt + 1, of, &last_reason);
                    user = format!(
                        "{}\n\n{}",
                        prompt.user,
                        correction_user(&last_reason, &reply)
                    );
                }
            }
        }
    }
    Err(LlmReplyError::Invalid {
        reason: format!("{last_reason} (after {of} attempts)"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::LlmError;
    use std::cell::RefCell;

    struct QueueLlm {
        replies: RefCell<Vec<Result<String, LlmError>>>,
        prompts: RefCell<Vec<String>>,
    }

    impl LlmGenerator for QueueLlm {
        fn generate(&self, _model: &str, _system: &str, user: &str) -> Result<String, LlmError> {
            self.prompts.borrow_mut().push(user.to_string());
            let mut replies = self.replies.borrow_mut();
            if replies.is_empty() {
                return Err(LlmError("script exhausted".into()));
            }
            replies.remove(0)
        }
    }

    fn prompt() -> RenderedPrompt {
        RenderedPrompt {
            section: "proposal".into(),
            system: "reply JSON".into(),
            user: "split this".into(),
        }
    }

    fn parse_ok(text: &str) -> Result<String, String> {
        if text.starts_with('[') {
            Ok(text.to_string())
        } else {
            Err("not a JSON array".into())
        }
    }

    #[test]
    fn a_valid_first_reply_is_returned_without_retrying() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![Ok("[ok]".into())]),
            prompts: RefCell::new(Vec::new()),
        };
        let retries = RefCell::new(0u32);
        let value = generate_valid(&llm, "m", &prompt(), 3, parse_ok, |_, _, _| {
            *retries.borrow_mut() += 1;
        })
        .unwrap();
        assert_eq!(value, "[ok]");
        assert_eq!(*retries.borrow(), 0);
        assert_eq!(llm.prompts.borrow().len(), 1);
    }

    #[test]
    fn an_invalid_reply_is_retried_with_the_prior_response_until_valid() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![
                Ok("Sure!".into()),
                Ok("still prose".into()),
                Ok("[fixed]".into()),
            ]),
            prompts: RefCell::new(Vec::new()),
        };
        let notices = RefCell::new(Vec::new());
        let value = generate_valid(&llm, "m", &prompt(), 3, parse_ok, |attempt, of, reason| {
            notices.borrow_mut().push((attempt, of, reason.to_string()));
        })
        .unwrap();
        assert_eq!(value, "[fixed]");
        assert_eq!(
            *notices.borrow(),
            vec![
                (2, 3, "not a JSON array".into()),
                (3, 3, "not a JSON array".into()),
            ]
        );
        let prompts = llm.prompts.borrow();
        assert_eq!(prompts.len(), 3);
        assert_eq!(prompts[0], "split this");
        assert!(prompts[1].contains("split this"));
        assert!(prompts[1].contains("Your previous reply was invalid"));
        assert!(prompts[1].contains("Sure!"));
        assert!(prompts[2].contains("still prose"));
    }

    #[test]
    fn exhausting_attempts_returns_the_last_reason() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![Ok("nope".into()), Ok("still no".into())]),
            prompts: RefCell::new(Vec::new()),
        };
        let error = generate_valid(&llm, "m", &prompt(), 2, parse_ok, |_, _, _| {}).unwrap_err();
        match error {
            LlmReplyError::Invalid { reason } => {
                assert!(reason.contains("not a JSON array"));
                assert!(reason.contains("after 2 attempts"));
            }
            other => panic!("expected invalid, got {other:?}"),
        }
        assert_eq!(llm.prompts.borrow().len(), 2);
    }

    #[test]
    fn a_transport_failure_is_not_retried() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![Err(LlmError("connection refused".into()))]),
            prompts: RefCell::new(Vec::new()),
        };
        let error = generate_valid(&llm, "m", &prompt(), 3, parse_ok, |_, _, _| {}).unwrap_err();
        match error {
            LlmReplyError::Call(e) => assert_eq!(e.0, "connection refused"),
            other => panic!("expected call error, got {other:?}"),
        }
        assert_eq!(llm.prompts.borrow().len(), 1);
    }

    #[test]
    fn generate_logged_warns_on_an_empty_reply() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![Ok("  ".into())]),
            prompts: RefCell::new(Vec::new()),
        };
        assert_eq!(generate_logged(&llm, "m", &prompt()).unwrap(), "  ");
    }

    #[test]
    fn generate_logged_records_a_non_empty_reply() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![Ok("hello".into())]),
            prompts: RefCell::new(Vec::new()),
        };
        assert_eq!(generate_logged(&llm, "m", &prompt()).unwrap(), "hello");
    }

    #[test]
    fn an_exhausted_llm_script_is_a_call_error() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![]),
            prompts: RefCell::new(Vec::new()),
        };
        let error = generate_logged(&llm, "m", &prompt()).unwrap_err();
        assert!(error.0.contains("exhausted"));
    }

    #[test]
    fn zero_attempts_is_treated_as_one() {
        let llm = QueueLlm {
            replies: RefCell::new(vec![Ok("[ok]".into())]),
            prompts: RefCell::new(Vec::new()),
        };
        let value = generate_valid(&llm, "m", &prompt(), 0, parse_ok, |_, _, _| {}).unwrap();
        assert_eq!(value, "[ok]");
    }
}

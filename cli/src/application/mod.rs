//! Use-case services. Each service is composed from domain logic and
//! [`crate::ports`] traits through constructor injection; no service names
//! a concrete adapter.

pub mod agent_service;
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
pub mod tool_call_service;
pub mod tool_service;

use crate::domain::config_report::DEFAULT_LLM_RETRY;
use crate::domain::tools::{ChatMessage, ChatTurn, ToolDefinition};
use crate::ports::{LlmConversation, LlmError};

/// How many times a model call is tried when the reply fails
/// validation. Overridden by `--retry` or `[llm] retry` in
/// `.bdd.toml`.
pub const DEFAULT_LLM_ATTEMPTS: u32 = DEFAULT_LLM_RETRY as u32;

/// A model round trip that either never reached a reply, or whose
/// reply failed validation after every attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmReplyError {
    Call(LlmError),
    Invalid { reason: String },
}

/// The single production entry point into [`LlmConversation`]. Logs
/// the offered tools, every message, the reply content, and each
/// requested call.
pub(crate) fn chat_logged<C: LlmConversation + ?Sized>(
    chat: &C,
    model: &str,
    section: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
) -> Result<ChatTurn, LlmError> {
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    tracing::debug!(
        model,
        tools = tools.len(),
        offered = ?tool_names,
        messages = messages.len(),
        "[{section}] LLM chat request"
    );
    for (index, message) in messages.iter().enumerate() {
        tracing::debug!(
            index,
            role = ?message.role,
            content = %message.content,
            tool_calls = message.tool_calls.len(),
            tool_name = ?message.tool_name,
            "[{section}] LLM chat message"
        );
    }
    let turn = chat.chat(model, messages, tools);
    match &turn {
        Ok(turn) => {
            tracing::debug!(
                content = %turn.content,
                tool_calls = turn.tool_calls.len(),
                "[{section}] LLM chat response"
            );
            for call in &turn.tool_calls {
                tracing::debug!(
                    name = %call.name,
                    arguments = %call.arguments,
                    "[{section}] LLM requested tool"
                );
            }
        }
        Err(error) => tracing::debug!(error = %error.0, "[{section}] LLM chat failed"),
    }
    turn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolCall, text_turn};
    use crate::ports::LlmError;
    use std::cell::RefCell;

    struct QueueChat {
        turns: RefCell<Vec<Result<ChatTurn, LlmError>>>,
        offered: RefCell<Vec<Vec<String>>>,
    }

    impl LlmConversation for QueueChat {
        fn chat(
            &self,
            _model: &str,
            _messages: &[ChatMessage],
            tools: &[ToolDefinition],
        ) -> Result<ChatTurn, LlmError> {
            self.offered
                .borrow_mut()
                .push(tools.iter().map(|t| t.name.clone()).collect());
            let mut turns = self.turns.borrow_mut();
            if turns.is_empty() {
                return Err(LlmError("script exhausted".into()));
            }
            turns.remove(0)
        }
    }

    #[test]
    fn chat_logged_records_a_text_reply() {
        let chat = QueueChat {
            turns: RefCell::new(vec![Ok(text_turn("hello"))]),
            offered: RefCell::new(Vec::new()),
        };
        let turn = chat_logged(&chat, "m", "ask", &[], &[]).unwrap();
        assert_eq!(turn.content, "hello");
        assert!(turn.tool_calls.is_empty());
    }

    #[test]
    fn chat_logged_records_tool_calls_and_the_offered_names() {
        let chat = QueueChat {
            turns: RefCell::new(vec![Ok(ChatTurn {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    name: "list_requirements".into(),
                    arguments: serde_json::json!({}),
                }],
            })]),
            offered: RefCell::new(Vec::new()),
        };
        let tools = [ToolDefinition {
            name: "list_requirements".into(),
            description: String::new(),
            schema: serde_json::json!({"type": "object"}),
            origin: crate::domain::tools::ToolOrigin::Builtin,
        }];
        let turn = chat_logged(&chat, "m", "proposal", &[ChatMessage::user("hi")], &tools).unwrap();
        assert_eq!(turn.tool_calls[0].name, "list_requirements");
        assert_eq!(chat.offered.borrow()[0], vec!["list_requirements"]);
    }

    #[test]
    fn chat_logged_propagates_a_transport_error() {
        let chat = QueueChat {
            turns: RefCell::new(vec![Err(LlmError("connection refused".into()))]),
            offered: RefCell::new(Vec::new()),
        };
        let error = chat_logged(&chat, "m", "ask", &[], &[]).unwrap_err();
        assert_eq!(error.0, "connection refused");
    }

    #[test]
    fn an_exhausted_chat_script_is_a_call_error() {
        let chat = QueueChat {
            turns: RefCell::new(vec![]),
            offered: RefCell::new(Vec::new()),
        };
        let error = chat_logged(&chat, "m", "ask", &[], &[]).unwrap_err();
        assert!(error.0.contains("exhausted"));
    }
}

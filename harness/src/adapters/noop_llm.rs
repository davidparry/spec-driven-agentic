//! Documents that MCP-side generation is template-only: the client
//! driving `mcp serve` is itself an LLM. This conversation is never called.

use crate::domain::tools::{ChatMessage, ChatTurn, ToolDefinition};
use crate::ports::{LlmConversation, LlmError};

pub struct NoLlm;

impl LlmConversation for NoLlm {
    fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ChatTurn, LlmError> {
        Err(LlmError(
            "NoLlm is a type placeholder - MCP generation is template-only".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_llm_refuses_every_call() {
        let error = NoLlm.chat("m", &[], &[]).unwrap_err();
        assert!(error.0.contains("template-only"), "{}", error.0);
    }
}

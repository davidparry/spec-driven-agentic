//! Ollama `/api/chat` with a `tools` array. The HTTP shell is thin; JSON
//! translation is pure so it is unit-testable without a network.

use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, error, info};

use crate::adapters::ollama::{KEEP_ALIVE, describe};
use crate::domain::tools::{
    ChatMessage, ChatRole, ChatTurn, ToolCall, ToolDefinition, arguments_object,
};
use crate::ports::{LlmConversation, LlmError};

pub struct OllamaChat {
    endpoint: String,
    timeout: Duration,
    client: reqwest::blocking::Client,
}

impl OllamaChat {
    pub fn new(endpoint: String) -> Self {
        Self::with_timeout(
            endpoint,
            crate::adapters::ollama::DEFAULT_GENERATION_TIMEOUT,
        )
    }

    pub fn with_timeout(endpoint: String, timeout: Duration) -> Self {
        Self {
            client: crate::adapters::ollama::http_client(timeout),
            endpoint,
            timeout,
        }
    }
}

impl LlmConversation for OllamaChat {
    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatTurn, LlmError> {
        let url = format!("{}/api/chat", self.endpoint.trim_end_matches('/'));
        info!(model, endpoint = %self.endpoint, tools = tools.len(), "sending LLM chat request");
        let started = std::time::Instant::now();
        let body = self
            .client
            .post(&url)
            .json(&chat_body(model, messages, tools))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.text())
            .map_err(|e| {
                let detail = describe(&e, self.timeout);
                error!(model, error = %detail, "LLM chat request failed");
                LlmError(format!("Ollama at {} - {detail}", self.endpoint))
            })?;
        debug!(model, raw_response = %body, "LLM raw /api/chat response body");
        let turn = parse_chat(&body).inspect_err(
            |e| error!(model, error = %e.0, "LLM chat response could not be parsed"),
        )?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        info!(
            model,
            elapsed_ms,
            response_chars = turn.content.len(),
            tool_calls = turn.tool_calls.len(),
            "LLM chat response received"
        );
        Ok(turn)
    }
}

pub fn chat_body(
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages.iter().map(chat_message).collect::<Vec<_>>(),
        "stream": false,
        "keep_alive": KEEP_ALIVE,
    });
    if !tools.is_empty() {
        body.as_object_mut().expect("object").insert(
            "tools".into(),
            serde_json::Value::Array(tools.iter().map(tool_schema).collect()),
        );
    }
    body
}

fn chat_message(message: &ChatMessage) -> serde_json::Value {
    let role = match message.role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    };
    let mut object = serde_json::json!({
        "role": role,
        "content": message.content,
    });
    let map = object.as_object_mut().expect("object");
    if !message.tool_calls.is_empty() {
        map.insert(
            "tool_calls".into(),
            serde_json::Value::Array(
                message
                    .tool_calls
                    .iter()
                    .map(|call| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments,
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(name) = &message.tool_name {
        map.insert("tool_name".into(), serde_json::Value::String(name.clone()));
    }
    object
}

pub fn tool_schema(tool: &ToolDefinition) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.schema,
        }
    })
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    message: Option<ChatMessageBody>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessageBody {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
struct WireToolCall {
    #[serde(default)]
    function: Option<WireFunction>,
}

#[derive(Deserialize)]
struct WireFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

pub fn parse_chat(body: &str) -> Result<ChatTurn, LlmError> {
    let response: ChatResponse = serde_json::from_str(body)
        .map_err(|e| LlmError(format!("unexpected /api/chat response - {e}")))?;
    if let Some(error) = response.error {
        return Err(LlmError(error));
    }
    let Some(message) = response.message else {
        return Err(LlmError(
            "unexpected /api/chat response - missing message".into(),
        ));
    };
    Ok(ChatTurn {
        content: message.content,
        tool_calls: message
            .tool_calls
            .into_iter()
            .filter_map(|call| {
                let function = call.function?;
                if function.name.is_empty() {
                    return None;
                }
                Some(ToolCall {
                    name: function.name,
                    arguments: arguments_object(&function.arguments),
                })
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolOrigin;

    fn sample_tool() -> ToolDefinition {
        ToolDefinition {
            name: "get_tdd_state".into(),
            description: "Where the bar is.".into(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Builtin,
        }
    }

    #[test]
    fn chat_body_carries_messages_tools_keep_alive_and_no_temperature() {
        let body = chat_body(
            "llama3",
            &[
                ChatMessage::system("sys"),
                ChatMessage::user("do it"),
                ChatMessage::tool("get_tdd_state", "{}"),
            ],
            &[sample_tool()],
        );
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["stream"], false);
        assert_eq!(body["keep_alive"], KEEP_ALIVE);
        assert!(body.get("temperature").is_none());
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_name"], "get_tdd_state");
        assert_eq!(body["tools"][0]["function"]["name"], "get_tdd_state");
    }

    #[test]
    fn an_empty_tool_list_omits_the_tools_field() {
        let body = chat_body("m", &[ChatMessage::user("u")], &[]);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn parse_chat_reads_content_and_tool_calls_including_string_arguments() {
        let body = r#"{
            "message": {
                "role": "assistant",
                "content": "looking up",
                "tool_calls": [
                    {"function": {"name": "get_requirement", "arguments": {"id": "REQ-001"}}},
                    {"function": {"name": "validate_spec", "arguments": "{}"}}
                ]
            }
        }"#;
        let turn = parse_chat(body).unwrap();
        assert_eq!(turn.content, "looking up");
        assert_eq!(turn.tool_calls[0].name, "get_requirement");
        assert_eq!(turn.tool_calls[0].arguments["id"], "REQ-001");
        assert_eq!(turn.tool_calls[1].name, "validate_spec");
        assert!(turn.tool_calls[1].arguments.is_object());
    }

    #[test]
    fn parse_chat_reports_a_model_error_body() {
        let error = parse_chat(r#"{"error":"model does not support tools"}"#).unwrap_err();
        assert!(error.0.contains("does not support tools"), "{}", error.0);
    }

    #[test]
    fn malformed_chat_json_is_a_structured_error() {
        let error = parse_chat("{}").unwrap_err();
        assert!(error.0.contains("missing message"), "{}", error.0);
    }

    #[test]
    fn a_reachable_endpoint_sends_chat_and_returns_the_turn() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 8192];
            let read = stream.read(&mut request).unwrap();
            let body = r#"{"message":{"content":"ok","tool_calls":[]}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        let chat = OllamaChat::new(format!("http://127.0.0.1:{port}/"));
        let turn = chat
            .chat("llama3", &[ChatMessage::user("hello")], &[sample_tool()])
            .unwrap();
        let request = server.join().unwrap();
        assert_eq!(turn.content, "ok");
        assert!(request.contains(r#""keep_alive":"30m""#), "{request}");
        assert!(!request.contains("temperature"), "{request}");
        assert!(request.contains("get_tdd_state"), "{request}");
    }

    #[test]
    fn an_unreachable_chat_endpoint_reports_the_endpoint() {
        let chat =
            OllamaChat::with_timeout("http://127.0.0.1:9".into(), Duration::from_millis(300));
        let error = chat
            .chat("llama3", &[ChatMessage::user("x")], &[])
            .unwrap_err();
        assert!(error.0.contains("http://127.0.0.1:9"), "got: {}", error.0);
    }

    #[test]
    fn a_slow_chat_reply_is_named_a_timeout() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            std::thread::sleep(Duration::from_millis(600));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        });
        let chat = OllamaChat::with_timeout(
            format!("http://127.0.0.1:{port}"),
            Duration::from_millis(200),
        );
        let error = chat
            .chat("llama3", &[ChatMessage::user("x")], &[])
            .unwrap_err();
        server.join().unwrap();
        assert!(error.0.contains("no reply within 0s"), "got: {}", error.0);
    }
}

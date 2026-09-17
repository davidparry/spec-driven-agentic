//! Names, schemas, and conversation vocabulary for tool-calling.
//! Pure: no IO. Routing resolves by exact catalog lookup first.

use sha2::{Digest, Sha256};

pub const NAMESPACE_SEPARATOR: &str = "__";
const ORIGIN_SEPARATOR: char = ':';
/// Reserved origin for the harness's own tools. An mcp.json server must not rely on this name.
pub const BUILTIN_ORIGIN: &str = "builtin";
pub const MAX_TOOL_NAME: usize = 64;
pub const TOOL_REPLY_CAP: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }

    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            tool_calls,
            tool_name: None,
        }
    }

    pub fn tool(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: Some(name.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChatTurn {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ToolOrigin {
    Builtin,
    Server(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub origin: ToolOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolOutcome {
    pub text: String,
    pub is_error: bool,
}

pub fn sanitize_segment(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub fn namespaced(server: &str, tool: &str) -> String {
    let server = sanitize_segment(server);
    let tool = sanitize_segment(tool);
    let joined = format!("{server}{NAMESPACE_SEPARATOR}{tool}");
    if joined.len() <= MAX_TOOL_NAME {
        return joined;
    }
    let digest = {
        let mut hasher = Sha256::new();
        hasher.update(joined.as_bytes());
        let bytes = hasher.finalize();
        format!("{:02x}{:02x}", bytes[0], bytes[1])
    };
    let suffix = format!("{NAMESPACE_SEPARATOR}{tool}");
    let budget = MAX_TOOL_NAME.saturating_sub(suffix.len() + 5);
    let server_keep = if budget == 0 {
        String::new()
    } else {
        server.chars().take(budget).collect()
    };
    format!("{server_keep}_{digest}{suffix}")
}

pub fn server_of(name: &str) -> Option<&str> {
    name.split_once(NAMESPACE_SEPARATOR)
        .map(|(server, _)| server)
}

/// Split `origin:tool` as written in `.spec.toml`. The namespaced catalog
/// form `server__tool` is not a qualified reference.
fn qualified_parts(name: &str) -> Option<(&str, &str)> {
    let (origin, tool) = name.split_once(ORIGIN_SEPARATOR)?;
    if origin.is_empty() || tool.is_empty() {
        None
    } else {
        Some((origin, tool))
    }
}

pub fn find<'a>(catalog: &'a [ToolDefinition], name: &str) -> Result<&'a ToolDefinition, String> {
    if let Some((origin, tool)) = qualified_parts(name) {
        return pick(
            catalog,
            name,
            catalog
                .iter()
                .filter(|definition| matches_qualified(definition, origin, tool))
                .collect(),
        );
    }
    if let Some(found) = catalog.iter().find(|tool| tool.name == name) {
        return Ok(found);
    }
    let suffix = format!("{NAMESPACE_SEPARATOR}{name}");
    pick(
        catalog,
        name,
        catalog
            .iter()
            .filter(|tool| tool.name.ends_with(&suffix))
            .collect(),
    )
}

fn matches_qualified(definition: &ToolDefinition, origin: &str, tool: &str) -> bool {
    match &definition.origin {
        ToolOrigin::Builtin => origin == BUILTIN_ORIGIN && definition.name == tool,
        ToolOrigin::Server(server) => {
            origin == server && definition.name == namespaced(server, tool)
        }
    }
}

fn pick<'a>(
    catalog: &'a [ToolDefinition],
    name: &str,
    matches: Vec<&'a ToolDefinition>,
) -> Result<&'a ToolDefinition, String> {
    match matches.as_slice() {
        [one] => Ok(one),
        [] => {
            let offered: Vec<_> = catalog.iter().map(|t| t.name.clone()).collect();
            Err(unknown_tool_reply(name, &offered))
        }
        many => {
            let names: Vec<_> = many.iter().map(|t| t.name.as_str()).take(5).collect();
            Err(format!(
                "'{name}' is ambiguous; candidates: {}",
                names.join(", ")
            ))
        }
    }
}

pub fn arguments_object(raw: &serde_json::Value) -> serde_json::Value {
    match raw {
        serde_json::Value::Object(_) => raw.clone(),
        serde_json::Value::String(text) => serde_json::from_str(text)
            .ok()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({})),
        _ => serde_json::json!({}),
    }
}

pub fn narrate_call(name: &str, args: &serde_json::Value) -> String {
    let inner = match args {
        serde_json::Value::Object(map) if map.is_empty() => String::new(),
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(key, value)| match value {
                serde_json::Value::String(text) => format!("{key}={text}"),
                other => format!("{key}={other}"),
            })
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    };
    format!("{name}({inner})")
}

pub fn unknown_tool_reply(name: &str, offered: &[String]) -> String {
    let mut candidates: Vec<_> = offered
        .iter()
        .filter(|candidate| candidate.contains(name) || name.contains(candidate.as_str()))
        .cloned()
        .collect();
    if candidates.is_empty() {
        candidates = offered.iter().take(5).cloned().collect();
    }
    candidates.truncate(5);
    if candidates.is_empty() {
        format!("unknown tool '{name}'")
    } else {
        format!(
            "unknown tool '{name}' — did you mean {}?",
            candidates.join(", ")
        )
    }
}

pub fn declined_reply(name: &str) -> String {
    format!("the developer declined {name}")
}

pub fn confirm_question(name: &str, args: &serde_json::Value) -> String {
    format!("Run {}? [y/N]", narrate_call(name, args))
}

pub fn truncate_for_model(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!("{kept}\n… [truncated]")
}

pub fn first_sentence(description: &str) -> &str {
    match description.split_once(". ") {
        Some((head, _)) => {
            // Keep the period that ended the sentence.
            &description[..head.len() + 1]
        }
        None => description,
    }
}

/// The system prompt and last user message of a chat history. Fakes
/// that used to implement generate(system, user) record this pair.
pub fn system_and_user(messages: &[ChatMessage]) -> (String, String) {
    let system = messages
        .iter()
        .find(|m| m.role == ChatRole::System)
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let user = messages
        .iter()
        .rev()
        .find(|m| m.role == ChatRole::User)
        .map(|m| m.content.clone())
        .unwrap_or_default();
    (system, user)
}

/// A text-only model turn, the shape of a terminal answer.
pub fn text_turn(content: impl Into<String>) -> ChatTurn {
    ChatTurn {
        content: content.into(),
        tool_calls: Vec::new(),
    }
}

pub fn missing_required(schema: &serde_json::Value, args: &serde_json::Value) -> Vec<String> {
    let Some(required) = schema.get("required").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let object = args.as_object();
    required
        .iter()
        .filter_map(|value| value.as_str())
        .filter(|key| object.is_none_or(|map| !map.contains_key(*key)))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: String::new(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Builtin,
        }
    }

    fn server_def(server: &str, tool: &str) -> ToolDefinition {
        ToolDefinition {
            name: namespaced(server, tool),
            description: String::new(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Server(server.into()),
        }
    }

    #[test]
    fn namespacing_joins_with_a_double_underscore() {
        assert_eq!(
            namespaced("playwright", "browser_navigate"),
            "playwright__browser_navigate"
        );
    }

    #[test]
    fn namespacing_sanitizes_and_stays_under_64() {
        let name = namespaced("my.server!", "tool name");
        assert_eq!(name, "my_server___tool_name");
        assert!(name.len() <= MAX_TOOL_NAME);
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        );
    }

    #[test]
    fn a_long_server_segment_is_truncated_with_a_digest() {
        let server = "a".repeat(80);
        let name = namespaced(&server, "list");
        assert!(name.len() <= MAX_TOOL_NAME, "{name}");
        assert!(name.ends_with("__list"), "{name}");
        assert!(name.contains('_'), "{name}");
    }

    #[test]
    fn find_is_exact_then_unique_suffix() {
        let catalog = vec![def("get_requirement"), def("playwright__browser_navigate")];
        assert_eq!(
            find(&catalog, "get_requirement").unwrap().name,
            "get_requirement"
        );
        assert_eq!(
            find(&catalog, "browser_navigate").unwrap().name,
            "playwright__browser_navigate"
        );
    }

    #[test]
    fn find_reports_ambiguity_and_a_miss() {
        let catalog = vec![def("a__t"), def("b__t")];
        let error = find(&catalog, "t").unwrap_err();
        assert!(error.contains("ambiguous"), "{error}");
        let miss = find(&catalog, "nope").unwrap_err();
        assert!(miss.contains("unknown tool"), "{miss}");
    }

    #[test]
    fn a_bare_name_is_the_builtin_when_an_mcp_tool_shares_the_short_name() {
        let catalog = vec![def("validate_spec"), server_def("self", "validate_spec")];
        assert_eq!(
            find(&catalog, "validate_spec").unwrap().origin,
            ToolOrigin::Builtin
        );
        assert_eq!(
            find(&catalog, "builtin:validate_spec").unwrap().origin,
            ToolOrigin::Builtin
        );
        let mcp = find(&catalog, "self:validate_spec").unwrap();
        assert_eq!(mcp.name, "self__validate_spec");
        assert_eq!(mcp.origin, ToolOrigin::Server("self".into()));
        assert_eq!(
            find(&catalog, "self__validate_spec").unwrap().name,
            "self__validate_spec"
        );
    }

    #[test]
    fn builtin_qualifier_does_not_fall_through_to_an_mcp_tool() {
        let catalog = vec![server_def("self", "validate_spec")];
        let error = find(&catalog, "builtin:validate_spec").unwrap_err();
        assert!(error.contains("unknown tool"), "{error}");
        assert_eq!(
            find(&catalog, "self:validate_spec").unwrap().name,
            "self__validate_spec"
        );
    }

    #[test]
    fn arguments_object_accepts_object_string_null_and_junk() {
        assert_eq!(
            arguments_object(&serde_json::json!({"id": "REQ-001"})),
            serde_json::json!({"id": "REQ-001"})
        );
        assert_eq!(
            arguments_object(&serde_json::json!("{\"id\":\"REQ-001\"}")),
            serde_json::json!({"id": "REQ-001"})
        );
        assert_eq!(
            arguments_object(&serde_json::Value::Null),
            serde_json::json!({})
        );
        assert_eq!(
            arguments_object(&serde_json::json!("nope")),
            serde_json::json!({})
        );
        assert_eq!(
            arguments_object(&serde_json::json!([1])),
            serde_json::json!({})
        );
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate_for_model("abcd", 4), "abcd");
        assert_eq!(truncate_for_model("abcde", 4), "abcd\n… [truncated]");
    }

    #[test]
    fn missing_required_names_each_absent_property() {
        let schema = serde_json::json!({"required": ["id", "req_id"]});
        assert_eq!(
            missing_required(&schema, &serde_json::json!({})),
            vec!["id".to_string(), "req_id".to_string()]
        );
        assert!(
            missing_required(&schema, &serde_json::json!({"id": "1", "req_id": "2"})).is_empty()
        );
        assert!(missing_required(&serde_json::json!({}), &serde_json::json!({})).is_empty());
    }

    #[test]
    fn narration_and_replies_are_stable() {
        assert_eq!(
            narrate_call("get_requirement", &serde_json::json!({"id": "REQ-003"})),
            "get_requirement(id=REQ-003)"
        );
        assert_eq!(
            declined_reply("command_run"),
            "the developer declined command_run"
        );
        assert!(
            confirm_question("command_run", &serde_json::json!({"command": ["mvn"]}))
                .contains("command_run")
        );
        assert_eq!(
            first_sentence("Run the tests. Updates the bar."),
            "Run the tests."
        );
        assert_eq!(first_sentence("No period"), "No period");
        assert_eq!(
            server_of("playwright__browser_navigate"),
            Some("playwright")
        );
        assert_eq!(server_of("get_requirement"), None);
    }

    #[test]
    fn system_and_user_read_the_system_and_last_user_messages() {
        let (system, user) = system_and_user(&[
            ChatMessage::system("sys"),
            ChatMessage::user("first"),
            ChatMessage::assistant("ok", Vec::new()),
            ChatMessage::user("second"),
        ]);
        assert_eq!(system, "sys");
        assert_eq!(user, "second");
        assert_eq!(system_and_user(&[]), (String::new(), String::new()));
        assert_eq!(text_turn("hi").content, "hi");
        assert!(text_turn("hi").tool_calls.is_empty());
    }
}

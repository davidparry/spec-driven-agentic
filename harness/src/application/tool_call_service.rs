//! `bdd mcp call`: one throwaway session, one tools/call, print, exit.

use crate::domain::tools::{ToolDefinition, arguments_object, find, missing_required};
use crate::ports::{ToolBroker, ToolError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CallEnvelope {
    pub tool: String,
    #[serde(rename = "isError")]
    pub is_error: bool,
    pub content: String,
}

pub struct ToolCallService;

impl ToolCallService {
    pub fn merge_arguments(
        base: Option<&str>,
        pairs: &[(String, String)],
    ) -> Result<serde_json::Value, ToolError> {
        let mut object = match base {
            Some(text) => serde_json::from_str::<serde_json::Value>(text)
                .map_err(|e| ToolError(format!("--args is not JSON: {e}")))?,
            None => serde_json::json!({}),
        };
        if !object.is_object() {
            return Err(ToolError("--args must be a JSON object".into()));
        }
        let map = object.as_object_mut().expect("object");
        for (key, value) in pairs {
            map.insert(key.clone(), coerce_arg(value));
        }
        Ok(object)
    }

    pub fn prepare<'a>(
        catalog: &'a [ToolDefinition],
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(&'a ToolDefinition, serde_json::Value), ToolError> {
        let definition = find(catalog, name).map_err(ToolError)?;
        let arguments = arguments_object(arguments);
        let missing = missing_required(&definition.schema, &arguments);
        if !missing.is_empty() {
            return Err(ToolError(format!(
                "missing required argument{}: {}",
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", ")
            )));
        }
        Ok((definition, arguments))
    }

    pub fn call(
        broker: &impl ToolBroker,
        catalog: &[ToolDefinition],
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<CallEnvelope, ToolError> {
        let (definition, arguments) = Self::prepare(catalog, name, arguments)?;
        let outcome = broker.call(&definition.name, &arguments)?;
        Ok(CallEnvelope {
            tool: definition.name.clone(),
            is_error: outcome.is_error,
            content: outcome.text,
        })
    }
}

fn coerce_arg(raw: &str) -> serde_json::Value {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        return value;
    }
    serde_json::Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolDefinition, ToolOrigin, ToolOutcome};
    use std::cell::RefCell;

    struct FakeBroker {
        calls: RefCell<Vec<(String, serde_json::Value)>>,
        outcome: ToolOutcome,
    }

    impl ToolBroker for FakeBroker {
        fn call(
            &self,
            name: &str,
            arguments: &serde_json::Value,
        ) -> Result<ToolOutcome, ToolError> {
            self.calls
                .borrow_mut()
                .push((name.to_string(), arguments.clone()));
            Ok(self.outcome.clone())
        }
    }

    fn catalog() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "list_requirements".into(),
                description: "list".into(),
                schema: serde_json::json!({"type": "object"}),
                origin: ToolOrigin::Builtin,
            },
            ToolDefinition {
                name: "get_requirement".into(),
                description: "one".into(),
                schema: serde_json::json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": {"id": {"type": "string"}}
                }),
                origin: ToolOrigin::Builtin,
            },
        ]
    }

    #[test]
    fn merge_lets_arg_override_args() {
        let merged = ToolCallService::merge_arguments(
            Some(r#"{"id":"REQ-001","extra":[1]}"#),
            &[("id".into(), "REQ-002".into())],
        )
        .unwrap();
        assert_eq!(merged["id"], "REQ-002");
        assert_eq!(merged["extra"], serde_json::json!([1]));
    }

    #[test]
    fn missing_required_is_named_before_any_call() {
        let error = ToolCallService::prepare(&catalog(), "get_requirement", &serde_json::json!({}))
            .unwrap_err();
        assert!(error.0.contains("id"), "{}", error.0);
    }

    #[test]
    fn unknown_tool_names_candidates() {
        let error =
            ToolCallService::prepare(&catalog(), "nope", &serde_json::json!({})).unwrap_err();
        assert!(error.0.contains("unknown"), "{}", error.0);
    }

    #[test]
    fn a_tool_error_is_an_envelope_with_is_error() {
        let broker = FakeBroker {
            calls: RefCell::new(Vec::new()),
            outcome: ToolOutcome {
                text: "refused".into(),
                is_error: true,
            },
        };
        let envelope = ToolCallService::call(
            &broker,
            &catalog(),
            "list_requirements",
            &serde_json::json!({}),
        )
        .unwrap();
        assert!(envelope.is_error);
        assert_eq!(envelope.content, "refused");
        assert_eq!(broker.calls.borrow().len(), 1);
    }

    #[test]
    fn json_envelope_shape() {
        let broker = FakeBroker {
            calls: RefCell::new(Vec::new()),
            outcome: ToolOutcome {
                text: "{}".into(),
                is_error: false,
            },
        };
        let envelope = ToolCallService::call(
            &broker,
            &catalog(),
            "list_requirements",
            &serde_json::json!({}),
        )
        .unwrap();
        let json = serde_json::json!({
            "tool": envelope.tool,
            "isError": envelope.is_error,
            "content": envelope.content,
        });
        assert_eq!(json["tool"], "list_requirements");
        assert_eq!(json["isError"], false);
    }
}

//! MCP client adapter: loopback duplex for built-in tools, child process
//! for registered servers. Owns a tokio runtime and `block_on`s.
//!
//! Never construct this from inside `bdd mcp serve`. `block_on` panics
//! when a runtime is already running; if an MCP tool ever needed the
//! agent, check `tokio::runtime::Handle::try_current()` first.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, ProtocolVersion, Tool};
use rmcp::service::{Peer, RoleClient, RoleServer, RunningService};
use rmcp::transport::child_process::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{ClientLifecycleMode, ClientServiceExt, ServiceExt as _};
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::domain::config_report::{
    DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS, DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS,
};
use crate::domain::mcp_registry::ServerSpec;
use crate::domain::tools::{
    NAMESPACE_SEPARATOR, ToolDefinition, ToolOrigin, ToolOutcome, namespaced, server_of,
};
use crate::ports::{ToolBroker, ToolDiscovery, ToolError};

const DEFAULT_CONNECT: Duration = Duration::from_secs(DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS);
const DEFAULT_CALL: Duration = Duration::from_secs(DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SessionKey {
    Builtin,
    Server(String),
}

pub struct McpToolBroker<S>
where
    S: rmcp::service::Service<RoleServer> + Clone + Send + Sync + 'static,
{
    runtime: tokio::runtime::Runtime,
    builtin: S,
    servers: Vec<ServerSpec>,
    connect_timeout: Duration,
    call_timeout: Duration,
    sessions: Mutex<HashMap<SessionKey, RunningService<RoleClient, ()>>>,
    /// When set, built-in tools are invoked by spawning this command
    /// (`bdd mcp call --stdio`) instead of the in-process duplex.
    self_stdio: Option<ServerSpec>,
}

impl<S> McpToolBroker<S>
where
    S: rmcp::service::Service<RoleServer> + Clone + Send + Sync + 'static,
{
    pub fn new(builtin: S, servers: Vec<ServerSpec>) -> Self {
        Self::with_timeouts(builtin, servers, DEFAULT_CONNECT, DEFAULT_CALL)
    }

    pub fn with_timeouts(
        builtin: S,
        servers: Vec<ServerSpec>,
        connect_timeout: Duration,
        call_timeout: Duration,
    ) -> Self {
        if tokio::runtime::Handle::try_current().is_ok() {
            panic!(
                "McpToolBroker must not be constructed on a tokio runtime \
                 (never inside bdd mcp serve)"
            );
        }
        Self {
            runtime: tokio::runtime::Runtime::new().expect("tokio runtime"),
            builtin,
            servers,
            connect_timeout,
            call_timeout,
            sessions: Mutex::new(HashMap::new()),
            self_stdio: None,
        }
    }

    pub fn with_self_stdio(mut self, spec: ServerSpec) -> Self {
        self.self_stdio = Some(spec);
        self
    }

    /// List tools from a throwaway built-in session (loopback or `--stdio`).
    pub fn list_builtin_tools(&self) -> Result<Vec<ToolDefinition>, ToolError> {
        let peer = self.peer(&SessionKey::Builtin)?;
        let timeout = self.call_timeout;
        let tools = self.runtime.block_on(async move {
            tokio::time::timeout(timeout, peer.list_all_tools())
                .await
                .map_err(|_| ToolError("timed out listing tools".into()))?
                .map_err(|error| ToolError(error.to_string()))
        })?;
        Ok(tools
            .into_iter()
            .map(|tool| definition_from(tool, ToolOrigin::Builtin))
            .collect())
    }

    fn sessions(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<SessionKey, RunningService<RoleClient, ()>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn peer(&self, key: &SessionKey) -> Result<Peer<RoleClient>, ToolError> {
        {
            let guard = self.sessions();
            if let Some(existing) = guard.get(key) {
                return Ok(existing.peer().clone());
            }
        }
        let connected = match key {
            SessionKey::Builtin => self.connect_builtin()?,
            SessionKey::Server(name) => {
                let spec = self
                    .servers
                    .iter()
                    .find(|server| server.name == *name)
                    .ok_or_else(|| ToolError(format!("no registered server named {name}")))?;
                self.connect_server(spec)?
            }
        };
        let mut guard = self.sessions();
        let session = guard.entry(key.clone()).or_insert(connected);
        Ok(session.peer().clone())
    }

    fn connect_builtin(&self) -> Result<RunningService<RoleClient, ()>, ToolError> {
        if let Some(spec) = &self.self_stdio {
            return self.connect_server(spec);
        }
        let server = self.builtin.clone();
        let timeout = self.connect_timeout;
        self.runtime.block_on(async move {
            let (server_transport, client_transport) = tokio::io::duplex(65536);
            let (fail_tx, fail_rx) = oneshot::channel();
            tokio::spawn(async move {
                match server.serve(server_transport).await {
                    Ok(service) => {
                        let _ = service.waiting().await;
                    }
                    Err(error) => {
                        let _ = fail_tx.send(error.to_string());
                    }
                }
            });
            tokio::select! {
                fail = fail_rx => {
                    Err(ToolError(format!(
                        "loopback MCP server failed to start: {}",
                        fail.unwrap_or_else(|_| "server task dropped".into())
                    )))
                }
                result = tokio::time::timeout(
                    timeout,
                    ().serve_with_lifecycle(client_transport, discover_lifecycle()),
                ) => {
                    result
                        .map_err(|_| ToolError("timed out connecting to built-in tools".into()))?
                        .map_err(|error| ToolError(error.to_string()))
                }
            }
        })
    }

    fn connect_server(
        &self,
        spec: &ServerSpec,
    ) -> Result<RunningService<RoleClient, ()>, ToolError> {
        let timeout = self.connect_timeout;
        let command = spawn_command(spec);
        self.runtime.block_on(async move {
            let transport = TokioChildProcess::new(command)
                .map_err(|error| ToolError(format!("failed to start MCP server: {error}")))?;
            tokio::time::timeout(
                timeout,
                ().serve_with_lifecycle(transport, discover_lifecycle()),
            )
            .await
            .map_err(|_| ToolError("timed out connecting to MCP server".into()))?
            .map_err(|error| ToolError(error.to_string()))
        })
    }
}

fn discover_lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    }
}

pub fn spawn_command(spec: &ServerSpec) -> Command {
    Command::new(&spec.program).configure(|cmd| {
        cmd.args(&spec.args);
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
    })
}

pub fn outcome_from(result: CallToolResult) -> ToolOutcome {
    let joined: String = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.as_str()))
        .collect();
    let text = if !joined.is_empty() {
        joined
    } else if let Some(value) = result.structured_content {
        value.to_string()
    } else {
        "(the tool returned no text)".into()
    };
    ToolOutcome {
        text,
        is_error: result.is_error.unwrap_or(false),
    }
}

pub fn definition_from(tool: Tool, origin: ToolOrigin) -> ToolDefinition {
    let mut schema = serde_json::Value::Object((*tool.input_schema).clone());
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
        object.remove("title");
    }
    let raw_name = tool.name.to_string();
    let name = match &origin {
        ToolOrigin::Builtin => raw_name,
        ToolOrigin::Server(server) => namespaced(server, &raw_name),
    };
    ToolDefinition {
        name,
        description: tool.description.as_deref().unwrap_or("").to_string(),
        schema,
        origin,
    }
}

fn invocation_name(name: &str) -> String {
    name.rsplit_once(NAMESPACE_SEPARATOR)
        .map(|(_, tool)| tool.to_string())
        .unwrap_or_else(|| name.to_string())
}

fn arguments_map(arguments: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    arguments.as_object().cloned().unwrap_or_default()
}

impl<S> ToolBroker for McpToolBroker<S>
where
    S: rmcp::service::Service<RoleServer> + Clone + Send + Sync + 'static,
{
    fn call(&self, name: &str, arguments: &serde_json::Value) -> Result<ToolOutcome, ToolError> {
        let key = match server_of(name) {
            None => SessionKey::Builtin,
            Some(server) => SessionKey::Server(server.to_string()),
        };
        let remote = match &key {
            SessionKey::Builtin => name.to_string(),
            SessionKey::Server(_) => invocation_name(name),
        };
        let peer = self.peer(&key)?;
        let timeout = self.call_timeout;
        let params = CallToolRequestParams::new(remote).with_arguments(arguments_map(arguments));
        let result = self.runtime.block_on(async move {
            tokio::time::timeout(timeout, peer.call_tool(params))
                .await
                .map_err(|_| ToolError("timed out calling tool".into()))?
                .map_err(|error| ToolError(error.to_string()))
        })?;
        Ok(outcome_from(result))
    }
}

impl<S> ToolDiscovery for McpToolBroker<S>
where
    S: rmcp::service::Service<RoleServer> + Clone + Send + Sync + 'static,
{
    fn discover(&self, server: &ServerSpec) -> Result<Vec<ToolDefinition>, ToolError> {
        let origin = ToolOrigin::Server(server.name.clone());
        let session = self.connect_server(server)?;
        let timeout = self.connect_timeout;
        let peer = session.peer().clone();
        // Keep `session` alive for the duration of the list call.
        let tools = self.runtime.block_on(async move {
            let listed = tokio::time::timeout(timeout, peer.list_all_tools())
                .await
                .map_err(|_| ToolError("timed out listing tools".into()))?
                .map_err(|error| ToolError(error.to_string()));
            drop(session);
            listed
        })?;
        Ok(tools
            .into_iter()
            .map(|tool| definition_from(tool, origin.clone()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::WorkflowServer;
    use rmcp::model::ContentBlock;
    use std::fs;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("requirements")).unwrap();
        fs::write(
            dir.path().join("requirements/requirements.json"),
            r#"{"project":"Demo","requirements":[]}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn spawn_command_maps_program_args_and_env() {
        let spec = ServerSpec {
            name: "self".into(),
            program: "bdd".into(),
            args: vec!["mcp".into(), "serve".into()],
            env: vec![("A".into(), "1".into())],
        };
        let cmd = spawn_command(&spec);
        let std = cmd.as_std();
        assert_eq!(std.get_program(), "bdd");
        let args: Vec<_> = std
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["mcp", "serve"]);
    }

    #[test]
    fn outcome_from_concatenates_text_then_structured_then_placeholder() {
        let text = outcome_from(CallToolResult::success(vec![
            ContentBlock::text("hello"),
            ContentBlock::text("world"),
        ]));
        assert_eq!(text.text, "helloworld");
        assert!(!text.is_error);

        let mut structured = CallToolResult::success(vec![]);
        structured.structured_content = Some(serde_json::json!({"ok": true}));
        structured.is_error = Some(true);
        let structured = outcome_from(structured);
        assert!(structured.text.contains("ok"));
        assert!(structured.is_error);

        let empty = outcome_from(CallToolResult::success(vec![]));
        assert_eq!(empty.text, "(the tool returned no text)");
        assert!(!empty.is_error);
    }

    #[test]
    fn definition_from_namespaces_external_tools_and_strips_schema_title() {
        let mut schema = serde_json::Map::new();
        schema.insert("$schema".into(), serde_json::json!("https://example"));
        schema.insert("title".into(), serde_json::json!("X"));
        schema.insert("type".into(), serde_json::json!("object"));
        let tool = Tool::new("browser_navigate", "Go", std::sync::Arc::new(schema));
        let definition = definition_from(tool, ToolOrigin::Server("playwright".into()));
        assert_eq!(definition.name, "playwright__browser_navigate");
        assert!(definition.schema.get("$schema").is_none());
        assert!(definition.schema.get("title").is_none());
    }

    #[test]
    fn loopback_lists_and_calls_builtin_tools() {
        let dir = project();
        let broker = McpToolBroker::new(WorkflowServer::new(dir.path().to_path_buf()), vec![]);
        let outcome = broker
            .call("list_requirements", &serde_json::json!({}))
            .unwrap();
        assert!(!outcome.is_error, "{}", outcome.text);
        assert!(outcome.text.contains("requirements"), "{}", outcome.text);
        let again = broker
            .call("list_requirements", &serde_json::json!({}))
            .unwrap();
        assert_eq!(again.text, outcome.text);
    }

    #[test]
    fn a_missing_tool_name_is_reported() {
        let dir = project();
        let broker = McpToolBroker::new(WorkflowServer::new(dir.path().to_path_buf()), vec![]);
        let result = broker.call("no_such_tool", &serde_json::json!({}));
        match result {
            Ok(outcome) => assert!(
                outcome.is_error
                    || outcome.text.to_lowercase().contains("not found")
                    || outcome.text.to_lowercase().contains("unknown"),
                "{}",
                outcome.text
            ),
            Err(error) => assert!(!error.0.is_empty()),
        }
    }

    #[test]
    fn a_child_process_failure_is_an_io_error() {
        let dir = project();
        let missing = ServerSpec {
            name: "gone".into(),
            program: "this-binary-does-not-exist-12345".into(),
            args: vec![],
            env: vec![],
        };
        let broker = McpToolBroker::new(
            WorkflowServer::new(dir.path().to_path_buf()),
            vec![missing.clone()],
        );
        let error = broker.discover(&missing).unwrap_err();
        assert!(
            error.0.contains("failed to start")
                || error.0.contains("No such")
                || error.0.contains("not found")
                || error.0.contains("os error"),
            "{}",
            error.0
        );
    }
}

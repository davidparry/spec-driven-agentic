//! Child-process success path: register `CARGO_BIN_EXE_bdd mcp serve` as
//! an external MCP server and list/call through [`McpToolBroker`].

use std::fs;

use bdd_cli::adapters::mcp_client::McpToolBroker;
use bdd_cli::domain::mcp_registry::ServerSpec;
use bdd_cli::mcp::WorkflowServer;
use bdd_cli::ports::{ToolBroker, ToolDiscovery};

const BDD: &str = env!("CARGO_BIN_EXE_bdd");

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

fn self_spec(root: &std::path::Path) -> ServerSpec {
    ServerSpec {
        name: "self".into(),
        program: BDD.into(),
        args: vec![
            "mcp".into(),
            "serve".into(),
            "--root".into(),
            root.display().to_string(),
        ],
        env: vec![],
    }
}

#[test]
fn child_process_discovers_and_calls_list_requirements() {
    let dir = project();
    let spec = self_spec(dir.path());
    let broker = McpToolBroker::new(
        WorkflowServer::new(dir.path().to_path_buf()),
        vec![spec.clone()],
    );
    let tools = broker.discover(&spec).expect("discover");
    assert!(
        tools.iter().any(|t| t.name.ends_with("list_requirements")),
        "{:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    let namespaced = tools
        .iter()
        .find(|t| t.name.ends_with("list_requirements"))
        .unwrap()
        .name
        .clone();
    let outcome = broker
        .call(&namespaced, &serde_json::json!({}))
        .expect("call");
    assert!(!outcome.is_error, "{}", outcome.text);
    assert!(outcome.text.contains("requirements"), "{}", outcome.text);
}

#[test]
fn stdio_and_loopback_list_requirements_match() {
    let dir = project();
    let loopback = McpToolBroker::new(WorkflowServer::new(dir.path().to_path_buf()), vec![]);
    let loopback_out = loopback
        .call("list_requirements", &serde_json::json!({}))
        .expect("loopback");

    let stdio = McpToolBroker::new(WorkflowServer::new(dir.path().to_path_buf()), vec![])
        .with_self_stdio(self_spec(dir.path()));
    let stdio_out = stdio
        .call("list_requirements", &serde_json::json!({}))
        .expect("stdio");
    assert_eq!(loopback_out.is_error, stdio_out.is_error);
    assert_eq!(loopback_out.text, stdio_out.text);
}

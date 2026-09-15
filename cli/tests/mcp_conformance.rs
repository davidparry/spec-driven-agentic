//! MCP conformance: drive the embedded server exactly as an external MCP
//! host would — `server/discover` / `tools/list` / `tools/call`, no
//! `initialize` handshake — and assert the seven frozen tools carry the
//! workshop server's names and reply shapes. Most tests use an in-memory
//! duplex transport around [`WorkflowServer`]; the final test speaks raw
//! newline-delimited JSON-RPC to the real `bdd mcp serve` child process
//! over stdio.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::service::{RoleClient, RunningService};
use rmcp::{ClientLifecycleMode, ClientServiceExt, ServiceExt as _};
use serde_json::{Value, json};

use bdd_cli::domain::model::TestRunSummary;
use bdd_cli::mcp::WorkflowServer;
use bdd_cli::ports::{RunnerError, TestFilter, TestRunner};

const SPEC: &str = r#"{
  "project": "String Calculator Kata",
  "requirements": [
    {
      "id": "REQ-001",
      "title": "Empty string returns zero",
      "status": "pending",
      "story": "As a user, I want an empty string to return 0 so that sums start clean.",
      "acceptanceCriteria": [
        "Given an empty string \"\", when add is called, then the result is 0"
      ],
      "featureFile": "features/calc.feature"
    }
  ]
}"#;

const FEATURE: &str = "@REQ-001\nFeature: String calculator\n\n  @REQ-001\n  Scenario: Empty string returns zero\n    Given a calculator\n    When add is called with \"\"\n    Then the result is 0\n";

fn write_project(root: &Path) {
    fs::create_dir_all(root.join("requirements")).unwrap();
    fs::write(root.join("requirements/requirements.json"), SPEC).unwrap();
    fs::create_dir_all(root.join("features")).unwrap();
    fs::write(root.join("features/calc.feature"), FEATURE).unwrap();
}

struct ScriptedRunner(Result<TestRunSummary, RunnerError>);

impl TestRunner for ScriptedRunner {
    fn run(&self, _: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        self.0.clone()
    }
}

async fn connect(server: WorkflowServer) -> RunningService<RoleClient, ()> {
    let (server_transport, client_transport) = tokio::io::duplex(65536);
    tokio::spawn(async move {
        let service = server
            .serve(server_transport)
            .await
            .expect("server should start");
        let _ = service.waiting().await;
    });
    ().serve_with_lifecycle(
        client_transport,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .expect("client should connect via server/discover")
}

async fn connect_default(root: &Path) -> RunningService<RoleClient, ()> {
    connect(WorkflowServer::new(root.to_path_buf())).await
}

async fn connect_scripted(
    root: &Path,
    outcome: Result<TestRunSummary, RunnerError>,
) -> RunningService<RoleClient, ()> {
    let server = WorkflowServer::with_runner_factory(
        root.to_path_buf(),
        Arc::new(move |_root| Ok(Box::new(ScriptedRunner(outcome.clone())) as Box<dyn TestRunner>)),
    );
    connect(server).await
}

async fn call(
    client: &RunningService<RoleClient, ()>,
    tool: &str,
    arguments: Value,
) -> (Option<bool>, String) {
    let result = client
        .call_tool(
            CallToolRequestParams::new(tool.to_string())
                .with_arguments(arguments.as_object().cloned().unwrap_or_default()),
        )
        .await
        .expect("tool call should complete");
    let text = result
        .content
        .first()
        .and_then(|block| block.as_text())
        .expect("a text content block")
        .text
        .clone();
    (result.is_error, text)
}

async fn call_json(client: &RunningService<RoleClient, ()>, tool: &str, arguments: Value) -> Value {
    let (is_error, text) = call(client, tool, arguments).await;
    assert_ne!(is_error, Some(true), "unexpected error from {tool}: {text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{tool} reply is not JSON ({e}): {text}"))
}

#[tokio::test]
async fn the_server_identifies_as_the_workshop_server_and_lists_all_tools() {
    let dir = tempfile::tempdir().unwrap();
    let client = connect_default(dir.path()).await;

    let info = client.peer_info().expect("server info");
    let implementation = info.server_info.as_ref().expect("server implementation");
    assert_eq!(implementation.name, "spec-driven-server");
    assert_eq!(implementation.title.as_deref(), Some("Spec Driven"));
    assert_eq!(implementation.version, "1.0.0");
    assert_eq!(
        implementation.description.as_deref(),
        Some("Serves spec-driven TDD and BDD tools. The requirements spec is the source of truth.")
    );
    assert_eq!(
        implementation.website_url.as_deref(),
        Some("https://davidparry.github.io/tdd-bdd-agentic/")
    );
    let icons = implementation.icons.as_ref().expect("icons");
    assert_eq!(icons.len(), 1);
    assert_eq!(
        icons[0].src,
        "https://davidparry.github.io/tdd-bdd-agentic/assets/bdd-cli-mark.png"
    );
    assert_eq!(icons[0].mime_type.as_deref(), Some("image/png"));
    assert_eq!(icons[0].sizes, Some(vec!["1024x1024".into()]));
    assert_eq!(icons[0].theme, Some(rmcp::model::IconTheme::Dark));

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for frozen in [
        "list_requirements",
        "get_requirement",
        "validate_spec",
        "refine_requirement",
        "run_tests",
        "get_tdd_state",
        "start_refactor",
    ] {
        assert!(
            names.contains(&frozen),
            "frozen tool {frozen} missing: {names:?}"
        );
    }
    for additive in [
        "project_root",
        "project_inspect",
        "feature_list",
        "feature_read",
        "feature_create",
        "scenario_add",
        "scenario_update",
        "scenario_delete",
        "changes_show",
        "changes_validate",
        "changes_commit",
        "changes_discard",
        "command_run",
        "requirement_mark_implemented",
        "step_definitions_find",
        "step_definition_create",
        "unit_test_create",
    ] {
        assert!(
            names.contains(&additive),
            "additive tool {additive} missing: {names:?}"
        );
    }
    assert_eq!(tools.len(), 24, "tools: {names:?}");

    let root_body = call_json(&client, "project_root", json!({})).await;
    let expected_root = std::path::absolute(dir.path()).unwrap();
    assert_eq!(
        root_body["root"].as_str().unwrap(),
        expected_root.to_string_lossy().as_ref()
    );
    assert!(
        Path::new(root_body["root"].as_str().unwrap()).is_absolute(),
        "project_root must be absolute: {root_body}"
    );
    assert_eq!(
        root_body["nextStep"],
        "Call list_requirements to see the backlog at this root."
    );

    let command_run = tools
        .iter()
        .find(|t| t.name.as_ref() == "command_run")
        .unwrap();
    let schema = serde_json::to_value(command_run.input_schema.as_ref()).unwrap();
    assert!(
        schema["properties"]["command"].is_object(),
        "schema: {schema}"
    );
    assert!(
        schema["properties"]["timeout_secs"].is_object(),
        "schema: {schema}"
    );
    assert_eq!(schema["required"], json!(["command"]), "schema: {schema}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn list_requirements_returns_the_java_shaped_body() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "list_requirements", json!({})).await;
    assert_eq!(body["project"], "String Calculator Kata");
    assert_eq!(body["requirements"][0]["id"], "REQ-001");
    assert_eq!(
        body["requirements"][0]["title"],
        "Empty string returns zero"
    );
    assert_eq!(body["requirements"][0]["status"], "pending");

    client.cancel().await.unwrap();
}

/// A spec split into an include tree serves the same frozen reply
/// shapes: the merged view is what the workshop tools see.
#[tokio::test]
async fn a_spec_split_across_included_files_serves_the_merged_view() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    fs::write(
        dir.path().join("requirements/requirements.json"),
        r#"{
          "project": "String Calculator Kata",
          "includes": ["core/math.json"],
          "requirements": [
            {
              "id": "REQ-001",
              "title": "Empty string returns zero",
              "status": "pending",
              "story": "As a user, I want an empty string to return 0 so that sums start clean.",
              "acceptanceCriteria": [
                "Given an empty string \"\", when add is called, then the result is 0"
              ],
              "featureFile": "features/calc.feature"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("requirements/core")).unwrap();
    fs::write(
        dir.path().join("requirements/core/math.json"),
        r#"{
          "requirements": [
            {
              "id": "REQ-002",
              "title": "Two numbers are summed",
              "status": "pending",
              "story": "As a user, I want comma sums so that totals come from one input.",
              "acceptanceCriteria": [
                "Given \"1,2\", when add is called, then the result is 3"
              ]
            }
          ]
        }"#,
    )
    .unwrap();
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "list_requirements", json!({})).await;
    assert_eq!(body["project"], "String Calculator Kata");
    assert_eq!(body["requirements"][0]["id"], "REQ-001");
    assert_eq!(body["requirements"][1]["id"], "REQ-002");

    let shown = call_json(&client, "get_requirement", json!({"id": "REQ-002"})).await;
    assert_eq!(shown["id"], "REQ-002");
    assert!(shown["workflowHint"].as_str().unwrap().contains("@REQ-002"));

    let validation = call_json(&client, "validate_spec", json!({})).await;
    assert_eq!(validation["valid"], true, "got: {validation}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn get_requirement_is_enriched_and_unknown_ids_name_the_recovery_tool() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "get_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(body["id"], "REQ-001");
    assert_eq!(body["featureLocation"], "features/calc.feature");
    assert!(body["workflowHint"].as_str().unwrap().contains("@REQ-001"));
    assert!(
        body["stepDefinitions"]
            .as_str()
            .unwrap()
            .ends_with("StringCalculatorSteps.java")
    );

    let (is_error, text) = call(&client, "get_requirement", json!({"id": "REQ-999"})).await;
    assert_eq!(is_error, Some(true));
    assert_eq!(
        text,
        "No requirement with id 'REQ-999'. Call list_requirements to see valid ids."
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn validate_spec_reports_valid_with_the_forward_looking_next_step() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "validate_spec", json!({})).await;
    assert_eq!(body["valid"], true);
    assert_eq!(body["issues"], json!([]));
    assert!(
        body["nextStep"]
            .as_str()
            .unwrap()
            .starts_with("The spec is valid.")
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn refine_requirement_reports_clean_and_unknown_ids_error() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(body["id"], "REQ-001");
    assert_eq!(body["clean"], true);
    assert_eq!(body["findings"], json!([]));

    let (is_error, text) = call(&client, "refine_requirement", json!({"id": "REQ-999"})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.starts_with("No requirement with id 'REQ-999'"));

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn run_tests_reports_red_then_start_refactor_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let failing = Ok(TestRunSummary {
        tests: 3,
        failures: 1,
        failure_details: vec!["CalcTest.adds: expected 0".into()],
        ..Default::default()
    });
    let client = connect_scripted(dir.path(), failing).await;

    let body = call_json(&client, "run_tests", json!({})).await;
    assert_eq!(body["phase"], "RED");
    assert_eq!(body["tests"], 3);
    assert_eq!(body["failures"], 1);
    assert_eq!(body["failureDetails"], json!(["CalcTest.adds: expected 0"]));
    assert!(
        body["nextStep"]
            .as_str()
            .unwrap()
            .starts_with("Tests are failing.")
    );

    let (is_error, text) = call(&client, "start_refactor", json!({"note": "cleanup"})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("Never refactor on a red bar"), "got: {text}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn a_green_run_permits_refactor_and_state_carries_the_log() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let passing = Ok(TestRunSummary {
        tests: 3,
        ..Default::default()
    });
    let client = connect_scripted(dir.path(), passing).await;

    let body = call_json(&client, "run_tests", json!({})).await;
    assert_eq!(body["phase"], "GREEN");

    let refactor = call_json(&client, "start_refactor", json!({"note": "extract parser"})).await;
    assert_eq!(refactor["phase"], "REFACTOR");

    let state = call_json(&client, "get_tdd_state", json!({})).await;
    assert_eq!(state["phase"], "REFACTOR");
    assert_eq!(state["lastRun"]["tests"], 3);
    assert_eq!(state["refactorLog"], json!(["extract parser"]));
    assert!(
        state["instructions"]
            .as_str()
            .unwrap()
            .contains("three most recent entries")
    );
    assert_eq!(state["entries"].as_array().unwrap().len(), 2);
    assert!(
        state["entries"][0]["timestamp"]
            .as_str()
            .unwrap()
            .contains('T')
    );
    assert_eq!(state["entries"][1]["phase"], "REFACTOR");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn a_missing_runtime_is_the_structured_refusal() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let missing = Err(RunnerError::RuntimeMissing {
        runtime: "mvn".into(),
        hint: "Install Maven and a JDK.".into(),
    });
    let client = connect_scripted(dir.path(), missing).await;

    let (is_error, text) = call(&client, "run_tests", json!({})).await;
    assert_eq!(is_error, Some(true));
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"], "runtime_missing");
    assert_eq!(body["runtime"], "mvn");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn run_tests_without_a_project_names_the_detection_failure() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let (is_error, text) = call(&client, "run_tests", json!({})).await;
    assert_eq!(is_error, Some(true));
    assert!(
        text.starts_with("No supported project detected"),
        "got: {text}"
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn the_additive_tools_inspect_read_mutate_and_commit() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
    let client = connect_default(dir.path()).await;

    let inspection = call_json(&client, "project_inspect", json!({})).await;
    assert_eq!(inspection["languages"][0]["language"], "Java");

    let features = call_json(&client, "feature_list", json!({})).await;
    assert_eq!(features[0]["path"], "features/calc.feature");

    let doc = call_json(
        &client,
        "feature_read",
        json!({"path": "features/calc.feature"}),
    )
    .await;
    assert_eq!(doc["name"], "String calculator");
    let (is_error, text) = call(
        &client,
        "feature_read",
        json!({"path": "features/nope.feature"}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("no such feature file"));

    let created = call_json(
        &client,
        "feature_create",
        json!({"path": "features/new.feature", "name": "New rules"}),
    )
    .await;
    assert_eq!(created["staged"], true);

    let listed = call_json(&client, "feature_list", json!({})).await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["path"] == "features/new.feature"),
        "staged feature missing from feature_list: {listed}"
    );
    let created_doc = call_json(
        &client,
        "feature_read",
        json!({"path": "features/new.feature"}),
    )
    .await;
    assert_eq!(created_doc["name"], "New rules");

    let validated = call_json(&client, "changes_validate", json!({})).await;
    assert_eq!(validated["valid"], true, "{validated}");

    let added = call_json(
        &client,
        "scenario_add",
        json!({
            "feature": "features/new.feature",
            "req": "REQ-001",
            "name": "First rule",
            "steps": ["Given a calculator", "Then the result is 0"],
        }),
    )
    .await;
    assert_eq!(added["action"], "add");

    let updated = call_json(
        &client,
        "scenario_update",
        json!({
            "feature": "features/new.feature",
            "name": "First rule",
            "steps": ["Given a calculator", "Then the result is 1"],
        }),
    )
    .await;
    assert_eq!(updated["action"], "update");

    let shown = call_json(&client, "changes_show", json!({})).await;
    assert_eq!(shown["changes"].as_array().unwrap().len(), 1);

    let committed = call_json(&client, "changes_commit", json!({})).await;
    assert_eq!(committed["changes"].as_array().unwrap().len(), 1);
    assert!(dir.path().join("features/new.feature").exists());

    let deleted = call_json(
        &client,
        "scenario_delete",
        json!({"feature": "features/new.feature", "name": "First rule"}),
    )
    .await;
    assert_eq!(deleted["action"], "delete");
    let discarded = call_json(&client, "changes_discard", json!({})).await;
    assert_eq!(discarded["changes"].as_array().unwrap().len(), 1);

    let (is_error, text) = call(
        &client,
        "scenario_delete",
        json!({"feature": "features/nope.feature", "name": "X"}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("no such feature file"), "got: {text}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn broken_project_state_surfaces_as_tool_errors_not_crashes() {
    let dir = tempfile::tempdir().unwrap();
    // Corrupt spec: list_requirements reports the repository error.
    fs::create_dir_all(dir.path().join("requirements")).unwrap();
    fs::write(
        dir.path().join("requirements/requirements.json"),
        "not json",
    )
    .unwrap();
    // Broken feature file: feature_list reports the parse error.
    fs::create_dir_all(dir.path().join("features")).unwrap();
    fs::write(dir.path().join("features/broken.feature"), "not gherkin").unwrap();
    // Corrupt TDD state: get_tdd_state reports the state error.
    fs::write(dir.path().join(".bdd-state.json"), "{{{").unwrap();
    // Corrupt staging manifest: the changes tools report the staging error.
    fs::create_dir_all(dir.path().join(".bdd-staged")).unwrap();
    fs::write(dir.path().join(".bdd-staged/manifest.json"), "{{{").unwrap();

    let client = connect_default(dir.path()).await;

    let (is_error, text) = call(&client, "list_requirements", json!({})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("not readable JSON"), "got: {text}");

    let (is_error, text) = call(&client, "feature_list", json!({})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("not valid Gherkin"), "got: {text}");

    let (is_error, _) = call(&client, "get_tdd_state", json!({})).await;
    assert_eq!(is_error, Some(true));

    for tool in ["changes_show", "changes_commit", "changes_discard"] {
        let (is_error, _) = call(&client, tool, json!({})).await;
        assert_eq!(
            is_error,
            Some(true),
            "{tool} should report the broken manifest"
        );
    }

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn mutation_conflicts_surface_as_tool_errors() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let (is_error, text) = call(
        &client,
        "feature_create",
        json!({"path": "features/calc.feature", "name": "Duplicate"}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("already exists"), "got: {text}");

    let (is_error, text) = call(
        &client,
        "scenario_add",
        json!({
            "feature": "features/nope.feature",
            "req": "REQ-001",
            "name": "X",
            "steps": ["Given a"],
        }),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("no such feature file"), "got: {text}");

    let (is_error, text) = call(
        &client,
        "scenario_update",
        json!({"feature": "features/calc.feature", "name": "Nope", "steps": ["Given a"]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("Nope"), "got: {text}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn command_run_refuses_disallowed_programs_and_escapes() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let (is_error, text) = call(
        &client,
        "command_run",
        json!({"command": ["rm", "-rf", "."]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("not on the allowlist"), "got: {text}");

    let (is_error, text) = call(
        &client,
        "command_run",
        json!({"command": ["cargo", "build", "--manifest-path", "../other/Cargo.toml"]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains(".."), "got: {text}");

    let (is_error, text) = call(
        &client,
        "command_run",
        json!({"command": ["javac", "/etc/passwd"]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("absolute paths"), "got: {text}");

    let (is_error, text) = call(
        &client,
        "command_run",
        json!({"command": ["node", "-e", "process.exit(0)"]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("arbitrary code"), "got: {text}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn command_run_is_refused_off_a_red_bar() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    // A fresh project is at START: no RED bar, no implementation phase.
    let (is_error, text) = call(
        &client,
        "command_run",
        json!({"command": ["cargo", "check"]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("current phase: START"), "got: {text}");
    assert!(text.contains("run_tests"), "got: {text}");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn command_run_on_a_red_bar_executes_inside_the_root() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let failing = Ok(TestRunSummary {
        tests: 1,
        failures: 1,
        failure_details: vec!["CalcTest.adds: expected 0".into()],
        ..Default::default()
    });
    let client = connect_scripted(dir.path(), failing).await;

    let body = call_json(&client, "run_tests", json!({})).await;
    assert_eq!(body["phase"], "RED");

    // `cargo` is on the allowlist and guaranteed present under cargo test.
    let report = call_json(
        &client,
        "command_run",
        json!({"command": ["cargo", "--version"], "timeout_secs": 60}),
    )
    .await;
    assert_eq!(report["command"], json!(["cargo", "--version"]));
    assert_eq!(report["exitCode"], 0);
    assert!(
        report["stdout"].as_str().unwrap().contains("cargo"),
        "got: {report}"
    );
    assert_eq!(report["timedOut"], false);
    assert!(report["durationMs"].is_u64());
    assert!(
        report["nextStep"].as_str().unwrap().contains("run_tests"),
        "got: {report}"
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn requirement_mark_implemented_is_gated_on_green_and_a_tagged_scenario() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;
    let (is_error, text) = call(
        &client,
        "requirement_mark_implemented",
        json!({"id": "REQ-001"}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("GREEN"), "{text}");
    assert!(text.contains("START"), "{text}");
    client.cancel().await.unwrap();

    let passing = Ok(TestRunSummary {
        tests: 1,
        ..Default::default()
    });
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    fs::write(
        dir.path().join("requirements/requirements.json"),
        r#"{
  "project": "String Calculator Kata",
  "requirements": [
    {
      "id": "REQ-001",
      "title": "Empty string returns zero",
      "status": "pending",
      "story": "As a user, I want an empty string to return 0 so that sums start clean.",
      "acceptanceCriteria": ["Given an empty string \"\", when add is called, then the result is 0"]
    }
  ]
}"#,
    )
    .unwrap();
    fs::remove_file(dir.path().join("features/calc.feature")).unwrap();
    let client = connect_scripted(dir.path(), passing.clone()).await;
    call_json(&client, "run_tests", json!({})).await;
    let (is_error, text) = call(
        &client,
        "requirement_mark_implemented",
        json!({"id": "REQ-001"}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("No scenario is tagged @REQ-001"), "{text}");
    client.cancel().await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_scripted(dir.path(), passing).await;
    let run = call_json(&client, "run_tests", json!({})).await;
    assert_eq!(run["phase"], "GREEN");
    let body = call_json(
        &client,
        "requirement_mark_implemented",
        json!({"id": "REQ-001"}),
    )
    .await;
    assert_eq!(body["id"], "REQ-001");
    assert_eq!(body["status"], "implemented");
    assert_eq!(body["staged"], true);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn generation_tools_are_template_only_and_name_bdd_inspect_without_a_language() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
    let client = connect_default(dir.path()).await;

    let missing = call_json(&client, "step_definitions_find", json!({})).await;
    assert!(missing["missing"].is_array(), "{missing}");
    assert_eq!(missing["language"], "Java");

    let created = call_json(&client, "step_definition_create", json!({})).await;
    assert_eq!(created["source"], "template");
    assert_eq!(created["staged"], true);

    let unit = call_json(&client, "unit_test_create", json!({"req_id": "REQ-001"})).await;
    assert_eq!(unit["source"], "template");
    assert_eq!(unit["staged"], true);

    let (is_error, text) = call(&client, "unit_test_create", json!({"req_id": "REQ-999"})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("REQ-999"), "{text}");
    client.cancel().await.unwrap();

    let empty = tempfile::tempdir().unwrap();
    let client = connect_default(empty.path()).await;
    let (is_error, text) = call(&client, "step_definitions_find", json!({})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("bdd inspect"), "{text}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn new_tool_schemas_require_the_documented_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let client = connect_default(dir.path()).await;
    let tools = client.list_all_tools().await.unwrap();
    let schema_of = |name: &str| {
        let tool = tools.iter().find(|t| t.name.as_ref() == name).unwrap();
        serde_json::to_value(tool.input_schema.as_ref()).unwrap()
    };
    assert_eq!(
        schema_of("requirement_mark_implemented")["required"],
        json!(["id"])
    );
    assert_eq!(schema_of("unit_test_create")["required"], json!(["req_id"]));
    let find = schema_of("step_definitions_find");
    let required = find.get("required");
    assert!(required.is_none() || required == Some(&json!([])), "{find}");
    let create = schema_of("step_definition_create");
    let required = create.get("required");
    assert!(
        required.is_none() || required == Some(&json!([])),
        "{create}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn builtin_tool_definitions_match_the_wire_list() {
    use bdd_cli::mcp::builtin_tool_definitions;
    let dir = tempfile::tempdir().unwrap();
    let client = connect_default(dir.path()).await;
    let wire = client.list_all_tools().await.unwrap();
    let offline = builtin_tool_definitions();
    assert_eq!(offline.len(), wire.len());
    let mut offline_names: Vec<_> = offline.iter().map(|t| t.name.as_str()).collect();
    let mut wire_names: Vec<_> = wire.iter().map(|t| t.name.as_ref()).collect();
    offline_names.sort();
    wire_names.sort();
    assert_eq!(offline_names, wire_names);
    for tool in &wire {
        let offline_tool = offline
            .iter()
            .find(|t| t.name == tool.name.as_ref())
            .unwrap();
        let mut wire_schema = serde_json::to_value(tool.input_schema.as_ref()).unwrap();
        if let Some(object) = wire_schema.as_object_mut() {
            object.remove("$schema");
            object.remove("title");
        }
        assert_eq!(offline_tool.schema, wire_schema, "{}", tool.name);
    }
    client.cancel().await.unwrap();
}

fn request_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {
            "name": "conformance-test",
            "version": "0"
        },
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// The real binary over real stdio: tools/list then tools/call, no
/// `initialize` handshake. Newline-delimited JSON-RPC with per-request
/// `_meta`, the 2026-07-28 lifecycle.
#[test]
fn the_bdd_binary_serves_mcp_over_child_process_stdio_without_initialize() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_bdd"))
        .args(["--root", dir.path().to_str().unwrap(), "mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("bdd mcp serve starts");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |value: Value| {
        stdin
            .write_all((value.to_string() + "\n").as_bytes())
            .unwrap();
    };
    let mut receive = || -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };

    send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": { "_meta": request_meta() },
    }));
    let tools = receive();
    assert!(
        tools.get("error").is_none(),
        "tools/list without initialize should succeed: {tools}"
    );
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"list_requirements"), "tools: {names:?}");

    send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "list_requirements",
            "arguments": {},
            "_meta": request_meta(),
        },
    }));
    let reply = receive();
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let body: Value = serde_json::from_str(text).unwrap();
    assert_eq!(body["project"], "String Calculator Kata");
    assert_eq!(body["requirements"][0]["id"], "REQ-001");

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success(), "server exited with {status}");
}

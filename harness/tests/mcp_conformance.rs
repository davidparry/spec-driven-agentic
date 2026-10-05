//! MCP conformance: drive the embedded server exactly as an external MCP
//! host would — `server/discover` / `tools/list` / `tools/call`, no
//! `initialize` handshake — and assert the seven frozen tools carry the
//! workshop server's names and reply shapes. Most tests use an in-memory
//! duplex transport around [`WorkflowServer`]; the final test speaks raw
//! newline-delimited JSON-RPC to the real `spec mcp serve` child process
//! over stdio.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::service::{RoleClient, RunningService};
use rmcp::{ClientLifecycleMode, ClientServiceExt, ServiceExt as _};
use serde_json::{Value, json};

use spec_harness::adapters::spec_home::spec_file;
use spec_harness::domain::model::TestRunSummary;
use spec_harness::domain::{STAGED_DIR, STATE_FILE};
use spec_harness::mcp::WorkflowServer;
use spec_harness::ports::{RunnerError, TestFilter, TestRunner};

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
        Some("https://davidparry.github.io/spec-driven-agentic/")
    );
    let icons = implementation.icons.as_ref().expect("icons");
    assert_eq!(icons.len(), 1);
    assert_eq!(
        icons[0].src,
        "https://davidparry.github.io/spec-driven-agentic/assets/spec-harness-mark.png"
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
        "requirement_reword",
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
    assert_eq!(tools.len(), 25, "tools: {names:?}");

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
    assert_eq!(
        body["requirements"][0]["file"],
        "requirements/requirements.json"
    );
    // The frozen fields keep their names and their wire order; file is
    // added after them, so nothing downstream shifts. Checked on the raw
    // text because a parsed map sorts its keys.
    let (_, text) = call(&client, "list_requirements", json!({})).await;
    let at = |field: &str| text.find(&format!("\"{field}\"")).expect(field);
    assert!(at("project") < at("id") && at("id") < at("title"), "{text}");
    assert!(
        at("title") < at("status") && at("status") < at("file"),
        "{text}"
    );

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
    // Merged is not enough once the catalog is split: an agent over MCP
    // has to know which document holds a requirement, the way spec list
    // has always told the shell.
    assert_eq!(
        body["requirements"][0]["file"],
        "requirements/requirements.json"
    );
    assert_eq!(
        body["requirements"][1]["file"],
        "requirements/core/math.json"
    );

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

/// `decision.mode = "off"` has to reach this tool, which judges on its
/// own initiative rather than because a human typed anything.
///
/// The endpoint is a closed port, so the two cases are told apart by
/// what the reply carries: honouring `off` asks nothing and says
/// nothing, while asking would fail against that port and leave a
/// `judgmentNote`. Driven over the real transport because the bug this
/// pins was in the tool's own wiring, not in the service beneath it.
#[tokio::test]
async fn refine_requirement_asks_no_judgment_when_the_mode_is_off() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    write_decision_config(dir.path(), &closed_endpoint(), "off");
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;

    assert_eq!(body["clean"], true, "{body}");
    assert_eq!(body.get("judgments"), None, "{body}");
    assert_eq!(body.get("judgmentNote"), None, "nothing was asked: {body}");
    assert_eq!(body.get("judgmentAction"), None, "{body}");
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
    assert_eq!(body["source"], "working tree");

    // requirement_reword stages. Refining after it has to review the
    // staged wording, or vague text the developer just wrote comes back
    // clean because only the untouched on-disk copy was ever read.
    call_json(
        &client,
        "requirement_reword",
        json!({"id": "REQ-001", "story": "the calculator should handle newlines quickly"}),
    )
    .await;
    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(body["clean"], false, "{body}");
    assert_eq!(body["source"], "staged", "{body}");
    assert!(
        body["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f.as_str().unwrap().contains("missing the actor")),
        "{body}"
    );
    // And the loop converges without the agent inventing a commit.
    let next_step = body["nextStep"].as_str().unwrap();
    assert!(
        next_step.contains("no need to commit between passes"),
        "{next_step}"
    );

    // The pass right after a fixing reword is clean, still with nothing
    // committed, and points at the commit as the way to apply it.
    call_json(
        &client,
        "requirement_reword",
        json!({
            "id": "REQ-001",
            "story": "As a user, I want newline sums so that multi-line input works.",
        }),
    )
    .await;
    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(body["clean"], true, "{body}");
    assert_eq!(body["source"], "staged", "{body}");
    assert!(
        body["nextStep"]
            .as_str()
            .unwrap()
            .contains("changes_commit"),
        "{body}"
    );
    call_json(&client, "changes_discard", json!({})).await;

    let (is_error, text) = call(&client, "refine_requirement", json!({"id": "REQ-999"})).await;
    assert_eq!(is_error, Some(true));
    assert!(text.starts_with("No requirement with id 'REQ-999'"));

    client.cancel().await.unwrap();
}

/// A one-shot HTTP server answering a single decision request.
fn serve_one_decision(status: u16, body: &'static str) -> String {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buffer = [0u8; 16384];
        let _ = stream.read(&mut buffer);
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        );
    });
    format!("http://127.0.0.1:{port}")
}

/// A stub Ollama complete enough for the discovery path: `/api/tags`
/// lists one model, `/api/show` reports it decision-capable, and
/// anything else is answered as the judgment. Serves connections in a
/// loop because resolving the model and asking it are separate calls.
fn serve_discoverable_decision() -> String {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buffer = [0u8; 16384];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body = if request.contains("/api/tags") {
                r#"{"models":[{"name":"nimble:test"}]}"#
            } else if request.contains("/api/show") {
                r#"{"capabilities":["decision"]}"#
            } else {
                r#"{"model":"nimble:test",
                    "answers":{"measurable":{"type":"noul","noul":0.04}},
                    "usage":{"input_tokens":151,"output_tokens":1}}"#
            };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// A port nothing is listening on: a decision call aimed here fails
/// immediately, so "no judgment was asked for" and "a judgment failed"
/// stay distinguishable in the reply.
fn closed_endpoint() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("a local address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

fn write_decision_config(root: &Path, endpoint: &str, mode: &str) {
    fs::create_dir_all(root.join(".spec")).unwrap();
    fs::write(
        root.join(".spec/config.toml"),
        format!(
            "[decision]\nmodel = \"nimble:test\"\nendpoint = \"{endpoint}\"\nmode = \"{mode}\"\n"
        ),
    )
    .unwrap();
}

/// With nothing on this machine able to answer, the reply is exactly
/// what it has always been: the deterministic findings and no new keys.
///
/// Configuring a model is no longer what switches judgments on - they
/// are on wherever a decision-capable model is installed - so what
/// keeps the keys out is having nothing to ask. The closed endpoint is
/// what makes that true here whatever the machine running the test has
/// pulled; against a live Ollama this would turn on whether the
/// developer happened to have a decision model.
#[tokio::test]
async fn refine_requirement_carries_no_judgment_keys_when_nothing_can_answer() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    fs::create_dir_all(dir.path().join(".spec")).unwrap();
    fs::write(
        dir.path().join(".spec/config.toml"),
        format!("[decision]\nendpoint = \"{}\"\n", closed_endpoint()),
    )
    .unwrap();
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    for absent in [
        "judgments",
        "judgmentAction",
        "judgmentAdvisories",
        "judgmentNote",
    ] {
        assert!(
            body.get(absent).is_none(),
            "{absent} should be absent, keys were {keys:?}"
        );
    }

    client.cancel().await.unwrap();
}

/// Judgments are on by default: a project that configured nothing still
/// gets one, because the provider is asked which of its models can
/// answer and the first is borrowed for the run.
///
/// The end of the chain that the unit tests cover a link at a time -
/// here the model is never named anywhere, and a judgment comes back
/// over the wire regardless.
#[tokio::test]
async fn refine_requirement_judges_with_no_decision_model_configured_at_all() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    fs::create_dir_all(dir.path().join(".spec")).unwrap();
    fs::write(
        dir.path().join(".spec/config.toml"),
        format!(
            "[decision]\nendpoint = \"{}\"\n",
            serve_discoverable_decision()
        ),
    )
    .unwrap();
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert!(
        body.get("judgments").is_some(),
        "nothing was configured and a judgment still came back; keys were {:?}",
        body.as_object().unwrap().keys().collect::<Vec<_>>()
    );

    client.cancel().await.unwrap();
}

/// The gate, over the real transport. This tool used to weaken an
/// enforcing project to advisory on the grounds that an exit code is
/// what a human watches — but this is the surface the agent loop
/// actually drives, so that put the gate out of reach exactly where it
/// was needed.
///
/// The verdict has to land in `findings` and `clean`, because those are
/// the two fields the agent is already told to iterate on. A new key
/// would be a gate the loop has to be taught about.
#[tokio::test]
async fn refine_requirement_gates_on_a_judgment_when_the_project_enforces() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let endpoint = serve_one_decision(
        200,
        r#"{"model":"nimble:test",
            "answers":{"measurable":{"type":"noul","noul":0.04}},
            "usage":{"input_tokens":151,"output_tokens":1}}"#,
    );
    write_decision_config(dir.path(), &endpoint, "enforce");
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(
        body["clean"], false,
        "a gating judgment is not a clean reply: {body}"
    );
    let findings = body["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 1, "{body}");
    assert!(
        findings[0]
            .as_str()
            .unwrap()
            .starts_with("judgment (measurable/v1):"),
        "a probabilistic finding has to be labelled as one: {body}"
    );
    assert!(
        body["nextStep"]
            .as_str()
            .unwrap()
            .contains("requirement_reword"),
        "the advice has to name the fix: {body}"
    );

    let judgments = body["judgments"].as_array().expect("judgments");
    assert_eq!(judgments.len(), 1, "{body}");
    let judgment = &judgments[0];
    assert_eq!(judgment["question"], "measurable/v1");
    assert_eq!(judgment["model"], "nimble:test");
    assert_eq!(judgment["verdict"], "FAILS");
    assert_eq!(judgment["action"], "REWORK", "{body}");
    assert_eq!(judgment["mode"], "enforce", "{body}");
    assert!(
        judgment["provenance"]["state"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(body["judgmentAction"], "REWORK", "{body}");
    assert_eq!(
        body["judgmentAdvisories"], body["findings"],
        "the same lines, reported in both places: {body}"
    );

    client.cancel().await.unwrap();
}

/// `advisory` is the opt-out, and this is what opting out buys: the
/// judgment is reported beside the deterministic verdict and neither
/// `clean` nor `findings` moves. Kept so the gating test above is the
/// mode talking rather than something that now always happens.
#[tokio::test]
async fn refine_requirement_reports_without_gating_when_the_project_is_advisory() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let endpoint = serve_one_decision(
        200,
        r#"{"model":"nimble:test",
            "answers":{"measurable":{"type":"noul","noul":0.04}},
            "usage":{"input_tokens":151,"output_tokens":1}}"#,
    );
    write_decision_config(dir.path(), &endpoint, "advisory");
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(
        body["clean"], true,
        "the deterministic verdict stands: {body}"
    );
    assert_eq!(body["findings"], json!([]), "{body}");
    assert_eq!(body["judgments"][0]["verdict"], "FAILS", "{body}");
    assert_eq!(body["judgmentAction"], "CONTINUE", "{body}");
    assert_eq!(
        body["judgmentAdvisories"].as_array().unwrap().len(),
        1,
        "the line is still reported, just not acted on: {body}"
    );

    client.cancel().await.unwrap();
}

/// A question that was wanted and never answered is not wording an
/// agent can reword, so an enforcing project gets a tool error rather
/// than a finding the loop would retry forever. Never an approval
/// either way.
#[tokio::test]
async fn refine_requirement_refuses_rather_than_approving_when_an_enforced_judgment_cannot_be_taken()
 {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    write_decision_config(dir.path(), &closed_endpoint(), "enforce");
    let client = connect_default(dir.path()).await;

    let (is_error, text) = call(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(is_error, Some(true), "{text}");
    assert!(
        text.contains("decision gate refused to pass without an answer"),
        "{text}"
    );

    client.cancel().await.unwrap();
}

/// An unreachable decision model leaves the wording review working and
/// says plainly that no judgment was taken.
#[tokio::test]
async fn refine_requirement_survives_an_unreachable_decision_model() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    write_decision_config(dir.path(), &closed_endpoint(), "advisory");
    let client = connect_default(dir.path()).await;

    let body = call_json(&client, "refine_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(body["clean"], true, "{body}");
    assert!(body.get("judgments").is_none(), "{body}");
    let note = body["judgmentNote"].as_str().expect("a note");
    assert!(note.contains("no judgment"), "{note}");

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
async fn requirement_reword_stages_criteria_whose_escaping_survives_the_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    // A custom-delimiter criterion: embedded quotes plus a literal
    // backslash-n. Hand-editing this in the spec file is what the tool
    // exists to replace, so the escaping has to survive untouched.
    let criterion = "Given \"//+\\n1+2\", when add is called, then the result is 3";
    let body = call_json(
        &client,
        "requirement_reword",
        json!({
            "id": "REQ-001",
            "title": "Custom delimiters can be declared on the first line",
            "acceptance_criteria": [criterion],
        }),
    )
    .await;
    assert_eq!(body["id"], "REQ-001");
    assert_eq!(body["staged"], true);
    // Over MCP the next step names tools, never harness commands the agent
    // has no shell to run.
    let next_step = body["nextStep"].as_str().unwrap();
    assert!(next_step.contains("changes_commit"), "{next_step}");
    assert!(!next_step.contains("spec "), "{next_step}");

    let validated = call_json(&client, "changes_validate", json!({})).await;
    assert_eq!(validated["valid"], true, "{validated}");
    call_json(&client, "changes_commit", json!({})).await;

    let shown = call_json(&client, "get_requirement", json!({"id": "REQ-001"})).await;
    assert_eq!(shown["acceptanceCriteria"], json!([criterion]));
    assert_eq!(
        shown["title"],
        "Custom delimiters can be declared on the first line"
    );
    // The story and featureFile were not passed, so they stand.
    assert_eq!(shown["featureLocation"], "features/calc.feature", "{shown}");

    let on_disk = fs::read_to_string(dir.path().join("requirements/requirements.json")).unwrap();
    assert!(
        on_disk.contains(r#""Given \"//+\\n1+2\", when add is called, then the result is 3""#),
        "escaping was mangled on disk: {on_disk}"
    );
    // Spec JSON is a text file like the Gherkin and Java the harness
    // writes: it ends in a newline, so students' diffs do not carry a
    // "\ No newline at end of file" marker.
    assert!(
        on_disk.ends_with("}\n") && !on_disk.ends_with("}\n\n"),
        "spec file does not end in exactly one newline: {:?}",
        &on_disk[on_disk.len().saturating_sub(16)..]
    );

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn requirement_reword_refuses_unknown_ids_and_criteria_that_are_not_given_when_then() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let client = connect_default(dir.path()).await;

    let (is_error, text) = call(&client, "requirement_reword", json!({"id": "REQ-999"})).await;
    assert_eq!(is_error, Some(true));
    assert!(
        text.starts_with("No requirement with id 'REQ-999'"),
        "{text}"
    );

    let (is_error, text) = call(
        &client,
        "requirement_reword",
        json!({"id": "REQ-001", "acceptance_criteria": ["the result should be 6"]}),
    )
    .await;
    assert_eq!(is_error, Some(true));
    assert!(text.contains("must be phrased Given/When/Then"), "{text}");

    let shown = call_json(&client, "changes_show", json!({})).await;
    assert_eq!(
        shown["changes"].as_array().unwrap().len(),
        0,
        "a rejected reword must not stage anything: {shown}"
    );

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
    let state = spec_file(dir.path(), STATE_FILE);
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(&state, "{{{").unwrap();
    // Corrupt staging manifest: the changes tools report the staging error.
    let staged = spec_file(dir.path(), STAGED_DIR);
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("manifest.json"), "{{{").unwrap();

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
async fn generation_tools_are_template_only_and_name_spec_inspect_without_a_language() {
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
    assert!(text.contains("spec inspect"), "{text}");
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
    assert_eq!(schema_of("requirement_reword")["required"], json!(["id"]));
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
    use spec_harness::mcp::builtin_tool_definitions;
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
fn the_spec_binary_serves_mcp_over_child_process_stdio_without_initialize() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_spec"))
        .args(["--root", dir.path().to_str().unwrap(), "mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spec mcp serve starts");

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

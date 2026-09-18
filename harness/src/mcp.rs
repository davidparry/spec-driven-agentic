//! The embedded MCP server: `spec mcp serve` exposes the workflow over
//! stdio. Frozen seven-tool reply shapes stay (`harness/tests/mcp_conformance.rs`);
//! the additive typed tools expose inspection and staged mutation. This
//! module is a delivery mechanism like `main.rs`: it wires the same
//! application services onto a transport, so it is allowed to name
//! concrete adapters. The test-runner factory is injectable so every
//! reply path is testable without real runtimes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Icon, IconTheme, Implementation, ProtocolVersion,
    ServerCapabilities, ServerConfig,
};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::adapters::fs_project::FsProjectFiles;
use crate::adapters::fs_spec::FsSpecRepository;
use crate::adapters::fs_staging::FsChangeStore;
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::noop_llm::NoLlm;
use crate::adapters::process_exec::ProcessCommandExecutor;
use crate::adapters::process_runtime::ProcessRuntimeProbe;
use crate::adapters::runners::detect_runner;
use crate::application::agent_service::NullPrompter;
use crate::application::command_service::CommandService;
use crate::application::generation_service::{GenerationService, ResolvedLlm};
use crate::application::inspect_service::InspectService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::tdd_service::{TddError, TddService};
use crate::domain::tools::{ToolDefinition, ToolOrigin};
use crate::ports::{FeatureCatalog as _, SpecRepository as _, TestFilter, TestRunner};
use crate::wiring;
use crate::workspace::{primary_language, workshop_layout};

type RunnerFactory = Arc<dyn Fn(&Path) -> Result<Box<dyn TestRunner>, String> + Send + Sync>;
type McpGenerationService = GenerationService<
    wiring::OverlayFeatures,
    wiring::OverlayTree,
    FsChangeStore,
    FsSpecRepository,
    NoLlm,
>;

#[derive(Deserialize, JsonSchema)]
pub struct IdParam {
    /// The requirement id, e.g. REQ-003
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReqIdParam {
    /// The requirement id, e.g. REQ-003
    pub req_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct NoteParam {
    /// What you intend to refactor and why
    #[serde(default)]
    #[schemars(schema_with = "nullable_string")]
    pub note: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FeaturePathParam {
    /// Feature file path relative to the project root
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct FeatureCreateParams {
    /// Feature file path relative to the project root
    pub path: String,
    /// Feature name (the text after "Feature:")
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScenarioAddParams {
    /// Feature file path relative to the project root
    pub feature: String,
    /// Requirement id the scenario implements (tagged @REQ-...)
    pub req: String,
    /// Scenario name
    pub name: String,
    /// Full Gherkin steps, e.g. "Given a calculator"
    pub steps: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScenarioUpdateParams {
    /// Feature file path relative to the project root
    pub feature: String,
    /// Scenario name
    pub name: String,
    /// New requirement id for the tag; omit to keep the current tag
    #[serde(default)]
    #[schemars(schema_with = "nullable_string")]
    pub req: Option<String>,
    /// New steps; omit to keep the current steps
    #[serde(default)]
    #[schemars(schema_with = "nullable_string_array")]
    pub steps: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScenarioDeleteParams {
    /// Feature file path relative to the project root
    pub feature: String,
    /// Scenario name
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CommandRunParams {
    /// The command as argv: the program first, then its arguments. No
    /// shell is involved, so pipes, redirection, and chaining do not work.
    pub command: Vec<String>,
    /// Timeout in seconds; default and maximum 300.
    #[serde(default)]
    #[schemars(schema_with = "nullable_u64")]
    pub timeout_secs: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RequirementRewordParams {
    /// The requirement id, e.g. REQ-003
    pub id: String,
    /// New title; omit to keep the current one
    #[serde(default)]
    #[schemars(schema_with = "nullable_string")]
    pub title: Option<String>,
    /// New user story; omit to keep the current one
    #[serde(default)]
    #[schemars(schema_with = "nullable_string")]
    pub story: Option<String>,
    /// The full new set of acceptance criteria, each phrased
    /// Given/When/Then. Replaces the existing list; omit to keep it.
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

/// The MCP delivery of the workflow: seven frozen tools plus the
/// additive typed tools, all backed by the same application services the
/// harness commands use.
pub struct WorkflowServer {
    root: PathBuf,
    runner_factory: RunnerFactory,
    staging: StagingLock,
    // Read by the `#[tool_handler]`-generated `ServerHandler` impl.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl Clone for WorkflowServer {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            runner_factory: self.runner_factory.clone(),
            // Every clone guards the same staging area, so the lock has to
            // be shared rather than rebuilt.
            staging: Arc::clone(&self.staging),
            tool_router: Self::tool_router(),
        }
    }
}

/// Serializes the staging area's read-modify-write cycle.
///
/// Staging a mutation is three steps — read the effective content, apply
/// the edit, write the file and the manifest — spread across a service and
/// an adapter. Hosts are free to dispatch a batch of tool calls
/// concurrently, and two interleaved cycles silently lose one edit: both
/// callers read the same base, and the second write wins. Tools that touch
/// the staging area take this lock for the whole cycle.
type StagingLock = Arc<tokio::sync::Mutex<()>>;

/// JSON Schema `type: ["string","null"]` is legal but several MCP clients
/// read `type` as a single string and drop the constraint. `anyOf` with one
/// type per branch is the portable form Inspector asks for.
fn nullable(inner: serde_json::Value) -> schemars::Schema {
    serde_json::json!({
        "anyOf": [inner, { "type": "null" }]
    })
    .try_into()
    .expect("object schema")
}

fn nullable_string(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    nullable(serde_json::json!({ "type": "string" }))
}

fn nullable_string_array(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    nullable(serde_json::json!({
        "type": "array",
        "items": { "type": "string" }
    }))
}

fn nullable_u64(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    nullable(serde_json::json!({ "type": "integer", "minimum": 0 }))
}

/// Logs a tool invocation and opens a span so the result events emitted
/// by `json_result`/`error_result` carry the tool name. Safe to hold in
/// the async handlers because none of them await while it is alive.
fn tool_call(tool: &'static str) -> tracing::span::EnteredSpan {
    let span = tracing::info_span!("mcp", tool);
    let entered = span.entered();
    tracing::info!("MCP tool invoked");
    entered
}

fn json_result<T: serde::Serialize>(body: &T) -> CallToolResult {
    match serde_json::to_string_pretty(body) {
        Ok(text) => {
            tracing::debug!(result = %text, "MCP tool result");
            CallToolResult::success(vec![ContentBlock::text(text)])
        }
        Err(error) => error_result(format!("could not serialize tool result: {error}")),
    }
}

fn error_result(message: impl std::fmt::Display) -> CallToolResult {
    let message = message.to_string();
    tracing::warn!(error = %message, "MCP tool error");
    CallToolResult::error(vec![ContentBlock::text(message)])
}

/// A TDD-loop failure as a tool reply: missing runtimes keep the harness's
/// structured `runtime_missing` shape, everything else is the message.
fn tdd_error_result(error: TddError) -> CallToolResult {
    match error {
        TddError::RuntimeMissing { runtime, hint } => error_result(
            serde_json::to_string_pretty(&serde_json::json!({
                "error": "runtime_missing",
                "runtime": runtime,
                "hint": hint,
            }))
            .unwrap_or_else(|_| {
                format!(r#"{{"error":"runtime_missing","runtime":"{runtime}","hint":"{hint}"}}"#)
            }),
        ),
        TddError::Other(message) => error_result(message),
    }
}

#[tool_router]
impl WorkflowServer {
    pub fn new(root: PathBuf) -> Self {
        Self::with_runner_factory(root, Arc::new(detect_runner))
    }

    /// Constructor for tests: inject a scripted runner factory so every
    /// `run_tests` reply path is reachable without real runtimes.
    pub fn with_runner_factory(root: PathBuf, runner_factory: RunnerFactory) -> Self {
        Self {
            root: std::path::absolute(&root).unwrap_or(root),
            runner_factory,
            staging: StagingLock::default(),
            tool_router: Self::tool_router(),
        }
    }

    /// Hold for the whole read-modify-write cycle of a staging mutation.
    async fn staging_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.staging.lock().await
    }

    fn spec_repository(&self) -> FsSpecRepository {
        wiring::spec_repository(&self.root)
    }

    fn spec_service(
        &self,
    ) -> crate::application::spec_service::SpecService<
        FsSpecRepository,
        crate::adapters::fs_spec::FsFeatureFiles,
    > {
        wiring::spec_service(&self.root, workshop_layout())
    }

    fn scenario_service(
        &self,
    ) -> crate::application::scenario_service::ScenarioService<FsChangeStore, wiring::OverlayFeatures>
    {
        wiring::scenario_service(&self.root)
    }

    fn change_service(
        &self,
    ) -> crate::application::change_service::ChangeService<
        FsChangeStore,
        FsSpecRepository,
        wiring::OverlayFeatures,
    > {
        wiring::change_service(&self.root)
    }

    fn tdd_service(&self) -> TddService<FsStateStore> {
        wiring::tdd_service(&self.root)
    }

    fn mutation_service(
        &self,
    ) -> SpecMutationService<FsSpecRepository, wiring::OverlayFeatures, FsChangeStore, FsStateStore>
    {
        wiring::mutation_service(&self.root, crate::application::DEFAULT_LLM_ATTEMPTS)
    }

    fn overlay_catalog(&self) -> wiring::OverlayFeatures {
        wiring::overlay_catalog(&self.root)
    }

    fn generation_service(&self) -> Result<McpGenerationService, String> {
        let language = primary_language(&self.root)?;
        let layout = crate::workspace::project_layout(&self.root);
        Ok(GenerationService::new(
            wiring::overlay_catalog(&self.root),
            wiring::overlay_sources(&self.root, layout.module_root.as_deref()),
            wiring::change_store(&self.root),
            self.spec_repository(),
            language,
            layout,
            None::<ResolvedLlm<NoLlm>>,
        ))
    }

    // ---- the seven frozen tools (workshop server contract) ----------------

    #[tool(
        description = "List every requirement of the kata with its id, title, and \
        implementation status. Use this to find pending work."
    )]
    async fn list_requirements(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("list_requirements");
        // Typed body (not json!) so the field order matches the Java
        // server's LinkedHashMap output: project, then id/title/status.
        #[derive(serde::Serialize)]
        struct Row<'a> {
            id: &'a str,
            title: &'a str,
            status: &'a str,
        }
        #[derive(serde::Serialize)]
        struct Body<'a> {
            project: &'a str,
            requirements: Vec<Row<'a>>,
        }
        Ok(match self.spec_repository().load() {
            Err(e) => error_result(e),
            Ok(spec) => json_result(&Body {
                project: &spec.project,
                requirements: spec
                    .requirements
                    .iter()
                    .map(|r| Row {
                        id: &r.id,
                        title: &r.title,
                        status: &r.status,
                    })
                    .collect(),
            }),
        })
    }

    #[tool(
        description = "Get the user story and acceptance criteria for one requirement. \
        Turn each acceptance criterion into a failing test before writing production code."
    )]
    async fn get_requirement(
        &self,
        Parameters(params): Parameters<IdParam>,
    ) -> Result<CallToolResult, McpError> {
        let _span = tool_call("get_requirement");
        tracing::debug!(id = %params.id, "tool arguments");
        Ok(match self.spec_service().get_requirement(&params.id) {
            Ok(requirement) => json_result(&requirement),
            Err(e) => error_result(e),
        })
    }

    #[tool(
        description = "Validate the requirements spec on disk. Call this after every edit \
        to the requirements file and fix the reported issues until valid is true — only a \
        valid spec is worth turning into scenarios and code. Implemented requirements must \
        have tagged Gherkin scenarios in their feature file."
    )]
    async fn validate_spec(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("validate_spec");
        Ok(json_result(&self.spec_service().validate_spec()))
    }

    #[tool(
        description = "Review one requirement's wording for quality: ambiguous language, a \
        story missing its actor or rationale, outcomes that are not measurable, criteria \
        covering more than one action, and missing edge cases. Reword the requirement from \
        the findings and call again - iterate until there are no findings, then have the \
        developer approve the wording before writing any scenario."
    )]
    async fn refine_requirement(
        &self,
        Parameters(params): Parameters<IdParam>,
    ) -> Result<CallToolResult, McpError> {
        let _span = tool_call("refine_requirement");
        tracing::debug!(id = %params.id, "tool arguments");
        Ok(match self.spec_service().refine_requirement(&params.id) {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(
        description = "Run the project test suite and report the outcome. Updates the \
        Red/Green/Refactor state: failures mean RED, all-passing means GREEN."
    )]
    async fn run_tests(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("run_tests");
        Ok(match (self.runner_factory)(&self.root) {
            Err(message) => error_result(message),
            Ok(runner) => {
                match self
                    .tdd_service()
                    .run_tests(runner.as_ref(), &TestFilter::default())
                {
                    Ok(report) => json_result(&report),
                    Err(error) => tdd_error_result(error),
                }
            }
        })
    }

    #[tool(
        description = "Get the current phase of the Red/Green/Refactor cycle, the last \
        test run summary, interpretation instructions, at most the three latest dated \
        state entries, and a suggested next step."
    )]
    async fn get_tdd_state(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("get_tdd_state");
        Ok(match self.tdd_service().state() {
            Ok(report) => json_result(&report),
            Err(error) => tdd_error_result(error),
        })
    }

    #[tool(
        description = "Begin a refactor step. Only allowed when the bar is GREEN — never \
        refactor on failing tests. Run run_tests afterwards to prove the refactor was safe."
    )]
    async fn start_refactor(
        &self,
        Parameters(params): Parameters<NoteParam>,
    ) -> Result<CallToolResult, McpError> {
        let _span = tool_call("start_refactor");
        tracing::debug!(note = ?params.note, "tool arguments");
        Ok(match self.tdd_service().refactor(params.note.as_deref()) {
            Ok(report) => json_result(&report),
            Err(error) => tdd_error_result(error),
        })
    }

    // ---- additive typed tools ---------------------------------------------

    #[tool(
        description = "The absolute project root this server uses for every other tool \
        (the --root this process was started with; default \".\")."
    )]
    async fn project_root(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("project_root");
        Ok(json_result(&serde_json::json!({
            "root": self.root.to_string_lossy(),
            "nextStep": "Call list_requirements to see the backlog at this root.",
        })))
    }

    #[tool(
        description = "Detect the project's languages, BDD frameworks, runtimes, and \
        whether test execution is possible. Authoring never requires a runtime."
    )]
    async fn project_inspect(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("project_inspect");
        let service =
            InspectService::new(FsProjectFiles::new(self.root.clone()), ProcessRuntimeProbe);
        Ok(json_result(&service.inspect_mcp()))
    }

    #[tool(
        description = "List every Gherkin feature file with its name and scenario count. Staged files are included as if already committed."
    )]
    async fn feature_list(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("feature_list");
        Ok(match self.overlay_catalog().list() {
            Ok(summaries) => json_result(&summaries),
            Err(e) => error_result(e),
        })
    }

    #[tool(
        description = "Read one parsed feature file: tags, scenarios, and steps. Staged content wins over the working tree."
    )]
    async fn feature_read(
        &self,
        Parameters(params): Parameters<FeaturePathParam>,
    ) -> Result<CallToolResult, McpError> {
        let _span = tool_call("feature_read");
        tracing::debug!(path = %params.path, "tool arguments");
        Ok(match self.overlay_catalog().read(&params.path) {
            Ok(doc) => json_result(&doc),
            Err(e) => error_result(e),
        })
    }

    #[tool(description = "Create an empty feature file (staged; apply with changes_commit).")]
    async fn feature_create(
        &self,
        Parameters(params): Parameters<FeatureCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("feature_create");
        tracing::debug!(path = %params.path, name = %params.name, "tool arguments");
        Ok(
            match self
                .scenario_service()
                .create_feature(&params.path, &params.name)
            {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        )
    }

    #[tool(
        description = "Append a scenario tagged @REQ-... to a feature file (staged; \
        apply with changes_commit). Steps must be full Gherkin lines."
    )]
    async fn scenario_add(
        &self,
        Parameters(params): Parameters<ScenarioAddParams>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("scenario_add");
        tracing::debug!(feature = %params.feature, name = %params.name, "tool arguments");
        Ok(
            match self.scenario_service().add_scenario(
                &params.feature,
                &params.req,
                &params.name,
                params.steps,
            ) {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        )
    }

    #[tool(
        description = "Replace a scenario's steps and/or requirement tag (staged; apply \
        with changes_commit)."
    )]
    async fn scenario_update(
        &self,
        Parameters(params): Parameters<ScenarioUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("scenario_update");
        tracing::debug!(feature = %params.feature, name = %params.name, "tool arguments");
        Ok(
            match self.scenario_service().update_scenario(
                &params.feature,
                &params.name,
                params.steps.unwrap_or_default(),
                params.req.as_deref(),
            ) {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        )
    }

    #[tool(
        description = "Remove a scenario from a feature file (staged; apply with \
        changes_commit)."
    )]
    async fn scenario_delete(
        &self,
        Parameters(params): Parameters<ScenarioDeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("scenario_delete");
        tracing::debug!(feature = %params.feature, name = %params.name, "tool arguments");
        Ok(
            match self
                .scenario_service()
                .delete_scenario(&params.feature, &params.name)
            {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        )
    }

    #[tool(description = "Show every staged change waiting for review.")]
    async fn changes_show(&self) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("changes_show");
        Ok(match self.change_service().show() {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(
        description = "Validate the effective spec and staged Gherkin together — staged \
        content wins over the working tree. Frozen validate_spec still reads the \
        committed spec on disk; call this after staging and before changes_commit."
    )]
    async fn changes_validate(&self) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("changes_validate");
        Ok(match self.change_service().validate() {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(description = "Apply every staged change to the working tree and clear the area.")]
    async fn changes_commit(&self) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("changes_commit");
        Ok(match self.change_service().commit() {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(description = "Drop every staged change without applying it.")]
    async fn changes_discard(&self) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("changes_discard");
        Ok(match self.change_service().discard() {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(
        description = "Run one allowlisted dev-tool command (cargo, mvn, npm, npx, node, \
        dotnet, java, javac, tsc) inside the project root during the implementation \
        phase — the bar must be RED. The command executes directly as argv with no \
        shell, so pipes, redirection, and chaining are inert; arguments may not be \
        absolute paths or contain '..'. Use it to build, compile, or install what the \
        failing tests need, then call run_tests."
    )]
    async fn command_run(
        &self,
        Parameters(params): Parameters<CommandRunParams>,
    ) -> Result<CallToolResult, McpError> {
        let _span = tool_call("command_run");
        tracing::debug!(command = %params.command.join(" "), "tool arguments");
        let service = CommandService::new(
            FsStateStore::new(self.root.clone()),
            ProcessCommandExecutor,
            self.root.clone(),
        );
        Ok(match service.run(&params.command, params.timeout_secs) {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(
        description = "Reword one requirement's title, story, or acceptance criteria \
        (staged; apply with changes_commit). Use this to repair whatever validate_spec \
        or refine_requirement reported - never hand-edit the requirements file, whose \
        JSON escaping and indentation differ from what the read tools return. Passing \
        acceptance_criteria replaces the whole list; status and featureFile are kept."
    )]
    async fn requirement_reword(
        &self,
        Parameters(params): Parameters<RequirementRewordParams>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("requirement_reword");
        tracing::debug!(
            id = %params.id,
            criteria = params.acceptance_criteria.len(),
            "tool arguments"
        );
        Ok(
            match self.mutation_service().reword_direct(
                &params.id,
                params.title,
                params.story,
                params.acceptance_criteria,
            ) {
                // The service words its next step for the harness. Over MCP the
                // agent has tools, not a shell, so name the tools instead —
                // the mirror of what `spec validate` does to
                // `validate_spec`.
                Ok(mut report) => {
                    report.next_step = if report.findings.is_empty() {
                        "Review with changes_show, check it with changes_validate, then \
                         apply with changes_commit."
                            .to_string()
                    } else {
                        format!(
                            "Staged {} with open wording findings. Call \
                             requirement_reword again to address them, then \
                             changes_commit.",
                            report.id
                        )
                    };
                    json_result(&report)
                }
                Err(e) => error_result(e),
            },
        )
    }

    #[tool(
        description = "Flip a requirement's status to implemented (staged). Only \
        allowed on GREEN when a scenario tagged with the requirement id exists."
    )]
    async fn requirement_mark_implemented(
        &self,
        Parameters(params): Parameters<IdParam>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("requirement_mark_implemented");
        tracing::debug!(id = %params.id, "tool arguments");
        Ok(match self.mutation_service().mark_implemented(&params.id) {
            Ok(report) => json_result(&report),
            Err(e) => error_result(e),
        })
    }

    #[tool(description = "List Gherkin steps that have no matching step definition.")]
    async fn step_definitions_find(&self) -> Result<CallToolResult, McpError> {
        let _span = tool_call("step_definitions_find");
        Ok(match self.generation_service() {
            Err(message) => error_result(message),
            Ok(service) => match service.steps_missing() {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        })
    }

    #[tool(
        description = "Stage pending step definitions for every undefined step \
        (template only over MCP; apply with changes_commit)."
    )]
    async fn step_definition_create(&self) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("step_definition_create");
        Ok(match self.generation_service() {
            Err(message) => error_result(message),
            Ok(service) => match service.steps_generate(&mut NullPrompter) {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        })
    }

    #[tool(
        description = "Stage a failing unit test for one requirement (template \
        only over MCP; apply with changes_commit)."
    )]
    async fn unit_test_create(
        &self,
        Parameters(params): Parameters<ReqIdParam>,
    ) -> Result<CallToolResult, McpError> {
        let _staging = self.staging_guard().await;
        let _span = tool_call("unit_test_create");
        tracing::debug!(req_id = %params.req_id, "tool arguments");
        Ok(match self.generation_service() {
            Err(message) => error_result(message),
            Ok(service) => match service.unittest_generate(&mut NullPrompter, &params.req_id) {
                Ok(report) => json_result(&report),
                Err(e) => error_result(e),
            },
        })
    }
}

#[tool_handler]
impl ServerHandler for WorkflowServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(
                Implementation::new("spec-driven-server", "1.0.0")
                    .with_title("Spec Driven")
                    .with_description(
                        "Serves spec-driven TDD and BDD tools. The requirements spec is the source of truth.",
                    )
                    .with_icons(vec![Icon::new(
                        "https://davidparry.github.io/spec-driven-agentic/assets/spec-harness-mark.png",
                    )
                    .with_mime_type("image/png")
                    .with_sizes(vec!["1024x1024".into()])
                    .with_theme(IconTheme::Dark)])
                    .with_website_url("https://davidparry.github.io/spec-driven-agentic/"),
            )
            .with_instructions(crate::domain::prompts::mcp_instructions())
    }

    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        std::borrow::Cow::Borrowed(ProtocolVersion::known_up_to(&ProtocolVersion::V_2026_07_28))
    }
}

/// Offline list of the built-in tools. Synchronous, no tokio, no IO.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    WorkflowServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| {
            let mut schema = serde_json::Value::Object((*tool.input_schema).clone());
            if let Some(object) = schema.as_object_mut() {
                object.remove("$schema");
                object.remove("title");
            }
            ToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.as_deref().unwrap_or("").to_string(),
                schema,
                origin: ToolOrigin::Builtin,
            }
        })
        .collect()
}

/// Serve the workflow over stdio until the client disconnects.
pub async fn serve_stdio(root: PathBuf) -> anyhow::Result<()> {
    use rmcp::ServiceExt as _;
    tracing::info!(root = %root.display(), "MCP server starting on stdio");
    crate::greenfield::refresh_project_memory(&root, None);
    let service = WorkflowServer::new(root)
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    tracing::info!("MCP server stopped (client disconnected)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::ports::ToolBroker;

    /// A project the server can stage a feature mutation into.
    fn staging_project() -> (tempfile::TempDir, WorkflowServer) {
        let dir = tempfile::tempdir().unwrap();
        let features = dir.path().join("kata/src/test/resources/features");
        fs::create_dir_all(&features).unwrap();
        fs::write(
            features.join("calc.feature"),
            "Feature: Calc\n\n  @REQ-001\n  Scenario: Base\n    Given a calculator\n",
        )
        .unwrap();
        let server = WorkflowServer::new(dir.path().to_path_buf());
        (dir, server)
    }

    fn add_scenario_params(name: &str) -> Parameters<ScenarioAddParams> {
        Parameters(ScenarioAddParams {
            feature: "kata/src/test/resources/features/calc.feature".into(),
            req: "REQ-002".into(),
            name: name.into(),
            steps: vec!["Given a calculator".into()],
        })
    }

    fn staged_scenarios(dir: &tempfile::TempDir) -> Vec<String> {
        let staged = dir
            .path()
            .join(crate::domain::STAGED_DIR)
            .join("files/kata/src/test/resources/features/calc.feature");
        fs::read_to_string(staged)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Scenario: ").map(String::from))
            .collect()
    }

    /// A host is free to dispatch a batch of tool calls concurrently, and
    /// each one lands on its own task. Two interleaved read-modify-write
    /// cycles used to lose one edit while both replies still said
    /// `staged: true`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_parallel_staging_batch_keeps_every_edit() {
        let (dir, server) = staging_project();
        let names: Vec<String> = (0..8).map(|i| format!("Scenario{i}")).collect();
        let batch: Vec<_> = names
            .iter()
            .map(|name| {
                // A clone is what a host hands each concurrent call.
                let server = server.clone();
                let params = add_scenario_params(name);
                tokio::spawn(async move { server.scenario_add(params).await })
            })
            .collect();
        for task in batch {
            let reply = task.await.unwrap().unwrap();
            assert_ne!(
                reply.is_error,
                Some(true),
                "got: {}",
                reply.content[0].as_text().unwrap().text
            );
        }
        let mut staged = staged_scenarios(&dir);
        staged.sort();
        let mut expected = names;
        expected.push("Base".into());
        expected.sort();
        assert_eq!(staged, expected, "a parallel staging batch dropped an edit");
    }

    /// The guard is what serializes those cycles: while it is held, a
    /// staging mutation has to wait rather than read a stale base.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_staging_mutation_waits_for_the_staging_lock() {
        let (_dir, server) = staging_project();
        let held = server.staging_guard().await;
        let blocked = tokio::spawn({
            let server = server.clone();
            async move { server.scenario_add(add_scenario_params("Blocked")).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        assert!(
            !blocked.is_finished(),
            "the mutation ran while the staging lock was held"
        );
        drop(held);
        let reply = blocked.await.unwrap().unwrap();
        assert_ne!(reply.is_error, Some(true));
    }

    #[test]
    fn every_clone_guards_the_same_staging_area() {
        let (_dir, server) = staging_project();
        assert!(Arc::ptr_eq(&server.staging, &server.clone().staging));
    }

    #[test]
    fn a_runner_is_detected_for_every_supported_marker() {
        for marker in ["pom.xml", "package.json", "Cargo.toml", "app.csproj"] {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join(marker), "").unwrap();
            assert!(detect_runner(dir.path()).is_ok(), "no runner for {marker}");
        }
        let typescript = tempfile::tempdir().unwrap();
        fs::write(typescript.path().join("package.json"), "{}").unwrap();
        fs::write(typescript.path().join("tsconfig.json"), "{}").unwrap();
        assert!(detect_runner(typescript.path()).is_ok());
    }

    #[test]
    fn an_unrecognized_directory_has_no_runner() {
        let dir = tempfile::tempdir().unwrap();
        let error = detect_runner(dir.path())
            .err()
            .expect("no runner in an empty directory");
        assert!(
            error.starts_with("No supported project detected"),
            "got: {error}"
        );
    }

    #[test]
    fn a_missing_runtime_becomes_the_structured_refusal() {
        let result = tdd_error_result(TddError::RuntimeMissing {
            runtime: "mvn".into(),
            hint: "Install Maven.".into(),
        });
        assert_eq!(result.is_error, Some(true));
        let text = result.content[0].as_text().unwrap();
        assert!(text.text.contains("\"error\": \"runtime_missing\""));
        assert!(text.text.contains("Install Maven."));
    }

    #[test]
    fn other_tdd_errors_become_plain_error_text() {
        let result = tdd_error_result(TddError::Other("Never refactor on a red bar".into()));
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.content[0].as_text().unwrap().text,
            "Never refactor on a red bar"
        );
    }

    fn tool_schema(name: &str) -> serde_json::Value {
        builtin_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == name)
            .unwrap()
            .schema
    }

    fn type_is_not_an_array(schema: &serde_json::Value) {
        fn walk(value: &serde_json::Value) {
            if let Some(object) = value.as_object() {
                if let Some(kind) = object.get("type") {
                    assert!(
                        !kind.is_array(),
                        "MCP clients mishandle type arrays: {value}"
                    );
                }
                for nested in object.values() {
                    walk(nested);
                }
            } else if let Some(items) = value.as_array() {
                for nested in items {
                    walk(nested);
                }
            }
        }
        walk(schema);
    }

    #[test]
    fn optional_tool_fields_use_anyof_instead_of_type_arrays() {
        for name in ["scenario_update", "start_refactor", "command_run"] {
            let schema = tool_schema(name);
            type_is_not_an_array(&schema);
        }
        let update = tool_schema("scenario_update");
        let req = &update["properties"]["req"];
        assert_eq!(req["anyOf"][0]["type"], "string");
        assert_eq!(req["anyOf"][1]["type"], "null");
        let steps = &update["properties"]["steps"];
        assert_eq!(steps["anyOf"][0]["type"], "array");
        assert_eq!(steps["anyOf"][1]["type"], "null");
        let refactor = tool_schema("start_refactor");
        let required = refactor
            .get("required")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !required.iter().any(|name| name == "note"),
            "note must stay optional (absent ≠ null); schema={refactor}"
        );
        for name in ["req", "steps"] {
            let required = update
                .get("required")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            assert!(
                !required.iter().any(|value| value == name),
                "{name} must stay optional; schema={update}"
            );
        }
        let command_run = tool_schema("command_run");
        let required = command_run
            .get("required")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !required.iter().any(|name| name == "timeout_secs"),
            "timeout_secs must stay optional; schema={command_run}"
        );
    }

    #[test]
    fn the_server_prefers_the_stateless_protocol() {
        let server = WorkflowServer::new(std::path::PathBuf::from("."));
        assert_eq!(
            server.get_info().protocol_version,
            ProtocolVersion::V_2026_07_28
        );
        assert!(
            server
                .supported_protocol_versions()
                .contains(&ProtocolVersion::V_2026_07_28)
        );
        assert!(
            server
                .supported_protocol_versions()
                .contains(&ProtocolVersion::V_2024_11_05)
        );
    }

    #[test]
    fn start_refactor_off_green_is_a_tool_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("requirements")).unwrap();
        std::fs::write(
            dir.path().join("requirements/requirements.json"),
            r#"{"project":"Kata","requirements":[]}"#,
        )
        .unwrap();
        let broker = crate::adapters::mcp_client::McpToolBroker::new(
            WorkflowServer::new(dir.path().to_path_buf()),
            vec![],
        );
        let outcome = broker
            .call("start_refactor", &serde_json::json!({}))
            .expect("loopback call");
        assert!(
            outcome.is_error,
            "expected a tool-level error, got: {}",
            outcome.text
        );
        assert!(
            outcome.text.contains("GREEN"),
            "expected the GREEN refusal, got: {}",
            outcome.text
        );
    }

    #[test]
    fn the_server_stores_an_absolute_root() {
        let server = WorkflowServer::new(std::path::PathBuf::from("."));
        let expected = std::path::absolute(".").unwrap();
        assert!(server.root.is_absolute());
        assert_eq!(server.root, expected);
    }
}

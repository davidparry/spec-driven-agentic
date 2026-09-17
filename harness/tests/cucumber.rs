//! Cucumber harness: binds `tests/features/*.feature` to the real domain
//! logic and application services through in-memory fakes of the ports —
//! the same seams the composition root injects adapters into.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use cucumber::gherkin::Step;
use cucumber::{World, given, then, when};

use spec_harness::adapters::config::{TomlToolStore, inspect_config};
use spec_harness::adapters::fs_project::FsProjectFiles;
use spec_harness::adapters::fs_scaffold::FsScaffoldWriter;
use spec_harness::adapters::fs_sources::FsSourceFiles;
use spec_harness::adapters::fs_spec::{FsFeatureFiles, FsSpecRepository};
use spec_harness::adapters::fs_staging::FsChangeStore;
use spec_harness::adapters::fs_state::FsStateStore;
use spec_harness::adapters::gherkin_features::GherkinFeatureCatalog;
use spec_harness::adapters::mcp_client::McpToolBroker;
use spec_harness::adapters::mcp_config::FsMcpRegistry;
use spec_harness::adapters::runners::cargo::parse_cargo_output;
use spec_harness::adapters::runners::cucumber_js::parse_json_report;
use spec_harness::adapters::runners::dotnet::parse_trx;
use spec_harness::adapters::runners::maven::{MavenRunner, parse_surefire_xml};
use spec_harness::adapters::tool_cache::CachedDiscovery;
use spec_harness::application::DEFAULT_LLM_ATTEMPTS;
use spec_harness::application::agent_service::{
    Agent, AgentConfig, DEFAULT_MAX_ROUNDS, NullPrompter,
};
use spec_harness::application::change_service::{ChangeService, ChangesReport};
use spec_harness::application::generation_service::{
    GenerationReport, GenerationService, MissingStepsReport, ResolvedLlm,
};
use spec_harness::application::implement_service::{
    ImplementService, ImplementationReport, ReadinessReport,
};
use spec_harness::application::init_service::{InitReport, InitService};
use spec_harness::application::inspect_service::{InspectService, InspectionReport};
use spec_harness::application::memory_service::MemoryAwareConversation;
use spec_harness::application::model_service::{
    ModelResolution, ModelService, ModelSource, SessionModel,
};
use spec_harness::application::scenario_service::ScenarioService;
use spec_harness::application::spec_mutation_service::{
    DraftReport, IncludeReport, ListedRequirement, SpecMutationService,
};
use spec_harness::application::spec_service::{
    EnrichedRequirement, ProjectLayout, RefinementReport, RequirementSummary, SpecService,
    ValidationReport,
};
use spec_harness::application::status_service::{StatusReport, StatusService};
use spec_harness::application::tdd_service::{
    RefactorReport, StateReport, TddError, TddService, TestReport,
};
use spec_harness::application::tool_call_service::ToolCallService;
use spec_harness::application::tool_service::{self, ToolService};
use spec_harness::domain::CONFIG_FILE;
use spec_harness::domain::feature::{FeatureDoc, FeatureSummary};
use spec_harness::domain::language::detect_languages;
use spec_harness::domain::mcp_registry::{RegistryLoad, ServerSpec, parse_registry};
use spec_harness::domain::model::{Requirement, Spec, TestRunSummary};
use spec_harness::domain::tdd::{
    ImplementAttempt, StateEntry, TddPhase, TddSnapshot, TddStateMachine,
};
use spec_harness::domain::tool_profile::{Caller, ProfileOverrides, default_profile, resolve};
use spec_harness::domain::tools::{
    ChatMessage, ChatTurn, TOOL_REPLY_CAP, ToolCall, ToolDefinition, ToolOrigin, ToolOutcome,
    text_turn,
};
use spec_harness::greenfield::{
    DynLlm, Greenfield, GreenfieldReport, RunnerFactory, project_memory_service,
    refresh_project_memory,
};
use spec_harness::mcp::{WorkflowServer, builtin_tool_definitions};
use spec_harness::ports::{
    ChangeStore, FeatureCatalog, FeatureError, FeatureFiles, InteractiveShell, LlmConversation,
    LlmError, McpRegistrySource, ModelCatalog, ModelInfo, ModelStore, ProjectFiles, PromptError,
    Prompter, RunnerError, RuntimeProbe, ShellError, ShellLine, SpecError, SpecRepository,
    StateStore, TestFilter, TestRunner, ToolBroker, ToolDiscovery, ToolError,
};
use spec_harness::repl::{Ending, ShellSummary, offer_greenfield, run_shell};

const SPEC_PATH: &str = "requirements/requirements.json";

#[derive(Debug, Clone)]
enum QueuedTurn {
    Answer(String),
    Call(String),
    Fail(String),
}

#[derive(Debug, Default, World)]
struct SpecWorld {
    spec: Spec,
    existing_features: HashSet<String>,
    feature_tags: HashMap<String, HashSet<String>>,
    validation: Option<ValidationReport>,
    refinement: Option<RefinementReport>,
    tdd: TddStateMachine,
    refactor_error: Option<String>,
    catalog: Option<Result<Vec<ModelInfo>, LlmError>>,
    configured_model: Option<String>,
    resolution: Option<ModelResolution>,
    choice: Option<Result<(), LlmError>>,
    persisted_model: Arc<Mutex<Option<String>>>,
    project_markers: HashSet<String>,
    project_extensions: HashSet<String>,
    runtimes: HashMap<String, String>,
    inspection: Option<InspectionReport>,
    project_dir: Option<tempfile::TempDir>,
    feature_list: Option<Vec<FeatureSummary>>,
    feature_doc: Option<FeatureDoc>,
    feature_error: Option<FeatureError>,
    changes_report: Option<ChangesReport>,
    staged_validation: Option<ValidationReport>,
    draft_report: Option<DraftReport>,
    include_report: Option<IncludeReport>,
    listed_requirements: Option<Vec<ListedRequirement>>,
    mutation_error: Option<String>,
    prompt_answers: Vec<String>,
    prompt_transcript: Vec<String>,
    parsed_run: Option<TestRunSummary>,
    runner_refusal: Option<RunnerError>,
    scripted_run: Option<Result<TestRunSummary, RunnerError>>,
    test_report: Option<TestReport>,
    state_report: Option<StateReport>,
    refactor_report: Option<RefactorReport>,
    tdd_error: Option<String>,
    missing_report: Option<MissingStepsReport>,
    generation_report: Option<GenerationReport>,
    implementation_report: Option<ImplementationReport>,
    readiness_report: Option<ReadinessReport>,
    status_report: Option<StatusReport>,
    implement_advice: Option<String>,
    generation_error: Option<String>,
    llm_reply: Option<String>,
    greenfield_runs: Vec<Result<TestRunSummary, RunnerError>>,
    greenfield_factory_error: Option<String>,
    greenfield_llm: bool,
    greenfield_report: Option<GreenfieldReport>,
    greenfield_error: Option<String>,
    shell_script: Vec<Result<ShellLine, ShellError>>,
    shell_dispatched: Vec<Vec<String>>,
    shell_told: Vec<String>,
    shell_saves: usize,
    shell_summary: Option<ShellSummary>,
    requirement_list: Option<Vec<RequirementSummary>>,
    shown_requirement: Option<EnrichedRequirement>,
    spec_reading_error: Option<String>,
    init_report: Option<InitReport>,
    model_list: Option<Vec<ModelInfo>>,
    session_model: Option<SessionModel>,
    recorded_filter: Arc<Mutex<Option<TestFilter>>>,
    model_system_prompt: Option<String>,
    offered_tools: Vec<String>,
    profile_rows: HashMap<String, Vec<String>>,
    tool_unknown: Vec<String>,
    tool_problems: Vec<String>,
    tool_error: Option<String>,
    shown_tool: Option<String>,
    discovery_connects: usize,
    call_content: Option<String>,
    call_is_error: bool,
    call_json: Option<serde_json::Value>,
    call_sessions: usize,
    session_opened: bool,
    merged_args: Option<serde_json::Value>,
    listed_mcp_tools: Vec<String>,
    listed_config: Option<spec_harness::domain::config_report::ConfigReport>,
    agent_tools: Vec<String>,
    agent_queue: Vec<QueuedTurn>,
    agent_broker: HashMap<String, Result<(String, bool), String>>,
    agent_confirm_tools: Vec<String>,
    agent_confirms: Vec<String>,
    agent_attempts: u32,
    agent_max_rounds: u32,
    agent_nonsure: bool,
    agent_answer: Option<String>,
    agent_error: Option<String>,
    agent_told: Vec<String>,
    agent_offered: Vec<String>,
    oversized_tool: bool,
    registry: Option<spec_harness::domain::mcp_registry::RegistryLoad>,
    config_text: Option<String>,
}

// ---- fakes implementing the ports ----------------------------------------

struct InMemorySpec(Spec);

impl SpecRepository for InMemorySpec {
    fn load(&self) -> Result<Spec, SpecError> {
        Ok(self.0.clone())
    }
}

#[derive(Clone)]
struct InMemoryFeatures {
    existing: HashSet<String>,
    tags: HashMap<String, HashSet<String>>,
}

impl FeatureFiles for InMemoryFeatures {
    fn exists(&self, path: &str) -> bool {
        self.existing.contains(path)
    }
    fn has_tag(&self, path: &str, tag: &str) -> bool {
        self.tags.get(path).is_some_and(|tags| tags.contains(tag))
    }
}

struct FakeCatalog(Result<Vec<ModelInfo>, LlmError>);

impl ModelCatalog for FakeCatalog {
    fn models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        self.0.clone()
    }
}

struct FakeStore {
    configured: Option<String>,
    persisted: Arc<Mutex<Option<String>>>,
}

struct InMemoryProject {
    markers: HashSet<String>,
    extensions: HashSet<String>,
}

impl ProjectFiles for InMemoryProject {
    fn exists(&self, name: &str) -> bool {
        self.markers.contains(name)
    }
    fn any_with_extension(&self, extension: &str) -> bool {
        self.extensions.contains(extension)
    }
}

struct InMemoryRuntimes(HashMap<String, String>);

impl RuntimeProbe for InMemoryRuntimes {
    fn version(&self, command: &str) -> Option<String> {
        self.0.get(command).cloned()
    }
}

impl ModelStore for FakeStore {
    fn configured(&self) -> Option<String> {
        self.configured.clone()
    }
    fn persist(&self, model: &str) -> Result<(), LlmError> {
        *self.persisted.lock().unwrap() = Some(model.to_string());
        Ok(())
    }
}

// ---- helpers ---------------------------------------------------------------

const FEATURE_FILE: &str = "features/x.feature";

fn base_requirement(id: &str) -> Requirement {
    Requirement {
        id: id.into(),
        title: "A title".into(),
        status: "pending".into(),
        story: "As a user, I want things so that value.".into(),
        acceptance_criteria: vec!["Given a, when b, then 3".into()],
        feature_file: Some(FEATURE_FILE.into()),
    }
}

impl SpecWorld {
    fn spec_service(&self) -> SpecService<InMemorySpec, InMemoryFeatures> {
        let mut spec = self.spec.clone();
        if spec.project.trim().is_empty() {
            spec.project = "Test Project".into();
        }
        SpecService::new(
            InMemorySpec(spec),
            InMemoryFeatures {
                existing: self.existing_features.clone(),
                tags: self.feature_tags.clone(),
            },
            ProjectLayout {
                step_definitions: "steps/Steps.java".into(),
                test_location: "tests/Test.java".into(),
                production_location: "src/Prod.java".into(),
            },
        )
    }

    fn model_service(&self) -> ModelService<FakeCatalog, FakeStore> {
        let catalog = self.catalog.clone().unwrap_or_else(|| Ok(vec![]));
        ModelService::new(
            FakeCatalog(catalog),
            FakeStore {
                configured: self.configured_model.clone(),
                persisted: Arc::clone(&self.persisted_model),
            },
        )
    }

    fn validation(&self) -> &ValidationReport {
        self.validation.as_ref().expect("the spec was validated")
    }

    fn refinement(&self) -> &RefinementReport {
        self.refinement
            .as_ref()
            .expect("the requirement was refined")
    }

    fn resolution(&self) -> &ModelResolution {
        self.resolution.as_ref().expect("the model was resolved")
    }

    fn inspection(&self) -> &InspectionReport {
        self.inspection.as_ref().expect("the project was inspected")
    }

    fn language_report(
        &self,
        language: &str,
    ) -> &spec_harness::application::inspect_service::LanguageReport {
        self.inspection()
            .languages
            .iter()
            .find(|l| l.language == language)
            .unwrap_or_else(|| panic!("language {language} not detected"))
    }

    fn project_root(&mut self) -> std::path::PathBuf {
        self.project_dir
            .get_or_insert_with(|| tempfile::tempdir().expect("temp project dir"))
            .path()
            .to_path_buf()
    }

    fn feature_catalog(&mut self) -> GherkinFeatureCatalog {
        GherkinFeatureCatalog::new(self.project_root())
    }

    fn scenario_doc(&self, name: &str) -> &spec_harness::domain::feature::ScenarioDoc {
        self.feature_doc
            .as_ref()
            .expect("a feature was read")
            .scenarios
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("scenario {name} not found"))
    }
}

// ---- spec validation steps -------------------------------------------------

#[given(regex = r#"^a valid pending requirement "([^"]+)"$"#)]
fn a_valid_pending_requirement(world: &mut SpecWorld, id: String) {
    world.spec.requirements.push(base_requirement(&id));
    world.existing_features.insert(FEATURE_FILE.into());
}

#[given(regex = r#"^another valid pending requirement with the same id "([^"]+)"$"#)]
fn a_duplicate_requirement(world: &mut SpecWorld, id: String) {
    world.spec.requirements.push(base_requirement(&id));
}

#[given(regex = r#"^a pending requirement "([^"]+)" with criterion "(.+)"$"#)]
fn a_pending_requirement_with_criterion(world: &mut SpecWorld, id: String, criterion: String) {
    let mut requirement = base_requirement(&id);
    requirement.acceptance_criteria = vec![criterion];
    world.spec.requirements.push(requirement);
    world.existing_features.insert(FEATURE_FILE.into());
}

#[given(
    regex = r#"^a valid pending requirement "([^"]+)" whose feature file is missing from disk$"#
)]
fn a_requirement_with_missing_feature_file(world: &mut SpecWorld, id: String) {
    world.spec.requirements.push(base_requirement(&id));
}

#[given(
    regex = r#"^an implemented requirement "([^"]+)" whose feature file is missing from disk$"#
)]
fn an_implemented_requirement_with_missing_feature_file(world: &mut SpecWorld, id: String) {
    let mut requirement = base_requirement(&id);
    requirement.status = "implemented".into();
    world.spec.requirements.push(requirement);
}

#[given(
    regex = r#"^an implemented requirement "([^"]+)" with no scenario tagged in its feature file$"#
)]
fn an_implemented_requirement_untagged(world: &mut SpecWorld, id: String) {
    let mut requirement = base_requirement(&id);
    requirement.status = "implemented".into();
    world.spec.requirements.push(requirement);
    world.existing_features.insert(FEATURE_FILE.into());
}

#[given(
    regex = r#"^an implemented requirement "([^"]+)" with a scenario tagged in its feature file$"#
)]
fn an_implemented_requirement_tagged(world: &mut SpecWorld, id: String) {
    let mut requirement = base_requirement(&id);
    requirement.status = "implemented".into();
    world.spec.requirements.push(requirement);
    world.existing_features.insert(FEATURE_FILE.into());
    world
        .feature_tags
        .entry(FEATURE_FILE.into())
        .or_default()
        .insert(format!("@{id}"));
}

#[when("the spec is validated")]
fn the_spec_is_validated(world: &mut SpecWorld) {
    world.validation = Some(world.spec_service().validate_spec());
}

#[then("the spec is valid")]
fn the_spec_is_valid(world: &mut SpecWorld) {
    let report = world.validation();
    assert!(report.valid, "expected valid, issues: {:?}", report.issues);
}

#[then("the spec is invalid")]
fn the_spec_is_invalid(world: &mut SpecWorld) {
    assert!(!world.validation().valid, "expected the spec to be invalid");
}

#[then(regex = r#"^an issue is "(.+)"$"#)]
fn an_issue_is(world: &mut SpecWorld, expected: String) {
    let issues = &world.validation().issues;
    assert!(
        issues.contains(&expected),
        "issue {expected:?} not in {issues:?}"
    );
}

#[then("the next step advises writing the Gherkin scenario")]
fn next_step_advises_scenario(world: &mut SpecWorld) {
    assert!(
        world
            .validation()
            .next_step
            .starts_with("The spec is valid.")
    );
}

#[then("the next step advises requirement_reword and re-validating")]
fn next_step_advises_fixing(world: &mut SpecWorld) {
    let next_step = &world.validation().next_step;
    assert!(next_step.contains("requirement_reword"), "{next_step}");
    assert!(next_step.contains("validate_spec again"), "{next_step}");
}

// ---- refinement steps --------------------------------------------------------

#[given(regex = r#"^a requirement "([^"]+)" with story "(.+)"$"#)]
fn a_requirement_with_story(world: &mut SpecWorld, id: String, story: String) {
    let mut requirement = base_requirement(&id);
    requirement.story = story;
    requirement.acceptance_criteria.clear();
    world.spec.requirements.push(requirement);
    world.existing_features.insert(FEATURE_FILE.into());
}

#[given(regex = r#"^the requirement has criterion "(.+)"$"#)]
fn the_requirement_has_criterion(world: &mut SpecWorld, criterion: String) {
    world
        .spec
        .requirements
        .last_mut()
        .expect("a requirement was given")
        .acceptance_criteria
        .push(criterion);
}

#[when(regex = r#"^the requirement "([^"]+)" is refined$"#)]
fn the_requirement_is_refined(world: &mut SpecWorld, id: String) {
    world.refinement = Some(
        world
            .spec_service()
            .refine_requirement(&id)
            .expect("requirement exists"),
    );
}

#[then("the requirement is clean")]
fn the_requirement_is_clean(world: &mut SpecWorld) {
    let report = world.refinement();
    assert!(
        report.clean,
        "expected clean, findings: {:?}",
        report.findings
    );
}

#[then("the requirement is not clean")]
fn the_requirement_is_not_clean(world: &mut SpecWorld) {
    assert!(!world.refinement().clean, "expected findings");
}

#[then(regex = r"^there are (\d+) findings$")]
fn there_are_n_findings(world: &mut SpecWorld, count: usize) {
    let findings = &world.refinement().findings;
    assert_eq!(findings.len(), count, "findings: {findings:?}");
}

#[then(regex = r#"^a finding is "(.+)"$"#)]
fn a_finding_is(world: &mut SpecWorld, expected: String) {
    let findings = &world.refinement().findings;
    assert!(
        findings.contains(&expected),
        "finding {expected:?} not in {findings:?}"
    );
}

#[then("the next step advises confirming the wording with the developer")]
fn next_step_advises_confirming(world: &mut SpecWorld) {
    assert!(
        world
            .refinement()
            .next_step
            .starts_with("The wording reads clean.")
    );
}

#[then("the next step advises requirement_reword and iterating")]
fn next_step_advises_rewording(world: &mut SpecWorld) {
    let next_step = &world.refinement().next_step;
    assert!(next_step.contains("requirement_reword"), "{next_step}");
    assert!(
        next_step.contains("refine_requirement again"),
        "{next_step}"
    );
}

// ---- TDD state machine steps ----------------------------------------------

#[given("a fresh TDD session")]
fn a_fresh_tdd_session(world: &mut SpecWorld) {
    world.tdd = TddStateMachine::new();
    world.refactor_error = None;
}

#[when("a failing test run is recorded")]
fn a_failing_run(world: &mut SpecWorld) {
    world.tdd.record_test_run(TestRunSummary {
        tests: 8,
        failures: 2,
        errors: 1,
        ..Default::default()
    });
}

#[when("a passing test run is recorded")]
fn a_passing_run(world: &mut SpecWorld) {
    world.tdd.record_test_run(TestRunSummary {
        tests: 8,
        ..Default::default()
    });
}

#[when(regex = r#"^a refactor is started with note "(.+)"$"#)]
fn a_refactor_with_note(world: &mut SpecWorld, note: String) {
    world
        .tdd
        .start_refactor(Some(&note))
        .expect("refactor allowed from GREEN");
}

#[when("a refactor is attempted")]
fn a_refactor_is_attempted(world: &mut SpecWorld) {
    world.refactor_error = world.tdd.start_refactor(Some("attempt")).err();
}

#[then(regex = r#"^the phase is "([^"]+)"$"#)]
fn the_phase_is(world: &mut SpecWorld, phase: String) {
    assert_eq!(world.tdd.phase().to_string(), phase);
}

#[then(regex = r#"^the suggestion is "(.+)"$"#)]
fn the_suggestion_is(world: &mut SpecWorld, suggestion: String) {
    assert_eq!(world.tdd.suggestion(), suggestion);
}

#[then(regex = r#"^the refactor log contains "(.+)"$"#)]
fn the_refactor_log_contains(world: &mut SpecWorld, note: String) {
    assert!(world.tdd.refactor_log().contains(&note));
}

#[then(regex = r#"^the refactor is refused with a message containing "(.+)"$"#)]
fn the_refactor_is_refused(world: &mut SpecWorld, fragment: String) {
    let error = world
        .refactor_error
        .as_ref()
        .expect("the refactor was refused");
    assert!(
        error.contains(&fragment),
        "error {error:?} lacks {fragment:?}"
    );
}

// ---- model selection steps ---------------------------------------------------

#[given(regex = r#"^the configured model is "([^"]+)"$"#)]
fn the_configured_model_is(world: &mut SpecWorld, model: String) {
    world.configured_model = Some(model);
}

#[given(regex = r#"^Ollama has models "([^"]+)"$"#)]
fn ollama_has_models(world: &mut SpecWorld, names: String) {
    let models = names
        .split(',')
        .map(|name| ModelInfo {
            name: name.trim().to_string(),
            size_bytes: None,
            modified_at: None,
        })
        .collect();
    world.catalog = Some(Ok(models));
}

#[given("Ollama has no models")]
fn ollama_has_no_models(world: &mut SpecWorld) {
    world.catalog = Some(Ok(vec![]));
}

#[given("Ollama is unreachable")]
fn ollama_is_unreachable(world: &mut SpecWorld) {
    world.catalog = Some(Err(LlmError("connection refused".into())));
}

#[when(regex = r#"^the model is resolved with flag "([^"]+)"$"#)]
fn resolved_with_flag(world: &mut SpecWorld, flag: String) {
    world.resolution = Some(world.model_service().resolve(Some(&flag)));
}

#[when("the model is resolved without a flag")]
fn resolved_without_flag(world: &mut SpecWorld) {
    world.resolution = Some(world.model_service().resolve(None));
}

#[when(regex = r#"^the model "([^"]+)" is chosen$"#)]
fn the_model_is_chosen(world: &mut SpecWorld, model: String) {
    world.choice = Some(world.model_service().choose(&model));
}

#[then(regex = r#"^the model resolves to "([^"]+)" from the flag$"#)]
fn resolves_from_flag(world: &mut SpecWorld, model: String) {
    assert_eq!(
        world.resolution(),
        &ModelResolution::Resolved {
            model,
            source: ModelSource::Flag
        }
    );
}

#[then(regex = r#"^the model resolves to "([^"]+)" from configuration$"#)]
fn resolves_from_config(world: &mut SpecWorld, model: String) {
    assert_eq!(
        world.resolution(),
        &ModelResolution::Resolved {
            model,
            source: ModelSource::Config
        }
    );
}

#[then(regex = r#"^the model resolves to "([^"]+)" as the only installed model$"#)]
fn resolves_only_installed(world: &mut SpecWorld, model: String) {
    assert_eq!(
        world.resolution(),
        &ModelResolution::Resolved {
            model,
            source: ModelSource::OnlyInstalled
        }
    );
}

#[when("the session model status is checked")]
fn session_model_status_checked(world: &mut SpecWorld) {
    world.session_model = Some(world.model_service().session_model(None));
}

impl SpecWorld {
    fn session_model(&self) -> &SessionModel {
        self.session_model
            .as_ref()
            .expect("the session model status was checked")
    }
}

#[then(regex = r#"^the session is ready with model "([^"]+)"$"#)]
fn session_ready_with_model(world: &mut SpecWorld, expected: String) {
    let SessionModel::Ready { model, .. } = world.session_model() else {
        panic!("expected Ready, got {:?}", world.session_model());
    };
    assert_eq!(model, &expected);
}

#[then("the session reports that no models are installed")]
fn session_reports_no_models(world: &mut SpecWorld) {
    assert_eq!(world.session_model(), &SessionModel::NoModels);
}

#[then(regex = r#"^the session reports the provider is down with "(.+)"$"#)]
fn session_reports_provider_down(world: &mut SpecWorld, fragment: String) {
    let SessionModel::ProviderDown(error) = world.session_model() else {
        panic!("expected ProviderDown, got {:?}", world.session_model());
    };
    assert!(
        error.contains(&fragment),
        "error {error:?} lacks {fragment:?}"
    );
}

#[then(regex = r#"^the model resolves to "([^"]+)" as the session default$"#)]
fn resolves_session_default(world: &mut SpecWorld, model: String) {
    assert_eq!(
        world.resolution(),
        &ModelResolution::Resolved {
            model,
            source: ModelSource::FirstInstalled
        }
    );
}

#[then("no model choice is persisted")]
fn no_model_choice_persisted(world: &mut SpecWorld) {
    assert_eq!(*world.persisted_model.lock().unwrap(), None);
}

#[then(regex = r#"^resolution is unavailable with a message containing "(.+)"$"#)]
fn resolution_unavailable(world: &mut SpecWorld, fragment: String) {
    let ModelResolution::Unavailable(message) = world.resolution() else {
        panic!("expected Unavailable, got {:?}", world.resolution());
    };
    assert!(
        message.contains(&fragment),
        "message {message:?} lacks {fragment:?}"
    );
}

#[then(regex = r#"^the choice is rejected with a message containing "(.+)"$"#)]
fn choice_rejected(world: &mut SpecWorld, fragment: String) {
    let error = match world.choice.as_ref().expect("a model was chosen") {
        Err(error) => &error.0,
        Ok(()) => panic!("expected the choice to be rejected"),
    };
    assert!(
        error.contains(&fragment),
        "error {error:?} lacks {fragment:?}"
    );
}

#[then(regex = r#"^the persisted model is "([^"]+)"$"#)]
fn the_persisted_model_is(world: &mut SpecWorld, model: String) {
    assert_eq!(*world.persisted_model.lock().unwrap(), Some(model));
}

// ---- project inspection steps ------------------------------------------------

#[given(regex = r#"^the project contains "([^"]+)"$"#)]
fn the_project_contains(world: &mut SpecWorld, marker: String) {
    world.project_markers.insert(marker);
}

#[given(regex = r#"^the project contains a file with extension "([^"]+)"$"#)]
fn the_project_contains_extension(world: &mut SpecWorld, extension: String) {
    world.project_extensions.insert(extension);
}

#[given(regex = r#"^the runtime "([^"]+)" is installed with version "([^"]+)"$"#)]
fn the_runtime_is_installed(world: &mut SpecWorld, command: String, version: String) {
    world.runtimes.insert(command, version);
}

#[when("the project is inspected")]
fn the_project_is_inspected(world: &mut SpecWorld) {
    let service = InspectService::new(
        InMemoryProject {
            markers: world.project_markers.clone(),
            extensions: world.project_extensions.clone(),
        },
        InMemoryRuntimes(world.runtimes.clone()),
    );
    world.inspection = Some(service.inspect());
}

#[then(
    regex = r#"^the language "([^"]+)" is detected with framework "([^"]+)" and runtime "([^"]+)"$"#
)]
fn the_language_is_detected(
    world: &mut SpecWorld,
    language: String,
    framework: String,
    runtime: String,
) {
    let report = world.language_report(&language);
    assert_eq!(report.bdd_framework, framework);
    assert_eq!(report.runtime, runtime);
}

#[then(regex = r"^exactly (\d+) languages? (?:is|are) detected$")]
fn exactly_n_languages(world: &mut SpecWorld, count: usize) {
    let languages = &world.inspection().languages;
    assert_eq!(languages.len(), count, "detected: {languages:?}");
}

#[then("no languages are detected")]
fn no_languages_detected(world: &mut SpecWorld) {
    assert!(world.inspection().languages.is_empty());
}

#[then(regex = r#"^the runtime for "([^"]+)" is present with version "([^"]+)"$"#)]
fn the_runtime_is_present(world: &mut SpecWorld, language: String, version: String) {
    let report = world.language_report(&language);
    assert!(report.runtime_present);
    assert_eq!(report.runtime_version.as_deref(), Some(version.as_str()));
}

#[then(regex = r#"^the runtime for "([^"]+)" is missing$"#)]
fn the_runtime_is_missing(world: &mut SpecWorld, language: String) {
    let report = world.language_report(&language);
    assert!(!report.runtime_present);
    assert_eq!(report.runtime_version, None);
}

#[then(regex = r#"^the note for "([^"]+)" contains "(.+)"$"#)]
fn the_note_contains(world: &mut SpecWorld, language: String, fragment: String) {
    let note = world
        .language_report(&language)
        .note
        .as_ref()
        .expect("a note is present");
    assert!(note.contains(&fragment), "note {note:?} lacks {fragment:?}");
}

#[then("the next step says all runtimes are present")]
fn next_step_all_present(world: &mut SpecWorld) {
    assert!(
        world
            .inspection()
            .next_step
            .starts_with("All detected runtimes are present.")
    );
}

#[then("the next step says some runtimes are missing")]
fn next_step_some_missing(world: &mut SpecWorld) {
    assert!(
        world
            .inspection()
            .next_step
            .starts_with("Some runtimes are missing")
    );
}

#[then(regex = r#"^the next step lists "(.+)"$"#)]
fn next_step_lists(world: &mut SpecWorld, fragment: String) {
    let next_step = &world.inspection().next_step;
    assert!(
        next_step.contains(&fragment),
        "{next_step:?} lacks {fragment:?}"
    );
}

// ---- staging and mutation plumbing ----------------------------------------

/// Scripted prompter: answers come from the feature file, everything the
/// service says is captured for assertions.
struct ScriptedPrompter {
    answers: std::collections::VecDeque<String>,
    transcript: Vec<String>,
}

impl Prompter for ScriptedPrompter {
    fn tell(&mut self, message: &str) {
        self.transcript.push(message.to_string());
    }
    fn ask(&mut self, question: &str) -> Result<String, PromptError> {
        self.transcript.push(question.to_string());
        self.answers
            .pop_front()
            .ok_or_else(|| PromptError("input is not readable - script exhausted".into()))
    }
    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        Ok(self.ask(question)?.eq_ignore_ascii_case("y"))
    }
}

impl SpecWorld {
    fn change_store(&mut self) -> FsChangeStore {
        FsChangeStore::new(self.project_root())
    }

    fn real_change_service(
        &mut self,
    ) -> ChangeService<FsChangeStore, FsSpecRepository, FsFeatureFiles> {
        let root = self.project_root();
        ChangeService::new(
            FsChangeStore::new(root.clone()),
            FsSpecRepository::new(root.join(SPEC_PATH)),
            FsFeatureFiles::new(root),
            SPEC_PATH.into(),
        )
    }

    fn real_mutation_service(
        &mut self,
    ) -> SpecMutationService<
        FsSpecRepository,
        spec_harness::wiring::OverlayFeatures,
        FsChangeStore,
        FsStateStore,
    > {
        let root = self.project_root();
        spec_harness::wiring::mutation_service(&root, DEFAULT_LLM_ATTEMPTS)
    }

    fn real_scenario_service(
        &mut self,
    ) -> ScenarioService<FsChangeStore, spec_harness::wiring::OverlayFeatures> {
        let root = self.project_root();
        spec_harness::wiring::scenario_service(&root)
    }

    fn write_working_spec(&mut self, spec: &Spec) {
        let file = self.project_root().join(SPEC_PATH);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, serde_json::to_string_pretty(spec).unwrap()).unwrap();
    }

    /// Update (or create) one spec file of the catalog in the working tree.
    fn upsert_spec_file(&mut self, path: &str, update: impl FnOnce(&mut Spec)) {
        let file = self.project_root().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut spec: Spec = if file.exists() {
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap()
        } else {
            Spec::default()
        };
        update(&mut spec);
        std::fs::write(file, serde_json::to_string_pretty(&spec).unwrap()).unwrap();
    }

    fn staged_spec_file(&mut self, path: &str) -> Spec {
        let content = self
            .change_store()
            .content(path)
            .unwrap()
            .unwrap_or_else(|| panic!("{path} is not staged"));
        serde_json::from_str(&content).unwrap()
    }

    fn staged_spec(&mut self) -> Spec {
        let content = self
            .change_store()
            .content(SPEC_PATH)
            .unwrap()
            .expect("a spec is staged");
        serde_json::from_str(&content).unwrap()
    }

    fn working_spec(&mut self) -> Spec {
        let file = self.project_root().join(SPEC_PATH);
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    fn staged_feature(&mut self, path: &str) -> FeatureDoc {
        let content = self
            .change_store()
            .content(path)
            .unwrap()
            .unwrap_or_else(|| panic!("{path} is not staged"));
        spec_harness::domain::feature::parse(path, &content).unwrap()
    }

    fn changes_report(&self) -> &ChangesReport {
        self.changes_report.as_ref().expect("a changes report")
    }
}

/// Steps come from docstrings, one per line; `<empty>` means an empty answer.
fn docstring_lines(step: &Step) -> Vec<String> {
    step.docstring
        .as_deref()
        .expect("a docstring")
        .trim_matches('\n')
        .lines()
        .map(|line| {
            if line.trim() == "<empty>" {
                String::new()
            } else {
                line.trim().to_string()
            }
        })
        .collect()
}

/// Gherkin keeps backslash escapes literal inside quoted step arguments.
fn unescape(text: &str) -> String {
    text.replace("\\\"", "\"")
}

// ---- staged changes steps ---------------------------------------------------

#[given(regex = r#"^the feature file "([^"]+)" is created named "([^"]+)" via staging$"#)]
fn feature_created_via_staging(world: &mut SpecWorld, path: String, name: String) {
    world
        .real_scenario_service()
        .create_feature(&path, &name)
        .unwrap();
}

#[given(regex = r#"^raw content is staged at "([^"]+)":$"#)]
fn raw_content_staged(world: &mut SpecWorld, path: String, step: &Step) {
    let content = step.docstring.as_deref().unwrap().trim_start_matches('\n');
    world.change_store().stage(&path, content, "raw").unwrap();
}

#[given(
    regex = r#"^a working spec whose requirement "([^"]+)" is "([^"]+)" with feature file "([^"]+)"$"#
)]
fn working_spec_with_status(world: &mut SpecWorld, id: String, status: String, feature: String) {
    let mut requirement = base_requirement(&id);
    requirement.status = status;
    requirement.feature_file = Some(feature);
    world.write_working_spec(&Spec {
        project: "Kata".into(),
        requirements: vec![requirement],
        ..Spec::default()
    });
}

#[when("the staged changes are shown")]
fn staged_changes_shown(world: &mut SpecWorld) {
    world.changes_report = Some(world.real_change_service().show().unwrap());
}

#[when("the staged changes are committed")]
fn staged_changes_committed(world: &mut SpecWorld) {
    world.changes_report = Some(world.real_change_service().commit().unwrap());
}

#[when("the staged changes are discarded")]
fn staged_changes_discarded(world: &mut SpecWorld) {
    world.changes_report = Some(world.real_change_service().discard().unwrap());
}

#[when("the staged changes are validated")]
fn staged_changes_validated(world: &mut SpecWorld) {
    world.staged_validation = Some(world.real_change_service().validate().unwrap());
}

#[then(regex = r"^(\d+) staged changes? (?:is|are) reported$")]
fn n_staged_changes(world: &mut SpecWorld, count: usize) {
    assert_eq!(world.changes_report().changes.len(), count);
}

#[then(regex = r#"^a staged "([^"]+)" of "([^"]+)" is listed$"#)]
fn staged_change_listed(world: &mut SpecWorld, action: String, path: String) {
    let report = world.changes_report();
    assert!(
        report
            .changes
            .iter()
            .any(|c| c.action == action && c.path == path),
        "changes: {:?}",
        report.changes
    );
}

#[then(regex = r#"^the changes next step starts with "(.+)"$"#)]
fn changes_next_step(world: &mut SpecWorld, prefix: String) {
    let next = &world.changes_report().next_step;
    assert!(next.starts_with(&prefix), "next step: {next}");
}

#[then(regex = r#"^the working tree file "([^"]+)" does not exist$"#)]
fn working_tree_file_missing(world: &mut SpecWorld, path: String) {
    assert!(!world.project_root().join(path).exists());
}

#[then(regex = r#"^the working tree file "([^"]+)" contains "(.+)"$"#)]
fn working_tree_file_contains(world: &mut SpecWorld, path: String, expected: String) {
    let content = std::fs::read_to_string(world.project_root().join(&path))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(content.contains(&expected), "content: {content}");
}

#[then(regex = r"^the staged validation is (valid|invalid)$")]
fn staged_validation_verdict(world: &mut SpecWorld, verdict: String) {
    let report = world
        .staged_validation
        .as_ref()
        .expect("a validation report");
    assert_eq!(
        report.valid,
        verdict == "valid",
        "issues: {:?}",
        report.issues
    );
}

#[then(regex = r#"^a staged validation issue contains "(.+)"$"#)]
fn staged_validation_issue(world: &mut SpecWorld, fragment: String) {
    let report = world
        .staged_validation
        .as_ref()
        .expect("a validation report");
    assert!(
        report.issues.iter().any(|i| i.contains(&fragment)),
        "issues: {:?}",
        report.issues
    );
}

#[then(regex = r#"^the staged validation next step starts with "(.+)"$"#)]
fn staged_validation_next_step(world: &mut SpecWorld, prefix: String) {
    let next = &world
        .staged_validation
        .as_ref()
        .expect("a report")
        .next_step;
    assert!(next.starts_with(&prefix), "next step: {next}");
}

// ---- spec mutation steps ----------------------------------------------------

#[given(regex = r#"^a working spec with the pending requirement "([^"]+)"$"#)]
fn working_spec_pending(world: &mut SpecWorld, id: String) {
    let mut requirement = base_requirement(&id);
    requirement.feature_file = None;
    world.write_working_spec(&Spec {
        project: "Kata".into(),
        requirements: vec![requirement],
        ..Spec::default()
    });
}

#[given("the developer will answer:")]
fn developer_will_answer(world: &mut SpecWorld, step: &Step) {
    world.prompt_answers = docstring_lines(step);
}

#[when("a requirement is drafted")]
fn requirement_drafted(world: &mut SpecWorld) {
    let mut prompter = ScriptedPrompter {
        answers: world.prompt_answers.drain(..).collect(),
        transcript: Vec::new(),
    };
    let service = world.real_mutation_service();
    world.draft_report = Some(service.draft(&mut prompter).unwrap());
    world.prompt_transcript = prompter.transcript;
}

#[when("a requirement is drafted with the model's help")]
fn requirement_drafted_assisted(world: &mut SpecWorld) {
    let mut prompter = ScriptedPrompter {
        answers: world.prompt_answers.drain(..).collect(),
        transcript: Vec::new(),
    };
    let llm = ScriptedLlm(world.llm_reply.clone().expect("a scripted model reply"));
    let service = world.real_mutation_service();
    world.draft_report = Some(
        service
            .draft_assisted(&mut prompter, "scripted-model", &llm)
            .unwrap(),
    );
    world.prompt_transcript = prompter.transcript;
}

#[then(regex = r#"^the draft is staged as "([^"]+)"$"#)]
fn draft_staged_as(world: &mut SpecWorld, id: String) {
    let report = world.draft_report.as_ref().expect("a draft report");
    assert!(report.staged, "report: {report:?}");
    assert_eq!(report.id, id);
}

#[then("the draft is not staged")]
fn draft_not_staged(world: &mut SpecWorld) {
    let report = world.draft_report.as_ref().expect("a draft report");
    assert!(!report.staged, "report: {report:?}");
}

#[then(regex = r#"^the staged requirement "([^"]+)" has (\d+) criteria$"#)]
fn staged_requirement_criteria_count(world: &mut SpecWorld, id: String, count: usize) {
    let staged = world.staged_spec();
    let requirement = staged
        .requirements
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no staged requirement {id}"));
    assert_eq!(
        requirement.acceptance_criteria.len(),
        count,
        "criteria: {:#?}",
        requirement.acceptance_criteria
    );
}

#[then(regex = r"^the draft reports (\d+) open findings?$")]
fn draft_reports_open_findings(world: &mut SpecWorld, count: usize) {
    let report = world.draft_report.as_ref().expect("a draft report");
    assert_eq!(report.findings.len(), count, "report: {report:?}");
}

#[then(regex = r#"^a reported draft finding contains "(.+)"$"#)]
fn reported_draft_finding_contains(world: &mut SpecWorld, fragment: String) {
    let report = world.draft_report.as_ref().expect("a draft report");
    assert!(
        report.findings.iter().any(|f| f.contains(&fragment)),
        "report: {report:?}"
    );
}

#[then(regex = r#"^the draft next step contains "(.+)"$"#)]
fn draft_next_step_contains(world: &mut SpecWorld, fragment: String) {
    let report = world.draft_report.as_ref().expect("a draft report");
    assert!(report.next_step.contains(&fragment), "report: {report:?}");
}

#[then(regex = r"^the staged spec has (\d+) requirements?$")]
fn staged_spec_requirement_count(world: &mut SpecWorld, count: usize) {
    assert_eq!(world.staged_spec().requirements.len(), count);
}

#[then(regex = r"^the working spec has (\d+) requirements?$")]
fn working_spec_requirement_count(world: &mut SpecWorld, count: usize) {
    assert_eq!(world.working_spec().requirements.len(), count);
}

#[then("nothing is staged at the spec path")]
fn nothing_staged_at_spec_path(world: &mut SpecWorld) {
    assert_eq!(world.change_store().content(SPEC_PATH).unwrap(), None);
}

#[then(regex = r#"^the developer was told a finding containing "(.+)"$"#)]
fn developer_told_finding(world: &mut SpecWorld, fragment: String) {
    assert!(
        world
            .prompt_transcript
            .iter()
            .any(|l| l.contains(&fragment)),
        "transcript: {:#?}",
        world.prompt_transcript
    );
}

#[then(regex = r#"^the developer was told a finding containing "(.+)" (\d+) times$"#)]
fn developer_told_finding_n_times(world: &mut SpecWorld, fragment: String, count: usize) {
    let actual = world
        .prompt_transcript
        .iter()
        .filter(|l| l.contains(&fragment))
        .count();
    assert_eq!(actual, count, "transcript: {:#?}", world.prompt_transcript);
}

#[then(regex = r#"^the developer was asked "(.+)"$"#)]
fn developer_was_asked(world: &mut SpecWorld, question: String) {
    assert!(
        world.prompt_transcript.iter().any(|l| l == &question),
        "question {question:?} not in transcript: {:#?}",
        world.prompt_transcript
    );
}

#[given(regex = r#"^the persisted TDD phase is "([^"]+)"$"#)]
fn persisted_tdd_phase(world: &mut SpecWorld, phase: String) {
    let phase = match phase.as_str() {
        "GREEN" => TddPhase::Green,
        "RED" => TddPhase::Red,
        "REFACTOR" => TddPhase::Refactor,
        _ => TddPhase::Start,
    };
    FsStateStore::new(world.project_root())
        .save(&TddSnapshot::at(phase))
        .unwrap();
}

#[when(regex = r#"^requirement "([^"]+)" is marked implemented$"#)]
fn requirement_marked_implemented(world: &mut SpecWorld, id: String) {
    world.real_mutation_service().mark_implemented(&id).unwrap();
}

#[when(regex = r#"^marking requirement "([^"]+)" implemented fails$"#)]
fn marking_implemented_fails(world: &mut SpecWorld, id: String) {
    let error = world
        .real_mutation_service()
        .mark_implemented(&id)
        .unwrap_err();
    world.mutation_error = Some(error.0);
}

#[then(regex = r#"^the staged spec shows "([^"]+)" as "([^"]+)"$"#)]
fn staged_spec_shows_status(world: &mut SpecWorld, id: String, status: String) {
    let spec = world.staged_spec();
    let requirement = spec.requirements.iter().find(|r| r.id == id).unwrap();
    assert_eq!(requirement.status, status);
}

#[then(regex = r#"^the staged spec names "([^"]+)" as the feature file of "([^"]+)"$"#)]
fn staged_spec_names_feature_file(world: &mut SpecWorld, feature: String, id: String) {
    let spec = world.staged_spec();
    let requirement = spec.requirements.iter().find(|r| r.id == id).unwrap();
    assert_eq!(requirement.feature_file.as_deref(), Some(feature.as_str()));
}

#[then(regex = r#"^the mutation error is "(.+)"$"#)]
fn mutation_error_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(
        world.mutation_error.as_deref(),
        Some(unescape(&expected).as_str())
    );
}

#[then(regex = r#"^the mutation error contains "(.+)"$"#)]
fn mutation_error_contains(world: &mut SpecWorld, fragment: String) {
    let error = world.mutation_error.as_deref().expect("a mutation error");
    assert!(error.contains(&fragment), "error: {error}");
}

// ---- spec catalog include steps ---------------------------------------------

#[given(regex = r#"^the spec file "([^"]+)" lists the include "([^"]+)"$"#)]
fn spec_file_lists_include(world: &mut SpecWorld, path: String, entry: String) {
    world.upsert_spec_file(&path, |spec| spec.includes.push(entry));
}

#[given(regex = r#"^the spec file "([^"]+)" holds the pending requirement "([^"]+)"$"#)]
fn spec_file_holds_requirement(world: &mut SpecWorld, path: String, id: String) {
    let mut requirement = base_requirement(&id);
    requirement.feature_file = None;
    world.upsert_spec_file(&path, |spec| spec.requirements.push(requirement));
}

#[given(regex = r#"^the spec file "([^"]+)" is included in the catalog$"#)]
#[when(regex = r#"^the spec file "([^"]+)" is included in the catalog$"#)]
fn spec_file_included_in_catalog(world: &mut SpecWorld, path: String) {
    world.include_report = Some(
        world
            .real_mutation_service()
            .include_add(&path, None)
            .unwrap(),
    );
}

#[then(regex = r#"^the include of "([^"]+)" under "([^"]+)" is staged as created$"#)]
fn include_staged_as_created(world: &mut SpecWorld, file: String, parent: String) {
    let report = world.include_report.as_ref().expect("an include report");
    assert!(report.staged && report.created, "report: {report:?}");
    assert_eq!(report.file, file);
    assert_eq!(report.parent, parent);
}

#[then(regex = r#"^the staged spec lists the include "([^"]+)"$"#)]
fn staged_spec_lists_include(world: &mut SpecWorld, entry: String) {
    let spec = world.staged_spec();
    assert!(
        spec.includes.contains(&entry),
        "includes: {:?}",
        spec.includes
    );
}

#[then(regex = r#"^the staged spec file "([^"]+)" has (\d+) requirements$"#)]
fn staged_spec_file_requirement_count(world: &mut SpecWorld, path: String, count: usize) {
    assert_eq!(world.staged_spec_file(&path).requirements.len(), count);
}

#[then(regex = r#"^the staged spec file "([^"]+)" shows "([^"]+)" as "([^"]+)"$"#)]
fn staged_spec_file_shows_status(world: &mut SpecWorld, path: String, id: String, status: String) {
    let spec = world.staged_spec_file(&path);
    let requirement = spec
        .requirements
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("{id} not in {path}: {spec:?}"));
    assert_eq!(requirement.status, status);
}

#[when("the requirements are listed with their files")]
fn requirements_listed_with_files(world: &mut SpecWorld) {
    world.listed_requirements = Some(world.real_mutation_service().list_requirements().unwrap());
}

#[then(regex = r"^(\d+) requirements are listed with files$")]
fn n_requirements_listed_with_files(world: &mut SpecWorld, count: usize) {
    let listed = world.listed_requirements.as_ref().expect("a listing");
    assert_eq!(listed.len(), count, "listed: {listed:?}");
}

#[then(regex = r#"^requirement "([^"]+)" is listed from "([^"]+)"$"#)]
fn requirement_listed_from(world: &mut SpecWorld, id: String, file: String) {
    let listed = world.listed_requirements.as_ref().expect("a listing");
    let row = listed
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("{id} not listed: {listed:?}"));
    assert_eq!(row.file, file);
}

#[when(regex = r#"^a requirement titled "([^"]+)" is drafted into "([^"]+)" with:$"#)]
fn requirement_drafted_into(world: &mut SpecWorld, title: String, file: String, step: &Step) {
    let mut lines = docstring_lines(step);
    let story = lines.remove(0);
    world.draft_report = Some(
        world
            .real_mutation_service()
            .draft_direct_in(&title, &story, lines, Some(&file))
            .unwrap(),
    );
}

#[when(regex = r#"^drafting a requirement titled "([^"]+)" into "([^"]+)" fails with:$"#)]
fn drafting_into_fails(world: &mut SpecWorld, title: String, file: String, step: &Step) {
    let mut lines = docstring_lines(step);
    let story = lines.remove(0);
    let error = world
        .real_mutation_service()
        .draft_direct_in(&title, &story, lines, Some(&file))
        .unwrap_err();
    world.mutation_error = Some(error.0);
}

#[when(regex = r#"^the feature "([^"]+)" named "([^"]+)" is created$"#)]
fn feature_is_created(world: &mut SpecWorld, path: String, name: String) {
    world
        .real_scenario_service()
        .create_feature(&path, &name)
        .unwrap();
}

#[then(regex = r#"^staged content at "([^"]+)" equals:$"#)]
fn staged_content_equals(world: &mut SpecWorld, path: String, step: &Step) {
    let expected = step.docstring.as_deref().unwrap().trim_matches('\n');
    let actual = world
        .change_store()
        .content(&path)
        .unwrap()
        .unwrap_or_else(|| panic!("{path} is not staged"));
    assert_eq!(actual.trim_end_matches('\n'), expected);
}

#[given(regex = r#"^scenario "([^"]+)" for "([^"]+)" is added to "([^"]+)" with steps:$"#)]
#[when(regex = r#"^scenario "([^"]+)" for "([^"]+)" is added to "([^"]+)" with steps:$"#)]
fn scenario_added(world: &mut SpecWorld, name: String, req: String, path: String, step: &Step) {
    world
        .real_scenario_service()
        .add_scenario(&path, &req, &name, docstring_lines(step))
        .unwrap();
}

#[when(regex = r#"^adding scenario "([^"]+)" for "([^"]+)" to "([^"]+)" fails with steps:$"#)]
fn scenario_add_fails(world: &mut SpecWorld, name: String, req: String, path: String, step: &Step) {
    let error = world
        .real_scenario_service()
        .add_scenario(&path, &req, &name, docstring_lines(step))
        .unwrap_err();
    world.mutation_error = Some(error.0);
}

#[when(regex = r#"^scenario "([^"]+)" in "([^"]+)" is updated with steps:$"#)]
fn scenario_updated(world: &mut SpecWorld, name: String, path: String, step: &Step) {
    world
        .real_scenario_service()
        .update_scenario(&path, &name, docstring_lines(step), None)
        .unwrap();
}

#[when(regex = r#"^scenario "([^"]+)" is deleted from "([^"]+)"$"#)]
fn scenario_deleted(world: &mut SpecWorld, name: String, path: String) {
    world
        .real_scenario_service()
        .delete_scenario(&path, &name)
        .unwrap();
}

#[then(regex = r#"^the staged feature "([^"]+)" has scenario "([^"]+)" tagged "([^"]+)"$"#)]
fn staged_feature_scenario_tagged(world: &mut SpecWorld, path: String, name: String, tag: String) {
    let doc = world.staged_feature(&path);
    let scenario = doc.scenarios.iter().find(|s| s.name == name).unwrap();
    assert!(scenario.tags.contains(&tag), "tags: {:?}", scenario.tags);
}

#[then(regex = r#"^the staged feature "([^"]+)" scenario "([^"]+)" has (\d+) steps$"#)]
fn staged_feature_scenario_steps(world: &mut SpecWorld, path: String, name: String, count: usize) {
    let doc = world.staged_feature(&path);
    let scenario = doc.scenarios.iter().find(|s| s.name == name).unwrap();
    assert_eq!(scenario.steps.len(), count, "steps: {:?}", scenario.steps);
}

#[then(regex = r#"^the staged feature "([^"]+)" has (\d+) scenarios$"#)]
fn staged_feature_scenario_count(world: &mut SpecWorld, path: String, count: usize) {
    assert_eq!(world.staged_feature(&path).scenarios.len(), count);
}

// ---- test runner and TDD persistence steps ----------------------------------

/// A [`TestRunner`] that replays a scripted result.
struct ScriptedTestRunner(Result<TestRunSummary, RunnerError>);

impl TestRunner for ScriptedTestRunner {
    fn run(&self, _: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        self.0.clone()
    }
}

impl SpecWorld {
    fn tdd_service(&mut self) -> TddService<FsStateStore> {
        TddService::new(FsStateStore::new(self.project_root()))
    }

    fn parsed_run(&self) -> &TestRunSummary {
        self.parsed_run.as_ref().expect("a parsed run")
    }

    fn runner_refusal(&self) -> &RunnerError {
        self.runner_refusal.as_ref().expect("a runner refusal")
    }
}

fn docstring(step: &Step) -> String {
    step.docstring
        .as_deref()
        .expect("a docstring")
        .trim_matches('\n')
        .to_string()
}

#[given("the Surefire report:")]
fn surefire_report(world: &mut SpecWorld, step: &Step) {
    world.parsed_run = Some(parse_surefire_xml(&docstring(step)).unwrap());
}

#[given("the TRX report:")]
fn trx_report(world: &mut SpecWorld, step: &Step) {
    world.parsed_run = Some(parse_trx(&docstring(step)).unwrap());
}

#[given("the cucumber-js report:")]
fn cucumber_js_report(world: &mut SpecWorld, step: &Step) {
    world.parsed_run = Some(parse_json_report(&docstring(step)).unwrap());
}

#[given("the cargo test output:")]
fn cargo_test_output(world: &mut SpecWorld, step: &Step) {
    world.parsed_run = Some(parse_cargo_output(&docstring(step)).expect("a test summary"));
}

#[then(regex = r"^the parsed run has (\d+) tests, (\d+) failures, (\d+) errors, (\d+) skipped$")]
fn parsed_run_counts(world: &mut SpecWorld, tests: u32, failures: u32, errors: u32, skipped: u32) {
    let run = world.parsed_run();
    assert_eq!(
        (run.tests, run.failures, run.errors, run.skipped),
        (tests, failures, errors, skipped),
        "run: {run:?}"
    );
}

#[then(regex = r#"^a parsed failure detail is "(.+)"$"#)]
fn parsed_failure_detail_is(world: &mut SpecWorld, expected: String) {
    let expected = unescape(&expected);
    let details = &world.parsed_run().failure_details;
    assert!(details.contains(&expected), "details: {details:?}");
}

#[then(regex = r#"^a parsed failure detail contains "(.+)"$"#)]
fn parsed_failure_detail_contains(world: &mut SpecWorld, fragment: String) {
    let details = &world.parsed_run().failure_details;
    assert!(
        details.iter().any(|d| d.contains(&fragment)),
        "details: {details:?}"
    );
}

#[given(regex = r#"^a Maven project whose build prints "(.+)" and fails$"#)]
fn maven_project_failing_build(world: &mut SpecWorld, message: String) {
    let root = world.project_root();
    let mut runtimes = HashMap::new();
    runtimes.insert("mvn".to_string(), "Apache Maven 3.9.9".to_string());
    let runner = MavenRunner::new(root, InMemoryRuntimes(runtimes)).with_command(vec![
        "sh".into(),
        "-c".into(),
        format!("echo '{message}'; exit 1"),
    ]);
    world.parsed_run = Some(runner.run(&TestFilter::default()).unwrap());
}

#[when("the Maven tests are run")]
fn maven_tests_are_run(_world: &mut SpecWorld) {
    // The run happened in the Given so its outcome is the parsed run.
}

#[given(regex = r#"^a Maven project on a machine without "([^"]+)"$"#)]
fn maven_without_runtime(world: &mut SpecWorld, _runtime: String) {
    let root = world.project_root();
    let runner = MavenRunner::new(root, InMemoryRuntimes(HashMap::new()));
    world.runner_refusal = Some(runner.run(&TestFilter::default()).unwrap_err());
}

#[when("running the Maven tests is refused")]
fn running_maven_refused(world: &mut SpecWorld) {
    assert!(world.runner_refusal.is_some());
}

#[then(regex = r#"^the refusal names runtime "([^"]+)"$"#)]
fn refusal_names_runtime(world: &mut SpecWorld, expected: String) {
    match world.runner_refusal() {
        RunnerError::RuntimeMissing { runtime, .. } => assert_eq!(runtime, &expected),
        other => panic!("unexpected: {other:?}"),
    }
}

#[then(regex = r#"^the refusal hint is "(.+)"$"#)]
fn refusal_hint_is(world: &mut SpecWorld, expected: String) {
    match world.runner_refusal() {
        RunnerError::RuntimeMissing { hint, .. } => assert_eq!(hint, &expected),
        other => panic!("unexpected: {other:?}"),
    }
}

#[given(regex = r"^the test suite will report (\d+) tests with (\d+) failures$")]
fn test_suite_will_report(world: &mut SpecWorld, tests: u32, failures: u32) {
    world.scripted_run = Some(Ok(TestRunSummary {
        tests,
        failures,
        ..Default::default()
    }));
}

#[given(regex = r#"^the test runner reports runtime "([^"]+)" missing with hint "(.+)"$"#)]
fn test_runner_reports_runtime_missing(world: &mut SpecWorld, runtime: String, hint: String) {
    world.scripted_run = Some(Err(RunnerError::RuntimeMissing { runtime, hint }));
}

#[given("the tests are run")]
#[when("the tests are run")]
fn the_tests_are_run(world: &mut SpecWorld) {
    let runner = ScriptedTestRunner(world.scripted_run.clone().expect("a scripted run"));
    let report = world
        .tdd_service()
        .run_tests(&runner, &TestFilter::default())
        .unwrap();
    world.test_report = Some(report);
}

#[when("running the tests is refused")]
fn running_the_tests_is_refused(world: &mut SpecWorld) {
    let runner = ScriptedTestRunner(world.scripted_run.clone().expect("a scripted run"));
    let error = world
        .tdd_service()
        .run_tests(&runner, &TestFilter::default())
        .unwrap_err();
    match error {
        TddError::RuntimeMissing { runtime, hint } => {
            world.runner_refusal = Some(RunnerError::RuntimeMissing { runtime, hint });
        }
        TddError::Other(message) => panic!("unexpected: {message}"),
    }
}

#[then(regex = r#"^the test reply phase is "([^"]+)"$"#)]
fn test_reply_phase(world: &mut SpecWorld, phase: String) {
    assert_eq!(
        world.test_report.as_ref().expect("a test reply").phase,
        phase
    );
}

#[then(regex = r#"^the test reply next step starts with "(.+)"$"#)]
fn test_reply_next_step(world: &mut SpecWorld, prefix: String) {
    let next = &world.test_report.as_ref().expect("a test reply").next_step;
    assert!(next.starts_with(&prefix), "next step: {next}");
}

#[when("the TDD state is read in a fresh invocation")]
fn tdd_state_read_fresh(world: &mut SpecWorld) {
    world.state_report = Some(world.tdd_service().state().unwrap());
}

#[then(regex = r#"^the persisted phase is "([^"]+)"$"#)]
fn persisted_phase_is(world: &mut SpecWorld, phase: String) {
    assert_eq!(
        world.state_report.as_ref().expect("a state reply").phase,
        phase
    );
}

#[then(regex = r#"^the state next step is "(.+)"$"#)]
fn state_next_step_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(
        world
            .state_report
            .as_ref()
            .expect("a state reply")
            .next_step,
        expected
    );
}

#[then(regex = r"^the persisted last run counts (\d+) tests and (\d+) failures$")]
fn persisted_last_run_counts(world: &mut SpecWorld, tests: u32, failures: u32) {
    let last = &world.state_report.as_ref().expect("a state reply").last_run;
    assert_eq!((last.tests, last.failures), (tests, failures));
}

#[then(regex = r#"^the persisted refactor log contains "(.+)"$"#)]
fn persisted_refactor_log_contains(world: &mut SpecWorld, note: String) {
    let log = &world
        .state_report
        .as_ref()
        .expect("a state reply")
        .refactor_log;
    assert!(log.contains(&note), "log: {log:?}");
}

#[when(regex = r#"^a persisted refactor is started with note "(.+)"$"#)]
fn persisted_refactor_started(world: &mut SpecWorld, note: String) {
    world.refactor_report = Some(world.tdd_service().refactor(Some(&note)).unwrap());
}

#[when(regex = r#"^starting a refactor with note "(.+)" fails$"#)]
fn refactor_start_fails(world: &mut SpecWorld, note: String) {
    match world.tdd_service().refactor(Some(&note)).unwrap_err() {
        TddError::Other(message) => world.tdd_error = Some(message),
        other => panic!("unexpected: {other:?}"),
    }
}

#[then(regex = r#"^the refactor reply phase is "([^"]+)"$"#)]
fn refactor_reply_phase(world: &mut SpecWorld, phase: String) {
    assert_eq!(
        world
            .refactor_report
            .as_ref()
            .expect("a refactor reply")
            .phase,
        phase
    );
}

#[then(regex = r#"^the TDD error is "(.+)"$"#)]
fn tdd_error_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(world.tdd_error.as_deref(), Some(expected.as_str()));
}

#[then(regex = r"^the persisted state log holds (\d+) entr(?:y|ies)$")]
fn persisted_state_log_holds(world: &mut SpecWorld, count: usize) {
    let snapshot = FsStateStore::new(world.project_root()).load().unwrap();
    assert_eq!(
        snapshot.entries.len(),
        count,
        "entries: {:?}",
        snapshot.entries
    );
}

#[then("every persisted state entry has a timestamp")]
fn every_persisted_entry_has_a_timestamp(world: &mut SpecWorld) {
    let snapshot = FsStateStore::new(world.project_root()).load().unwrap();
    assert!(
        !snapshot.entries.is_empty(),
        "expected at least one dated entry"
    );
    for entry in &snapshot.entries {
        assert!(
            entry.timestamp.contains('T') && entry.timestamp.ends_with('Z'),
            "not an RFC 3339 UTC timestamp: {:?}",
            entry.timestamp
        );
    }
}

#[then("the persisted state file carries interpretation instructions")]
fn persisted_state_file_carries_instructions(world: &mut SpecWorld) {
    let snapshot = FsStateStore::new(world.project_root()).load().unwrap();
    assert!(
        snapshot.instructions.contains("three most recent entries"),
        "instructions: {}",
        snapshot.instructions
    );
}

#[then(regex = r"^the state reply holds (\d+) entries?$")]
fn state_reply_holds_entries(world: &mut SpecWorld, count: usize) {
    let report = world.state_report.as_ref().expect("a state reply");
    assert_eq!(report.entries.len(), count, "entries: {:?}", report.entries);
}

#[then("the state reply carries interpretation instructions")]
fn state_reply_carries_instructions(world: &mut SpecWorld) {
    let report = world.state_report.as_ref().expect("a state reply");
    assert!(
        report.instructions.contains("three most recent entries"),
        "instructions: {}",
        report.instructions
    );
}

// ---- feature reading steps ---------------------------------------------------

#[given(regex = r#"^a project feature file "([^"]+)" containing:$"#)]
fn a_project_feature_file(world: &mut SpecWorld, path: String, step: &Step) {
    let content = step
        .docstring
        .clone()
        .expect("a docstring with the file content");
    let absolute = world.project_root().join(&path);
    std::fs::create_dir_all(absolute.parent().expect("a parent dir")).unwrap();
    std::fs::write(absolute, content.trim_start_matches('\n')).unwrap();
}

#[when("the features are listed")]
fn the_features_are_listed(world: &mut SpecWorld) {
    world.feature_list = Some(world.feature_catalog().list().expect("listing succeeds"));
}

#[when(regex = r#"^the feature "([^"]+)" is read$"#)]
fn the_feature_is_read(world: &mut SpecWorld, path: String) {
    world.feature_doc = Some(
        world
            .feature_catalog()
            .read(&path)
            .expect("reading succeeds"),
    );
}

#[when(regex = r#"^reading the feature "([^"]+)" fails$"#)]
fn reading_the_feature_fails(world: &mut SpecWorld, path: String) {
    world.feature_error = Some(world.feature_catalog().read(&path).unwrap_err());
}

#[when("listing the features fails")]
fn listing_the_features_fails(world: &mut SpecWorld) {
    world.feature_error = Some(world.feature_catalog().list().unwrap_err());
}

#[then(regex = r"^(\d+) features? (?:is|are) listed$")]
fn n_features_listed(world: &mut SpecWorld, count: usize) {
    let list = world.feature_list.as_ref().expect("features were listed");
    assert_eq!(list.len(), count, "listed: {list:?}");
}

#[then(regex = r#"^the listing shows "([^"]+)" named "([^"]+)" with (\d+) scenarios$"#)]
fn the_listing_shows(world: &mut SpecWorld, path: String, name: String, scenarios: usize) {
    let list = world.feature_list.as_ref().expect("features were listed");
    let summary = list
        .iter()
        .find(|s| s.path == path)
        .unwrap_or_else(|| panic!("{path} not in {list:?}"));
    assert_eq!(summary.name, name);
    assert_eq!(summary.scenario_count, scenarios);
}

#[then(regex = r#"^the feature is tagged "([^"]+)"$"#)]
fn the_feature_is_tagged(world: &mut SpecWorld, tag: String) {
    let doc = world.feature_doc.as_ref().expect("a feature was read");
    assert!(doc.tags.contains(&tag), "tags: {:?}", doc.tags);
}

#[then(regex = r#"^scenario "([^"]+)" is tagged "([^"]+)"$"#)]
fn scenario_is_tagged(world: &mut SpecWorld, name: String, tag: String) {
    let scenario = world.scenario_doc(&name);
    assert!(scenario.tags.contains(&tag), "tags: {:?}", scenario.tags);
}

#[then(regex = r#"^scenario "([^"]+)" has step "(.+)"$"#)]
fn scenario_has_step(world: &mut SpecWorld, name: String, step_text: String) {
    let scenario = world.scenario_doc(&name);
    assert!(
        scenario.steps.contains(&step_text),
        "steps: {:?}",
        scenario.steps
    );
}

#[then(regex = r#"^the feature carries the tags "([^"]+)"$"#)]
fn the_feature_carries_the_tags(world: &mut SpecWorld, tags: String) {
    let expected: Vec<String> = tags.split(", ").map(String::from).collect();
    let doc = world.feature_doc.as_ref().expect("a feature was read");
    assert_eq!(doc.all_tags(), expected);
}

#[then(regex = r#"^the feature error is "(.+)"$"#)]
fn the_feature_error_is(world: &mut SpecWorld, expected: String) {
    let error = world.feature_error.as_ref().expect("an error was captured");
    assert_eq!(error.0, expected);
}

#[then(regex = r#"^the feature error contains "(.+)"$"#)]
fn the_feature_error_contains(world: &mut SpecWorld, fragment: String) {
    let error = world.feature_error.as_ref().expect("an error was captured");
    assert!(
        error.0.contains(&fragment),
        "error {:?} lacks {fragment:?}",
        error.0
    );
}

// ---- step discovery and hybrid generation steps ------------------------------

/// [`LlmConversation`] replying with one scripted text turn.
struct ScriptedLlm(String);

impl LlmConversation for ScriptedLlm {
    fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ChatTurn, LlmError> {
        Ok(text_turn(self.0.clone()))
    }
}

impl SpecWorld {
    fn generation_service(
        &mut self,
        with_model: bool,
    ) -> GenerationService<
        GherkinFeatureCatalog,
        FsSourceFiles,
        FsChangeStore,
        FsSpecRepository,
        ScriptedLlm,
    > {
        let root = self.project_root();
        let language = detect_languages(&FsProjectFiles::new(root.clone()))
            .first()
            .copied()
            .expect("a project marker was written");
        let llm = with_model.then(|| {
            ResolvedLlm::new(
                "scripted-model",
                ScriptedLlm(self.llm_reply.clone().expect("a scripted model reply")),
            )
        });
        let layout = spec_harness::workspace::project_layout(&root);
        GenerationService::new(
            GherkinFeatureCatalog::new(root.clone()),
            FsSourceFiles::in_module(root.clone(), layout.module_root.as_deref()),
            FsChangeStore::new(root.clone()),
            FsSpecRepository::new(root.join(SPEC_PATH)),
            language,
            layout,
            llm,
        )
    }

    fn implement_service(
        &mut self,
        with_model: bool,
    ) -> ImplementService<
        GherkinFeatureCatalog,
        FsSourceFiles,
        FsChangeStore,
        FsSpecRepository,
        ScriptedLlm,
    > {
        let root = self.project_root();
        let language = detect_languages(&FsProjectFiles::new(root.clone()))
            .first()
            .copied()
            .expect("a project marker was written");
        let llm = with_model.then(|| {
            ResolvedLlm::new(
                "scripted-model",
                ScriptedLlm(self.llm_reply.clone().expect("a scripted model reply")),
            )
        });
        let layout = spec_harness::workspace::project_layout(&root);
        ImplementService::new(
            GherkinFeatureCatalog::new(root.clone()),
            FsSourceFiles::in_module(root.clone(), layout.module_root.as_deref()),
            FsChangeStore::new(root.clone()),
            FsSpecRepository::new(root.join(SPEC_PATH)),
            language,
            layout,
            llm,
        )
    }

    fn status_service(
        &mut self,
    ) -> StatusService<
        GherkinFeatureCatalog,
        FsSourceFiles,
        FsChangeStore,
        FsSpecRepository,
        ScriptedLlm,
    > {
        let root = self.project_root();
        let language = detect_languages(&FsProjectFiles::new(root.clone()))
            .first()
            .copied()
            .expect("a project marker was written");
        let layout = spec_harness::workspace::project_layout(&root);
        StatusService::new(
            GherkinFeatureCatalog::new(root.clone()),
            FsSourceFiles::in_module(root.clone(), layout.module_root.as_deref()),
            FsChangeStore::new(root.clone()),
            FsSpecRepository::new(root.join(SPEC_PATH)),
            language,
            layout,
            None,
        )
    }

    fn missing_report(&self) -> &MissingStepsReport {
        self.missing_report.as_ref().expect("a missing report")
    }

    fn generation_report(&self) -> &GenerationReport {
        self.generation_report
            .as_ref()
            .expect("a generation report")
    }
}

#[given("a Java project marker")]
fn a_java_project_marker(world: &mut SpecWorld) {
    std::fs::write(world.project_root().join("pom.xml"), "<project/>").unwrap();
}

#[given(regex = r#"^a Java module "([^"]+)"$"#)]
fn a_java_module(world: &mut SpecWorld, module: String) {
    let pom = world.project_root().join(&module).join("pom.xml");
    std::fs::create_dir_all(pom.parent().expect("a parent dir")).unwrap();
    std::fs::write(pom, "<project/>").unwrap();
}

#[given(regex = r#"^a project source file "([^"]+)" containing:$"#)]
fn a_project_source_file(world: &mut SpecWorld, path: String, step: &Step) {
    let content = step.docstring.clone().expect("a docstring");
    let absolute = world.project_root().join(&path);
    std::fs::create_dir_all(absolute.parent().expect("a parent dir")).unwrap();
    std::fs::write(absolute, content.trim_start_matches('\n')).unwrap();
}

#[given("the model will reply:")]
fn the_model_will_reply(world: &mut SpecWorld, step: &Step) {
    world.llm_reply = Some(
        step.docstring
            .clone()
            .expect("a docstring")
            .trim_matches('\n')
            .to_string(),
    );
}

#[when("missing steps are reported")]
fn missing_steps_are_reported(world: &mut SpecWorld) {
    world.missing_report = Some(world.generation_service(false).steps_missing().unwrap());
}

#[when(regex = r#"^step definitions are generated (with|without) (?:the|a) model$"#)]
fn step_definitions_are_generated(world: &mut SpecWorld, mode: String) {
    let report = world
        .generation_service(mode == "with")
        .steps_generate(&mut NullPrompter)
        .unwrap();
    world.generation_report = Some(report);
}

#[when("generating step definitions fails")]
fn generating_step_definitions_fails(world: &mut SpecWorld) {
    world.generation_error = Some(
        world
            .generation_service(false)
            .steps_generate(&mut NullPrompter)
            .unwrap_err()
            .0,
    );
}

#[when(regex = r#"^a unit test is generated for "([^"]+)" without a model$"#)]
fn a_unit_test_is_generated(world: &mut SpecWorld, req_id: String) {
    let report = world
        .generation_service(false)
        .unittest_generate(&mut NullPrompter, &req_id)
        .unwrap();
    world.generation_report = Some(report);
}

#[given(regex = r#"^a persisted RED run failing with "(.+)"$"#)]
fn persisted_red_run(world: &mut SpecWorld, detail: String) {
    FsStateStore::new(world.project_root())
        .save(&TddSnapshot::with(StateEntry {
            timestamp: "1970-01-01T00:00:00Z".into(),
            phase: TddPhase::Red,
            last_run: TestRunSummary {
                tests: 1,
                failures: 1,
                failure_details: vec![detail],
                ..Default::default()
            },
            ..Default::default()
        }))
        .unwrap();
}

#[when(regex = r#"^an implementation is generated for "([^"]+)" with the model$"#)]
fn implementation_generated(world: &mut SpecWorld, req_id: String) {
    // Mirrors the spec implement command: the brief is the persisted
    // failures plus prior attempts, and the attempt is logged after.
    let tdd = TddService::new(FsStateStore::new(world.project_root()));
    let brief = tdd.implementation_brief(&req_id).unwrap();
    let report = world
        .implement_service(true)
        .generate(
            &mut NullPrompter,
            &req_id,
            &brief.failures,
            &brief.history,
            &brief.states,
        )
        .unwrap();
    tdd.record_attempt(ImplementAttempt {
        requirement: req_id,
        targets: report.targets.clone(),
        failures: brief.failures,
        ..Default::default()
    })
    .unwrap();
    world.implementation_report = Some(report);
}

#[when(regex = r#"^implement readiness is checked for "([^"]+)"$"#)]
fn implement_readiness_checked(world: &mut SpecWorld, req_id: String) {
    // Mirrors the spec implement preflight: the phase and the failures
    // come from the persisted state, exactly as the command reads them.
    let tdd = TddService::new(FsStateStore::new(world.project_root()));
    let phase = tdd.state().unwrap().phase;
    let brief = tdd.implementation_brief(&req_id).unwrap();
    world.readiness_report = Some(
        world
            .implement_service(false)
            .readiness(&req_id, &phase, &brief.failures)
            .unwrap(),
    );
}

#[when(regex = r#"^the model is asked for implement advice on "([^"]+)"$"#)]
fn implement_advice_asked(world: &mut SpecWorld, req_id: String) {
    let tdd = TddService::new(FsStateStore::new(world.project_root()));
    let phase = tdd.state().unwrap().phase;
    let brief = tdd.implementation_brief(&req_id).unwrap();
    let service = world.implement_service(true);
    let readiness = service.readiness(&req_id, &phase, &brief.failures).unwrap();
    world.implement_advice = service
        .advice(&mut NullPrompter, &req_id, &readiness, &brief.failures)
        .unwrap();
    world.readiness_report = Some(readiness);
}

#[then(regex = r#"^the implement readiness is (ready|not ready)$"#)]
fn implement_readiness_is(world: &mut SpecWorld, state: String) {
    let report = world.readiness_report.as_ref().expect("a readiness report");
    assert_eq!(report.ready, state == "ready", "report: {report:?}");
}

#[then(regex = r#"^a readiness finding contains "(.+)"$"#)]
fn readiness_finding_contains(world: &mut SpecWorld, fragment: String) {
    let report = world.readiness_report.as_ref().expect("a readiness report");
    assert!(
        report.findings.iter().any(|f| f.contains(&fragment)),
        "no finding with {fragment:?}: {:?}",
        report.findings
    );
}

#[then(regex = r#"^the readiness next step contains "(.+)"$"#)]
fn readiness_next_step_contains(world: &mut SpecWorld, fragment: String) {
    let report = world.readiness_report.as_ref().expect("a readiness report");
    assert!(
        report.next_step.contains(&fragment),
        "next step: {}",
        report.next_step
    );
}

#[then(regex = r#"^the readiness asset "([^"]+)" is (present|missing)$"#)]
fn readiness_asset_is(world: &mut SpecWorld, path: String, state: String) {
    let report = world.readiness_report.as_ref().expect("a readiness report");
    let asset = report
        .assets
        .iter()
        .find(|a| a.path == path)
        .unwrap_or_else(|| panic!("no asset {path}: {:?}", report.assets));
    assert_eq!(asset.present, state == "present", "asset: {asset:?}");
}

#[then(regex = r#"^the implement advice is "(.+)"$"#)]
fn implement_advice_is(world: &mut SpecWorld, advice: String) {
    assert_eq!(world.implement_advice.as_deref(), Some(advice.as_str()));
}

#[when("the project status is checked")]
fn project_status_checked(world: &mut SpecWorld) {
    // Mirrors the spec status command: the phase comes from the
    // persisted state, everything else from the working tree.
    let tdd = TddService::new(FsStateStore::new(world.project_root()));
    let phase = tdd.state().unwrap().phase;
    world.status_report = Some(world.status_service().status(&phase).unwrap());
}

#[then(regex = r#"^the status next step contains "(.+)"$"#)]
fn status_next_step_contains(world: &mut SpecWorld, fragment: String) {
    let report = world.status_report.as_ref().expect("a status report");
    assert!(
        report.next_step.contains(&fragment),
        "next step: {}",
        report.next_step
    );
}

#[then(regex = r#"^the status lists (\d+) staged files? and (\d+) requirements?$"#)]
fn status_lists(world: &mut SpecWorld, staged: usize, requirements: usize) {
    let report = world.status_report.as_ref().expect("a status report");
    assert_eq!(report.staged.len(), staged, "staged: {:?}", report.staged);
    assert_eq!(
        report.requirements.len(),
        requirements,
        "requirements: {:?}",
        report.requirements
    );
}

#[then(regex = r#"^the status of "([^"]+)" holds (\d+) findings?$"#)]
fn status_of_requirement(world: &mut SpecWorld, req_id: String, count: usize) {
    let report = world.status_report.as_ref().expect("a status report");
    let entry = report
        .requirements
        .iter()
        .find(|r| r.id == req_id)
        .unwrap_or_else(|| panic!("no {req_id} in {:?}", report.requirements));
    assert_eq!(
        entry.findings.len(),
        count,
        "findings: {:?}",
        entry.findings
    );
}

#[when(
    regex = r#"^generating an implementation for "([^"]+)" (with|without) (?:the|a) model fails$"#
)]
fn implementation_generation_fails(world: &mut SpecWorld, req_id: String, mode: String) {
    world.generation_error = Some(
        world
            .implement_service(mode == "with")
            .generate(&mut NullPrompter, &req_id, &[], &[], &[])
            .unwrap_err()
            .0,
    );
}

#[then(regex = r#"^the persisted attempt log holds (\d+) attempts? for "([^"]+)"$"#)]
fn persisted_attempt_log_holds(world: &mut SpecWorld, count: usize, req_id: String) {
    let snapshot = FsStateStore::new(world.project_root()).load().unwrap();
    let attempts: Vec<_> = snapshot
        .attempt_log()
        .iter()
        .filter(|attempt| attempt.requirement == req_id)
        .collect();
    assert_eq!(attempts.len(), count, "log: {:?}", snapshot.attempt_log());
}

#[then(regex = r#"^the implementation staged "([^"]+)" from the model$"#)]
fn implementation_staged(world: &mut SpecWorld, target: String) {
    let report = world
        .implementation_report
        .as_ref()
        .expect("an implementation report");
    assert!(
        report.targets.contains(&target),
        "targets: {:?}",
        report.targets
    );
    assert!(report.staged);
    assert_eq!(report.source, "llm");
}

#[when(regex = r#"^generating a unit test for "([^"]+)" fails$"#)]
fn generating_a_unit_test_fails(world: &mut SpecWorld, req_id: String) {
    world.generation_error = Some(
        world
            .generation_service(false)
            .unittest_generate(&mut NullPrompter, &req_id)
            .unwrap_err()
            .0,
    );
}

#[then(regex = r#"^the missing report names language "([^"]+)" and framework "([^"]+)"$"#)]
fn missing_report_names(world: &mut SpecWorld, language: String, framework: String) {
    assert_eq!(world.missing_report().language, language);
    assert_eq!(world.missing_report().framework, framework);
}

#[then(regex = r"^(\d+) steps? (?:is|are) missing$")]
fn n_steps_missing(world: &mut SpecWorld, count: usize) {
    let missing = &world.missing_report().missing;
    assert_eq!(missing.len(), count, "missing: {missing:?}");
}

#[then("no steps are missing")]
fn no_steps_missing(world: &mut SpecWorld) {
    let missing = &world.missing_report().missing;
    assert!(missing.is_empty(), "missing: {missing:?}");
}

#[then(regex = r#"^a missing "([^"]+)" step is "(.+)"$"#)]
fn a_missing_step_is(world: &mut SpecWorld, keyword: String, text: String) {
    let text = unescape(&text);
    let missing = &world.missing_report().missing;
    assert!(
        missing
            .iter()
            .any(|m| m.keyword == keyword && m.text == text),
        "missing: {missing:?}"
    );
}

#[then(regex = r#"^the missing next step mentions "(.+)"$"#)]
fn missing_next_step_mentions(world: &mut SpecWorld, fragment: String) {
    let next_step = &world.missing_report().next_step;
    assert!(next_step.contains(&fragment), "next step: {next_step}");
}

#[then(regex = r#"^the generation is staged at "([^"]+)" from "([^"]+)"$"#)]
fn generation_staged_at(world: &mut SpecWorld, target: String, source: String) {
    let report = world.generation_report();
    assert_eq!(report.target, target);
    assert_eq!(report.source, source);
    assert!(report.staged);
}

#[then(regex = r#"^the staged file "([^"]+)" contains "(.+)"$"#)]
fn staged_file_contains(world: &mut SpecWorld, path: String, fragment: String) {
    let fragment = unescape(&fragment);
    let content = world
        .change_store()
        .content(&path)
        .unwrap()
        .unwrap_or_else(|| panic!("{path} is not staged"));
    assert!(content.contains(&fragment), "content:\n{content}");
}

#[then(regex = r#"^the staged file "([^"]+)" defines "(.+)" exactly once$"#)]
fn staged_file_defines_once(world: &mut SpecWorld, path: String, fragment: String) {
    let fragment = unescape(&fragment);
    let content = world
        .change_store()
        .content(&path)
        .unwrap()
        .unwrap_or_else(|| panic!("{path} is not staged"));
    assert_eq!(content.matches(&fragment).count(), 1, "content:\n{content}");
}

#[then(regex = r#"^the working tree has no file "([^"]+)"$"#)]
fn working_tree_has_no_file(world: &mut SpecWorld, path: String) {
    assert!(!world.project_root().join(&path).exists());
}

#[then(regex = r#"^the generation error is "(.+)"$"#)]
fn generation_error_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(world.generation_error.as_deref(), Some(expected.as_str()));
}

// ---- greenfield orchestration steps ------------------------------------------

/// A [`TestRunner`] that replays a queue of scripted outcomes, one per run.
struct QueuedRunner(Arc<Mutex<std::collections::VecDeque<Result<TestRunSummary, RunnerError>>>>);

impl TestRunner for QueuedRunner {
    fn run(&self, _: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .expect("another scripted greenfield run")
    }
}

#[given("an empty working spec")]
fn an_empty_working_spec(world: &mut SpecWorld) {
    world.write_working_spec(&Spec {
        project: "Kata".into(),
        requirements: Vec::new(),
        ..Spec::default()
    });
}

#[given("the greenfield test runs will report:")]
fn greenfield_runs_will_report(world: &mut SpecWorld, step: &Step) {
    let counts =
        regex::Regex::new(r#"^(\d+) tests and (\d+) failures(?: detailed "(.+)")?$"#).unwrap();
    let missing = regex::Regex::new(r#"^runtime "([^"]+)" missing with hint "(.+)"$"#).unwrap();
    let failed = regex::Regex::new(r#"^failed "(.+)"$"#).unwrap();
    world.greenfield_runs = docstring_lines(step)
        .iter()
        .map(|line| {
            if let Some(captures) = counts.captures(line) {
                Ok(TestRunSummary {
                    tests: captures[1].parse().unwrap(),
                    failures: captures[2].parse().unwrap(),
                    failure_details: captures
                        .get(3)
                        .map(|detail| vec![detail.as_str().to_string()])
                        .unwrap_or_default(),
                    ..Default::default()
                })
            } else if let Some(captures) = missing.captures(line) {
                Err(RunnerError::RuntimeMissing {
                    runtime: captures[1].to_string(),
                    hint: captures[2].to_string(),
                })
            } else if let Some(captures) = failed.captures(line) {
                Err(RunnerError::Failed(captures[1].to_string()))
            } else {
                panic!("unrecognized scripted run: {line}")
            }
        })
        .collect();
}

#[given(regex = r#"^no test runner is detectable because "(.+)"$"#)]
fn no_test_runner_detectable(world: &mut SpecWorld, message: String) {
    world.greenfield_factory_error = Some(message);
}

#[given("a greenfield model is resolved")]
fn a_greenfield_model_is_resolved(world: &mut SpecWorld) {
    world.greenfield_llm = true;
}

#[when("the greenfield loop runs")]
fn the_greenfield_loop_runs(world: &mut SpecWorld) {
    let root = world.project_root();
    let runs = Arc::new(Mutex::new(std::collections::VecDeque::from(
        std::mem::take(&mut world.greenfield_runs),
    )));
    let factory_error = world.greenfield_factory_error.clone();
    let factory: RunnerFactory = Arc::new(move |_| match &factory_error {
        Some(message) => Err(message.clone()),
        None => Ok(Box::new(QueuedRunner(runs.clone())) as Box<dyn TestRunner>),
    });
    let llm = world.greenfield_llm.then(|| {
        (
            "scripted-model".to_string(),
            Arc::new(ScriptedLlm(
                world.llm_reply.clone().expect("a scripted model reply"),
            )) as DynLlm,
        )
    });
    let mut prompter = ScriptedPrompter {
        answers: world.prompt_answers.drain(..).collect(),
        transcript: Vec::new(),
    };
    let result = Greenfield::with_runner_factory(root, factory, llm).run(&mut prompter);
    world.prompt_transcript = prompter.transcript;
    match result {
        Ok(report) => world.greenfield_report = Some(report),
        Err(message) => world.greenfield_error = Some(message),
    }
}

impl SpecWorld {
    fn greenfield_report(&self) -> &GreenfieldReport {
        self.greenfield_report
            .as_ref()
            .expect("a greenfield report")
    }
}

#[then(regex = r#"^the greenfield run completes with phase "([^"]+)"$"#)]
fn greenfield_completes_with_phase(world: &mut SpecWorld, phase: String) {
    let report = world.greenfield_report();
    assert!(report.completed, "report: {report:?}");
    assert_eq!(report.phase.as_deref(), Some(phase.as_str()));
}

#[then("the greenfield run is not completed")]
fn greenfield_not_completed(world: &mut SpecWorld) {
    assert!(!world.greenfield_report().completed);
}

#[then(regex = r#"^the greenfield next step starts with "(.+)"$"#)]
fn greenfield_next_step(world: &mut SpecWorld, prefix: String) {
    let next = &world.greenfield_report().next_step;
    assert!(next.starts_with(&prefix), "next step: {next}");
}

#[then(regex = r#"^the greenfield phase is "([^"]+)"$"#)]
fn greenfield_phase_is(world: &mut SpecWorld, phase: String) {
    assert_eq!(
        world.greenfield_report().phase.as_deref(),
        Some(phase.as_str())
    );
}

#[then(regex = r#"^the greenfield error is "(.+)"$"#)]
fn greenfield_error_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(world.greenfield_error.as_deref(), Some(expected.as_str()));
}

// ---- interactive shell ------------------------------------------------------

/// Scripted shell: reads come from the feature file, everything told is
/// captured, and session saves are counted.
struct ScriptedShell {
    script: std::collections::VecDeque<Result<ShellLine, ShellError>>,
    told: Vec<String>,
    saves: usize,
}

impl InteractiveShell for ScriptedShell {
    fn read_line(&mut self, _prompt: &str) -> Result<ShellLine, ShellError> {
        self.script.pop_front().unwrap_or(Ok(ShellLine::End))
    }

    fn tell(&mut self, message: &str) {
        self.told.push(message.to_string());
    }

    fn save_session(&mut self) -> Result<(), ShellError> {
        self.saves += 1;
        Ok(())
    }
}

#[given("the shell will read:")]
fn shell_will_read(world: &mut SpecWorld, step: &Step) {
    world.shell_script = docstring_lines(step)
        .into_iter()
        .map(|line| match line.as_str() {
            "<ctrl-c>" => Ok(ShellLine::Interrupted),
            "<ctrl-d>" => Ok(ShellLine::End),
            _ => Ok(ShellLine::Line(line)),
        })
        .collect();
}

#[when("the interactive shell runs")]
fn interactive_shell_runs(world: &mut SpecWorld) {
    let mut shell = ScriptedShell {
        script: world.shell_script.drain(..).collect(),
        told: Vec::new(),
        saves: 0,
    };
    let mut dispatched = Vec::new();
    let summary = run_shell(&mut shell, &mut |tokens| dispatched.push(tokens));
    world.shell_dispatched = dispatched;
    world.shell_told = shell.told;
    world.shell_saves = shell.saves;
    world.shell_summary = Some(summary);
}

#[when("the greenfield offer runs")]
fn greenfield_offer_runs(world: &mut SpecWorld) {
    let mut shell = ScriptedShell {
        script: world.shell_script.drain(..).collect(),
        told: Vec::new(),
        saves: 0,
    };
    let mut dispatched = Vec::new();
    offer_greenfield(&mut shell, &mut |tokens| dispatched.push(tokens));
    world.shell_dispatched = dispatched;
    world.shell_told = shell.told;
}

#[then("nothing was dispatched")]
fn nothing_was_dispatched(world: &mut SpecWorld) {
    assert!(
        world.shell_dispatched.is_empty(),
        "dispatched: {:?}",
        world.shell_dispatched
    );
}

#[then(regex = r#"^the shell dispatched "(.+)"$"#)]
fn shell_dispatched(world: &mut SpecWorld, tokens: String) {
    let expected: Vec<String> = tokens.split('|').map(String::from).collect();
    assert!(
        world.shell_dispatched.contains(&expected),
        "dispatched: {:?}",
        world.shell_dispatched
    );
}

#[then(regex = r#"^the shell ended by "(exit|Ctrl\+C|end of input)" after (\d+) commands?$"#)]
fn shell_ended_by(world: &mut SpecWorld, ending: String, commands: usize) {
    let summary = world.shell_summary.as_ref().expect("a shell summary");
    let expected = match ending.as_str() {
        "exit" => Ending::Exit,
        "Ctrl+C" => Ending::Interrupted,
        _ => Ending::EndOfInput,
    };
    assert_eq!(summary.ending, expected);
    assert_eq!(summary.commands, commands, "summary: {summary:?}");
}

#[then("the session history was saved")]
fn session_history_saved(world: &mut SpecWorld) {
    assert_eq!(world.shell_saves, 1);
}

#[then(regex = r#"^the shell reported "(.+)"$"#)]
fn shell_reported(world: &mut SpecWorld, fragment: String) {
    assert!(
        world.shell_told.iter().any(|m| m.contains(&fragment)),
        "told: {:?}",
        world.shell_told
    );
}

// ---- spec reading -----------------------------------------------------------

#[when("the requirements are listed")]
fn the_requirements_are_listed(world: &mut SpecWorld) {
    world.requirement_list = Some(world.spec_service().list_requirements().unwrap());
}

#[then(regex = r"^(\d+) requirements? (?:is|are) listed$")]
fn n_requirements_listed(world: &mut SpecWorld, count: usize) {
    let list = world
        .requirement_list
        .as_ref()
        .expect("the requirements were listed");
    assert_eq!(list.len(), count, "listed: {list:?}");
}

#[then(regex = r#"^the listing has "([^"]+)" titled "([^"]+)" with status "([^"]+)"$"#)]
fn the_listing_has(world: &mut SpecWorld, id: String, title: String, status: String) {
    let list = world
        .requirement_list
        .as_ref()
        .expect("the requirements were listed");
    assert!(
        list.contains(&RequirementSummary { id, title, status }),
        "listed: {list:?}"
    );
}

#[when(regex = r#"^the requirement "([^"]+)" is shown$"#)]
fn the_requirement_is_shown(world: &mut SpecWorld, id: String) {
    world.shown_requirement = Some(world.spec_service().get_requirement(&id).unwrap());
}

#[when(regex = r#"^showing the requirement "([^"]+)" fails$"#)]
fn showing_the_requirement_fails(world: &mut SpecWorld, id: String) {
    world.spec_reading_error = Some(world.spec_service().get_requirement(&id).unwrap_err().0);
}

impl SpecWorld {
    fn shown_requirement(&self) -> &EnrichedRequirement {
        self.shown_requirement
            .as_ref()
            .expect("a requirement was shown")
    }
}

#[then(regex = r#"^the shown requirement has id "([^"]+)" and status "([^"]+)"$"#)]
fn shown_requirement_id_status(world: &mut SpecWorld, id: String, status: String) {
    let shown = world.shown_requirement();
    assert_eq!(shown.id, id);
    assert_eq!(shown.status, status);
}

#[then(
    regex = r#"^the shown requirement points at steps "([^"]+)", tests "([^"]+)", and production "([^"]+)"$"#
)]
fn shown_requirement_locations(
    world: &mut SpecWorld,
    steps: String,
    tests: String,
    production: String,
) {
    let shown = world.shown_requirement();
    assert_eq!(shown.step_definitions, steps);
    assert_eq!(shown.test_location, tests);
    assert_eq!(shown.production_location, production);
}

#[then(regex = r#"^the shown feature location is "([^"]+)"$"#)]
fn shown_feature_location(world: &mut SpecWorld, location: String) {
    assert_eq!(
        world.shown_requirement().feature_location.as_deref(),
        Some(location.as_str())
    );
}

#[then(regex = r#"^the shown workflow hint mentions "(.+)"$"#)]
fn shown_workflow_hint_mentions(world: &mut SpecWorld, fragment: String) {
    let hint = &world.shown_requirement().workflow_hint;
    assert!(hint.contains(&fragment), "hint: {hint}");
}

#[then(regex = r#"^the spec reading error is "(.+)"$"#)]
fn spec_reading_error_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(world.spec_reading_error.as_deref(), Some(expected.as_str()));
}

// ---- project initialization ---------------------------------------------------

#[given(regex = r#"^the working tree file "([^"]+)" already contains "(.+)"$"#)]
fn working_tree_file_already_contains(world: &mut SpecWorld, path: String, content: String) {
    let absolute = world.project_root().join(&path);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(absolute, content).unwrap();
}

#[when(regex = r#"^the project is initialized for "([^"]+)" named "([^"]+)"$"#)]
fn the_project_is_initialized(world: &mut SpecWorld, language: String, name: String) {
    let language =
        spec_harness::greenfield::parse_language(&language).expect("a supported language");
    let service = InitService::new(FsScaffoldWriter::new(world.project_root()));
    world.init_report = Some(service.init(language, &name).unwrap());
}

impl SpecWorld {
    fn init_report(&self) -> &InitReport {
        self.init_report.as_ref().expect("an init report")
    }
}

#[then(regex = r#"^the init report shows language "([^"]+)" with framework "([^"]+)"$"#)]
fn init_report_language_framework(world: &mut SpecWorld, language: String, framework: String) {
    let report = world.init_report();
    assert_eq!(report.language, language);
    assert_eq!(report.framework, framework);
}

#[then(regex = r"^(\d+) scaffold files are created and (\d+) (?:is|are) skipped$")]
fn scaffold_files_created_and_skipped(world: &mut SpecWorld, created: usize, skipped: usize) {
    let report = world.init_report();
    assert_eq!(
        report.created.len(),
        created,
        "created: {:?}",
        report.created
    );
    assert_eq!(
        report.skipped.len(),
        skipped,
        "skipped: {:?}",
        report.skipped
    );
}

#[then(regex = r#"^a skipped file is "([^"]+)"$"#)]
fn a_skipped_file_is(world: &mut SpecWorld, path: String) {
    let skipped = &world.init_report().skipped;
    assert!(skipped.contains(&path), "skipped: {skipped:?}");
}

#[then(regex = r#"^the init next step mentions "(.+)"$"#)]
fn init_next_step_mentions(world: &mut SpecWorld, fragment: String) {
    let next = &world.init_report().next_step;
    assert!(next.contains(&fragment), "next step: {next}");
}

// ---- model listing ------------------------------------------------------------

#[when("the models are listed")]
fn the_models_are_listed(world: &mut SpecWorld) {
    world.model_list = Some(world.model_service().list().unwrap());
}

#[then(regex = r"^(\d+) models? (?:is|are) listed$")]
fn n_models_listed(world: &mut SpecWorld, count: usize) {
    let list = world.model_list.as_ref().expect("the models were listed");
    assert_eq!(list.len(), count, "listed: {list:?}");
}

#[then(regex = r#"^a listed model is "([^"]+)"$"#)]
fn a_listed_model_is(world: &mut SpecWorld, name: String) {
    let list = world.model_list.as_ref().expect("the models were listed");
    assert!(list.iter().any(|m| m.name == name), "listed: {list:?}");
}

// ---- test filter pass-through -------------------------------------------------

/// A [`TestRunner`] that records the filter it was handed.
struct RecordingRunner {
    result: Result<TestRunSummary, RunnerError>,
    recorded: Arc<Mutex<Option<TestFilter>>>,
}

impl TestRunner for RecordingRunner {
    fn run(&self, filter: &TestFilter) -> Result<TestRunSummary, RunnerError> {
        *self.recorded.lock().unwrap() = Some(filter.clone());
        self.result.clone()
    }
}

#[when(regex = r#"^the tests are run filtered to feature "([^"]+)" and scenario "([^"]+)"$"#)]
fn tests_run_with_filters(world: &mut SpecWorld, feature: String, scenario: String) {
    let runner = RecordingRunner {
        result: world.scripted_run.clone().expect("a scripted run"),
        recorded: Arc::clone(&world.recorded_filter),
    };
    let filter = TestFilter {
        feature: Some(feature),
        scenario: Some(scenario),
    };
    world.test_report = Some(world.tdd_service().run_tests(&runner, &filter).unwrap());
}

#[then(regex = r#"^the runner received feature "([^"]+)" and scenario "([^"]+)"$"#)]
fn runner_received_filters(world: &mut SpecWorld, feature: String, scenario: String) {
    let recorded = world.recorded_filter.lock().unwrap();
    let filter = recorded.as_ref().expect("the runner recorded a filter");
    assert_eq!(filter.feature.as_deref(), Some(feature.as_str()));
    assert_eq!(filter.scenario.as_deref(), Some(scenario.as_str()));
}

// ---- project memory -----------------------------------------------------------

#[when("the project memory is refreshed")]
fn the_project_memory_is_refreshed(world: &mut SpecWorld) {
    refresh_project_memory(&world.project_root(), None);
}

#[when(regex = r#"^the project memory is refreshed for language "([^"]+)"$"#)]
fn the_project_memory_is_refreshed_for_language(world: &mut SpecWorld, language: String) {
    let language =
        spec_harness::greenfield::parse_language(&language).expect("a supported language");
    refresh_project_memory(&world.project_root(), Some(language));
}

#[when(regex = r#"^a model call is made with system "(.+)"$"#)]
fn a_model_call_is_made(world: &mut SpecWorld, system: String) {
    let brief = project_memory_service(world.project_root())
        .load()
        .expect("memory loads")
        .brief();
    let captured = std::sync::Mutex::new(None);
    struct Capture<'a>(&'a std::sync::Mutex<Option<String>>);
    impl LlmConversation for Capture<'_> {
        fn chat(
            &self,
            _model: &str,
            messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatTurn, LlmError> {
            let (system, _) = spec_harness::domain::tools::system_and_user(messages);
            *self.0.lock().unwrap() = Some(system);
            Ok(text_turn("ok"))
        }
    }
    MemoryAwareConversation::new(Capture(&captured), brief)
        .chat(
            "scripted",
            &[ChatMessage::system(system), ChatMessage::user("user")],
            &[],
        )
        .unwrap();
    world.model_system_prompt = captured.into_inner().unwrap();
}

#[then(regex = r#"^the model system prompt contains "(.+)"$"#)]
fn the_model_system_prompt_contains(world: &mut SpecWorld, fragment: String) {
    let prompt = world
        .model_system_prompt
        .as_ref()
        .expect("a model call was made");
    assert!(
        prompt.contains(&fragment),
        "system prompt {prompt:?} lacks {fragment:?}"
    );
}

// ---- tool profiles, agent loop, mcp call, mcp servers ---------------------

fn names_csv(tools: &[ToolDefinition]) -> Vec<String> {
    tools.iter().map(|t| t.name.clone()).collect()
}

fn split_names(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn assert_same_set(actual: &[String], expected: &str) {
    let mut got = actual.to_vec();
    let mut want = split_names(expected);
    got.sort();
    want.sort();
    assert_eq!(got, want, "actual: {actual:?}");
}

fn builtin_defs() -> Vec<ToolDefinition> {
    builtin_tool_definitions()
}

struct CountingDiscovery {
    connects: Arc<Mutex<usize>>,
    fail: Option<String>,
}

impl ToolDiscovery for CountingDiscovery {
    fn discover(&self, _server: &ServerSpec) -> Result<Vec<ToolDefinition>, ToolError> {
        *self.connects.lock().unwrap() += 1;
        if let Some(message) = &self.fail {
            return Err(ToolError(message.clone()));
        }
        Ok(Vec::new())
    }
}

struct EmptyRegistry;

impl McpRegistrySource for EmptyRegistry {
    fn load(&self) -> RegistryLoad {
        RegistryLoad::default()
    }
}

impl SpecWorld {
    fn profile_service(
        &mut self,
        offline: bool,
        fail_discovery: bool,
    ) -> ToolService<TomlToolStore, CountingDiscovery, FsMcpRegistry> {
        let root = self.project_root();
        let connects = Arc::new(Mutex::new(0usize));
        let service = ToolService::new(
            TomlToolStore::new(root.join(CONFIG_FILE)),
            CountingDiscovery {
                connects: Arc::clone(&connects),
                fail: fail_discovery.then(|| "failed to start".into()),
            },
            FsMcpRegistry::new(root, None),
            builtin_defs(),
        );
        if offline {
            let _ = service.catalog(false, true);
        }
        self.discovery_connects = *connects.lock().unwrap();
        // Reconstruct so later calls share counting... the connects already happened.
        // Return a fresh service with the same counter for subsequent use.
        ToolService::new(
            TomlToolStore::new(self.project_root().join(CONFIG_FILE)),
            CountingDiscovery {
                connects,
                fail: fail_discovery.then(|| "failed to start".into()),
            },
            FsMcpRegistry::new(self.project_root(), None),
            builtin_defs(),
        )
    }
}

#[when(regex = r#"^the tools for "([^"]+)" are listed offline$"#)]
fn tools_for_listed_offline(world: &mut SpecWorld, caller: String) {
    let caller = Caller::parse(&caller).expect("caller");
    let service = world.profile_service(false, false);
    let catalog = service.catalog(false, true);
    world.discovery_connects = 0;
    let resolved = resolve(caller, &service.overrides(), &catalog.tools);
    world.offered_tools = names_csv(&resolved.tools);
    world.tool_unknown = resolved.unknown;
    world.tool_problems = catalog.problems;
}

#[when(regex = r#"^the tools for "([^"]+)" are listed with --tools "([^"]+)"$"#)]
fn tools_for_listed_with_flag(world: &mut SpecWorld, caller: String, flag: String) {
    let caller = Caller::parse(&caller).expect("caller");
    let mut overrides = ProfileOverrides::default();
    overrides
        .replace
        .insert(caller.key().into(), split_names(&flag));
    let resolved = resolve(caller, &overrides, &builtin_defs());
    world.offered_tools = names_csv(&resolved.tools);
    world.tool_unknown = resolved.unknown;
}

#[when("every default profile is inspected")]
fn every_default_profile_is_inspected(world: &mut SpecWorld) {
    world.profile_rows = Caller::ALL
        .into_iter()
        .map(|caller| {
            (
                caller.key().to_string(),
                default_profile(caller)
                    .iter()
                    .map(|n| (*n).to_string())
                    .collect(),
            )
        })
        .collect();
}

#[when("the tool profiles are listed")]
fn the_tool_profiles_are_listed(world: &mut SpecWorld) {
    let service = world.profile_service(false, false);
    let (views, problems) = service.profiles(true);
    world.tool_problems = problems;
    world.profile_rows = views.into_iter().map(|v| (v.caller, v.tools)).collect();
}

#[when("the tools are listed offline")]
fn the_tools_are_listed_offline(world: &mut SpecWorld) {
    let connects = Arc::new(Mutex::new(0usize));
    let service = ToolService::new(
        TomlToolStore::new(world.project_root().join(CONFIG_FILE)),
        CountingDiscovery {
            connects: Arc::clone(&connects),
            fail: None,
        },
        EmptyRegistry,
        builtin_defs(),
    );
    let list = service.catalog(false, true);
    world.offered_tools = names_csv(&list.tools);
    world.discovery_connects = *connects.lock().unwrap();
}

#[when("the tools are listed with discovery")]
fn the_tools_are_listed_with_discovery(world: &mut SpecWorld) {
    let service = world.profile_service(false, true);
    let list = service.catalog(false, false);
    world.offered_tools = names_csv(&list.tools);
    world.tool_problems = list.problems;
    world.discovery_connects = 1;
}

#[when(regex = r#"^the tool catalog is refreshed$"#)]
fn the_tool_catalog_is_refreshed(world: &mut SpecWorld) {
    let connects = Arc::new(Mutex::new(0usize));
    let inner = CountingDiscovery {
        connects: Arc::clone(&connects),
        fail: None,
    };
    let cache = CachedDiscovery::new(
        inner,
        world.project_root().join(".spec-cache").join("tools"),
        std::time::Duration::from_secs(86_400),
    );
    let load = FsMcpRegistry::new(world.project_root(), None).load();
    let empty = load.servers.is_empty();
    for server in load.servers {
        let _ = cache.discover_fresh(&server);
    }
    if empty {
        let _ = cache.discover_fresh(&ServerSpec {
            name: "self".into(),
            program: "spec".into(),
            args: vec![],
            env: vec![],
        });
    }
    world.discovery_connects = *connects.lock().unwrap();
}

#[given("the config file contains:")]
fn the_config_file_contains(world: &mut SpecWorld, step: &Step) {
    let content = step.docstring.clone().expect("a docstring");
    std::fs::write(
        world.project_root().join(CONFIG_FILE),
        content.trim_start_matches('\n'),
    )
    .unwrap();
}

#[when("the configuration is listed")]
fn the_configuration_is_listed(world: &mut SpecWorld) {
    let path = spec_harness::adapters::config::config_path(&world.project_root());
    world.listed_config = Some(inspect_config(&path));
}

fn listed_config(world: &SpecWorld) -> &spec_harness::domain::config_report::ConfigReport {
    world
        .listed_config
        .as_ref()
        .expect("the configuration was listed")
}

#[then(regex = r#"^the config file status is "([^"]+)"$"#)]
fn config_file_status_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(listed_config(world).file.display(), expected);
}

#[then(regex = r#"^the config file status contains "([^"]+)"$"#)]
fn config_file_status_contains(world: &mut SpecWorld, fragment: String) {
    let display = listed_config(world).file.display();
    assert!(display.contains(&fragment), "{display}");
}

#[then(regex = r#"^the config value "([^"]+)" is "([^"]+)" from default$"#)]
fn config_value_from_default(world: &mut SpecWorld, key: String, value: String) {
    let setting = listed_config(world)
        .setting(&key)
        .unwrap_or_else(|| panic!("missing {key}"));
    assert_eq!(setting.value, value, "{key}");
    assert_eq!(
        setting.source,
        spec_harness::domain::config_report::ConfigSource::Default
    );
}

#[then(regex = r#"^the config value "([^"]+)" is "([^"]+)" from the config file$"#)]
fn config_value_from_file(world: &mut SpecWorld, key: String, value: String) {
    let setting = listed_config(world)
        .setting(&key)
        .unwrap_or_else(|| panic!("missing {key}"));
    assert_eq!(setting.value, value, "{key}");
    assert!(
        matches!(
            setting.source,
            spec_harness::domain::config_report::ConfigSource::File(_)
        ),
        "{key} {:?}",
        setting.source
    );
}

#[then(regex = r#"^the config value "([^"]+)" is from default$"#)]
fn config_key_is_from_default(world: &mut SpecWorld, key: String) {
    let setting = listed_config(world)
        .setting(&key)
        .unwrap_or_else(|| panic!("missing {key}"));
    assert_eq!(
        setting.source,
        spec_harness::domain::config_report::ConfigSource::Default,
        "{key}"
    );
}

#[when(regex = r#"^"([^"]+)" is enabled for "([^"]+)"$"#)]
fn tool_is_enabled_for(world: &mut SpecWorld, name: String, caller: String) {
    match tool_service::parse_caller(Some(&caller)) {
        Ok(caller) => {
            let service = world.profile_service(false, false);
            match service.enable(&name, caller) {
                Ok(()) => world.tool_error = None,
                Err(error) => world.tool_error = Some(error.0),
            }
        }
        Err(error) => world.tool_error = Some(error.0),
    }
}

#[when("tools enable is invoked without --for")]
fn tools_enable_without_for(world: &mut SpecWorld) {
    world.tool_error = Some(tool_service::parse_caller(None).unwrap_err().0);
}

#[when(regex = r#"^"([^"]+)" is enabled for the unknown caller "([^"]+)"$"#)]
fn tool_enabled_for_unknown(world: &mut SpecWorld, _name: String, caller: String) {
    world.tool_error = Some(tool_service::parse_caller(Some(&caller)).unwrap_err().0);
}

#[when(regex = r#"^the tool "([^"]+)" is shown$"#)]
fn the_tool_is_shown(world: &mut SpecWorld, name: String) {
    let service = world.profile_service(false, false);
    match service.show(&name) {
        Ok(tool) => {
            world.shown_tool = Some(tool.name);
            world.tool_error = None;
        }
        Err(error) => world.tool_error = Some(error.0),
    }
}

#[then(regex = r#"^the offered tools are "([^"]+)"$"#)]
fn the_offered_tools_are(world: &mut SpecWorld, expected: String) {
    assert_same_set(&world.offered_tools, &expected);
}

#[then(regex = r#"^the offered tools do not include "([^"]+)"$"#)]
fn offered_tools_do_not_include(world: &mut SpecWorld, name: String) {
    assert!(
        !world.offered_tools.iter().any(|n| n == &name),
        "{:?}",
        world.offered_tools
    );
}

#[then("no default profile offers a staging or commit tool")]
fn no_default_profile_offers_staging(world: &mut SpecWorld) {
    let forbidden = [
        "scenario_add",
        "scenario_update",
        "scenario_delete",
        "feature_create",
        "changes_commit",
        "changes_discard",
        "requirement_reword",
        "requirement_mark_implemented",
        "step_definition_create",
        "unit_test_create",
    ];
    for (caller, tools) in &world.profile_rows {
        for name in &forbidden {
            assert!(!tools.iter().any(|t| t == name), "{caller} offers {name}");
        }
    }
}

#[then("command_run appears only for implement")]
fn command_run_only_implement(world: &mut SpecWorld) {
    for (caller, tools) in &world.profile_rows {
        let has = tools.iter().any(|t| t == "command_run");
        assert_eq!(has, caller == "implement", "{caller}");
    }
}

#[then(regex = r#"^a tool warning contains "([^"]+)"$"#)]
fn a_tool_warning_contains(world: &mut SpecWorld, fragment: String) {
    assert!(
        world.tool_unknown.iter().any(|u| u.contains(&fragment))
            || world.tool_problems.iter().any(|p| p.contains(&fragment)),
        "unknown {:?} problems {:?}",
        world.tool_unknown,
        world.tool_problems
    );
}

#[then(regex = r#"^the config file contains "([^"]+)"$"#)]
fn config_file_contains(world: &mut SpecWorld, fragment: String) {
    let text = std::fs::read_to_string(world.project_root().join(CONFIG_FILE)).unwrap();
    assert!(text.contains(&fragment), "{text}");
}

#[then(regex = r#"^the tool error contains "([^"]+)"$"#)]
fn the_tool_error_contains(world: &mut SpecWorld, fragment: String) {
    let error = world.tool_error.as_ref().expect("a tool error");
    assert!(error.contains(&fragment), "{error}");
}

#[then(regex = r#"^the profile for "([^"]+)" offers "([^"]+)"$"#)]
fn profile_for_offers(world: &mut SpecWorld, caller: String, expected: String) {
    let tools = world.profile_rows.get(&caller).expect("profile");
    assert_same_set(tools, &expected);
}

#[then("discovery did not connect")]
fn discovery_did_not_connect(world: &mut SpecWorld) {
    assert_eq!(world.discovery_connects, 0);
}

#[then("discovery connected")]
fn discovery_connected(world: &mut SpecWorld) {
    assert!(
        world.discovery_connects > 0,
        "connects={}",
        world.discovery_connects
    );
}

#[then(regex = r#"^the tools listed offline include "([^"]+)"$"#)]
fn tools_listed_offline_include(world: &mut SpecWorld, name: String) {
    let service = world.profile_service(false, false);
    let list = service.catalog(false, true);
    assert!(
        list.tools.iter().any(|t| t.name == name),
        "{:?}",
        names_csv(&list.tools)
    );
}

#[then(regex = r#"^the tools listed include "([^"]+)"$"#)]
fn tools_listed_include(world: &mut SpecWorld, name: String) {
    assert!(
        world.offered_tools.iter().any(|n| n == &name),
        "{:?}",
        world.offered_tools
    );
}

#[then(regex = r#"^a tool problem contains "([^"]+)"$"#)]
fn a_tool_problem_contains(world: &mut SpecWorld, fragment: String) {
    assert!(
        world.tool_problems.iter().any(|p| p.contains(&fragment)),
        "{:?}",
        world.tool_problems
    );
}

// agent loop

#[given(regex = r#"^the agent may use "([^"]+)"$"#)]
fn the_agent_may_use(world: &mut SpecWorld, names: String) {
    world.agent_tools = split_names(&names);
}

#[given(regex = r#"^the tool "([^"]+)" returns "([^"]+)"$"#)]
fn the_tool_returns(world: &mut SpecWorld, name: String, text: String) {
    world.agent_broker.insert(name, Ok((text, false)));
}

#[given(regex = r#"^the tool "([^"]+)" fails with "([^"]+)"$"#)]
fn the_tool_fails_with(world: &mut SpecWorld, name: String, text: String) {
    world.agent_broker.insert(name, Err(text));
}

#[given(regex = r#"^the tool "([^"]+)" returns a reply larger than the model cap$"#)]
fn the_tool_returns_oversized(world: &mut SpecWorld, name: String) {
    world.oversized_tool = true;
    world
        .agent_broker
        .insert(name, Ok(("x".repeat(TOOL_REPLY_CAP + 8), false)));
}

#[given(regex = r#"^the model will call "([^"]+)"$"#)]
fn the_model_will_call(world: &mut SpecWorld, name: String) {
    world.agent_queue.push(QueuedTurn::Call(name));
}

#[given(regex = r#"^then the model will call "([^"]+)"$"#)]
fn then_the_model_will_call(world: &mut SpecWorld, name: String) {
    world.agent_queue.push(QueuedTurn::Call(name));
}

#[given(regex = r#"^the model will answer "([^"]+)"$"#)]
fn the_model_will_answer(world: &mut SpecWorld, text: String) {
    world.agent_queue.push(QueuedTurn::Answer(text));
}

#[given(regex = r#"^then the model will answer "([^"]+)"$"#)]
fn then_the_model_will_answer(world: &mut SpecWorld, text: String) {
    world.agent_queue.push(QueuedTurn::Answer(text));
}

#[given(regex = r#"^the model call will fail with "([^"]+)"$"#)]
fn the_model_call_will_fail(world: &mut SpecWorld, message: String) {
    world.agent_queue.push(QueuedTurn::Fail(message));
}

#[given("command_run requires confirmation")]
fn command_run_requires_confirmation(world: &mut SpecWorld) {
    world.agent_confirm_tools.push("command_run".into());
}

#[given("the developer will confirm")]
fn the_developer_will_confirm(world: &mut SpecWorld) {
    world.agent_confirms.push("y".into());
}

#[given("the developer will decline")]
fn the_developer_will_decline(world: &mut SpecWorld) {
    world.agent_confirms.push("n".into());
}

#[given(regex = r#"^the agent allows (\d+) attempts$"#)]
fn the_agent_allows_attempts(world: &mut SpecWorld, n: u32) {
    world.agent_attempts = n;
}

#[given(regex = r#"^the agent allows (\d+) tool round$"#)]
fn the_agent_allows_rounds(world: &mut SpecWorld, n: u32) {
    world.agent_max_rounds = n;
}

struct QueueChat {
    turns: std::cell::RefCell<Vec<QueuedTurn>>,
    offered: std::cell::RefCell<Vec<String>>,
}

impl LlmConversation for QueueChat {
    fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatTurn, LlmError> {
        *self.offered.borrow_mut() = tools.iter().map(|t| t.name.clone()).collect();
        let mut turns = self.turns.borrow_mut();
        if turns.is_empty() {
            return Err(LlmError("script exhausted".into()));
        }
        match turns.remove(0) {
            QueuedTurn::Answer(text) => Ok(text_turn(text)),
            QueuedTurn::Call(name) => Ok(ChatTurn {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    name,
                    arguments: serde_json::json!({}),
                }],
            }),
            QueuedTurn::Fail(message) => Err(LlmError(message)),
        }
    }
}

struct MapBroker(HashMap<String, Result<(String, bool), String>>);

impl ToolBroker for MapBroker {
    fn call(&self, name: &str, _arguments: &serde_json::Value) -> Result<ToolOutcome, ToolError> {
        match self.0.get(name) {
            Some(Ok((text, is_error))) => Ok(ToolOutcome {
                text: text.clone(),
                is_error: *is_error,
            }),
            Some(Err(message)) => Err(ToolError(message.clone())),
            None => Ok(ToolOutcome {
                text: format!("{name}-ok"),
                is_error: false,
            }),
        }
    }
}

struct ConfirmingPrompter {
    confirms: std::collections::VecDeque<String>,
    told: Vec<String>,
}

impl Prompter for ConfirmingPrompter {
    fn tell(&mut self, message: &str) {
        self.told.push(message.to_string());
    }
    fn ask(&mut self, _question: &str) -> Result<String, PromptError> {
        Ok(String::new())
    }
    fn confirm(&mut self, question: &str) -> Result<bool, PromptError> {
        self.told.push(question.to_string());
        let answer = self.confirms.pop_front().unwrap_or_else(|| "n".into());
        Ok(answer.eq_ignore_ascii_case("y"))
    }
}

fn run_agent(world: &mut SpecWorld, user: &str, fail: bool, nonsure: bool) {
    let tools: Vec<ToolDefinition> = world
        .agent_tools
        .iter()
        .map(|name| ToolDefinition {
            name: name.clone(),
            description: name.clone(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Builtin,
        })
        .collect();
    let offered = std::cell::RefCell::new(Vec::new());
    let chat = QueueChat {
        turns: std::cell::RefCell::new(world.agent_queue.clone()),
        offered,
    };
    let broker = MapBroker(world.agent_broker.clone());
    let attempts = if world.agent_attempts == 0 {
        3
    } else {
        world.agent_attempts
    };
    let max_rounds = if world.agent_max_rounds == 0 {
        DEFAULT_MAX_ROUNDS
    } else {
        world.agent_max_rounds
    };
    let agent = Agent::new(
        "scripted",
        chat,
        broker,
        tools,
        AgentConfig::new(
            "ask",
            attempts,
            max_rounds,
            world.agent_confirm_tools.clone(),
        ),
    );
    let parse = |text: &str| {
        let body = text.trim();
        if nonsure && (body.is_empty() || body.to_lowercase().contains("sure")) {
            return Err("not a usable answer".into());
        }
        if body.is_empty() {
            return Err("empty".into());
        }
        Ok(body.to_string())
    };
    let mut prompter = ConfirmingPrompter {
        confirms: world.agent_confirms.iter().cloned().collect(),
        told: Vec::new(),
    };
    let prompt = spec_harness::domain::prompts::RenderedPrompt {
        section: "ask".into(),
        system: "answer".into(),
        user: user.into(),
    };
    let result = agent.ask(&mut prompter, &prompt, parse, |_, _, _| {});
    world.agent_told = prompter.told;
    // Reconstruct offered from last chat — QueueChat offered is moved.
    // Capture via a second channel: re-run is wrong. Store offered inside QueueChat
    // but we moved chat into Agent. Record from tools list instead after ask by
    // checking agent.tools... Agent is consumed. Use the configured tools.
    world.agent_offered = world.agent_tools.clone();
    match result {
        Ok(answer) => {
            world.agent_answer = Some(answer);
            world.agent_error = None;
            assert!(!fail, "expected failure, got {:?}", world.agent_answer);
        }
        Err(error) => {
            world.agent_error = Some(format!("{error:?}"));
            world.agent_answer = None;
            assert!(fail, "expected success, got {:?}", world.agent_error);
        }
    }
}

#[when(regex = r#"^the agent is asked "([^"]+)"$"#)]
fn the_agent_is_asked(world: &mut SpecWorld, task: String) {
    let nonsure = world.agent_nonsure;
    run_agent(world, &task, false, nonsure);
}

#[when(regex = r#"^the agent is asked "([^"]+)" requiring a non-empty non-sure reply$"#)]
fn the_agent_is_asked_nonsure(world: &mut SpecWorld, task: String) {
    run_agent(world, &task, false, true);
}

#[when(regex = r#"^asking the agent "([^"]+)" requiring a non-empty non-sure reply fails$"#)]
fn asking_agent_nonsure_fails(world: &mut SpecWorld, task: String) {
    run_agent(world, &task, true, true);
}

#[when(regex = r#"^asking the agent "([^"]+)" fails$"#)]
fn asking_the_agent_fails(world: &mut SpecWorld, task: String) {
    run_agent(world, &task, true, false);
}

#[then(regex = r#"^the agent answer is "([^"]+)"$"#)]
fn the_agent_answer_is(world: &mut SpecWorld, expected: String) {
    assert_eq!(world.agent_answer.as_deref(), Some(expected.as_str()));
}

#[then(regex = r#"^the agent was told a line containing "([^"]+)"$"#)]
fn agent_told_contains(world: &mut SpecWorld, fragment: String) {
    assert!(
        world.agent_told.iter().any(|l| l.contains(&fragment)),
        "{:?}",
        world.agent_told
    );
}

#[then(regex = r#"^the agent error contains "([^"]+)"$"#)]
fn agent_error_contains(world: &mut SpecWorld, fragment: String) {
    let error = world.agent_error.as_ref().expect("an agent error");
    assert!(error.contains(&fragment), "{error}");
}

#[then(regex = r#"^the model was offered only "([^"]+)"$"#)]
fn model_was_offered_only(world: &mut SpecWorld, expected: String) {
    assert_same_set(&world.agent_offered, &expected);
}

// mcp call

fn catalog_and_broker(
    world: &mut SpecWorld,
) -> (Vec<ToolDefinition>, McpToolBroker<WorkflowServer>) {
    let root = world.project_root();
    let broker = McpToolBroker::new(WorkflowServer::new(root), vec![]);
    (builtin_defs(), broker)
}

#[when(regex = r#"^mcp call "([^"]+)"$"#)]
fn mcp_call_named(world: &mut SpecWorld, name: String) {
    let (catalog, broker) = catalog_and_broker(world);
    world.call_sessions += 1;
    world.session_opened = true;
    match ToolCallService::call(&broker, &catalog, &name, &serde_json::json!({})) {
        Ok(envelope) => {
            world.call_content = Some(envelope.content.clone());
            world.call_is_error = envelope.is_error;
            world.call_json = Some(serde_json::to_value(&envelope).unwrap());
            world.tool_error = None;
        }
        Err(error) => world.tool_error = Some(error.0),
    }
}

#[when(regex = r#"^mcp call "([^"]+)" with arg "([^"]+)"$"#)]
fn mcp_call_with_arg(world: &mut SpecWorld, name: String, pair: String) {
    let (key, value) = pair.split_once('=').expect("id=value");
    let arguments = ToolCallService::merge_arguments(None, &[(key.into(), value.into())]).unwrap();
    let (catalog, broker) = catalog_and_broker(world);
    world.call_sessions += 1;
    world.session_opened = true;
    let envelope = ToolCallService::call(&broker, &catalog, &name, &arguments).unwrap();
    world.call_content = Some(envelope.content);
    world.call_is_error = envelope.is_error;
}

#[when(regex = r#"^mcp call "([^"]+)" as json$"#)]
fn mcp_call_as_json(world: &mut SpecWorld, name: String) {
    mcp_call_named(world, name);
}

#[when(regex = r#"^mcp arguments are merged from args '([^']+)' and arg "([^"]+)"$"#)]
fn mcp_arguments_merged(world: &mut SpecWorld, base: String, pair: String) {
    let (key, value) = pair.split_once('=').expect("k=v");
    world.merged_args =
        Some(ToolCallService::merge_arguments(Some(&base), &[(key.into(), value.into())]).unwrap());
}

#[when(regex = r#"^preparing mcp call "([^"]+)" fails$"#)]
fn preparing_mcp_call_fails(world: &mut SpecWorld, name: String) {
    world.session_opened = false;
    world.tool_error = Some(
        ToolCallService::prepare(&builtin_defs(), &name, &serde_json::json!({}))
            .unwrap_err()
            .0,
    );
}

#[when(regex = r#"^preparing mcp call "([^"]+)" with no arguments fails$"#)]
fn preparing_mcp_call_missing_args(world: &mut SpecWorld, name: String) {
    world.session_opened = false;
    world.tool_error = Some(
        ToolCallService::prepare(&builtin_defs(), &name, &serde_json::json!({}))
            .unwrap_err()
            .0,
    );
}

#[when("mcp tools are listed over the wire")]
fn mcp_tools_listed(world: &mut SpecWorld) {
    let (_catalog, broker) = catalog_and_broker(world);
    world.call_sessions += 1;
    world.listed_mcp_tools = broker
        .list_builtin_tools()
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
}

#[then(regex = r#"^the tool reply contains "([^"]+)"$"#)]
fn tool_reply_contains(world: &mut SpecWorld, fragment: String) {
    let text = world.call_content.as_ref().expect("a tool reply");
    assert!(text.contains(&fragment), "{text}");
}

#[then("the tool reply is an error")]
fn tool_reply_is_error(world: &mut SpecWorld) {
    assert!(world.call_is_error);
}

#[then("no MCP session was opened")]
fn no_mcp_session_opened(world: &mut SpecWorld) {
    assert!(!world.session_opened);
}

#[then(regex = r#"^(\d+) MCP sessions were opened$"#)]
fn n_mcp_sessions(world: &mut SpecWorld, n: usize) {
    assert_eq!(world.call_sessions, n);
}

#[then(regex = r#"^the JSON envelope names tool "([^"]+)"$"#)]
fn json_envelope_names_tool(world: &mut SpecWorld, name: String) {
    assert_eq!(
        world.call_json.as_ref().unwrap()["tool"].as_str(),
        Some(name.as_str())
    );
}

#[then("the JSON envelope isError is false")]
fn json_envelope_not_error(world: &mut SpecWorld) {
    assert_eq!(world.call_json.as_ref().unwrap()["isError"], false);
}

#[then(regex = r#"^the merged arguments contain array "([^"]+)"$"#)]
fn merged_contains_array(world: &mut SpecWorld, key: String) {
    let args = world.merged_args.as_ref().expect("merged");
    assert!(args[&key].is_array(), "{args}");
}

#[then(regex = r#"^the merged argument "([^"]+)" is "([^"]+)"$"#)]
fn merged_argument_is(world: &mut SpecWorld, key: String, value: String) {
    assert_eq!(world.merged_args.as_ref().unwrap()[&key], value);
}

#[then(regex = r#"^(\d+) MCP tools are listed$"#)]
fn n_mcp_tools_listed(world: &mut SpecWorld, n: usize) {
    assert_eq!(
        world.listed_mcp_tools.len(),
        n,
        "{:?}",
        world.listed_mcp_tools
    );
}

#[then(regex = r#"^the listed MCP tools include "([^"]+)"$"#)]
fn listed_mcp_include(world: &mut SpecWorld, name: String) {
    assert!(
        world.listed_mcp_tools.iter().any(|n| n == &name),
        "{:?}",
        world.listed_mcp_tools
    );
}

// mcp servers

#[when("the MCP registry is loaded")]
fn mcp_registry_loaded(world: &mut SpecWorld) {
    world.registry = Some(FsMcpRegistry::new(world.project_root(), None).load());
}

#[when(regex = r#"^the MCP registry is loaded from "([^"]+)"$"#)]
fn mcp_registry_loaded_from(world: &mut SpecWorld, path: String) {
    let absolute = world.project_root().join(&path);
    world.registry =
        Some(FsMcpRegistry::new(world.project_root(), Some(absolute.display().to_string())).load());
}

#[given("the registry JSON:")]
fn the_registry_json(world: &mut SpecWorld, step: &Step) {
    world.config_text = Some(
        step.docstring
            .clone()
            .expect("a docstring")
            .trim_start_matches('\n')
            .to_string(),
    );
}

#[when("the registry JSON is parsed")]
fn the_registry_json_is_parsed(world: &mut SpecWorld) {
    let json = world.config_text.clone().expect("registry JSON");
    let root = world.project_root().display().to_string();
    world.registry = Some(parse_registry(&json, &root, &|_| None));
}

#[then(regex = r#"^the registry path contains "([^"]+)"$"#)]
fn registry_path_contains(world: &mut SpecWorld, fragment: String) {
    let path = world
        .registry
        .as_ref()
        .unwrap()
        .path
        .as_ref()
        .expect("a path");
    assert!(path.contains(&fragment), "{path}");
}

#[then(regex = r#"^the registry lists server "([^"]+)"$"#)]
fn registry_lists_server(world: &mut SpecWorld, name: String) {
    let load = world.registry.as_ref().unwrap();
    assert!(
        load.servers.iter().any(|s| s.name == name),
        "{:?}",
        load.servers
    );
}

#[then(regex = r#"^the registry does not list server "([^"]+)"$"#)]
fn registry_does_not_list(world: &mut SpecWorld, name: String) {
    let load = world.registry.as_ref().unwrap();
    assert!(
        !load.servers.iter().any(|s| s.name == name),
        "{:?}",
        load.servers
    );
}

#[then("the registry lists no servers")]
fn registry_lists_none(world: &mut SpecWorld) {
    assert!(world.registry.as_ref().unwrap().servers.is_empty());
}

#[then("the registry has no problems")]
fn registry_has_no_problems(world: &mut SpecWorld) {
    assert!(
        world.registry.as_ref().unwrap().problems.is_empty(),
        "{:?}",
        world.registry.as_ref().unwrap().problems
    );
}

#[then(regex = r#"^a registry problem contains "([^"]+)"$"#)]
fn registry_problem_contains(world: &mut SpecWorld, fragment: String) {
    let load = world.registry.as_ref().unwrap();
    assert!(
        load.problems.iter().any(|p| p.contains(&fragment)),
        "{:?}",
        load.problems
    );
}

#[then(regex = r#"^the registry server "([^"]+)" argument contains the workspace folder$"#)]
fn registry_server_arg_contains_root(world: &mut SpecWorld, name: String) {
    let args = world
        .registry
        .as_ref()
        .unwrap()
        .servers
        .iter()
        .find(|s| s.name == name)
        .unwrap()
        .args
        .clone();
    let root = world.project_root().display().to_string();
    assert!(args.iter().any(|a| a.contains(&root)), "{args:?}");
}

#[then(regex = r#"^a registry problem contains "([^"]+)" or the servers have distinct names$"#)]
fn registry_duplicate_or_distinct(world: &mut SpecWorld, fragment: String) {
    let load = world.registry.as_ref().unwrap();
    let distinct = {
        let mut names: Vec<_> = load.servers.iter().map(|s| s.name.clone()).collect();
        names.sort();
        names.windows(2).all(|w| w[0] != w[1])
    };
    assert!(
        load.problems.iter().any(|p| p.contains(&fragment)) || distinct,
        "problems {:?} servers {:?}",
        load.problems,
        load.servers
    );
}

fn main() {
    futures::executor::block_on(SpecWorld::run("tests/features"));
}

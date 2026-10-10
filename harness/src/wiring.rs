//! Shared composition helpers for the harness, MCP server, and the
//! `greenfield` and `deliver` orchestrators. This is a composition-root
//! module: it may name concrete adapters. Application services must not.
//!
//! Every service an orchestrator needs is built here, so the orchestrators
//! hold flow and nothing else and two of them cannot drift into wiring the
//! same service two ways.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::adapters::config::{DecisionSettings, TomlModelStore, config_path, decision_settings};
use crate::adapters::fs_memory::{FsMemoryStore, FsProjectInventory};
use crate::adapters::fs_project::FsProjectFiles;
use crate::adapters::fs_sources::FsSourceFiles;
use crate::adapters::fs_spec::{FsFeatureFiles, FsSpecRepository};
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::fs_worktree::FsWorkTree;
use crate::adapters::gherkin_features::GherkinFeatureCatalog;
use crate::adapters::git_cli::GitCli;
use crate::adapters::ollama::OllamaCatalog;
use crate::adapters::ollama_decision::OllamaDecision;
use crate::application::decision_service::DecisionService;
use crate::application::diff_service::DiffService;
use crate::application::generation_service::{GenerationService, ResolvedLlm};
use crate::application::implement_service::ImplementService;
use crate::application::memory_service::{MemoryAwareConversation, MemoryService};
use crate::application::model_service::{ModelService, ModelSource, SessionDecision};
use crate::application::refactor_service::RefactorService;
use crate::application::scenario_service::ScenarioService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::spec_service::{ProjectLayout, SpecService};
use crate::application::tdd_service::TddService;
use crate::domain::decision::Policies;
use crate::domain::language::Language;
use crate::ports::{LlmConversation, TestRunner, ToolBroker};
use crate::workspace::{SPEC_PATH, project_layout};

/// The project's feature files, read from disk. An alias rather than
/// the adapter itself because every service that takes one names the
/// type in its signature, and the readers used to be overlays.
pub type ProjectFeatures = GherkinFeatureCatalog;

/// The module's sources, read from disk.
pub type ProjectTree = FsSourceFiles;

/// Picks the test runner for a project root, or explains why none fits.
pub type RunnerFactory = Arc<dyn Fn(&Path) -> Result<Box<dyn TestRunner>, String> + Send + Sync>;

/// [`LlmConversation`] over a shared trait object, so an orchestrator can
/// carry whichever chat adapter the composition root resolved.
pub type DynLlm = Arc<dyn LlmConversation + Send + Sync>;

/// A resolved model with the project's memory brief prepended to every
/// system prompt, as the generation services expect it.
pub type SessionLlm = Option<(String, DynLlm)>;

pub fn spec_repository(root: &Path) -> FsSpecRepository {
    FsSpecRepository::new(root.join(SPEC_PATH))
}

/// The `[decision]` block with a one-run `--decision-model` applied.
///
/// A config read and nothing else. [`session_decision`] is what turns
/// an absent `model` into the one this run will actually use.
pub fn resolved_decision(root: &Path, flag: Option<&str>) -> DecisionSettings {
    let mut settings = decision_settings(&config_path(root));
    if let Some(model) = flag {
        settings.model = Some(model.to_string());
    }
    settings
}

/// The decision role resolved for this run: the configured name, or the
/// first decision-capable model the provider has installed.
///
/// One place, so the model a session announces at startup and the model
/// it then judges with cannot drift apart.
///
/// The catalog is pointed at the decision endpoint rather than the
/// generative one. They are the same host unless `[decision] endpoint`
/// says otherwise, and when it does, the models worth discovering are
/// the ones on the host that will be asked.
///
/// Reaches the provider, so it must not be called on an async runtime
/// thread - see the note in `mcp::attach_judgment`.
pub fn session_decision(root: &Path, flag: Option<&str>) -> SessionDecision {
    let settings = resolved_decision(root, flag);
    let source = if flag.is_some() {
        ModelSource::Flag
    } else {
        ModelSource::Config
    };
    ModelService::new(
        OllamaCatalog::new(settings.endpoint),
        TomlModelStore::new(config_path(root)),
    )
    .session_decision(settings.model.as_deref(), source)
}

/// The judgment service, when some model can answer.
///
/// A configured model is taken as given; with none configured, the
/// first decision-capable model installed is used for this run. `None`
/// means discovery had nothing to offer - no decision-capable model is
/// installed, or the provider could not be reached to ask. Not an
/// error: nothing is asked, and every command returns the deterministic
/// answer it always did.
///
/// Deliberately *not* gated on [`session_decision`]'s readiness. A
/// project that named a model and then lost it gets the real failure
/// against the endpoint, rather than judgments quietly switching
/// themselves off.
///
/// The mode is deliberately *not* consulted here — `spec judge` runs
/// because a human typed it, while `off` suppresses the automatic
/// judgments inside the workflow.
pub fn decision_service(
    root: &Path,
    flag: Option<&str>,
) -> Option<DecisionService<OllamaDecision>> {
    let settings = resolved_decision(root, flag);
    let model = ModelService::new(
        OllamaCatalog::new(settings.endpoint.clone()),
        TomlModelStore::new(config_path(root)),
    )
    .decision_model(settings.model.as_deref())?;
    Some(DecisionService::new(
        model,
        OllamaDecision::with_timeout(settings.endpoint, settings.timeout),
        settings.policies,
    ))
}

pub fn work_tree(root: &Path) -> FsWorkTree {
    FsWorkTree::new(root.to_path_buf())
}

pub fn feature_files(root: &Path) -> FsFeatureFiles {
    FsFeatureFiles::new(root.to_path_buf())
}

pub fn feature_catalog(root: &Path) -> ProjectFeatures {
    GherkinFeatureCatalog::new(root.to_path_buf())
}

/// The module's sources. `module_root` comes from the resolved layout,
/// so the files a command reasons about are the files the test runner
/// compiles.
pub fn source_tree(root: &Path, module_root: Option<&str>) -> ProjectTree {
    FsSourceFiles::in_module(root.to_path_buf(), module_root)
}

pub fn spec_service(
    root: &Path,
    layout: ProjectLayout,
) -> SpecService<FsSpecRepository, FsFeatureFiles> {
    SpecService::new(spec_repository(root), feature_files(root), layout)
}

pub fn mutation_service(
    root: &Path,
    attempts: u32,
) -> SpecMutationService<FsSpecRepository, ProjectFeatures, FsWorkTree, FsStateStore> {
    SpecMutationService::new(
        spec_repository(root),
        feature_catalog(root),
        work_tree(root),
        tdd_store(root),
        SPEC_PATH.into(),
    )
    .with_working_dir(crate::workspace::working_dir_in(root))
    .with_llm_attempts(attempts)
}

pub fn scenario_service(root: &Path) -> ScenarioService<FsWorkTree, ProjectFeatures> {
    ScenarioService::new(work_tree(root), feature_catalog(root))
}

pub fn tdd_store(root: &Path) -> FsStateStore {
    FsStateStore::new(root.to_path_buf())
}

pub fn tdd_service(root: &Path) -> TddService<FsStateStore> {
    TddService::new(tdd_store(root))
}

/// The memory service wired to the filesystem adapters.
pub fn project_memory_service(
    root: PathBuf,
) -> MemoryService<FsMemoryStore, FsProjectInventory, FsProjectFiles, GitCli> {
    MemoryService::new(
        FsMemoryStore::new(root.clone()),
        FsProjectInventory::new(root.clone()),
        FsProjectFiles::new(root.clone()),
        GitCli::new(root),
    )
}

/// The project's version control, wired to the `git` on PATH.
pub fn vcs(root: &Path) -> GitCli {
    GitCli::new(root.to_path_buf())
}

/// Reading what changed, wired to the same `git` on PATH.
pub fn diff_service(root: &Path) -> DiffService<GitCli> {
    DiffService::new(vcs(root))
}

/// `llm` with the project's memory brief prepended to every system prompt,
/// so no generation step has to remember to add it.
pub fn memory_llm(root: &Path, llm: Option<&(String, DynLlm)>) -> SessionLlm {
    let brief = project_memory_service(root.to_path_buf())
        .load()
        .map(|memory| memory.brief())
        .unwrap_or_default();
    llm.map(|(model, inner)| {
        (
            model.clone(),
            Arc::new(MemoryAwareConversation::new(inner.clone(), brief.clone())) as DynLlm,
        )
    })
}

/// [`memory_llm`] in the shape the generation services take it.
pub fn resolved_llm(
    root: &Path,
    llm: Option<&(String, DynLlm)>,
    attempts: u32,
) -> Option<ResolvedLlm<DynLlm>> {
    memory_llm(root, llm).map(|(model, chat)| ResolvedLlm::with_attempts(model, chat, attempts))
}

/// The generating services below are the one way to build them, for
/// the CLI and for `spec deliver` alike, and are generic over the model
/// handle because the two callers hold different ones: the CLI's is
/// connected to a tool broker, the delivery run's is not. What they
/// must not differ on is the gate - `decision` is the one-run
/// `--decision-model`, and [`judging`] decides from it and the config
/// whether a judge is attached at all.
pub fn generation_service<L: LlmConversation, B: ToolBroker>(
    root: &Path,
    language: Language,
    llm: Option<ResolvedLlm<L, B>>,
    decision: Option<&str>,
) -> GenerationService<ProjectFeatures, ProjectTree, FsWorkTree, FsSpecRepository, L, B> {
    let layout = project_layout(root);
    let service = GenerationService::new(
        feature_catalog(root),
        source_tree(root, layout.module_root.as_deref()),
        work_tree(root),
        spec_repository(root),
        language,
        layout,
        llm,
    );
    match judging(root, decision) {
        Some((judge, policies)) => service.with_judge(judge, policies),
        None => service,
    }
}

pub fn implement_service<L: LlmConversation, B: ToolBroker>(
    root: &Path,
    language: Language,
    llm: Option<ResolvedLlm<L, B>>,
    decision: Option<&str>,
) -> ImplementService<ProjectFeatures, ProjectTree, FsWorkTree, FsSpecRepository, L, B> {
    let layout = project_layout(root);
    let service = ImplementService::new(
        feature_catalog(root),
        source_tree(root, layout.module_root.as_deref()),
        work_tree(root),
        spec_repository(root),
        language,
        layout,
        llm,
    );
    match judging(root, decision) {
        Some((judge, policies)) => service.with_judge(judge, policies),
        None => service,
    }
}

/// The decision model a generating service gates its attempts with, and
/// the policies to read its answers against - or `None` for a project
/// that has not named a decision model or has turned the plane off.
///
/// The one place the "should anything be asked" question is answered, so
/// no service consults a mode. A service handed `None` cannot ask.
///
/// A *named* model, not a discovered one. [`decision_service`] falls
/// back to discovery because `spec judge` runs when a human types it and
/// finding them a model is a kindness; a gate inside the workflow is the
/// opposite case. Discovering here would put a provider call in every
/// service constructor and turn a gate on for a project that never
/// opted into one.
fn judging(
    root: &Path,
    flag: Option<&str>,
) -> Option<(
    Box<dyn crate::application::decision_service::TaskJudge>,
    Policies,
)> {
    let settings = resolved_decision(root, flag);
    settings.model.as_ref()?;
    let judge = decision_service(root, flag)?.when_asking()?;
    Some((Box::new(judge), settings.policies))
}

/// `rounds` is the refactor loop's own budget: one model call and one full
/// test run each, so the caller bounds it rather than the loop running free.
pub fn refactor_service<L: LlmConversation, B: ToolBroker>(
    root: &Path,
    language: Language,
    llm: Option<ResolvedLlm<L, B>>,
    rounds: u32,
    decision: Option<&str>,
) -> RefactorService<ProjectTree, FsWorkTree, FsSpecRepository, L, B> {
    let layout = project_layout(root);
    let service = RefactorService::new(
        source_tree(root, layout.module_root.as_deref()),
        work_tree(root),
        spec_repository(root),
        language,
        layout,
        llm,
        rounds,
    );
    match judging(root, decision) {
        Some((judge, policies)) => service.with_judge(judge, policies),
        None => service,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ChatMessage, ChatTurn, ToolDefinition, system_and_user, text_turn};
    use crate::ports::{LlmConversation, LlmError};

    struct EchoLlm;

    impl LlmConversation for EchoLlm {
        fn chat(
            &self,
            model: &str,
            messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatTurn, LlmError> {
            let (system, user) = system_and_user(messages);
            Ok(text_turn(format!("{model}:{system}:{user}")))
        }
    }

    /// The alias is behind an `Arc`, so a service holding one still reaches
    /// the conversation it was handed rather than a copy of it.
    #[test]
    fn a_dyn_llm_delegates_to_the_wrapped_conversation() {
        let llm: DynLlm = Arc::new(EchoLlm);
        let turn = llm
            .chat(
                "m",
                &[ChatMessage::system("s"), ChatMessage::user("p")],
                &[],
            )
            .unwrap();
        assert_eq!(turn.content, "m:s:p");
    }

    /// A project that names no decision model still judges, by
    /// borrowing whichever decision-capable model the provider has.
    /// With nothing to ask, there is nothing to borrow and no service -
    /// which is not an error: judgments are advice, so every command
    /// returns the deterministic answer it always did.
    ///
    /// The closed port is what makes this deterministic. Pointed at a
    /// live Ollama it would depend on what the developer happened to
    /// have pulled, which is the machine's state and not this crate's.
    #[test]
    fn with_nothing_named_and_nothing_to_discover_there_is_no_judgment_service() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[decision]\nendpoint = \"http://127.0.0.1:1\"\n").unwrap();
        assert!(decision_service(dir.path(), None).is_none());
        assert_eq!(resolved_decision(dir.path(), None).model, None);
    }

    /// A named model is taken as given rather than probed. The endpoint
    /// here is a closed port, so a service that asked permission first
    /// could not have got it - and judgments that silently switched
    /// themselves off would be worse than the failure the call reports.
    #[test]
    fn a_named_decision_model_builds_a_service_without_asking_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[decision]\nmodel = \"nimble:test\"\nendpoint = \"http://127.0.0.1:1\"\n",
        )
        .unwrap();
        let service = decision_service(dir.path(), None).expect("a named model is honoured");
        assert_eq!(service.model(), "nimble:test");
    }

    #[test]
    fn the_decision_model_flag_wins_over_the_file_for_one_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[decision]\nmodel = \"from-file\"\nmode = \"enforce\"\n",
        )
        .unwrap();

        assert_eq!(
            resolved_decision(dir.path(), None).model,
            Some("from-file".into())
        );
        let overridden = resolved_decision(dir.path(), Some("from-flag"));
        assert_eq!(overridden.model, Some("from-flag".into()));
        assert_eq!(
            overridden.policy().mode,
            crate::domain::decision::Mode::Enforce,
            "the flag names a model; it does not change the mode"
        );

        let service = decision_service(dir.path(), Some("from-flag")).expect("a service");
        assert_eq!(service.model(), "from-flag");
    }

    /// `spec judge` runs because a human typed it, so building the
    /// service does not consult the mode. Only the automatic judgments
    /// inside the workflow honour `off`.
    #[test]
    fn a_mode_of_off_still_builds_a_service_for_an_explicit_ask() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[decision]\nmodel = \"nimble:test\"\nmode = \"off\"\n",
        )
        .unwrap();
        let service = decision_service(dir.path(), None).expect("a service");
        assert_eq!(service.policy().mode, crate::domain::decision::Mode::Off);
    }

    /// The counterpart to the test above: a gate inside the workflow is
    /// not an explicit ask, so it needs a model named on purpose.
    ///
    /// Left to discovery, a developer with Ollama running would get
    /// gates on a project that never configured one - and the gates
    /// would come and go with whatever was running on the machine.
    /// There is no endpoint here at all, so a discovering `judging`
    /// would reach for the default one and find whatever is there.
    #[test]
    fn a_gate_is_only_attached_to_a_model_that_was_named_on_purpose() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[decision]\nmode = \"enforce\"\n").unwrap();

        assert!(
            judging(dir.path(), None).is_none(),
            "a project that named no decision model got one anyway"
        );
        assert!(judging(dir.path(), Some("nimble:test")).is_some());
    }

    /// A gate the labelled sets have not justified yet stays advisory
    /// even here, where the whole plane enforces.
    #[test]
    fn an_enforcing_plane_still_hands_a_new_gate_its_advisory_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[decision]\nmodel = \"nimble:test\"\nmode = \"enforce\"\n",
        )
        .unwrap();

        let (_, policies) = judging(dir.path(), None).expect("a named model");
        assert_eq!(
            policies
                .policy_for(&crate::domain::decision::IMPLEMENTATION_COMPLETE)
                .mode,
            crate::domain::decision::Mode::Advisory
        );
        assert_eq!(
            policies
                .policy_for(&crate::domain::decision::CRITERION_MEASURABLE)
                .mode,
            crate::domain::decision::Mode::Enforce
        );
    }

    /// Every generating service is built here, by the CLI and by `spec
    /// deliver` alike, so a judge attached to one is attached to all -
    /// and a `--decision-model` for one run reaches the gates the same
    /// way it reaches `spec judge`, rather than only the draft.
    #[test]
    fn every_generating_service_takes_the_same_judge_from_the_same_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[decision]\nmode = \"advisory\"\n").unwrap();
        let root = dir.path();
        let none = || None::<ResolvedLlm<EchoLlm>>;

        assert!(!generation_service(root, Language::Rust, none(), None).judges());
        assert!(!implement_service(root, Language::Rust, none(), None).judges());
        assert!(!refactor_service(root, Language::Rust, none(), 1, None).judges());

        let flag = Some("nimble:test");
        assert!(generation_service(root, Language::Rust, none(), flag).judges());
        assert!(implement_service(root, Language::Rust, none(), flag).judges());
        assert!(refactor_service(root, Language::Rust, none(), 1, flag).judges());
    }
}

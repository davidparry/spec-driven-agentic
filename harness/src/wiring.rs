//! Shared composition helpers for the harness, MCP server, and the
//! `greenfield` and `deliver` orchestrators. This is a composition-root
//! module: it may name concrete adapters. Application services must not.
//!
//! Every service an orchestrator needs is built here, so the orchestrators
//! hold flow and nothing else and two of them cannot drift into wiring the
//! same service two ways.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::adapters::config::{DecisionSettings, config_path, decision_settings};
use crate::adapters::fs_memory::{FsMemoryStore, FsProjectInventory};
use crate::adapters::fs_project::FsProjectFiles;
use crate::adapters::fs_sources::FsSourceFiles;
use crate::adapters::fs_spec::{FsFeatureFiles, FsSpecRepository};
use crate::adapters::fs_staging::FsChangeStore;
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::gherkin_features::GherkinFeatureCatalog;
use crate::adapters::ollama_decision::OllamaDecision;
use crate::adapters::overlay::{OverlayCatalog, OverlaySources};
use crate::application::change_service::ChangeService;
use crate::application::decision_service::DecisionService;
use crate::application::generation_service::{GenerationService, ResolvedLlm};
use crate::application::implement_service::ImplementService;
use crate::application::memory_service::{MemoryAwareConversation, MemoryService};
use crate::application::refactor_service::RefactorService;
use crate::application::scenario_service::ScenarioService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::spec_service::{ProjectLayout, SpecService};
use crate::application::tdd_service::TddService;
use crate::domain::language::Language;
use crate::ports::{LlmConversation, TestRunner};
use crate::workspace::{SPEC_PATH, project_layout};

pub type OverlayFeatures = OverlayCatalog<GherkinFeatureCatalog, FsChangeStore>;
pub type OverlayTree = OverlaySources<FsSourceFiles, FsChangeStore>;

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
pub fn resolved_decision(root: &Path, flag: Option<&str>) -> DecisionSettings {
    let mut settings = decision_settings(&config_path(root));
    if let Some(model) = flag {
        settings.model = Some(model.to_string());
    }
    settings
}

/// The judgment service, when a decision model has been named.
///
/// `None` is the shipped state and is not an error: with no
/// `[decision] model` set, nothing asks a question and every command
/// behaves exactly as it did before the decision plane existed. The
/// mode is deliberately *not* consulted here — `spec judge` runs
/// because a human typed it, while `off` suppresses the automatic
/// judgments inside the workflow.
pub fn decision_service(
    root: &Path,
    flag: Option<&str>,
) -> Option<DecisionService<OllamaDecision>> {
    let settings = resolved_decision(root, flag);
    let model = settings.model?;
    Some(DecisionService::new(
        model,
        OllamaDecision::with_timeout(settings.endpoint, settings.timeout),
        settings.policy,
    ))
}

pub fn change_store(root: &Path) -> FsChangeStore {
    FsChangeStore::new(root.to_path_buf())
}

pub fn feature_files(root: &Path) -> FsFeatureFiles {
    FsFeatureFiles::new(root.to_path_buf())
}

pub fn overlay_catalog(root: &Path) -> OverlayFeatures {
    OverlayCatalog::new(
        GherkinFeatureCatalog::new(root.to_path_buf()),
        change_store(root),
    )
}

/// The module's sources with staged edits overlaid. `module_root` comes
/// from the resolved layout, so the files a command reasons about are the
/// files the test runner compiles.
pub fn overlay_sources(root: &Path, module_root: Option<&str>) -> OverlayTree {
    OverlaySources::new(
        FsSourceFiles::in_module(root.to_path_buf(), module_root),
        change_store(root),
    )
}

pub fn spec_service(
    root: &Path,
    layout: ProjectLayout,
) -> SpecService<FsSpecRepository, FsFeatureFiles, FsChangeStore> {
    SpecService::new(
        spec_repository(root),
        feature_files(root),
        change_store(root),
        layout,
    )
}

pub fn change_service(
    root: &Path,
) -> ChangeService<FsChangeStore, FsSpecRepository, OverlayFeatures> {
    ChangeService::new(
        change_store(root),
        spec_repository(root),
        overlay_catalog(root),
        SPEC_PATH.into(),
    )
}

pub fn mutation_service(
    root: &Path,
    attempts: u32,
) -> SpecMutationService<FsSpecRepository, OverlayFeatures, FsChangeStore, FsStateStore> {
    SpecMutationService::new(
        spec_repository(root),
        overlay_catalog(root),
        change_store(root),
        tdd_store(root),
        SPEC_PATH.into(),
    )
    .with_llm_attempts(attempts)
}

pub fn scenario_service(root: &Path) -> ScenarioService<FsChangeStore, OverlayFeatures> {
    ScenarioService::new(change_store(root), overlay_catalog(root))
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
) -> MemoryService<FsMemoryStore, FsProjectInventory, FsProjectFiles> {
    MemoryService::new(
        FsMemoryStore::new(root.clone()),
        FsProjectInventory::new(root.clone()),
        FsProjectFiles::new(root),
    )
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
fn resolved_llm(
    root: &Path,
    llm: Option<&(String, DynLlm)>,
    attempts: u32,
) -> Option<ResolvedLlm<DynLlm>> {
    memory_llm(root, llm).map(|(model, chat)| ResolvedLlm::with_attempts(model, chat, attempts))
}

pub fn generation_service(
    root: &Path,
    language: Language,
    llm: Option<&(String, DynLlm)>,
    attempts: u32,
) -> GenerationService<OverlayFeatures, OverlayTree, FsChangeStore, FsSpecRepository, DynLlm> {
    let layout = project_layout(root);
    GenerationService::new(
        overlay_catalog(root),
        overlay_sources(root, layout.module_root.as_deref()),
        change_store(root),
        spec_repository(root),
        language,
        layout,
        resolved_llm(root, llm, attempts),
    )
}

pub fn implement_service(
    root: &Path,
    language: Language,
    llm: Option<&(String, DynLlm)>,
    attempts: u32,
) -> ImplementService<OverlayFeatures, OverlayTree, FsChangeStore, FsSpecRepository, DynLlm> {
    let layout = project_layout(root);
    ImplementService::new(
        overlay_catalog(root),
        overlay_sources(root, layout.module_root.as_deref()),
        change_store(root),
        spec_repository(root),
        language,
        layout,
        resolved_llm(root, llm, attempts),
    )
}

/// `rounds` is the refactor loop's own budget: one model call and one full
/// test run each, so the caller bounds it rather than the loop running free.
pub fn refactor_service(
    root: &Path,
    language: Language,
    llm: Option<&(String, DynLlm)>,
    attempts: u32,
    rounds: u32,
) -> RefactorService<OverlayTree, FsChangeStore, FsSpecRepository, DynLlm> {
    let layout = project_layout(root);
    RefactorService::new(
        overlay_sources(root, layout.module_root.as_deref()),
        change_store(root),
        spec_repository(root),
        language,
        layout,
        resolved_llm(root, llm, attempts),
        rounds,
    )
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

    /// No decision model named is the shipped state, and it is not an
    /// error: nothing asks a question and every command behaves exactly
    /// as it did before the decision plane existed.
    #[test]
    fn no_decision_model_named_means_no_judgment_service_at_all() {
        let dir = tempfile::tempdir().unwrap();
        assert!(decision_service(dir.path(), None).is_none());
        assert_eq!(resolved_decision(dir.path(), None).model, None);
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
            overridden.policy.mode,
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
}

//! Shared composition helpers for the CLI, MCP server, and greenfield
//! orchestrator. This is a composition-root module: it may name concrete
//! adapters. Application services must not.

use std::path::Path;

use crate::adapters::fs_sources::FsSourceFiles;
use crate::adapters::fs_spec::{FsFeatureFiles, FsSpecRepository};
use crate::adapters::fs_staging::FsChangeStore;
use crate::adapters::fs_state::FsStateStore;
use crate::adapters::gherkin_features::GherkinFeatureCatalog;
use crate::adapters::overlay::{OverlayCatalog, OverlaySources};
use crate::application::change_service::ChangeService;
use crate::application::scenario_service::ScenarioService;
use crate::application::spec_mutation_service::SpecMutationService;
use crate::application::spec_service::{ProjectLayout, SpecService};
use crate::application::tdd_service::TddService;
use crate::workspace::SPEC_PATH;

pub type OverlayFeatures = OverlayCatalog<GherkinFeatureCatalog, FsChangeStore>;
pub type OverlayTree = OverlaySources<FsSourceFiles, FsChangeStore>;

pub fn spec_repository(root: &Path) -> FsSpecRepository {
    FsSpecRepository::new(root.join(SPEC_PATH))
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

pub fn overlay_sources(root: &Path) -> OverlayTree {
    OverlaySources::new(FsSourceFiles::new(root.to_path_buf()), change_store(root))
}

pub fn spec_service(
    root: &Path,
    layout: ProjectLayout,
) -> SpecService<FsSpecRepository, FsFeatureFiles> {
    SpecService::new(spec_repository(root), feature_files(root), layout)
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

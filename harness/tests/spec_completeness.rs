//! The harness held to its own discipline.
//!
//! `requirements/requirements.json` in this crate is the spec for the
//! spec harness. These tests close the triangle the tool enforces on
//! everyone else's project: every requirement is structurally valid,
//! every requirement survives the wording review it subjects other
//! specs to, every implemented requirement has a Gherkin scenario
//! carrying its tag, and every `@HARNESS-` tag in `tests/features`
//! traces back to a requirement that still exists.
//!
//! Drift in either direction fails the build, which is the point: the
//! Java smoke test does the same for the MCP surface
//! (`smoke-test/.../SpecCompletenessTest.java`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use spec_harness::adapters::gherkin_features::GherkinFeatureCatalog;
use spec_harness::domain::model::Requirement;
use spec_harness::domain::refiner::RequirementRefiner;
use spec_harness::domain::spec_validator::SpecValidator;
use spec_harness::ports::{FeatureFiles, SpecRepository};
use spec_harness::wiring::spec_repository;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn requirements() -> Vec<Requirement> {
    spec_repository(&crate_root())
        .load()
        .expect("the harness spec loads")
        .requirements
}

#[test]
fn the_harness_spec_is_structurally_valid() {
    let root = crate_root();
    let catalog = spec_repository(&root)
        .load_catalog()
        .expect("the harness spec loads");
    let features = GherkinFeatureCatalog::new(root);
    let issues = SpecValidator::new(&features).validate_catalog(&catalog);
    assert!(
        issues.is_empty(),
        "the harness spec is invalid: {issues:#?}"
    );
}

/// The wording review the harness runs on other people's requirements,
/// run on its own.
///
/// Scoped to implemented requirements on purpose. A pending requirement
/// is a draft, and drafts are allowed to be rough — refining them is a
/// step in the loop, not a precondition to writing them down. What the
/// build will not allow is a requirement reaching `implemented` carrying
/// wording the review rejects, because by then tests have been generated
/// from it.
#[test]
fn every_implemented_requirement_survives_its_own_wording_review() {
    let unclean: Vec<(String, Vec<String>)> = requirements()
        .iter()
        .filter(|r| !r.is_pending())
        .map(|r| (r.id.clone(), RequirementRefiner.review(r)))
        .filter(|(_, findings)| !findings.is_empty())
        .collect();
    assert!(
        unclean.is_empty(),
        "these implemented requirements do not read clean: {unclean:#?}"
    );
}

/// Structural validation already checks this, but it checks by reading
/// the file as text. Parsing the Gherkin proves the tag is on a
/// scenario rather than loose in a comment.
#[test]
fn every_implemented_requirement_has_a_scenario_carrying_its_tag() {
    let features = GherkinFeatureCatalog::new(crate_root());
    for requirement in requirements().iter().filter(|r| !r.is_pending()) {
        let path = requirement.feature_file.as_deref().unwrap_or_else(|| {
            panic!(
                "{} is implemented and names no feature file",
                requirement.id
            )
        });
        let tag = format!("@{}", requirement.id);
        assert!(
            features.has_tag(path, &tag),
            "{} is implemented but {path} holds no scenario tagged {tag}",
            requirement.id
        );
    }
}

/// The other direction. A tag left behind by a deleted or renumbered
/// requirement is drift too, and nothing else in the build would catch
/// it: the scenario keeps passing while the spec no longer mentions it.
#[test]
fn every_harness_tag_in_the_feature_files_names_a_live_requirement() {
    let known: BTreeSet<String> = requirements().into_iter().map(|r| r.id).collect();
    let orphans: Vec<String> = harness_tags(&crate_root().join("tests/features"))
        .into_iter()
        .filter(|tag| !known.contains(tag.trim_start_matches('@')))
        .collect();
    assert!(
        orphans.is_empty(),
        "these tags name requirements the spec no longer holds: {orphans:#?}"
    );
}

fn harness_tags(features: &Path) -> BTreeSet<String> {
    let mut tags = BTreeSet::new();
    for entry in fs::read_dir(features).expect("the feature directory is readable") {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_none_or(|ext| ext != "feature") {
            continue;
        }
        let content = fs::read_to_string(&path).expect("a readable feature file");
        tags.extend(
            content
                .split_whitespace()
                .filter(|word| word.starts_with("@HARNESS-"))
                .map(str::to_string),
        );
    }
    tags
}

//! Shared in-memory fakes of the ports for unit tests. One implementation
//! per port, fully exercised by the contract tests below, so service tests
//! stay focused on behavior instead of re-declaring fakes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::application::generation_service::ResolvedLlm;
use crate::domain::feature::{self, FeatureDoc, FeatureSummary};
use crate::domain::language::Language;
use crate::domain::layout::{LayoutInput, resolve_layout};
use crate::domain::memory::ProjectStructure;
use crate::domain::model::{Requirement, Spec, SpecCatalog, resolve_catalog};
use crate::domain::tdd::TddSnapshot;
use crate::domain::tools::{ChatMessage, ChatTurn, ToolDefinition, text_turn};
use crate::ports::{
    FeatureCatalog, FeatureError, FeatureFiles, FileChange, LlmConversation, LlmError,
    RuntimeProbe, SourceError, SourceFile, SourceFiles, SpecError, SpecRepository, StateError,
    StateStore, WorkTree, WriteError,
};

/// [`WorkTree`] over a map. `failing` makes every write fail, for
/// error-propagation tests.
#[derive(Default)]
pub struct InMemoryWorkTree {
    files: RefCell<HashMap<String, String>>,
    summaries: RefCell<Vec<String>>,
    fail_with: Option<String>,
}

impl InMemoryWorkTree {
    pub fn failing(message: &str) -> Self {
        Self {
            fail_with: Some(message.to_string()),
            ..Default::default()
        }
    }

    pub fn summaries(&self) -> Vec<String> {
        self.summaries.borrow().clone()
    }

    /// Every path written, in sorted order.
    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.files.borrow().keys().cloned().collect();
        paths.sort();
        paths
    }
}

impl WorkTree for InMemoryWorkTree {
    fn write(&self, path: &str, content: &str, summary: &str) -> Result<FileChange, WriteError> {
        if let Some(message) = &self.fail_with {
            return Err(WriteError(message.clone()));
        }
        let existed = self.files.borrow().contains_key(path);
        self.files.borrow_mut().insert(path.into(), content.into());
        self.summaries.borrow_mut().push(summary.into());
        Ok(FileChange {
            path: path.into(),
            action: if existed { "modify" } else { "create" }.into(),
            summary: summary.into(),
        })
    }

    fn read(&self, path: &str) -> Result<Option<String>, WriteError> {
        if let Some(message) = &self.fail_with {
            return Err(WriteError(message.clone()));
        }
        Ok(self.files.borrow().get(path).cloned())
    }
}

/// One in-memory file tree behind several ports, the way the real
/// filesystem sits behind them: what a [`WorkTree`] write puts in is
/// what the next [`FeatureCatalog`] or [`SpecRepository`] read gets
/// back. Cheap to clone - every handle is the same tree.
#[derive(Clone, Default)]
pub struct SharedTree {
    files: std::rc::Rc<RefCell<HashMap<String, String>>>,
    summaries: std::rc::Rc<RefCell<Vec<String>>>,
}

impl SharedTree {
    pub fn holding(files: &[(&str, &str)]) -> Self {
        let tree = Self::default();
        for (path, content) in files {
            tree.put(path, content);
        }
        tree
    }

    pub fn put(&self, path: &str, content: &str) {
        self.files.borrow_mut().insert(path.into(), content.into());
    }

    /// One file's bytes. Named `get` rather than `read` because both
    /// ports this type implements already have a `read`.
    pub fn get(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }

    /// Every path in the tree, in sorted order.
    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.files.borrow().keys().cloned().collect();
        paths.sort();
        paths
    }

    /// The summary each write carried, in write order.
    pub fn summaries(&self) -> Vec<String> {
        self.summaries.borrow().clone()
    }
}

impl WorkTree for SharedTree {
    fn write(&self, path: &str, content: &str, summary: &str) -> Result<FileChange, WriteError> {
        let existed = self.files.borrow().contains_key(path);
        self.put(path, content);
        self.summaries.borrow_mut().push(summary.into());
        Ok(FileChange {
            path: path.into(),
            action: if existed { "modify" } else { "create" }.into(),
            summary: summary.into(),
        })
    }

    fn read(&self, path: &str) -> Result<Option<String>, WriteError> {
        Ok(self.get(path))
    }
}

impl FeatureCatalog for SharedTree {
    fn list(&self) -> Result<Vec<FeatureSummary>, FeatureError> {
        self.paths()
            .into_iter()
            .filter(|path| path.ends_with(".feature"))
            .map(|path| FeatureCatalog::read(self, &path).map(|doc| doc.summary()))
            .collect()
    }

    fn read(&self, path: &str) -> Result<FeatureDoc, FeatureError> {
        let content = self.get(path).ok_or_else(|| {
            FeatureError(format!(
                "{path}: no such feature file. Call feature list to see valid paths."
            ))
        })?;
        feature::parse(path, &content).map_err(FeatureError)
    }

    fn exists(&self, path: &str) -> bool {
        self.files.borrow().contains_key(path)
    }
}

/// [`SpecRepository`] over a [`SharedTree`], resolving includes the way
/// the filesystem one does. Paired with the same tree as a [`WorkTree`]
/// it gives a mutation test the real contract: what the service writes
/// is what it reads back, includes and all.
///
/// `root` is the project path of the root document (e.g.
/// `requirements/requirements.json`); catalog-relative paths resolve
/// against its directory.
pub struct SharedSpecRepository {
    tree: SharedTree,
    root: String,
    /// A load failure to answer with instead of reading, for
    /// error-propagation tests.
    broken: Option<SpecError>,
}

impl SharedSpecRepository {
    pub fn new(tree: SharedTree, root: &str) -> Self {
        Self {
            tree,
            root: root.to_string(),
            broken: None,
        }
    }

    pub fn failing(error: SpecError) -> Self {
        Self {
            tree: SharedTree::default(),
            root: String::new(),
            broken: Some(error),
        }
    }

    /// The root document's name inside the catalog, mirroring
    /// `FsSpecRepository::root_label`.
    fn root_label(&self) -> String {
        self.root
            .rsplit('/')
            .next()
            .unwrap_or(&self.root)
            .to_string()
    }

    /// The project path of a catalog-relative path.
    fn project_path(&self, catalog_path: &str) -> String {
        if catalog_path == self.root_label() {
            return self.root.clone();
        }
        match self.root.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/{catalog_path}"),
            None => catalog_path.to_string(),
        }
    }
}

impl SpecRepository for SharedSpecRepository {
    fn load(&self) -> Result<Spec, SpecError> {
        self.load_catalog().map(|catalog| catalog.merged())
    }

    fn load_catalog(&self) -> Result<SpecCatalog, SpecError> {
        if let Some(error) = &self.broken {
            return Err(error.clone());
        }
        resolve_catalog(&self.root_label(), &mut |path| {
            self.tree
                .get(&self.project_path(path))
                .map(|content| (content, path.to_string()))
                .ok_or_else(|| format!("spec: {path} is not readable - no such file"))
        })
        .map_err(SpecError)
    }

    fn read_raw(&self, path: &str) -> Result<String, SpecError> {
        if let Some(error) = &self.broken {
            return Err(error.clone());
        }
        self.tree
            .get(&self.project_path(path))
            .ok_or_else(|| SpecError(format!("spec: {path} is not readable - no such file")))
    }
}

/// [`FeatureFiles`] over two sets.
#[derive(Default)]
pub struct FakeFeatureFiles {
    pub existing: HashSet<String>,
    pub tags: HashMap<String, HashSet<String>>,
}

impl FeatureFiles for FakeFeatureFiles {
    fn exists(&self, path: &str) -> bool {
        self.existing.contains(path)
    }

    fn has_tag(&self, path: &str, tag: &str) -> bool {
        self.tags.get(path).is_some_and(|tags| tags.contains(tag))
    }
}

/// [`SpecRepository`] returning a fixed result.
pub struct InMemorySpecRepository(pub Result<Spec, SpecError>);

impl SpecRepository for InMemorySpecRepository {
    fn load(&self) -> Result<Spec, SpecError> {
        self.0.clone()
    }
}

/// [`StateStore`] returning a fixed snapshot and recording saves.
pub struct FixedStateStore {
    pub snapshot: Result<TddSnapshot, StateError>,
    pub saved: RefCell<Vec<TddSnapshot>>,
}

impl FixedStateStore {
    pub fn holding(snapshot: TddSnapshot) -> Self {
        Self {
            snapshot: Ok(snapshot),
            saved: RefCell::new(Vec::new()),
        }
    }

    pub fn failing(message: &str) -> Self {
        Self {
            snapshot: Err(StateError(message.to_string())),
            saved: RefCell::new(Vec::new()),
        }
    }
}

impl StateStore for FixedStateStore {
    fn load(&self) -> Result<TddSnapshot, StateError> {
        self.snapshot.clone()
    }

    fn save(&self, snapshot: &TddSnapshot) -> Result<(), StateError> {
        self.saved.borrow_mut().push(snapshot.clone());
        Ok(())
    }
}

/// [`RuntimeProbe`] answering from a set of installed commands.
#[derive(Default)]
pub struct FakeRuntimeProbe {
    pub available: HashSet<String>,
}

impl FakeRuntimeProbe {
    pub fn with(commands: &[&str]) -> Self {
        Self {
            available: commands.iter().map(|c| c.to_string()).collect(),
        }
    }
}

impl RuntimeProbe for FakeRuntimeProbe {
    fn version(&self, command: &str) -> Option<String> {
        self.available
            .contains(command)
            .then(|| format!("{command} 1.0.0"))
    }
}

/// [`FeatureCatalog`] over raw Gherkin sources.
#[derive(Default)]
pub struct InMemoryFeatureCatalog {
    pub files: HashMap<String, String>,
}

impl FeatureCatalog for InMemoryFeatureCatalog {
    fn list(&self) -> Result<Vec<FeatureSummary>, FeatureError> {
        let mut paths: Vec<&String> = self.files.keys().collect();
        paths.sort();
        paths
            .into_iter()
            .map(|path| self.read(path).map(|doc| doc.summary()))
            .collect()
    }

    fn read(&self, path: &str) -> Result<FeatureDoc, FeatureError> {
        let content = self.files.get(path).ok_or_else(|| {
            FeatureError(format!(
                "{path}: no such feature file. Call feature list to see valid paths."
            ))
        })?;
        feature::parse(path, content).map_err(FeatureError)
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
}

impl FeatureFiles for InMemoryFeatureCatalog {
    fn exists(&self, path: &str) -> bool {
        FeatureCatalog::exists(self, path)
    }

    fn has_tag(&self, path: &str, tag: &str) -> bool {
        self.read(path)
            .map(|doc| doc.all_tags().iter().any(|t| t == tag))
            .unwrap_or(false)
    }
}

/// [`SourceFiles`] answering with a fixed list.
pub struct FakeSources(pub Vec<SourceFile>);

impl SourceFiles for FakeSources {
    fn sources(&self, _extension: &str) -> Result<Vec<SourceFile>, SourceError> {
        Ok(self.0.clone())
    }
}

/// [`SourceFiles`] failing every scan, for error-propagation tests.
pub struct FailingSources;

impl SourceFiles for FailingSources {
    fn sources(&self, _extension: &str) -> Result<Vec<SourceFile>, SourceError> {
        Err(SourceError("disk on fire".into()))
    }
}

/// Scripted [`LlmConversation`]: records each call's system and user
/// prompt joined with a newline, replies with a fixed text turn.
pub struct FakeLlm {
    pub response: Result<String, LlmError>,
    pub prompts: RefCell<Vec<String>>,
}

impl FakeLlm {
    pub fn replying(response: &str) -> ResolvedLlm<FakeLlm> {
        ResolvedLlm::new(
            "fake-model",
            FakeLlm {
                response: Ok(response.to_string()),
                prompts: RefCell::new(Vec::new()),
            },
        )
    }

    pub fn failing() -> ResolvedLlm<FakeLlm> {
        ResolvedLlm::new(
            "fake-model",
            FakeLlm {
                response: Err(LlmError("model crashed".into())),
                prompts: RefCell::new(Vec::new()),
            },
        )
    }
}

impl LlmConversation for FakeLlm {
    fn chat(
        &self,
        _model: &str,
        messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ChatTurn, LlmError> {
        let (system, user) = crate::domain::tools::system_and_user(messages);
        self.prompts.borrow_mut().push(format!("{system}\n{user}"));
        self.response.clone().map(text_turn)
    }
}

/// The calculator fixture shared by the generation, implement, and
/// status service tests: one tagged feature and a three-requirement spec.
pub const CALCULATOR_FEATURE: &str = "@REQ-001\nFeature: Calc\n\n  Scenario: Adds\n    Given a calculator\n    When add is called with \"1,2\"\n    Then the result is 3\n";

/// The layout a flat single-module project resolves to, which is what
/// the in-memory source fixtures here represent. Built through the real
/// resolver so these tests move with it.
pub fn flat_layout(language: Language) -> ProjectStructure {
    resolve_layout(&LayoutInput {
        language,
        build_tool: None,
        tree: &[],
        spec_features: &[],
    })
    .structure
}

pub fn calculator_catalog() -> InMemoryFeatureCatalog {
    let mut catalog = InMemoryFeatureCatalog::default();
    catalog
        .files
        .insert("features/calc.feature".into(), CALCULATOR_FEATURE.into());
    catalog
}

pub fn calculator_spec() -> Spec {
    Spec {
        project: "Kata".into(),
        requirements: vec![
            Requirement {
                id: "REQ-001".into(),
                title: "Adds two numbers".into(),
                status: "pending".into(),
                story: "As a user, I want sums so that I can add.".into(),
                acceptance_criteria: vec![
                    "Given \"1,2\", when add is called, then the result is 3".into(),
                ],
                feature_file: Some("features/calc.feature".into()),
            },
            // No scenario carries @REQ-002: the readiness preflight
            // reports the missing tag.
            Requirement {
                id: "REQ-002".into(),
                title: "Subtracts two numbers".into(),
                status: "pending".into(),
                story: "As a user, I want differences so that I can subtract.".into(),
                acceptance_criteria: vec![
                    "Given \"3,1\", when subtract is called, then the result is 2".into(),
                ],
                feature_file: None,
            },
            Requirement {
                id: "REQ-003".into(),
                title: "Already done".into(),
                status: "implemented".into(),
                story: "As a user, I want the done thing so that it stays done.".into(),
                acceptance_criteria: vec!["Given done, when checked, then it is done".into()],
                feature_file: None,
            },
        ],
        ..Spec::default()
    }
}

/// Java sources covering every step of [`CALCULATOR_FEATURE`].
pub fn covered_steps_source() -> SourceFile {
    SourceFile {
        path: "src/test/java/steps/Steps.java".into(),
        content: "@Given(\"a calculator\")\nvoid a() {}\n\
                  @When(\"add is called with {string}\")\nvoid b(String s) {}\n\
                  @Then(\"the result is {int}\")\nvoid c(int n) {}"
            .into(),
    }
}

/// The unit test asset the readiness preflight looks for on REQ-001.
pub fn unit_test_source() -> SourceFile {
    SourceFile {
        path: "src/test/java/Req001Test.java".into(),
        content: "class Req001Test {}".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tdd::TddPhase;

    #[test]
    fn the_work_tree_fake_honors_the_port_contract() {
        let store = InMemoryWorkTree::default();
        assert_eq!(
            store.write("b.txt", "two", "second").unwrap().action,
            "create"
        );
        store.write("a.txt", "one", "first").unwrap();
        assert_eq!(store.summaries(), ["second", "first"]);
        assert_eq!(store.read("a.txt").unwrap().as_deref(), Some("one"));
        assert_eq!(store.read("missing").unwrap(), None);
        assert_eq!(store.paths(), ["a.txt", "b.txt"]);
        // A second write to one path replaces it and reads as a modify.
        let again = store.write("a.txt", "ONE", "again").unwrap();
        assert_eq!(again.action, "modify");
        assert_eq!(store.read("a.txt").unwrap().as_deref(), Some("ONE"));
    }

    #[test]
    fn the_failing_work_tree_fails_reads_and_writes() {
        let store = InMemoryWorkTree::failing("boom");
        assert_eq!(
            store.write("a.txt", "x", "s").unwrap_err(),
            WriteError("boom".into())
        );
        assert_eq!(store.read("a.txt").unwrap_err(), WriteError("boom".into()));
    }

    #[test]
    fn the_feature_files_fake_answers_from_its_sets() {
        let mut fake = FakeFeatureFiles::default();
        fake.existing.insert("features/x.feature".into());
        fake.tags
            .entry("features/x.feature".into())
            .or_default()
            .insert("@REQ-001".into());
        assert!(fake.exists("features/x.feature"));
        assert!(!fake.exists("features/y.feature"));
        assert!(fake.has_tag("features/x.feature", "@REQ-001"));
        assert!(!fake.has_tag("features/x.feature", "@REQ-002"));
        assert!(!fake.has_tag("features/y.feature", "@REQ-001"));
    }

    #[test]
    fn the_spec_repository_fake_returns_its_result() {
        let ok = InMemorySpecRepository(Ok(Spec::default()));
        assert!(ok.load().is_ok());
        let err = InMemorySpecRepository(Err(SpecError("spec: boom".into())));
        assert_eq!(err.load().unwrap_err(), SpecError("spec: boom".into()));
    }

    #[test]
    fn the_state_store_fake_loads_and_records_saves() {
        let store = FixedStateStore::holding(TddSnapshot::at(TddPhase::Green));
        assert_eq!(store.load().unwrap().phase(), TddPhase::Green);
        store.save(&TddSnapshot::default()).unwrap();
        assert_eq!(store.saved.borrow().len(), 1);
        let failing = FixedStateStore::failing("boom");
        assert_eq!(failing.load().unwrap_err(), StateError("boom".into()));
    }

    #[test]
    fn the_runtime_probe_fake_answers_from_its_set() {
        let probe = FakeRuntimeProbe::with(&["cargo"]);
        assert_eq!(probe.version("cargo").as_deref(), Some("cargo 1.0.0"));
        assert_eq!(probe.version("mvn"), None);
    }

    #[test]
    fn the_feature_catalog_fake_lists_reads_and_answers_existence() {
        let mut catalog = InMemoryFeatureCatalog::default();
        catalog.files.insert(
            "features/x.feature".into(),
            "Feature: X\n\n  Scenario: S\n    Given a\n".into(),
        );
        assert!(FeatureCatalog::exists(&catalog, "features/x.feature"));
        assert!(!FeatureCatalog::exists(&catalog, "features/y.feature"));
        let summaries = catalog.list().unwrap();
        assert_eq!(summaries[0].name, "X");
        assert_eq!(
            catalog.read("features/x.feature").unwrap().scenarios.len(),
            1
        );
        catalog
            .files
            .insert("features/bad.feature".into(), "nope".into());
        assert!(catalog.list().unwrap_err().0.contains("not valid Gherkin"));
    }
}

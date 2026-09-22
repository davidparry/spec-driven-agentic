//! Project memory: refresh the recorded language, libraries, and layout
//! from a scan, and wrap an LLM so every system prompt carries the brief.

use std::cell::RefCell;

use crate::application::LlmReplyError;
use crate::application::agent_service::{Agent, AgentConfig, DEFAULT_MAX_ROUNDS, NullBroker};
use crate::domain::language::{Language, detect_languages};
use crate::domain::layout::{LayoutInput, layout_prompt, parse_layout_checked, resolve_in_module};
use crate::domain::memory::{
    Manifests, MemoryScan, ProjectMemory, ScanInput, apply_chosen, prepend_brief, scan_memory,
};
use crate::domain::model::Spec;
use crate::ports::{
    LlmConversation, LlmError, MemoryError, MemoryStore, ProjectFiles, ProjectInventory, Prompter,
};

/// Where the spec lives by convention, mirroring
/// [`crate::workspace::SPEC_PATH`] without depending on it.
const SPEC_PATH: &str = "requirements/requirements.json";

/// A model available to answer the one layout question discovery cannot,
/// and how many validated attempts it gets. `None` at the call site means
/// the resolver's choice stands.
pub struct LayoutAsk<'a> {
    pub model: &'a str,
    pub llm: &'a dyn LlmConversation,
    pub attempts: u32,
}

/// The developer has the last word on which module the harness works in:
/// it decides where every generated file lands. No human to ask is a no.
fn confirmed(prompter: &mut dyn Prompter, picked: &str, build_tool: Option<&str>) -> bool {
    let build = build_tool.unwrap_or("the build");
    prompter
        .confirm(&format!(
            "Work in {picked} - the module {build} compiles and runs tests in?"
        ))
        .unwrap_or(false)
}

/// Decorates any [`LlmConversation`] by prepending the project-memory brief
/// to the first system message. An empty brief is a no-op.
pub struct MemoryAwareConversation<C> {
    inner: C,
    brief: String,
}

impl<C> MemoryAwareConversation<C> {
    pub fn new(inner: C, brief: impl Into<String>) -> Self {
        Self {
            inner,
            brief: brief.into(),
        }
    }
}

impl<C: LlmConversation> LlmConversation for MemoryAwareConversation<C> {
    fn chat(
        &self,
        model: &str,
        messages: &[crate::domain::tools::ChatMessage],
        tools: &[crate::domain::tools::ToolDefinition],
    ) -> Result<crate::domain::tools::ChatTurn, LlmError> {
        use crate::domain::tools::{ChatMessage, ChatRole};
        let mut rewritten: Vec<ChatMessage> = messages.to_vec();
        if let Some(system) = rewritten.iter_mut().find(|m| m.role == ChatRole::System) {
            system.content = prepend_brief(&self.brief, &system.content);
        }
        tracing::debug!(
            has_memory = !self.brief.trim().is_empty(),
            "LLM chat system prompt project memory"
        );
        self.inner.chat(model, &rewritten, tools)
    }
}

pub struct MemoryService<S, I, P>
where
    S: MemoryStore,
    I: ProjectInventory,
    P: ProjectFiles,
{
    store: S,
    inventory: I,
    files: P,
}

impl<S, I, P> MemoryService<S, I, P>
where
    S: MemoryStore,
    I: ProjectInventory,
    P: ProjectFiles,
{
    pub fn new(store: S, inventory: I, files: P) -> Self {
        Self {
            store,
            inventory,
            files,
        }
    }

    /// Scan the project, preserve a chosen (or previously stored) language,
    /// and write `.spec/memory.json` when there is something to record.
    pub fn refresh(&self, chosen: Option<Language>) -> Result<ProjectMemory, MemoryError> {
        Ok(self.scan(chosen)?.memory)
    }

    /// [`Self::refresh`], settling the module root when discovery found
    /// several buildable modules and could not choose: the model picks
    /// from the candidates, the developer confirms, and the answer is
    /// recorded so the question is asked once per project.
    ///
    /// Every other layout is decided by the resolver, so this is the
    /// only branch a model sees. Without a model, without a human to
    /// confirm, or on a refusal, the resolver's provisional choice
    /// stands - exactly what [`Self::refresh`] would have returned.
    pub fn settle(
        &self,
        chosen: Option<Language>,
        ask: Option<LayoutAsk<'_>>,
        prompter: &mut dyn Prompter,
    ) -> Result<ProjectMemory, MemoryError> {
        // The tree walk is the slow part of shell startup, and it happens
        // before the prompt. The ellipsis runs for the whole read and
        // settles before anything is asked.
        let work = prompter.working("Reading the project - working");
        // Read before the scan writes: a module root already recorded is
        // the answer to this same question from an earlier session.
        let remembered = self
            .store
            .load()?
            .and_then(|memory| memory.structure.module_root);
        let scan = self.scan(chosen)?;
        let Some(language) = Language::parse(&scan.memory.language) else {
            drop(work);
            return Ok(scan.memory);
        };
        if scan.module_candidates.is_empty() {
            drop(work);
            return Ok(scan.memory);
        }
        let tree = self.inventory.list_tree();
        let spec_features = self.spec_features(&tree);
        let build_tool = scan.memory.build_tool.clone();
        drop(work);
        let input = LayoutInput {
            language,
            build_tool: build_tool.as_deref(),
            tree: &tree,
            spec_features: &spec_features,
        };
        let candidates = &scan.module_candidates;
        // Asked once per project: a recorded answer that is still one of
        // the candidates settles it without a model or a prompt.
        if let Some(module) = remembered.filter(|module| candidates.contains(module)) {
            if scan.memory.structure.module_root.as_deref() == Some(module.as_str()) {
                return Ok(scan.memory);
            }
            return self.record_module_root(scan.memory, &input, &module);
        }
        prompter.tell(&format!(
            "Several modules could be the one to work in: {}.",
            candidates.join(", ")
        ));
        let Some(ask) = ask else {
            self.provisional(prompter, &scan, "no model is configured");
            return Ok(scan.memory);
        };
        let picked = match self.propose_module_root(&input, candidates, &ask, prompter) {
            Ok(picked) => picked,
            Err(reason) => {
                self.provisional(prompter, &scan, &reason);
                return Ok(scan.memory);
            }
        };
        if !confirmed(prompter, &picked, build_tool.as_deref()) {
            self.provisional(prompter, &scan, "the choice was declined");
            return Ok(scan.memory);
        }
        let memory = self.record_module_root(scan.memory, &input, &picked)?;
        prompter.tell(&format!(
            "Recorded {picked} as this project's module - asked once; \
             spec inspect re-scans if that changes."
        ));
        Ok(memory)
    }

    /// The model's pick from the candidates, validated and retried
    /// through the same machinery every other model reply uses. `Err`
    /// carries why there is no answer, for the caller to report with
    /// the module it falls back to.
    fn propose_module_root(
        &self,
        input: &LayoutInput<'_>,
        candidates: &[String],
        ask: &LayoutAsk<'_>,
        prompter: &mut dyn Prompter,
    ) -> Result<String, String> {
        let prompt = layout_prompt(input, candidates);
        let notices = RefCell::new(Vec::new());
        let work = prompter.working(&format!("Asking {} which module - working", ask.model));
        let outcome = Agent::new(
            ask.model,
            ask.llm,
            NullBroker,
            Vec::new(),
            AgentConfig::new("layout", ask.attempts, DEFAULT_MAX_ROUNDS, Vec::new()),
        )
        .ask(
            prompter,
            &prompt,
            |reply| parse_layout_checked(reply, candidates),
            |attempt, of, reason| {
                notices.borrow_mut().push(format!(
                    "The model reply was invalid ({reason}) - asking again ({attempt} of {of})"
                ));
            },
        );
        drop(work);
        for notice in notices.into_inner() {
            prompter.warn(&notice);
        }
        outcome.map_err(|error| match error {
            LlmReplyError::Call(error) => format!("the model call failed - {}", error.0),
            LlmReplyError::Invalid { reason } => {
                format!("the model did not choose a module - {reason}")
            }
        })
    }

    /// Re-resolve the layout around the settled module and store it, so
    /// the paths generation writes to follow the module the developer
    /// named rather than the provisional one.
    fn record_module_root(
        &self,
        memory: ProjectMemory,
        input: &LayoutInput<'_>,
        module_root: &str,
    ) -> Result<ProjectMemory, MemoryError> {
        let mut settled = resolve_in_module(input, module_root);
        settled.structure.outline = memory.structure.outline;
        let memory = ProjectMemory {
            structure: settled.structure,
            ..memory
        };
        self.store.save(&memory)?;
        Ok(memory)
    }

    /// Say which module the run continues in when the question went
    /// unanswered, so an unattended run is never silently in the wrong
    /// one.
    fn provisional(&self, prompter: &mut dyn Prompter, scan: &MemoryScan, why: &str) {
        let provisional = scan
            .memory
            .structure
            .module_root
            .as_deref()
            .unwrap_or("the project root");
        prompter.warn(&format!(
            "Working in {provisional} for now ({why}). Run spec inspect to decide again."
        ));
    }

    /// [`Self::refresh`] keeping the module roots discovery could not
    /// choose between, for the caller that can ask about them.
    pub fn scan(&self, chosen: Option<Language>) -> Result<MemoryScan, MemoryError> {
        let existing = self.store.load()?;
        let detected = detect_languages(&self.files);
        let chosen = chosen.or_else(|| {
            existing
                .as_ref()
                .and_then(|memory| Language::parse(&memory.language))
        });
        let manifests = self.manifests();
        let tree = self.inventory.list_tree();
        let spec_features = self.spec_features(&tree);
        let now = now_rfc3339();
        let mut scan = scan_memory(&ScanInput {
            languages: &detected,
            chosen,
            manifests: &manifests,
            tree: &tree,
            spec_features: &spec_features,
            now: &now,
        });
        scan.memory = apply_chosen(scan.memory, chosen);
        if scan.memory.is_empty() {
            return Ok(scan);
        }
        self.store.save(&scan.memory)?;
        Ok(scan)
    }

    /// The feature files the spec's requirements name. Read here rather
    /// than in the resolver so resolution stays pure; a spec that will
    /// not parse simply contributes no references.
    fn spec_features(&self, tree: &[String]) -> Vec<String> {
        let Some(path) = tree
            .iter()
            .find(|path| *path == SPEC_PATH)
            .or_else(|| tree.iter().find(|path| path.ends_with("requirements.json")))
        else {
            return Vec::new();
        };
        let Some(text) = self.inventory.read(path) else {
            return Vec::new();
        };
        let Ok(spec) = serde_json::from_str::<Spec>(&text) else {
            return Vec::new();
        };
        let mut features: Vec<String> = spec
            .requirements
            .into_iter()
            .filter_map(|requirement| requirement.feature_file)
            .collect();
        features.sort();
        features.dedup();
        features
    }

    pub fn load(&self) -> Result<ProjectMemory, MemoryError> {
        Ok(self.store.load()?.unwrap_or_default())
    }

    fn manifests(&self) -> Manifests {
        let mut csproj = Vec::new();
        for path in self.inventory.list_tree() {
            if path.ends_with(".csproj")
                && let Some(text) = self.inventory.read(&path)
            {
                csproj.push(text);
            }
        }
        Manifests {
            pom_xml: self.inventory.read("pom.xml"),
            build_gradle: self.inventory.read("build.gradle"),
            build_gradle_kts: self.inventory.read("build.gradle.kts"),
            package_json: self.inventory.read("package.json"),
            cargo_toml: self.inventory.read("Cargo.toml"),
            csproj,
        }
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ChatTurn;
    use crate::ports::ProjectFiles;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeFiles {
        names: Vec<&'static str>,
    }

    impl ProjectFiles for FakeFiles {
        fn exists(&self, name: &str) -> bool {
            self.names.contains(&name)
        }
        fn any_with_extension(&self, _extension: &str) -> bool {
            false
        }
    }

    #[derive(Default)]
    struct FakeInventory {
        files: HashMap<String, String>,
        tree: Vec<String>,
    }

    impl ProjectInventory for FakeInventory {
        fn exists(&self, path: &str) -> bool {
            self.files.contains_key(path) || self.tree.iter().any(|p| p == path)
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn list_tree(&self) -> Vec<String> {
            self.tree.clone()
        }
    }

    #[derive(Default)]
    struct FakeStore {
        saved: RefCell<Option<ProjectMemory>>,
        load_error: Option<String>,
        save_error: Option<String>,
    }

    impl MemoryStore for FakeStore {
        fn load(&self) -> Result<Option<ProjectMemory>, MemoryError> {
            if let Some(message) = &self.load_error {
                return Err(MemoryError(message.clone()));
            }
            Ok(self.saved.borrow().clone())
        }
        fn save(&self, memory: &ProjectMemory) -> Result<(), MemoryError> {
            if let Some(message) = &self.save_error {
                return Err(MemoryError(message.clone()));
            }
            *self.saved.borrow_mut() = Some(memory.clone());
            Ok(())
        }
    }

    #[test]
    fn refresh_records_java_from_a_pom_and_preserves_a_later_choice() {
        let inventory = FakeInventory {
            files: [(
                "pom.xml".into(),
                "<dependency><artifactId>cucumber-java</artifactId>\
                 <version>7.20.1</version></dependency>"
                    .into(),
            )]
            .into_iter()
            .collect(),
            tree: vec![
                "pom.xml".into(),
                "src/main/java/".into(),
                "features/".into(),
            ],
        };
        let store = FakeStore::default();
        let service = MemoryService::new(
            store,
            inventory,
            FakeFiles {
                names: vec!["pom.xml"],
            },
        );
        let first = service.refresh(None).unwrap();
        assert_eq!(first.language, "Java");
        assert_eq!(first.libraries[0].name, "cucumber-java");

        let rust = ProjectMemory {
            language: "Rust".into(),
            bdd_framework: "cucumber-rs".into(),
            ..first
        };
        let store = FakeStore {
            saved: RefCell::new(Some(rust)),
            ..Default::default()
        };
        let inventory = FakeInventory {
            files: [
                ("pom.xml".into(), "<project/>".into()),
                ("package.json".into(), "{}".into()),
            ]
            .into_iter()
            .collect(),
            tree: vec!["pom.xml".into(), "package.json".into()],
        };
        let files = FakeFiles {
            names: vec!["pom.xml", "package.json"],
        };
        let service = MemoryService::new(store, inventory, files);
        let again = service.refresh(None).unwrap();
        assert_eq!(again.language, "Rust");
        assert_eq!(again.bdd_framework, "cucumber-rs");
    }

    #[test]
    fn refresh_does_not_write_when_nothing_is_detected() {
        let store = FakeStore::default();
        let service = MemoryService::new(store, FakeInventory::default(), FakeFiles::default());
        let memory = service.refresh(None).unwrap();
        assert!(memory.is_empty());
        assert!(service.load().unwrap().is_empty());
    }

    #[test]
    fn store_errors_surface() {
        let store = FakeStore {
            load_error: Some("boom".into()),
            ..Default::default()
        };
        let service = MemoryService::new(store, FakeInventory::default(), FakeFiles::default());
        assert_eq!(
            service.refresh(None).unwrap_err(),
            MemoryError("boom".into())
        );
    }

    #[test]
    fn conversation_wrapper_prepends_the_brief_to_the_system_message() {
        use crate::domain::tools::{ChatMessage, ChatTurn, ToolDefinition};
        let calls = RefCell::new(Vec::new());
        struct Shared<'a>(&'a RefCell<Vec<String>>);
        impl LlmConversation for Shared<'_> {
            fn chat(
                &self,
                _model: &str,
                messages: &[ChatMessage],
                _tools: &[ToolDefinition],
            ) -> Result<ChatTurn, LlmError> {
                self.0.borrow_mut().push(messages[0].content.clone());
                Ok(ChatTurn {
                    content: "ok".into(),
                    tool_calls: Vec::new(),
                })
            }
        }
        let wrapped =
            MemoryAwareConversation::new(Shared(&calls), "Project memory:\n- Language: Java");
        wrapped
            .chat(
                "m",
                &[
                    ChatMessage::system("You implement"),
                    ChatMessage::user("do it"),
                ],
                &[],
            )
            .unwrap();
        assert!(calls.borrow()[0].starts_with("Project memory:"));
    }

    /// The layout that shipped the step-generation bug: an aggregator
    /// root plus two buildable modules that both hold Gherkin, and no
    /// spec reference to separate them.
    fn two_module_inventory() -> FakeInventory {
        FakeInventory {
            files: [("pom.xml".into(), "<project/>".into())]
                .into_iter()
                .collect(),
            tree: [
                "pom.xml",
                "kata/pom.xml",
                "kata/src/main/java/com/example/kata/Calculator.java",
                "kata/src/test/java/com/example/kata/CalculatorSteps.java",
                "kata/src/test/resources/features/calculator.feature",
                "smoke-test/pom.xml",
                "smoke-test/src/main/java/com/example/smoke/Sweep.java",
                "smoke-test/src/test/java/com/example/smoke/SweepSteps.java",
                "smoke-test/src/test/resources/features/sweep.feature",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }

    struct ScriptedPrompter {
        confirm: bool,
        said: RefCell<Vec<String>>,
        started: RefCell<Vec<String>>,
    }

    impl ScriptedPrompter {
        fn new(confirm: bool) -> Self {
            Self {
                confirm,
                said: RefCell::new(Vec::new()),
                started: RefCell::new(Vec::new()),
            }
        }

        fn transcript(&self) -> String {
            self.said.borrow().join("\n")
        }
    }

    impl Prompter for ScriptedPrompter {
        fn tell(&mut self, message: &str) {
            self.said.borrow_mut().push(message.to_string());
        }
        fn working(&mut self, message: &str) -> Box<dyn crate::ports::Working> {
            self.started.borrow_mut().push(message.to_string());
            Box::new(crate::ports::ToldOnce)
        }
        fn ask(&mut self, _question: &str) -> Result<String, crate::ports::PromptError> {
            Err(crate::ports::PromptError("nobody to ask".into()))
        }
        fn confirm(&mut self, question: &str) -> Result<bool, crate::ports::PromptError> {
            self.said.borrow_mut().push(question.to_string());
            Ok(self.confirm)
        }
    }

    /// Answers the layout question with whatever the test scripted.
    struct SaysModule(&'static str);

    impl LlmConversation for SaysModule {
        fn chat(
            &self,
            _model: &str,
            _messages: &[crate::domain::tools::ChatMessage],
            _tools: &[crate::domain::tools::ToolDefinition],
        ) -> Result<ChatTurn, LlmError> {
            Ok(ChatTurn {
                content: self.0.to_string(),
                tool_calls: Vec::new(),
            })
        }
    }

    fn two_module_service(store: FakeStore) -> MemoryService<FakeStore, FakeInventory, FakeFiles> {
        MemoryService::new(
            store,
            two_module_inventory(),
            FakeFiles {
                names: vec!["pom.xml"],
            },
        )
    }

    fn ask_with(llm: &dyn LlmConversation) -> LayoutAsk<'_> {
        LayoutAsk {
            model: "test-model",
            llm,
            attempts: 2,
        }
    }

    #[test]
    fn an_unambiguous_project_is_never_asked_about_its_layout() {
        let inventory = FakeInventory {
            files: [("pom.xml".into(), "<project/>".into())]
                .into_iter()
                .collect(),
            tree: vec![
                "pom.xml".into(),
                "src/main/java/com/example/Calculator.java".into(),
                "src/test/java/com/example/CalculatorSteps.java".into(),
            ],
        };
        let service = MemoryService::new(
            FakeStore::default(),
            inventory,
            FakeFiles {
                names: vec!["pom.xml"],
            },
        );
        let llm = SaysModule(r#"{"moduleRoot": "kata"}"#);
        let mut prompter = ScriptedPrompter::new(true);
        let memory = service
            .settle(None, Some(ask_with(&llm)), &mut prompter)
            .unwrap();
        assert_eq!(memory.structure.module_root, None);
        assert_eq!(prompter.transcript(), "");
        assert_eq!(
            prompter.started.borrow().as_slice(),
            ["Reading the project - working"]
        );
    }

    #[test]
    fn a_confirmed_choice_moves_the_whole_layout_and_is_recorded() {
        let service = two_module_service(FakeStore::default());
        let llm = SaysModule(r#"{"moduleRoot": "smoke-test"}"#);
        let mut prompter = ScriptedPrompter::new(true);
        let memory = service
            .settle(None, Some(ask_with(&llm)), &mut prompter)
            .unwrap();
        assert_eq!(memory.structure.module_root.as_deref(), Some("smoke-test"));
        // Every path follows the module, which is the point: generated
        // steps have to land where the build compiles them.
        assert_eq!(
            memory.structure.step_definitions.as_deref(),
            Some("smoke-test/src/test/java/com/example/smoke/SweepSteps.java")
        );
        assert_eq!(
            memory.structure.tests.as_deref(),
            Some("smoke-test/src/test/java")
        );
        assert_eq!(
            memory.structure.package.as_deref(),
            Some("com.example.smoke")
        );
        let transcript = prompter.transcript();
        assert!(transcript.contains("kata, smoke-test"), "{transcript}");
        assert!(
            transcript.contains("Work in smoke-test - the module Maven"),
            "{transcript}"
        );
        // Recorded, so the question is asked once per project.
        assert_eq!(
            service.load().unwrap().structure.module_root.as_deref(),
            Some("smoke-test")
        );
    }

    #[test]
    fn a_recorded_answer_settles_a_later_session_without_asking() {
        let first = two_module_service(FakeStore::default());
        let llm = SaysModule(r#"{"moduleRoot": "smoke-test"}"#);
        let mut prompter = ScriptedPrompter::new(true);
        let recorded = first
            .settle(None, Some(ask_with(&llm)), &mut prompter)
            .unwrap();

        let again = two_module_service(FakeStore {
            saved: RefCell::new(Some(recorded)),
            ..Default::default()
        });
        let refuses = SaysModule("I will not answer.");
        let mut prompter = ScriptedPrompter::new(false);
        let memory = again
            .settle(None, Some(ask_with(&refuses)), &mut prompter)
            .unwrap();
        assert_eq!(memory.structure.module_root.as_deref(), Some("smoke-test"));
        assert_eq!(prompter.transcript(), "");
    }

    #[test]
    fn a_declined_choice_leaves_the_provisional_module_and_says_so() {
        let service = two_module_service(FakeStore::default());
        let llm = SaysModule(r#"{"moduleRoot": "smoke-test"}"#);
        let mut prompter = ScriptedPrompter::new(false);
        let memory = service
            .settle(None, Some(ask_with(&llm)), &mut prompter)
            .unwrap();
        assert_eq!(memory.structure.module_root.as_deref(), Some("kata"));
        assert!(
            prompter.transcript().contains("Working in kata for now"),
            "{}",
            prompter.transcript()
        );
    }

    #[test]
    fn without_a_model_the_provisional_module_stands() {
        let service = two_module_service(FakeStore::default());
        let mut prompter = ScriptedPrompter::new(true);
        let memory = service.settle(None, None, &mut prompter).unwrap();
        assert_eq!(memory.structure.module_root.as_deref(), Some("kata"));
        let transcript = prompter.transcript();
        assert!(
            transcript.contains("no model is configured"),
            "{transcript}"
        );
        assert!(transcript.contains("spec inspect"), "{transcript}");
    }

    #[test]
    fn an_invented_module_root_is_refused_and_nothing_is_recorded() {
        let service = two_module_service(FakeStore::default());
        let llm = SaysModule(r#"{"moduleRoot": "kata/src/test/java"}"#);
        let mut prompter = ScriptedPrompter::new(true);
        let memory = service
            .settle(None, Some(ask_with(&llm)), &mut prompter)
            .unwrap();
        assert_eq!(memory.structure.module_root.as_deref(), Some("kata"));
        let transcript = prompter.transcript();
        // The refusal names the module the run continues in, and quotes
        // the candidates back, so the reply is never silently dropped.
        assert!(
            transcript.contains("Working in kata for now"),
            "{transcript}"
        );
        assert!(
            transcript.contains("the model did not choose a module"),
            "{transcript}"
        );
        assert!(
            !transcript.contains("Work in kata/src/test/java"),
            "an unusable answer is never put to the developer: {transcript}"
        );
    }

    #[test]
    fn refresh_records_csproj_packages_when_dotnet_is_chosen() {
        let inventory = FakeInventory {
            files: [(
                "App.csproj".into(),
                r#"<PackageReference Include="Reqnroll" Version="2.2.1" />"#.into(),
            )]
            .into_iter()
            .collect(),
            tree: vec!["App.csproj".into()],
        };
        let store = FakeStore::default();
        let service = MemoryService::new(store, inventory, FakeFiles::default());
        let memory = service.refresh(Some(Language::DotNet)).unwrap();
        assert_eq!(memory.language, ".NET");
        assert!(
            memory.libraries.iter().any(|l| l.name == "Reqnroll"),
            "libraries: {:?}",
            memory.libraries
        );
    }
}

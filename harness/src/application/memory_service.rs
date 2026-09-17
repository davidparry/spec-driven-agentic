//! Project memory: refresh the recorded language, libraries, and layout
//! from a scan, and wrap an LLM so every system prompt carries the brief.

use crate::domain::language::{Language, detect_languages};
use crate::domain::memory::{
    Manifests, ProjectMemory, ScanInput, apply_chosen, prepend_brief, scan_memory,
};
use crate::ports::{
    LlmConversation, LlmError, MemoryError, MemoryStore, ProjectFiles, ProjectInventory,
};

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
    /// and write `.spec-memory.json` when there is something to record.
    pub fn refresh(&self, chosen: Option<Language>) -> Result<ProjectMemory, MemoryError> {
        let existing = self.store.load()?;
        let detected = detect_languages(&self.files);
        let chosen = chosen.or_else(|| {
            existing
                .as_ref()
                .and_then(|memory| Language::parse(&memory.language))
        });
        let manifests = self.manifests();
        let tree = self.inventory.list_tree();
        let now = now_rfc3339();
        let scanned = scan_memory(&ScanInput {
            languages: &detected,
            chosen,
            manifests: &manifests,
            tree: &tree,
            now: &now,
        });
        let memory = apply_chosen(scanned, chosen);
        if memory.is_empty() {
            return Ok(memory);
        }
        self.store.save(&memory)?;
        Ok(memory)
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

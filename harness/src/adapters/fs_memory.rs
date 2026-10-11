//! Filesystem project memory: the current session's slice of
//! `.spec/memory.json` plus a tree inventory of the project root
//! (skipping build output and dependency directories).

use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::fs_session::{SessionFiles, claim, fresh_id, now_rfc3339};
use crate::domain::SPEC_DIR;
use crate::domain::memory::ProjectMemory;
use crate::domain::session::SessionId;
use crate::domain::{MEMORY_FILE, spec_rel};
use crate::ports::{MemoryError, MemoryStore, ProjectInventory};

const SKIPPED_DIRS: [&str; 7] = [
    "target",
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".git",
    SPEC_DIR,
];

pub struct FsMemoryStore {
    root: PathBuf,
    /// The session this store is bound to; `None` follows the pointer.
    session: Option<SessionId>,
}

impl FsMemoryStore {
    /// A store on the current session: whichever `currentSession` names
    /// at the moment of each read or write.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            session: None,
        }
    }

    /// A store bound to one session regardless of the pointer.
    pub fn for_session(root: PathBuf, session: SessionId) -> Self {
        Self {
            root,
            session: Some(session),
        }
    }

    fn session_in(&self, files: &SessionFiles) -> Option<SessionId> {
        self.session.clone().or_else(|| files.current())
    }
}

impl MemoryStore for FsMemoryStore {
    fn load(&self) -> Result<Option<ProjectMemory>, MemoryError> {
        let files = SessionFiles::read(&self.root).map_err(MemoryError)?;
        Ok(self
            .session_in(&files)
            .and_then(|id| files.memory.body_of(&id).cloned()))
    }

    /// Under the project lock, like the state store, and for the same
    /// reason: the file holds every session, so a write from a stale
    /// read would drop the others.
    fn save(&self, memory: &ProjectMemory) -> Result<(), MemoryError> {
        let shown = spec_rel(MEMORY_FILE);
        let _claim = claim(&self.root, &shown).map_err(MemoryError)?;
        let mut files = SessionFiles::read(&self.root).map_err(MemoryError)?;
        let id = self.session_in(&files).unwrap_or_else(fresh_id);
        files.memory.put(&id, memory.clone(), &now_rfc3339());
        files.touch_memory();
        files.write().map_err(MemoryError)
    }
}

pub struct FsProjectInventory {
    root: PathBuf,
}

impl FsProjectInventory {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl ProjectInventory for FsProjectInventory {
    fn exists(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }

    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(self.root.join(path)).ok()
    }

    fn list_tree(&self) -> Vec<String> {
        let mut into = Vec::new();
        collect_tree(&self.root, &self.root, &mut into);
        into.sort();
        into
    }
}

fn collect_tree(dir: &Path, root: &Path, into: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            if SKIPPED_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            into.push(format!("{relative}/"));
            collect_tree(&path, root, into);
        } else {
            into.push(relative);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::fs_state::FsStateStore;
    use crate::adapters::spec_home::spec_file;
    use crate::domain::STATE_FILE;
    use crate::domain::memory::MEMORY_VERSION;
    use crate::domain::tdd::{TddPhase, TddSnapshot};
    use crate::ports::StateStore;

    #[test]
    fn a_missing_file_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsMemoryStore::new(dir.path().to_path_buf());
        assert_eq!(store.load().unwrap(), None);
    }

    /// The two files share one pointer: a session the state store
    /// opened is the one the memory store writes into, and the reverse.
    #[test]
    fn memory_follows_the_session_the_state_file_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        FsStateStore::new(root.clone())
            .save(&TddSnapshot::at(TddPhase::Red))
            .unwrap();
        let state = fs::read_to_string(spec_file(&root, STATE_FILE)).unwrap();
        let store = FsMemoryStore::new(root.clone());
        assert_eq!(store.load().unwrap(), None, "no memory recorded yet");
        store
            .save(&ProjectMemory {
                language: "Rust".into(),
                ..Default::default()
            })
            .unwrap();
        let memory = fs::read_to_string(spec_file(&root, MEMORY_FILE)).unwrap();
        let pointer = |text: &str| {
            text.lines()
                .find(|line| line.contains("\"currentSession\""))
                .map(str::to_string)
        };
        assert_eq!(pointer(&state), pointer(&memory), "{state}\n{memory}");
        assert_eq!(store.load().unwrap().unwrap().language, "Rust");
    }

    #[test]
    fn a_bound_store_keeps_to_its_own_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let current = FsMemoryStore::new(root.clone());
        current
            .save(&ProjectMemory {
                language: "Java".into(),
                ..Default::default()
            })
            .unwrap();
        let bound = FsMemoryStore::for_session(root, fresh_id());
        assert_eq!(bound.load().unwrap(), None);
        bound
            .save(&ProjectMemory {
                language: "Rust".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(bound.load().unwrap().unwrap().language, "Rust");
        assert_eq!(current.load().unwrap().unwrap().language, "Java");
    }

    #[test]
    fn a_file_from_before_sessions_loads_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), MEMORY_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"version":1,"language":"Rust","refreshedAt":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        let store = FsMemoryStore::new(dir.path().to_path_buf());
        assert_eq!(store.load().unwrap().unwrap().language, "Rust");
    }

    #[test]
    fn memory_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsMemoryStore::new(dir.path().to_path_buf());
        let memory = ProjectMemory {
            version: MEMORY_VERSION,
            language: "Java".into(),
            bdd_framework: "Cucumber-JVM".into(),
            build_tool: Some("Maven".into()),
            refreshed_at: "2026-08-21T01:00:00Z".into(),
            ..Default::default()
        };
        store.save(&memory).unwrap();
        assert_eq!(store.load().unwrap(), Some(memory));
        let text = fs::read_to_string(spec_file(dir.path(), MEMORY_FILE)).unwrap();
        assert!(text.contains("\"language\": \"Java\""));
    }

    #[test]
    fn corrupt_memory_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), MEMORY_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        let error = FsMemoryStore::new(dir.path().to_path_buf())
            .load()
            .unwrap_err();
        assert!(
            error.0.starts_with(".spec/memory.json is not valid JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn an_unwritable_location_is_a_structured_error() {
        let store = FsMemoryStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.save(&ProjectMemory::default()).unwrap_err();
        assert!(
            error.0.starts_with(".spec/memory.json is not writable -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn inventory_lists_files_and_dirs_and_skips_noise() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/main/java")).unwrap();
        fs::create_dir_all(root.join("target/classes")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("pom.xml"), "<project/>").unwrap();
        fs::write(root.join("src/main/java/App.java"), "class App {}").unwrap();
        fs::write(root.join("target/classes/App.class"), "x").unwrap();
        fs::write(root.join(".hidden/secret"), "x").unwrap();
        let inventory = FsProjectInventory::new(root.to_path_buf());
        assert!(inventory.exists("pom.xml"));
        assert_eq!(inventory.read("pom.xml").as_deref(), Some("<project/>"));
        let tree = inventory.list_tree();
        assert!(tree.contains(&"pom.xml".into()));
        assert!(tree.contains(&"src/".into()));
        assert!(tree.contains(&"src/main/java/App.java".into()));
        assert!(!tree.iter().any(|p| p.starts_with("target")));
        assert!(!tree.iter().any(|p| p.contains(".hidden")));
    }
}

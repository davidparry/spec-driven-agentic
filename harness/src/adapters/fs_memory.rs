//! Filesystem project memory: `.bdd-memory.json` plus a tree inventory
//! of the project root (skipping build output and dependency directories).

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::memory::ProjectMemory;
use crate::ports::{MemoryError, MemoryStore, ProjectInventory};

const SKIPPED_DIRS: [&str; 7] = [
    "target",
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".git",
    ".bdd-staged",
];

pub struct FsMemoryStore {
    file: PathBuf,
}

impl FsMemoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            file: root.join(".bdd-memory.json"),
        }
    }
}

impl MemoryStore for FsMemoryStore {
    fn load(&self) -> Result<Option<ProjectMemory>, MemoryError> {
        if !self.file.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(&self.file)
            .map_err(|e| MemoryError(format!(".bdd-memory.json is not readable - {e}")))?;
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| MemoryError(format!(".bdd-memory.json is not valid JSON - {e}")))
    }

    fn save(&self, memory: &ProjectMemory) -> Result<(), MemoryError> {
        let text =
            serde_json::to_string_pretty(memory).expect("project memory is always serializable");
        fs::write(&self.file, format!("{text}\n"))
            .map_err(|e| MemoryError(format!(".bdd-memory.json is not writable - {e}")))
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
    use crate::domain::memory::MEMORY_VERSION;

    #[test]
    fn a_missing_file_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsMemoryStore::new(dir.path().to_path_buf());
        assert_eq!(store.load().unwrap(), None);
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
        let text = fs::read_to_string(dir.path().join(".bdd-memory.json")).unwrap();
        assert!(text.contains("\"language\": \"Java\""));
    }

    #[test]
    fn corrupt_memory_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".bdd-memory.json"), "not json").unwrap();
        let error = FsMemoryStore::new(dir.path().to_path_buf())
            .load()
            .unwrap_err();
        assert!(
            error.0.starts_with(".bdd-memory.json is not valid JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn an_unwritable_location_is_a_structured_error() {
        let store = FsMemoryStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.save(&ProjectMemory::default()).unwrap_err();
        assert!(
            error.0.starts_with(".bdd-memory.json is not writable -"),
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

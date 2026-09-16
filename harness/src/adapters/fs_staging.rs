//! Filesystem implementation of the [`ChangeStore`] port. Staged files
//! live under `.bdd-staged/files/` mirroring the project layout, with a
//! `manifest.json` describing each change; `commit` copies them into the
//! working tree and clears the area.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ports::{ChangeStore, StageError, StagedChange};

pub struct FsChangeStore {
    root: PathBuf,
}

impl FsChangeStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn staged_dir(&self) -> PathBuf {
        self.root.join(".bdd-staged")
    }

    fn manifest_file(&self) -> PathBuf {
        self.staged_dir().join("manifest.json")
    }

    fn staged_file(&self, path: &str) -> Result<PathBuf, StageError> {
        let relative =
            crate::domain::paths::confine(path).map_err(|e| StageError(e.to_string()))?;
        Ok(self.staged_dir().join("files").join(relative))
    }

    fn manifest(&self) -> Result<Vec<StagedChange>, StageError> {
        let file = self.manifest_file();
        if !file.is_file() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&file)
            .map_err(|e| StageError(format!("staging manifest is not readable - {e}")))?;
        serde_json::from_str(&text)
            .map_err(|e| StageError(format!("staging manifest is not valid JSON - {e}")))
    }

    fn write_manifest(&self, changes: &[StagedChange]) -> Result<(), StageError> {
        let text = serde_json::to_string_pretty(changes).expect("manifest is always serializable");
        fs::write(self.manifest_file(), text)
            .map_err(|e| StageError(format!("staging manifest is not writable - {e}")))
    }

    fn clear(&self) -> Result<(), StageError> {
        fs::remove_dir_all(self.staged_dir())
            .map_err(|e| StageError(format!("staging area could not be cleared - {e}")))
    }
}

fn ensure_parent(path: &Path) -> Result<(), StageError> {
    let parent = path.parent().expect("staged paths always have a parent");
    fs::create_dir_all(parent).map_err(|e| {
        StageError(format!(
            "{}: directory not creatable - {e}",
            parent.display()
        ))
    })
}

impl ChangeStore for FsChangeStore {
    fn stage(&self, path: &str, content: &str, summary: &str) -> Result<StagedChange, StageError> {
        let path = crate::domain::paths::confine(path).map_err(|e| StageError(e.to_string()))?;
        let mut changes = self.manifest()?;
        let target = self.staged_file(&path)?;
        ensure_parent(&target)?;
        fs::write(&target, content)
            .map_err(|e| StageError(format!("{path}: staged file not writable - {e}")))?;
        let action = if self.root.join(&path).exists() {
            "modify"
        } else {
            "create"
        };
        let change = StagedChange {
            path: path.clone(),
            action: action.to_string(),
            summary: summary.to_string(),
        };
        changes.retain(|c| c.path != path);
        changes.push(change.clone());
        self.write_manifest(&changes)?;
        Ok(change)
    }

    fn changes(&self) -> Result<Vec<StagedChange>, StageError> {
        self.manifest()
    }

    fn content(&self, path: &str) -> Result<Option<String>, StageError> {
        if !self.manifest()?.iter().any(|c| c.path == path) {
            return Ok(None);
        }
        fs::read_to_string(self.staged_file(path)?)
            .map(Some)
            .map_err(|e| StageError(format!("{path}: staged file not readable - {e}")))
    }

    fn commit(&self) -> Result<Vec<StagedChange>, StageError> {
        let changes = self.manifest()?;
        for change in &changes {
            let relative = crate::domain::paths::confine(&change.path)
                .map_err(|e| StageError(format!("{}: {e}", change.path)))?;
            let target = self.root.join(&relative);
            ensure_parent(&target)?;
            fs::copy(self.staged_file(&relative)?, &target).map_err(|e| {
                StageError(format!(
                    "{}: could not apply staged file - {e}",
                    change.path
                ))
            })?;
        }
        if !changes.is_empty() {
            self.clear()?;
        }
        Ok(changes)
    }

    fn discard(&self) -> Result<Vec<StagedChange>, StageError> {
        let changes = self.manifest()?;
        if !changes.is_empty() {
            self.clear()?;
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, FsChangeStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsChangeStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn an_empty_store_has_no_changes_and_no_content() {
        let (_dir, store) = store();
        assert_eq!(store.changes().unwrap(), vec![]);
        assert_eq!(store.content("a.txt").unwrap(), None);
        assert_eq!(store.commit().unwrap(), vec![]);
        assert_eq!(store.discard().unwrap(), vec![]);
    }

    #[test]
    fn staging_a_new_path_records_a_create_and_keeps_the_working_tree_untouched() {
        let (dir, store) = store();
        let change = store
            .stage("features/x.feature", "Feature: X\n", "new feature")
            .unwrap();
        assert_eq!(change.action, "create");
        assert_eq!(change.summary, "new feature");
        assert!(!dir.path().join("features/x.feature").exists());
        assert_eq!(
            store.content("features/x.feature").unwrap().as_deref(),
            Some("Feature: X\n")
        );
    }

    #[test]
    fn staging_an_existing_path_records_a_modify() {
        let (dir, store) = store();
        fs::write(dir.path().join("notes.txt"), "old").unwrap();
        let change = store.stage("notes.txt", "new", "edit").unwrap();
        assert_eq!(change.action, "modify");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "old"
        );
    }

    #[test]
    fn restaging_the_same_path_replaces_the_earlier_entry() {
        let (_dir, store) = store();
        store.stage("a.txt", "one", "first").unwrap();
        store.stage("a.txt", "two", "second").unwrap();
        let changes = store.changes().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].summary, "second");
        assert_eq!(store.content("a.txt").unwrap().as_deref(), Some("two"));
    }

    #[test]
    fn commit_applies_every_change_and_clears_the_area() {
        let (dir, store) = store();
        store
            .stage("features/x.feature", "Feature: X\n", "f")
            .unwrap();
        store
            .stage("requirements/requirements.json", "{}", "spec")
            .unwrap();
        let applied = store.commit().unwrap();
        assert_eq!(applied.len(), 2);
        assert_eq!(
            fs::read_to_string(dir.path().join("features/x.feature")).unwrap(),
            "Feature: X\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("requirements/requirements.json")).unwrap(),
            "{}"
        );
        assert_eq!(store.changes().unwrap(), vec![]);
        assert!(!dir.path().join(".bdd-staged").exists());
    }

    #[test]
    fn discard_drops_everything_without_touching_the_working_tree() {
        let (dir, store) = store();
        store.stage("a.txt", "content", "s").unwrap();
        let dropped = store.discard().unwrap();
        assert_eq!(dropped.len(), 1);
        assert!(!dir.path().join("a.txt").exists());
        assert_eq!(store.changes().unwrap(), vec![]);
    }

    #[test]
    fn a_corrupt_manifest_is_a_structured_error() {
        let (dir, store) = store();
        fs::create_dir_all(dir.path().join(".bdd-staged")).unwrap();
        fs::write(dir.path().join(".bdd-staged/manifest.json"), "not json").unwrap();
        let error = store.changes().unwrap_err();
        assert!(
            error.0.starts_with("staging manifest is not valid JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn an_unwritable_staging_area_is_a_structured_error() {
        let store = FsChangeStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.stage("a.txt", "x", "s").unwrap_err();
        assert!(error.0.contains("not creatable"), "got: {}", error.0);
    }

    #[test]
    fn a_staged_file_missing_from_disk_is_a_structured_error() {
        let (dir, store) = store();
        store.stage("a.txt", "x", "s").unwrap();
        fs::remove_file(dir.path().join(".bdd-staged/files/a.txt")).unwrap();
        let read = store.content("a.txt").unwrap_err();
        assert!(
            read.0.contains("staged file not readable"),
            "got: {}",
            read.0
        );
        let commit = store.commit().unwrap_err();
        assert!(
            commit.0.contains("could not apply staged file"),
            "got: {}",
            commit.0
        );
    }

    #[test]
    fn absolute_and_escaping_paths_are_refused() {
        let (dir, store) = store();
        for path in [
            "/etc/passwd",
            "../outside.txt",
            r"C:\Windows\win.ini",
            "~/x",
        ] {
            let error = store.stage(path, "x", "s").unwrap_err();
            assert!(
                error.0.contains("absolute")
                    || error.0.contains("..")
                    || error.0.contains("home-directory"),
                "{path}: {}",
                error.0
            );
        }
        assert!(store.changes().unwrap().is_empty());
        assert!(!dir.path().join("etc").exists());
    }

    #[test]
    fn a_dotdot_that_stays_inside_the_root_is_normalized() {
        let (dir, store) = store();
        let change = store.stage("features/../notes.txt", "ok", "norm").unwrap();
        assert_eq!(change.path, "notes.txt");
        assert_eq!(store.content("notes.txt").unwrap().as_deref(), Some("ok"));
        store.commit().unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "ok"
        );
    }
}

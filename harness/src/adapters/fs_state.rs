//! Filesystem implementation of the [`StateStore`] port: the TDD state
//! machine persisted as `.spec-state.json` in the project root. The file
//! is a chronological log of timestamped entries plus interpretation
//! instructions, so separate harness invocations share one machine.

use std::fs;
use std::path::PathBuf;

use crate::domain::STATE_FILE;
use crate::domain::tdd::TddSnapshot;
use crate::ports::{StateError, StateStore};

pub struct FsStateStore {
    file: PathBuf,
}

impl FsStateStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            file: root.join(STATE_FILE),
        }
    }
}

impl StateStore for FsStateStore {
    fn load(&self) -> Result<TddSnapshot, StateError> {
        if !self.file.is_file() {
            return Ok(TddSnapshot::default());
        }
        let text = fs::read_to_string(&self.file)
            .map_err(|e| StateError(format!("{STATE_FILE} is not readable - {e}")))?;
        serde_json::from_str(&text)
            .map_err(|e| StateError(format!("{STATE_FILE} is not valid JSON - {e}")))
    }

    fn save(&self, snapshot: &TddSnapshot) -> Result<(), StateError> {
        let text = serde_json::to_string_pretty(snapshot).expect("snapshot is always serializable");
        fs::write(&self.file, text)
            .map_err(|e| StateError(format!("{STATE_FILE} is not writable - {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::TestRunSummary;
    use crate::domain::tdd::TddPhase;

    #[test]
    fn a_missing_file_loads_as_the_start_state() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = FsStateStore::new(dir.path().to_path_buf()).load().unwrap();
        assert_eq!(snapshot.phase(), TddPhase::Start);
        assert!(snapshot.instructions.contains("three most recent entries"));
        assert!(snapshot.entries.is_empty());
    }

    #[test]
    fn a_snapshot_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStateStore::new(dir.path().to_path_buf());
        let snapshot = TddSnapshot::with(crate::domain::tdd::StateEntry {
            timestamp: "2026-08-13T12:00:00Z".into(),
            phase: TddPhase::Green,
            last_run: TestRunSummary {
                tests: 5,
                ..Default::default()
            },
            refactor_log: vec!["extract parser".into()],
            ..Default::default()
        });
        store.save(&snapshot).unwrap();
        assert_eq!(store.load().unwrap(), snapshot);
    }

    #[test]
    fn corrupt_state_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(STATE_FILE), "not json").unwrap();
        let error = FsStateStore::new(dir.path().to_path_buf())
            .load()
            .unwrap_err();
        assert!(
            error.0.starts_with(".spec-state.json is not valid JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn an_unwritable_location_is_a_structured_error() {
        let store = FsStateStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.save(&TddSnapshot::default()).unwrap_err();
        assert!(
            error.0.starts_with(".spec-state.json is not writable -"),
            "got: {}",
            error.0
        );
    }
}

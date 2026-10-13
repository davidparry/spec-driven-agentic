//! Filesystem implementation of the [`StateStore`] port: the TDD state
//! machine persisted in `.spec/state.json`. The file holds one
//! chronological log of timestamped entries per session plus the
//! interpretation instructions; the store reads and writes the log of
//! whichever session `currentSession` names, so separate harness
//! invocations share one machine.

use std::path::PathBuf;

use crate::adapters::clock::SystemClock;
use crate::adapters::session_files::{SessionFiles, claim, fresh_id};
use crate::domain::tdd::TddSnapshot;
use crate::domain::{STATE_FILE, spec_rel};
use crate::ports::{Clock, StateError, StateStore};

pub struct FsStateStore<C: Clock = SystemClock> {
    root: PathBuf,
    clock: C,
}

impl FsStateStore<SystemClock> {
    pub fn new(root: PathBuf) -> Self {
        Self::with_clock(root, SystemClock)
    }
}

impl<C: Clock> FsStateStore<C> {
    pub fn with_clock(root: PathBuf, clock: C) -> Self {
        Self { root, clock }
    }
}

impl<C: Clock> StateStore for FsStateStore<C> {
    fn load(&self) -> Result<TddSnapshot, StateError> {
        let files = SessionFiles::read(&self.root, &self.clock.now()).map_err(StateError)?;
        Ok(match files.current() {
            Some(id) => files.state.snapshot_of(&id),
            None => TddSnapshot::default(),
        })
    }

    /// The whole read-modify-write runs under the project lock: two
    /// `spec` processes saving at once would otherwise each write the
    /// file from its own read and drop the other's session.
    fn save(&self, snapshot: &TddSnapshot) -> Result<(), StateError> {
        let shown = spec_rel(STATE_FILE);
        let _claim = claim(&self.root, &shown).map_err(StateError)?;
        let now = self.clock.now();
        let mut files = SessionFiles::read(&self.root, &now).map_err(StateError)?;
        // No session yet - a one-shot command on a fresh project - so
        // this save opens one, which the memory store will then follow.
        let id = files.current().unwrap_or_else(fresh_id);
        files.state.put(&id, snapshot, &now);
        files.touch_state();
        files.write().map_err(StateError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::spec_home::spec_file;
    use crate::domain::model::TestRunSummary;
    use crate::domain::tdd::TddPhase;
    use std::fs;

    #[test]
    fn a_missing_file_loads_as_the_start_state() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = FsStateStore::new(dir.path().to_path_buf()).load().unwrap();
        assert_eq!(snapshot.phase(), TddPhase::Start);
        assert!(snapshot.instructions.contains("three most recent entries"));
        assert!(snapshot.entries.is_empty());
    }

    /// The first save on a project with no session opens one, and the
    /// file is in the session shape from the start.
    #[test]
    fn the_first_save_opens_a_session_the_next_load_follows() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStateStore::new(dir.path().to_path_buf());
        store.save(&TddSnapshot::at(TddPhase::Red)).unwrap();
        let text = fs::read_to_string(spec_file(dir.path(), STATE_FILE)).unwrap();
        assert!(text.contains("\"currentSession\""), "{text}");
        assert!(text.contains("\"sessions\""), "{text}");
        assert_eq!(store.load().unwrap().phase(), TddPhase::Red);
        // A second save goes to the same session, not a new one.
        store.save(&TddSnapshot::at(TddPhase::Green)).unwrap();
        let again = fs::read_to_string(spec_file(dir.path(), STATE_FILE)).unwrap();
        assert_eq!(again.matches("\"startedAt\"").count(), 1, "{again}");
        assert_eq!(store.load().unwrap().phase(), TddPhase::Green);
    }

    #[test]
    fn a_file_from_before_sessions_loads_as_it_was_and_saves_in_the_new_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), STATE_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = TddSnapshot::at(TddPhase::Green);
        fs::write(&path, serde_json::to_string(&legacy).unwrap()).unwrap();
        let store = FsStateStore::new(dir.path().to_path_buf());
        assert_eq!(store.load().unwrap().entries, legacy.entries);
        store.save(&TddSnapshot::at(TddPhase::Red)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"sessions\""), "{text}");
        assert_eq!(
            text.matches("\"startedAt\"").count(),
            1,
            "one session: {text}"
        );
        assert_eq!(store.load().unwrap().phase(), TddPhase::Red);
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

    /// Every write is dated by the injected clock, so the session's
    /// `lastActiveAt` is the instant the caller chose.
    #[test]
    fn a_save_is_dated_by_the_clock_it_was_given() {
        struct Fixed;
        impl Clock for Fixed {
            fn now(&self) -> String {
                "2031-02-03T04:05:06Z".to_string()
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let store = FsStateStore::with_clock(dir.path().to_path_buf(), Fixed);
        store.save(&TddSnapshot::at(TddPhase::Red)).unwrap();
        let text = fs::read_to_string(spec_file(dir.path(), STATE_FILE)).unwrap();
        assert!(
            text.contains("\"lastActiveAt\": \"2031-02-03T04:05:06Z\""),
            "{text}"
        );
    }

    #[test]
    fn corrupt_state_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), STATE_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        let error = FsStateStore::new(dir.path().to_path_buf())
            .load()
            .unwrap_err();
        assert!(
            error.0.starts_with(".spec/state.json is not valid JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn an_unwritable_location_is_a_structured_error() {
        let store = FsStateStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.save(&TddSnapshot::default()).unwrap_err();
        assert!(
            error.0.starts_with(".spec/state.json is not writable -"),
            "got: {}",
            error.0
        );
    }
}

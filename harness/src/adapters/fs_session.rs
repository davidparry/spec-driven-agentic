//! Sessions on the filesystem: the [`SessionStore`] port over the pair
//! of files in [`crate::adapters::session_files`].
//!
//! Opening is the only write. It settles which session the run belongs
//! to and records that in both files, so every store that follows the
//! `currentSession` pointer afterwards agrees.

use std::path::{Path, PathBuf};

use crate::adapters::clock::SystemClock;
use crate::adapters::session_files::{SessionFiles, both_shown, claim, fresh_id};
use crate::domain::memory::seed_memory;
use crate::domain::session::{SessionId, SessionInfo, SessionOpened, check_requested};
use crate::domain::tdd::SessionEntries;
use crate::ports::{Clock, SessionError, SessionStore};

pub struct FsSessionStore<C: Clock = SystemClock> {
    root: PathBuf,
    clock: C,
}

impl FsSessionStore<SystemClock> {
    pub fn new(root: PathBuf) -> Self {
        Self::with_clock(root, SystemClock)
    }
}

impl<C: Clock> FsSessionStore<C> {
    pub fn with_clock(root: PathBuf, clock: C) -> Self {
        Self { root, clock }
    }

    fn read(&self) -> Result<SessionFiles, SessionError> {
        SessionFiles::read(&self.root, &self.clock.now()).map_err(SessionError)
    }
}

/// The session an unasked-for start belongs to: the one a pre-session
/// file was adopted under, else the current one if nobody recorded
/// anything in it. Both already exist, so neither mints an id - which
/// is what keeps a shell that is only ever opened and closed from
/// leaving a new empty session behind every time.
fn continuable(files: &SessionFiles) -> Option<SessionId> {
    files
        .adopted
        .clone()
        .or_else(|| files.state.reusable_current())
}

impl<C: Clock> SessionStore for FsSessionStore<C> {
    fn open(&self, requested: Option<&SessionId>) -> Result<SessionOpened, SessionError> {
        let _claim = claim(&self.root, &both_shown()).map_err(SessionError)?;
        let now = self.clock.now();
        let mut files = SessionFiles::read(&self.root, &now).map_err(SessionError)?;
        check_requested(requested, &files.known())
            .map_err(|refusal| SessionError(refusal.to_string()))?;
        let continued = continuable(&files);
        let wanted = requested.or(continued.as_ref());
        let fresh = fresh_id();
        let in_state = files.state.sessions.open(wanted, fresh.clone(), &now, |_| {
            Some(SessionEntries::default())
        });
        let in_memory = files.memory.open(wanted, fresh, &now, seed_memory);
        files.touch_state();
        files.touch_memory();
        files.write().map_err(SessionError)?;
        let id = in_state.id;
        let last_active_at = [in_state.last_active_at, in_memory.last_active_at]
            .into_iter()
            .flatten()
            .max();
        Ok(SessionOpened {
            phase: files.state.phase_of(&id),
            // Whichever file already held a slot settles this: a
            // session is resumed when there was something to resume.
            resumed: in_state.resumed || in_memory.resumed,
            last_active_at,
            id,
        })
    }

    fn list(&self) -> Result<Vec<SessionInfo>, SessionError> {
        Ok(self.read()?.list())
    }
}

/// The session the files name, for a caller that has a root and no
/// store. `None` means nothing has opened one yet.
pub fn current_session(root: &Path) -> Result<Option<SessionId>, SessionError> {
    SessionFiles::read(root, &SystemClock.now())
        .map(|files| files.current())
        .map_err(SessionError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::fs_memory::FsMemoryStore;
    use crate::adapters::fs_state::FsStateStore;
    use crate::adapters::spec_home::spec_file;
    use crate::domain::memory::ProjectMemory;
    use crate::domain::tdd::{TddPhase, TddSnapshot};
    use crate::domain::{MEMORY_FILE, STATE_FILE};
    use crate::ports::{MemoryStore, StateStore};
    use std::fs;

    fn recording(root: &Path, phase: TddPhase) {
        FsStateStore::new(root.to_path_buf())
            .save(&TddSnapshot::at(phase))
            .expect("the state saves");
    }

    #[test]
    fn an_empty_project_has_no_current_session_and_lists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsSessionStore::new(dir.path().to_path_buf());
        assert_eq!(current_session(dir.path()).unwrap(), None);
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn opening_without_a_request_creates_a_current_session_in_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = FsSessionStore::new(root.clone());
        let opened = store.open(None).unwrap();
        assert!(!opened.resumed);
        assert_eq!(opened.phase, TddPhase::Start);
        assert_eq!(opened.last_active_at, None);
        assert_eq!(current_session(&root).unwrap(), Some(opened.id.clone()));
        let state = fs::read_to_string(spec_file(&root, STATE_FILE)).unwrap();
        assert!(state.contains(opened.id.as_str()), "{state}");
        let memory = fs::read_to_string(spec_file(&root, MEMORY_FILE)).unwrap();
        assert!(memory.contains(opened.id.as_str()), "{memory}");
        assert!(
            !memory.contains("\"language\""),
            "nothing recorded means no memory slot: {memory}"
        );
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].current);
        assert_eq!(listed[0].phase, Some(TddPhase::Start));
        assert_eq!(listed[0].language, None);
    }

    /// The rule that keeps `.spec/state.json` from collecting one empty
    /// session per shell: a start nobody recorded anything in is the
    /// one the next start continues.
    #[test]
    fn repeated_starts_that_record_nothing_stay_one_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = FsSessionStore::new(root.clone());
        let first = store.open(None).unwrap();
        assert!(!first.resumed, "there was nothing to resume");
        for _ in 0..3 {
            let again = store.open(None).unwrap();
            assert_eq!(again.id, first.id);
            assert!(again.resumed);
            assert_eq!(again.phase, TddPhase::Start);
        }
        assert_eq!(store.list().unwrap().len(), 1);

        // Once the session has a log, the next start is its own.
        recording(&root, TddPhase::Red);
        let second = store.open(None).unwrap();
        assert_ne!(second.id, first.id);
        assert!(!second.resumed);
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn a_recorded_session_is_resumed_with_its_phase_and_the_pointer_moves() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = FsSessionStore::new(root.clone());
        let first = store.open(None).unwrap();
        recording(&root, TddPhase::Green);
        let second = store.open(None).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(current_session(&root).unwrap(), Some(second.id.clone()));
        assert_eq!(
            FsStateStore::new(root.clone()).load().unwrap().phase(),
            TddPhase::Start,
            "the new session starts at START"
        );

        let resumed = store.open(Some(&first.id)).unwrap();
        assert!(resumed.resumed);
        assert_eq!(resumed.id, first.id);
        assert_eq!(resumed.phase, TddPhase::Green);
        assert!(resumed.last_active_at.is_some());
        assert_eq!(current_session(&root).unwrap(), Some(first.id.clone()));
        assert_eq!(
            FsStateStore::new(root).load().unwrap().phase(),
            TddPhase::Green
        );
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, first.id, "the one just opened is newest");
        assert!(listed[0].current && !listed[1].current);
    }

    #[test]
    fn a_new_session_carries_the_previous_memory_forward() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = FsSessionStore::new(root.clone());
        store.open(None).unwrap();
        FsMemoryStore::new(root.clone())
            .save(&ProjectMemory {
                language: "Rust".into(),
                structure: crate::domain::memory::ProjectStructure {
                    module_root: Some("kata".into()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        recording(&root, TddPhase::Red);
        let next = store.open(None).unwrap();
        let memory = FsMemoryStore::new(root).load().unwrap().unwrap();
        assert_eq!(memory.structure.module_root.as_deref(), Some("kata"));
        let listed = store.list().unwrap();
        assert_eq!(listed[0].id, next.id);
        assert_eq!(listed[0].language.as_deref(), Some("Rust"));
    }

    #[test]
    fn an_unknown_id_is_refused_naming_both_files_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = FsSessionStore::new(root.clone());
        let known = store.open(None).unwrap();
        let unknown = SessionId::parse("0f0e0d0c-0b0a-4908-8706-050403020100").unwrap();
        let error = store.open(Some(&unknown)).unwrap_err();
        assert!(error.0.contains(".spec/state.json"), "{}", error.0);
        assert!(error.0.contains(".spec/memory.json"), "{}", error.0);
        assert!(error.0.contains(known.id.as_str()), "{}", error.0);
        assert_eq!(current_session(&root).unwrap(), Some(known.id));
    }

    #[test]
    fn files_from_before_sessions_are_adopted_as_one_session_on_the_first_open() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let state_path = spec_file(&root, STATE_FILE);
        fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        fs::write(
            &state_path,
            serde_json::to_string(&TddSnapshot::at(TddPhase::Red)).unwrap(),
        )
        .unwrap();
        fs::write(
            spec_file(&root, MEMORY_FILE),
            r#"{"version":1,"language":"Java","refreshedAt":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        let store = FsSessionStore::new(root.clone());
        // Before any open, the adopted session is already what is read.
        assert!(current_session(&root).unwrap().is_some());
        assert_eq!(
            FsStateStore::new(root.clone()).load().unwrap().phase(),
            TddPhase::Red
        );
        // Opening continues the one session there was, under one id
        // for both files, and puts both on disk in the new shape.
        let opened = store.open(None).unwrap();
        assert!(opened.resumed, "the recorded state is continued");
        assert_eq!(opened.phase, TddPhase::Red);
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].id, opened.id);
        assert_eq!(listed[0].phase, Some(TddPhase::Red));
        assert_eq!(listed[0].language.as_deref(), Some("Java"));
        assert_eq!(current_session(&root).unwrap(), Some(opened.id.clone()));
        let text = fs::read_to_string(&state_path).unwrap();
        assert!(text.contains("\"sessions\""), "{text}");
        assert!(text.contains(opened.id.as_str()), "{text}");
        let memory = fs::read_to_string(spec_file(&root, MEMORY_FILE)).unwrap();
        assert!(memory.contains(opened.id.as_str()), "{memory}");
        // The id is now on disk: a later read does not mint another.
        assert_eq!(current_session(&root).unwrap(), Some(opened.id));
    }

    #[test]
    fn a_broken_file_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let path = spec_file(&root, MEMORY_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        let store = FsSessionStore::new(root);
        let error = store.open(None).unwrap_err();
        assert!(
            error.0.starts_with(".spec/memory.json is not valid JSON -"),
            "{}",
            error.0
        );
        assert!(store.list().is_err());
    }

    #[test]
    fn an_unwritable_root_names_both_files() {
        let store = FsSessionStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.open(None).unwrap_err();
        assert!(
            error
                .0
                .starts_with(".spec/state.json and .spec/memory.json is not writable -"),
            "{}",
            error.0
        );
    }
}

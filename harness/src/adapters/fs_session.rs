//! Sessions on the filesystem: the [`SessionStore`] port over
//! `.spec/state.json` and `.spec/memory.json`, plus the file reading and
//! writing the state and memory stores share so the two files agree on
//! which session is current.
//!
//! Every write happens under the project's advisory lock
//! (`.spec/.lock`) and lands by scratch-file-and-rename, so a reader
//! never sees a torn file and two `spec` processes never lose each
//! other's session.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::adapters::spec_home::{spec_file, spec_home, write_atomically};
use crate::adapters::work_lock::{self, WorkClaim};
use crate::domain::memory::{MemoryFile, listed_memory, parse_memory_file, seed_memory};
use crate::domain::session::{
    SessionId, SessionInfo, SessionOpened, check_requested, merge_sessions,
};
use crate::domain::tdd::{SessionEntries, TddStateFile};
use crate::domain::{MEMORY_FILE, STATE_FILE, spec_rel};
use crate::ports::{SessionError, SessionStore};

/// A brand-new session id: a random (v4) GUID.
pub fn fresh_id() -> SessionId {
    SessionId::parse(&uuid::Uuid::new_v4().to_string()).expect("a v4 UUID is a GUID")
}

pub(crate) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The project's lock, in the words of the file about to be written.
pub(crate) fn claim(root: &Path, shown: &str) -> Result<WorkClaim, String> {
    work_lock::claim(&spec_home(root)).map_err(|e| format!("{shown} is not writable - {}", e.0))
}

fn read_text(path: &Path, shown: &str) -> Result<Option<String>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|e| format!("{shown} is not readable - {e}"))
}

fn write_text(path: &Path, shown: &str, text: String) -> Result<(), String> {
    write_atomically(path, &text).map_err(|e: io::Error| format!("{shown} is not writable - {e}"))
}

/// Both files as read, remembering whether either was adopted from the
/// shape before sessions, so a write puts it on disk in the new one.
pub(crate) struct SessionFiles {
    pub state: TddStateFile,
    pub memory: MemoryFile,
    /// The id a file from before sessions was adopted under - one id
    /// for both files, so what they recorded stays one session. It is
    /// the session to continue when nothing else is asked for.
    pub adopted: Option<SessionId>,
    state_dirty: bool,
    memory_dirty: bool,
    state_path: PathBuf,
    memory_path: PathBuf,
}

impl SessionFiles {
    pub(crate) fn read(root: &Path) -> Result<Self, String> {
        let state_path = spec_file(root, STATE_FILE);
        let memory_path = spec_file(root, MEMORY_FILE);
        let now = now_rfc3339();
        let state_shown = spec_rel(STATE_FILE);
        let memory_shown = spec_rel(MEMORY_FILE);
        let candidate = fresh_id();
        let mut adopted = None;
        let state = match read_text(&state_path, &state_shown)? {
            Some(text) => TddStateFile::parse(&text, &now, || {
                adopted = Some(candidate.clone());
                candidate.clone()
            })
            .map_err(|e| format!("{state_shown} is not valid JSON - {e}"))?,
            None => TddStateFile::default(),
        };
        let state_dirty = adopted.is_some();
        let mut memory_adopted = false;
        let memory = match read_text(&memory_path, &memory_shown)? {
            Some(text) => parse_memory_file(&text, &now, || {
                memory_adopted = true;
                adopted = Some(candidate.clone());
                candidate.clone()
            })
            .map_err(|e| format!("{memory_shown} is not valid JSON - {e}"))?,
            None => MemoryFile::default(),
        };
        Ok(Self {
            state,
            memory,
            adopted,
            state_dirty,
            memory_dirty: memory_adopted,
            state_path,
            memory_path,
        })
    }

    /// The session commands act on: the state file's pointer, else the
    /// memory file's. The two agree whenever both are set, because
    /// opening writes both.
    pub(crate) fn current(&self) -> Option<SessionId> {
        self.state
            .sessions
            .current_session
            .clone()
            .or_else(|| self.memory.current_session.clone())
    }

    /// Every id either file has a slot for.
    fn known(&self) -> Vec<SessionId> {
        let mut known = self.state.sessions.ids();
        for id in self.memory.ids() {
            if !known.contains(&id) {
                known.push(id);
            }
        }
        known.sort();
        known
    }

    pub(crate) fn touch_state(&mut self) {
        self.state_dirty = true;
    }

    pub(crate) fn touch_memory(&mut self) {
        self.memory_dirty = true;
    }

    /// Write whichever file changed - including one adopted from the
    /// old shape on the way in, so the id it was given sticks.
    pub(crate) fn write(&mut self) -> Result<(), String> {
        if self.state_dirty {
            let text =
                serde_json::to_string_pretty(&self.state).expect("a state file always serializes");
            write_text(&self.state_path, &spec_rel(STATE_FILE), text)?;
            self.state_dirty = false;
        }
        if self.memory_dirty {
            let text = serde_json::to_string_pretty(&self.memory)
                .expect("a memory file always serializes");
            write_text(
                &self.memory_path,
                &spec_rel(MEMORY_FILE),
                format!("{text}\n"),
            )?;
            self.memory_dirty = false;
        }
        Ok(())
    }

    fn list(&self) -> Vec<SessionInfo> {
        let current = self.current();
        merge_sessions(
            current.as_ref(),
            self.state.listed(),
            listed_memory(&self.memory),
        )
    }
}

pub struct FsSessionStore {
    root: PathBuf,
}

impl FsSessionStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl SessionStore for FsSessionStore {
    fn open(&self, requested: Option<&SessionId>) -> Result<SessionOpened, SessionError> {
        let _claim = claim(&self.root, &spec_rel(STATE_FILE)).map_err(SessionError)?;
        let mut files = SessionFiles::read(&self.root).map_err(SessionError)?;
        check_requested(requested, &files.known())
            .map_err(|refusal| SessionError(refusal.to_string()))?;
        // A file from before sessions is the one session there was, so
        // with nothing asked for it is continued rather than left behind
        // a new one - the developer who upgraded is where they were.
        let adopted = files.adopted.clone();
        let requested = requested.or(adopted.as_ref());
        let fresh = fresh_id();
        let now = now_rfc3339();
        let in_state = files
            .state
            .sessions
            .open(requested, fresh.clone(), &now, |_| {
                Some(SessionEntries::default())
            });
        let in_memory = files.memory.open(requested, fresh, &now, seed_memory);
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
            resumed: requested.is_some(),
            last_active_at,
            id,
        })
    }

    fn list(&self) -> Result<Vec<SessionInfo>, SessionError> {
        Ok(SessionFiles::read(&self.root).map_err(SessionError)?.list())
    }

    fn current(&self) -> Result<Option<SessionId>, SessionError> {
        Ok(SessionFiles::read(&self.root)
            .map_err(SessionError)?
            .current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::fs_memory::FsMemoryStore;
    use crate::adapters::fs_state::FsStateStore;
    use crate::domain::memory::ProjectMemory;
    use crate::domain::tdd::{TddPhase, TddSnapshot};
    use crate::ports::{MemoryStore, StateStore};

    #[test]
    fn a_fresh_id_is_a_guid_and_two_differ() {
        let one = fresh_id();
        assert_eq!(one.as_str().len(), 36);
        assert_ne!(one, fresh_id());
    }

    #[test]
    fn an_empty_project_has_no_current_session_and_lists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsSessionStore::new(dir.path().to_path_buf());
        assert_eq!(store.current().unwrap(), None);
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
        assert_eq!(store.current().unwrap(), Some(opened.id.clone()));
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

    #[test]
    fn a_recorded_session_is_resumed_with_its_phase_and_the_pointer_moves() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = FsSessionStore::new(root.clone());
        let first = store.open(None).unwrap();
        FsStateStore::new(root.clone())
            .save(&TddSnapshot::at(TddPhase::Green))
            .unwrap();
        let second = store.open(None).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(store.current().unwrap(), Some(second.id.clone()));
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
        assert_eq!(store.current().unwrap(), Some(first.id.clone()));
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
        assert_eq!(store.current().unwrap(), Some(known.id));
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
        assert!(store.current().unwrap().is_some());
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
        assert_eq!(store.current().unwrap(), Some(opened.id.clone()));
        let text = fs::read_to_string(&state_path).unwrap();
        assert!(text.contains("\"sessions\""), "{text}");
        assert!(text.contains(opened.id.as_str()), "{text}");
        let memory = fs::read_to_string(spec_file(&root, MEMORY_FILE)).unwrap();
        assert!(memory.contains(opened.id.as_str()), "{memory}");
        // The id is now on disk: a later read does not mint another.
        assert_eq!(store.current().unwrap(), Some(opened.id));
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
    fn an_unwritable_root_is_a_structured_error() {
        let store = FsSessionStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.open(None).unwrap_err();
        assert!(
            error.0.starts_with(".spec/state.json is not writable -"),
            "{}",
            error.0
        );
    }
}

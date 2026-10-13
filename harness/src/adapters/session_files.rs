//! The pair of files sessions live in: `.spec/state.json` and
//! `.spec/memory.json`, read and written together so they never
//! disagree about which session is current.
//!
//! Shared by the state store, the memory store, and the session store.
//! None of the three owns it, because all three read and write both
//! files - a save into one has to leave the other's sessions alone.
//!
//! Every write happens under the project's advisory lock
//! (`.spec/.lock`) and lands by scratch-file-and-rename, so a reader
//! never sees a torn file and two `spec` processes never lose each
//! other's session.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::adapters::spec_home::{spec_file, spec_home, write_atomically};
use crate::adapters::work_lock::{self, WorkClaim};
use crate::domain::memory::{MemoryFile, listed_memory, parse_memory_file};
use crate::domain::session::{SessionId, SessionInfo, merge_sessions};
use crate::domain::tdd::TddStateFile;
use crate::domain::{MEMORY_FILE, STATE_FILE, spec_rel};

/// A brand-new session id: a random (v4) GUID.
pub(crate) fn fresh_id() -> SessionId {
    SessionId::parse(&uuid::Uuid::new_v4().to_string()).expect("a v4 UUID is a GUID")
}

/// Both session files named together, for a claim or a failure that is
/// about the pair rather than one of them.
pub(crate) fn both_shown() -> String {
    format!("{} and {}", spec_rel(STATE_FILE), spec_rel(MEMORY_FILE))
}

/// The project's lock, in the words of the file (or files) the caller is
/// about to write.
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

/// Serialize a session file and write it whole, newline-terminated like
/// every other file the harness writes.
fn write_file<T: Serialize>(path: &Path, shown: &str, file: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(file).expect("a session file always serializes");
    write_atomically(path, &format!("{text}\n"))
        .map_err(|e: io::Error| format!("{shown} is not writable - {e}"))
}

/// Both files as read, remembering whether either was adopted from the
/// shape before sessions, so a write puts it on disk in the new one.
#[derive(Debug)]
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
    /// Both files as they stand. `now` dates a file adopted from the
    /// shape before sessions; the id naming it is minted only on that
    /// branch, and once for the pair, so a plain read of files already
    /// in the session shape asks for no randomness at all.
    pub(crate) fn read(root: &Path, now: &str) -> Result<Self, String> {
        let state_path = spec_file(root, STATE_FILE);
        let memory_path = spec_file(root, MEMORY_FILE);
        let state_shown = spec_rel(STATE_FILE);
        let memory_shown = spec_rel(MEMORY_FILE);
        let mut adopted = None;
        let mut state_dirty = false;
        let state = match read_text(&state_path, &state_shown)? {
            Some(text) => TddStateFile::parse(&text, now, || {
                state_dirty = true;
                adopted.get_or_insert_with(fresh_id).clone()
            })
            .map_err(|e| format!("{state_shown} is not valid JSON - {e}"))?,
            None => TddStateFile::default(),
        };
        let mut memory_dirty = false;
        let memory = match read_text(&memory_path, &memory_shown)? {
            Some(text) => parse_memory_file(&text, now, || {
                memory_dirty = true;
                adopted.get_or_insert_with(fresh_id).clone()
            })
            .map_err(|e| format!("{memory_shown} is not valid JSON - {e}"))?,
            None => MemoryFile::default(),
        };
        Ok(Self {
            state,
            memory,
            adopted,
            state_dirty,
            memory_dirty,
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
    pub(crate) fn known(&self) -> Vec<SessionId> {
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
            write_file(&self.state_path, &spec_rel(STATE_FILE), &self.state)?;
            self.state_dirty = false;
        }
        if self.memory_dirty {
            write_file(&self.memory_path, &spec_rel(MEMORY_FILE), &self.memory)?;
            self.memory_dirty = false;
        }
        Ok(())
    }

    /// Every session both files know, for the startup list.
    pub(crate) fn list(&self) -> Vec<SessionInfo> {
        let current = self.current();
        merge_sessions(
            current.as_ref(),
            self.state.listed(),
            listed_memory(&self.memory),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tdd::{TddPhase, TddSnapshot};

    #[test]
    fn a_fresh_id_is_a_guid_and_two_differ() {
        let one = fresh_id();
        assert_eq!(one.as_str().len(), 36);
        assert_ne!(one, fresh_id());
    }

    #[test]
    fn a_claim_on_an_unwritable_root_names_what_it_could_not_write() {
        let Err(error) = claim(Path::new("/dev/null/nowhere"), &both_shown()) else {
            panic!("an unwritable root cannot be claimed");
        };
        assert!(
            error.starts_with(".spec/state.json and .spec/memory.json is not writable -"),
            "{error}"
        );
    }

    /// Reading files already in the session shape mints nothing: only
    /// an adoption needs an id, and then one id serves both files.
    #[test]
    fn reading_current_files_adopts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut files = SessionFiles::read(root, "t0").unwrap();
        assert_eq!(files.adopted, None);
        assert_eq!(files.current(), None);
        assert!(files.known().is_empty());
        assert!(files.list().is_empty());

        let id = fresh_id();
        files.state.put(&id, &TddSnapshot::at(TddPhase::Red), "t1");
        files.touch_state();
        files.write().unwrap();

        let again = SessionFiles::read(root, "t2").unwrap();
        assert_eq!(again.adopted, None, "already in the session shape");
        assert_eq!(again.current(), Some(id.clone()));
        assert_eq!(again.known(), vec![id]);
    }

    /// Two files from before sessions are one session, under one id.
    #[test]
    fn both_legacy_files_are_adopted_under_a_single_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let state_path = spec_file(root, STATE_FILE);
        fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        fs::write(
            &state_path,
            serde_json::to_string(&TddSnapshot::at(TddPhase::Green)).unwrap(),
        )
        .unwrap();
        fs::write(
            spec_file(root, MEMORY_FILE),
            r#"{"version":1,"language":"Rust","refreshedAt":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let mut files = SessionFiles::read(root, "t0").unwrap();
        let adopted = files.adopted.clone().expect("both files were adopted");
        assert_eq!(files.current(), Some(adopted.clone()));
        assert_eq!(files.known(), vec![adopted.clone()]);
        assert_eq!(
            files.memory.body_of(&adopted).map(|m| m.language.as_str()),
            Some("Rust")
        );
        assert_eq!(files.state.phase_of(&adopted), TddPhase::Green);

        // Adoption alone marks both files for writing, so the id sticks.
        files.write().unwrap();
        let again = SessionFiles::read(root, "t1").unwrap();
        assert_eq!(again.adopted, None);
        assert_eq!(again.current(), Some(adopted));
    }

    #[test]
    fn a_written_file_ends_in_a_newline() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = SessionFiles::read(dir.path(), "t0").unwrap();
        files.state.put(&fresh_id(), &TddSnapshot::default(), "t1");
        files.touch_state();
        files.touch_memory();
        files.write().unwrap();
        for name in [STATE_FILE, MEMORY_FILE] {
            let text = fs::read_to_string(spec_file(dir.path(), name)).unwrap();
            assert!(text.ends_with("}\n"), "{name}: {text:?}");
        }
    }

    #[test]
    fn a_broken_file_names_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), MEMORY_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        let error = SessionFiles::read(dir.path(), "t0").unwrap_err();
        assert!(
            error.starts_with(".spec/memory.json is not valid JSON -"),
            "{error}"
        );
    }
}

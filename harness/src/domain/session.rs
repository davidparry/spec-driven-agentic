//! Sessions: every run of the harness belongs to one, named by a GUID,
//! and `.spec/state.json` and `.spec/memory.json` each keep one slice
//! per session beside a `currentSession` pointer. Bare `spec` opens a
//! new session (or resumes the one `--session` names); every other
//! command follows the pointer, so `spec test` and then `spec state`
//! still share one machine.
//!
//! Everything here is pure. Minting an id is randomness and reading a
//! clock is IO, so both arrive from the caller.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::domain::human::{Human, columns, sections};
use crate::domain::tdd::TddPhase;
use crate::domain::{MEMORY_FILE, STATE_FILE, spec_rel};

/// The length of a session id as text: `8-4-4-4-12` hex digits.
pub const SESSION_ID_LEN: usize = 36;

/// A session's name: a canonical lowercase GUID.
///
/// Validated on the way in, so a file key or a `--session` argument
/// that is not a GUID is refused rather than stored.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionId(String);

impl SessionId {
    /// The id a person typed or a file holds, normalized to lowercase.
    pub fn parse(text: &str) -> Result<Self, SessionRefusal> {
        let trimmed = text.trim();
        let lower = trimmed.to_ascii_lowercase();
        if is_guid(&lower) {
            Ok(Self(lower))
        } else {
            Err(SessionRefusal::Malformed(trimmed.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_guid(text: &str) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == GROUPS.len()
        && groups
            .iter()
            .zip(GROUPS)
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

impl TryFrom<String> for SessionId {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).map_err(|refusal| refusal.to_string())
    }
}

impl From<SessionId> for String {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a session could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRefusal {
    /// The text is not a GUID at all.
    Malformed(String),
    /// A well-formed id that neither file has on record.
    Unknown {
        requested: SessionId,
        known: Vec<SessionId>,
    },
}

impl fmt::Display for SessionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionRefusal::Malformed(text) => write!(
                f,
                "\"{text}\" is not a GUID - a session id looks like \
                 6f1c2d3e-4b5a-4c6d-8e9f-0a1b2c3d4e5f."
            ),
            SessionRefusal::Unknown { requested, known } => {
                let known = if known.is_empty() {
                    "none".to_string()
                } else {
                    known
                        .iter()
                        .map(SessionId::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                write!(
                    f,
                    "No session {requested} is on record in {} or {}. Known sessions: {known}. \
                     Start without --session to begin a new one.",
                    spec_rel(STATE_FILE),
                    spec_rel(MEMORY_FILE)
                )
            }
        }
    }
}

impl std::error::Error for SessionRefusal {}

/// The id a person asked for must be one of the ids on record. `None`
/// asks for nothing and is always fine: a new session will be opened.
pub fn check_requested(
    requested: Option<&SessionId>,
    known: &[SessionId],
) -> Result<(), SessionRefusal> {
    match requested {
        Some(id) if !known.contains(id) => Err(SessionRefusal::Unknown {
            requested: id.clone(),
            known: known.to_vec(),
        }),
        _ => Ok(()),
    }
}

/// One session's slice of a file: when it began, when it last did
/// anything, and the body the file keeps per session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSlot<T> {
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "lastActiveAt")]
    pub last_active_at: String,
    #[serde(flatten)]
    pub body: T,
}

/// A file's sessions: the pointer and one slot per id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnvelope<T> {
    #[serde(
        rename = "currentSession",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub current_session: Option<SessionId>,
    // A named default, so the derive asks nothing of `T` beyond serde.
    #[serde(default = "BTreeMap::new")]
    pub sessions: BTreeMap<SessionId, SessionSlot<T>>,
}

impl<T> Default for SessionEnvelope<T> {
    fn default() -> Self {
        Self {
            current_session: None,
            sessions: BTreeMap::new(),
        }
    }
}

/// What opening a session in one envelope did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub id: SessionId,
    /// A slot already on record was continued, as opposed to created.
    pub resumed: bool,
    /// When the slot was last active before this open; `None` when it
    /// was just created.
    pub last_active_at: Option<String>,
}

impl<T> SessionEnvelope<T> {
    /// A file written before sessions existed, read as one session that
    /// is current - so nothing it recorded is lost to the new shape.
    pub fn adopt_legacy(body: T, now: &str, id: SessionId) -> Self {
        let mut sessions = BTreeMap::new();
        sessions.insert(
            id.clone(),
            SessionSlot {
                started_at: now.to_string(),
                last_active_at: now.to_string(),
                body,
            },
        );
        Self {
            current_session: Some(id),
            sessions,
        }
    }

    pub fn ids(&self) -> Vec<SessionId> {
        self.sessions.keys().cloned().collect()
    }

    pub fn slot(&self, id: &SessionId) -> Option<&SessionSlot<T>> {
        self.sessions.get(id)
    }

    pub fn body_of(&self, id: &SessionId) -> Option<&T> {
        self.slot(id).map(|slot| &slot.body)
    }

    pub fn current_body(&self) -> Option<&T> {
        self.current_session
            .as_ref()
            .and_then(|id| self.body_of(id))
    }

    /// Open `requested`, or a session named `fresh` when nothing was
    /// requested. A requested id this file has no slot for is opened
    /// all the same - the other file may well have it. A new session's
    /// body comes from `seed`, handed whatever the previous current body
    /// was; `None` means there is nothing to record yet, so the pointer
    /// moves and no slot is written until [`Self::put`].
    pub fn open(
        &mut self,
        requested: Option<&SessionId>,
        fresh: SessionId,
        now: &str,
        seed: impl FnOnce(Option<&T>) -> Option<T>,
    ) -> Opened {
        let id = requested.cloned().unwrap_or(fresh);
        let opened = match self.sessions.get_mut(&id) {
            Some(slot) => {
                let last_active_at = std::mem::replace(&mut slot.last_active_at, now.to_string());
                Opened {
                    id: id.clone(),
                    resumed: true,
                    last_active_at: Some(last_active_at),
                }
            }
            None => {
                if let Some(body) = seed(self.current_body()) {
                    self.sessions.insert(
                        id.clone(),
                        SessionSlot {
                            started_at: now.to_string(),
                            last_active_at: now.to_string(),
                            body,
                        },
                    );
                }
                Opened {
                    id: id.clone(),
                    resumed: false,
                    last_active_at: None,
                }
            }
        };
        self.current_session = Some(id);
        opened
    }

    /// Replace one session's body, creating the slot when the file has
    /// none for it, and make it current when nothing was.
    pub fn put(&mut self, id: &SessionId, body: T, now: &str) {
        match self.sessions.get_mut(id) {
            Some(slot) => {
                slot.body = body;
                slot.last_active_at = now.to_string();
            }
            None => {
                self.sessions.insert(
                    id.clone(),
                    SessionSlot {
                        started_at: now.to_string(),
                        last_active_at: now.to_string(),
                        body,
                    },
                );
            }
        }
        if self.current_session.is_none() {
            self.current_session = Some(id.clone());
        }
    }
}

/// One session as the startup list shows it, gathered from both files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub started_at: String,
    pub last_active_at: String,
    /// The TDD phase the session's state log ends on; `None` when the
    /// state file holds nothing for it.
    pub phase: Option<TddPhase>,
    /// The language the session's memory recorded, when it has any.
    pub language: Option<String>,
    pub current: bool,
}

/// The sessions both files know, newest activity first, the current one
/// first among equals.
pub fn merge_sessions(
    current: Option<&SessionId>,
    state: impl IntoIterator<Item = (SessionId, String, String, TddPhase)>,
    memory: impl IntoIterator<Item = (SessionId, String, String, Option<String>)>,
) -> Vec<SessionInfo> {
    let mut by_id: BTreeMap<SessionId, SessionInfo> = BTreeMap::new();
    for (id, started_at, last_active_at, phase) in state {
        by_id.insert(
            id.clone(),
            SessionInfo {
                current: current == Some(&id),
                id,
                started_at,
                last_active_at,
                phase: Some(phase),
                language: None,
            },
        );
    }
    for (id, started_at, last_active_at, language) in memory {
        match by_id.get_mut(&id) {
            Some(info) => {
                if started_at < info.started_at {
                    info.started_at = started_at;
                }
                if last_active_at > info.last_active_at {
                    info.last_active_at = last_active_at;
                }
                info.language = language;
            }
            None => {
                by_id.insert(
                    id.clone(),
                    SessionInfo {
                        current: current == Some(&id),
                        id,
                        started_at,
                        last_active_at,
                        phase: None,
                        language,
                    },
                );
            }
        }
    }
    let mut sessions: Vec<SessionInfo> = by_id.into_values().collect();
    sessions.sort_by(|a, b| {
        b.last_active_at
            .cmp(&a.last_active_at)
            .then_with(|| b.current.cmp(&a.current))
            .then_with(|| b.started_at.cmp(&a.started_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    sessions
}

/// The session this run belongs to, as opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOpened {
    pub id: SessionId,
    pub resumed: bool,
    /// The phase the session's state log ends on - START for a new one.
    pub phase: TddPhase,
    /// When a resumed session was last active before now.
    pub last_active_at: Option<String>,
}

/// What a shell start says about sessions: the one it opened and every
/// one on record. A list and nothing more - picking one is done by
/// starting again with `--session`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStart {
    pub opened: SessionOpened,
    pub sessions: Vec<SessionInfo>,
}

/// The one-shot command that resumes a session, as the hint names it.
pub const RESUME_COMMAND: &str = "spec --session";

impl Human for SessionStart {
    fn human(&self) -> String {
        let opened = &self.opened;
        let line = if opened.resumed {
            let since = opened
                .last_active_at
                .as_deref()
                .map(|at| format!(", last active {at}"))
                .unwrap_or_default();
            format!("Session {} (resumed - {}{since}).", opened.id, opened.phase)
        } else {
            format!("Session {} (new).", opened.id)
        };
        let rows: Vec<Vec<String>> = self
            .sessions
            .iter()
            .map(|session| {
                vec![
                    format!("{} {}", if session.current { "*" } else { " " }, session.id),
                    format!("started {}", session.started_at),
                    format!("active {}", session.last_active_at),
                    session
                        .phase
                        .map(|phase| phase.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    session.language.clone().unwrap_or_default(),
                ]
            })
            .collect();
        let list = format!(
            "Sessions on record ({}):\n{}",
            self.sessions.len(),
            columns(&rows)
        );
        let hint = format!("Resume one with: {RESUME_COMMAND} <id>");
        sections(&[line, list, hint])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> SessionId {
        SessionId::parse(&format!("00000000-0000-4000-8000-{n:012x}")).unwrap()
    }

    #[test]
    fn a_session_id_is_a_lowercase_guid() {
        let parsed = SessionId::parse(" 6F1C2D3E-4B5A-4C6D-8E9F-0A1B2C3D4E5F ").unwrap();
        assert_eq!(parsed.as_str(), "6f1c2d3e-4b5a-4c6d-8e9f-0a1b2c3d4e5f");
        assert_eq!(parsed.to_string().len(), SESSION_ID_LEN);
        for bad in [
            "",
            "not-a-guid",
            "6f1c2d3e4b5a4c6d8e9f0a1b2c3d4e5f",
            "zzzzzzzz-0000-0000-0000-000000000000",
        ] {
            let refusal = SessionId::parse(bad).unwrap_err();
            assert!(refusal.to_string().contains("is not a GUID"), "{refusal}");
        }
    }

    #[test]
    fn a_session_id_serializes_as_its_text_and_refuses_bad_text() {
        let json = serde_json::to_string(&id(1)).unwrap();
        assert_eq!(json, "\"00000000-0000-4000-8000-000000000001\"");
        let back: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id(1));
        let error = serde_json::from_str::<SessionId>("\"nope\"").unwrap_err();
        assert!(error.to_string().contains("is not a GUID"), "{error}");
    }

    #[test]
    fn a_requested_id_must_be_on_record() {
        assert_eq!(check_requested(None, &[]), Ok(()));
        assert_eq!(check_requested(Some(&id(1)), &[id(1), id(2)]), Ok(()));
        let refusal = check_requested(Some(&id(3)), &[id(1)]).unwrap_err();
        let text = refusal.to_string();
        assert!(text.contains(".spec/state.json"), "{text}");
        assert!(text.contains(".spec/memory.json"), "{text}");
        assert!(text.contains(&id(1).to_string()), "{text}");
        assert!(text.contains(&id(3).to_string()), "{text}");
        let none = check_requested(Some(&id(3)), &[]).unwrap_err().to_string();
        assert!(none.contains("Known sessions: none"), "{none}");
    }

    #[test]
    fn opening_without_a_request_creates_a_seeded_current_session() {
        let mut envelope: SessionEnvelope<Vec<u32>> = SessionEnvelope::default();
        let opened = envelope.open(None, id(1), "t1", |_| Some(vec![7]));
        assert_eq!(
            opened,
            Opened {
                id: id(1),
                resumed: false,
                last_active_at: None
            }
        );
        assert_eq!(envelope.current_session, Some(id(1)));
        assert_eq!(envelope.current_body(), Some(&vec![7]));
        // A later new session is seeded from the one that was current.
        let next = envelope.open(None, id(2), "t2", |previous| previous.cloned());
        assert!(!next.resumed);
        assert_eq!(envelope.body_of(&id(2)), Some(&vec![7]));
        assert_eq!(envelope.ids(), vec![id(1), id(2)]);
    }

    /// Nothing to record yet: the pointer moves, no slot is written.
    #[test]
    fn a_seed_of_none_moves_the_pointer_without_a_slot() {
        let mut envelope: SessionEnvelope<Vec<u32>> = SessionEnvelope::default();
        let opened = envelope.open(None, id(1), "t1", |_| None);
        assert!(!opened.resumed);
        assert_eq!(envelope.current_session, Some(id(1)));
        assert!(envelope.ids().is_empty());
        assert_eq!(envelope.current_body(), None);
        envelope.put(&id(1), vec![1], "t2");
        assert_eq!(envelope.slot(&id(1)).unwrap().started_at, "t2");
    }

    #[test]
    fn opening_a_known_id_resumes_it_and_remembers_when_it_was_active() {
        let mut envelope: SessionEnvelope<Vec<u32>> = SessionEnvelope::default();
        envelope.open(None, id(1), "t1", |_| Some(vec![1]));
        envelope.open(None, id(2), "t2", |_| Some(vec![2]));
        let opened = envelope.open(Some(&id(1)), id(9), "t3", |_| Some(vec![0]));
        assert_eq!(
            opened,
            Opened {
                id: id(1),
                resumed: true,
                last_active_at: Some("t1".into())
            }
        );
        assert_eq!(envelope.current_session, Some(id(1)));
        assert_eq!(envelope.slot(&id(1)).unwrap().last_active_at, "t3");
        assert_eq!(envelope.slot(&id(1)).unwrap().started_at, "t1");
        assert_eq!(envelope.current_body(), Some(&vec![1]));
    }

    /// The other file may hold the id this one does not: the slot is
    /// created here rather than refused, with the seed as its body.
    #[test]
    fn opening_an_id_this_file_lacks_creates_its_slot() {
        let mut envelope: SessionEnvelope<Vec<u32>> = SessionEnvelope::default();
        envelope.open(None, id(1), "t1", |_| Some(vec![1]));
        let opened = envelope.open(Some(&id(2)), id(9), "t2", |previous| previous.cloned());
        assert!(!opened.resumed);
        assert_eq!(envelope.body_of(&id(2)), Some(&vec![1]));
        assert_eq!(envelope.current_session, Some(id(2)));
    }

    #[test]
    fn a_legacy_body_is_adopted_as_the_current_session() {
        let envelope = SessionEnvelope::adopt_legacy(vec![3, 4], "t0", id(5));
        assert_eq!(envelope.current_session, Some(id(5)));
        assert_eq!(envelope.current_body(), Some(&vec![3, 4]));
        assert_eq!(envelope.slot(&id(5)).unwrap().started_at, "t0");
    }

    #[test]
    fn put_replaces_a_body_or_creates_the_slot_and_names_a_current() {
        let mut envelope: SessionEnvelope<Vec<u32>> = SessionEnvelope::default();
        envelope.put(&id(1), vec![1], "t1");
        assert_eq!(envelope.current_session, Some(id(1)));
        envelope.put(&id(2), vec![2], "t2");
        assert_eq!(
            envelope.current_session,
            Some(id(1)),
            "put never moves the pointer"
        );
        envelope.put(&id(1), vec![9], "t3");
        let slot = envelope.slot(&id(1)).unwrap();
        assert_eq!(
            (
                slot.body.as_slice(),
                slot.started_at.as_str(),
                slot.last_active_at.as_str()
            ),
            ([9].as_slice(), "t1", "t3")
        );
    }

    #[test]
    fn an_envelope_round_trips_with_camel_case_keys_and_a_flattened_body() {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct Body {
            entries: Vec<u32>,
        }
        let mut envelope: SessionEnvelope<Body> = SessionEnvelope::default();
        envelope.open(None, id(1), "t1", |_| Some(Body { entries: vec![1] }));
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(
            json.contains("\"currentSession\":\"00000000-0000-4000-8000-000000000001\""),
            "{json}"
        );
        assert!(json.contains("\"startedAt\":\"t1\""), "{json}");
        assert!(json.contains("\"lastActiveAt\":\"t1\""), "{json}");
        assert!(json.contains("\"entries\":[1]"), "{json}");
        let back: SessionEnvelope<Body> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, envelope);
        let empty: SessionEnvelope<Body> = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, SessionEnvelope::default());
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{\"sessions\":{}}");
    }

    #[test]
    fn merged_sessions_come_newest_first_with_the_current_first_among_equals() {
        let state = vec![
            (id(1), "t1".to_string(), "t5".to_string(), TddPhase::Green),
            (id(2), "t2".to_string(), "t5".to_string(), TddPhase::Red),
            (id(3), "t3".to_string(), "t4".to_string(), TddPhase::Start),
        ];
        let memory = vec![
            (
                id(2),
                "t0".to_string(),
                "t5".to_string(),
                Some("Rust".to_string()),
            ),
            (
                id(4),
                "t4".to_string(),
                "t9".to_string(),
                Some("Java".to_string()),
            ),
        ];
        let merged = merge_sessions(Some(&id(2)), state, memory);
        let order: Vec<SessionId> = merged.iter().map(|s| s.id.clone()).collect();
        assert_eq!(order, vec![id(4), id(2), id(1), id(3)]);
        let two = &merged[1];
        assert!(two.current);
        assert_eq!(two.started_at, "t0", "the earlier start of the two files");
        assert_eq!(two.phase, Some(TddPhase::Red));
        assert_eq!(two.language.as_deref(), Some("Rust"));
        let four = &merged[0];
        assert_eq!(four.phase, None, "memory only");
        assert!(!four.current);
    }

    fn start(resumed: bool) -> SessionStart {
        SessionStart {
            opened: SessionOpened {
                id: id(2),
                resumed,
                phase: TddPhase::Green,
                last_active_at: resumed.then(|| "2026-10-11T19:00:00Z".to_string()),
            },
            sessions: vec![
                SessionInfo {
                    id: id(2),
                    started_at: "2026-10-11T18:00:00Z".into(),
                    last_active_at: "2026-10-11T19:05:00Z".into(),
                    phase: Some(TddPhase::Green),
                    language: Some("Rust".into()),
                    current: true,
                },
                SessionInfo {
                    id: id(1),
                    started_at: "2026-10-10T18:00:00Z".into(),
                    last_active_at: "2026-10-10T18:30:00Z".into(),
                    phase: None,
                    language: None,
                    current: false,
                },
            ],
        }
    }

    #[test]
    fn a_new_session_start_reads_as_a_line_a_list_and_the_resume_command() {
        let text = start(false).human();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "Session 00000000-0000-4000-8000-000000000002 (new)."
        );
        assert_eq!(lines[2], "Sessions on record (2):");
        assert!(
            lines[3].starts_with("* 00000000-0000-4000-8000-000000000002"),
            "{text}"
        );
        assert!(
            lines[3].contains("GREEN") && lines[3].contains("Rust"),
            "{text}"
        );
        assert!(
            lines[4].starts_with("  00000000-0000-4000-8000-000000000001"),
            "{text}"
        );
        assert!(lines[4].contains(" -"), "{text}");
        assert_eq!(
            lines.last().copied(),
            Some("Resume one with: spec --session <id>")
        );
        assert!(!text.contains('?'), "nothing is asked: {text}");
        assert_eq!(start(false).next_step(), None);
    }

    #[test]
    fn a_resumed_session_start_names_the_phase_and_when_it_was_last_active() {
        let text = start(true).human();
        assert!(
            text.starts_with(
                "Session 00000000-0000-4000-8000-000000000002 (resumed - GREEN, last active 2026-10-11T19:00:00Z)."
            ),
            "{text}"
        );
    }
}

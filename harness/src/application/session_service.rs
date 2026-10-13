//! Starting a session: open the one asked for (or a new one) and gather
//! the read-only list of every session the project has on record.

use crate::domain::session::{SessionId, SessionOpened, SessionStart};
use crate::ports::{SessionError, SessionStore};

pub struct SessionService<S: SessionStore> {
    store: S,
}

impl<S: SessionStore> SessionService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Open a session and describe it alongside every other session on
    /// record - what a shell start prints before its first prompt.
    pub fn start(&self, requested: Option<&str>) -> Result<SessionStart, SessionError> {
        let opened = self.resume(requested)?;
        let sessions = self.store.list()?;
        Ok(SessionStart { opened, sessions })
    }

    /// Open a session and say nothing else about the project - what a
    /// one-shot `spec --session <id> <command>` needs, where the list
    /// would only be read to be thrown away.
    ///
    /// `requested` is the raw `--session` text, if any: it has to be a
    /// GUID, and that GUID has to be one the project knows.
    pub fn resume(&self, requested: Option<&str>) -> Result<SessionOpened, SessionError> {
        let requested = requested
            .map(|text| SessionId::parse(text).map_err(|refusal| SessionError(refusal.to_string())))
            .transpose()?;
        self.store.open(requested.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::human::Human;
    use crate::domain::session::SessionInfo;
    use crate::domain::tdd::TddPhase;
    use std::cell::RefCell;

    const KNOWN: &str = "6f1c2d3e-4a5b-4c6d-8e9f-0a1b2c3d4e5f";

    struct FakeStore {
        opened: RefCell<Vec<Option<SessionId>>>,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                opened: RefCell::new(Vec::new()),
            }
        }
    }

    impl SessionStore for FakeStore {
        fn open(&self, requested: Option<&SessionId>) -> Result<SessionOpened, SessionError> {
            self.opened.borrow_mut().push(requested.cloned());
            match requested {
                Some(id) if id.as_str() == KNOWN => Ok(SessionOpened {
                    id: id.clone(),
                    resumed: true,
                    phase: TddPhase::Green,
                    last_active_at: Some("2026-10-11T10:00:00Z".into()),
                }),
                Some(id) => Err(SessionError(format!("No session {id} is on record"))),
                None => Ok(SessionOpened {
                    id: SessionId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
                    resumed: false,
                    phase: TddPhase::Start,
                    last_active_at: None,
                }),
            }
        }

        fn list(&self) -> Result<Vec<SessionInfo>, SessionError> {
            Ok(vec![SessionInfo {
                id: SessionId::parse(KNOWN).unwrap(),
                started_at: "2026-10-11T09:00:00Z".into(),
                last_active_at: "2026-10-11T10:00:00Z".into(),
                phase: Some(TddPhase::Green),
                language: Some("Rust".into()),
                current: true,
            }])
        }
    }

    #[test]
    fn starting_without_a_request_opens_a_new_session_and_lists_the_rest() {
        let service = SessionService::new(FakeStore::new());
        let start = service.start(None).unwrap();
        assert!(!start.opened.resumed);
        assert_eq!(start.sessions.len(), 1);
        let shown = start.human();
        assert!(shown.contains("(new)"), "{shown}");
        assert!(shown.contains(KNOWN), "{shown}");
    }

    #[test]
    fn a_known_guid_is_parsed_and_resumed() {
        let service = SessionService::new(FakeStore::new());
        let start = service.start(Some(&KNOWN.to_uppercase())).unwrap();
        assert!(start.opened.resumed);
        assert_eq!(start.opened.id.as_str(), KNOWN, "parsing lowercases");
        assert_eq!(
            service.store.opened.borrow().as_slice(),
            &[Some(SessionId::parse(KNOWN).unwrap())]
        );
    }

    #[test]
    fn text_that_is_not_a_guid_is_refused_before_the_store_is_asked() {
        let service = SessionService::new(FakeStore::new());
        let error = service.start(Some("not-a-guid")).unwrap_err();
        assert!(error.0.contains("not a GUID"), "{}", error.0);
        assert!(service.store.opened.borrow().is_empty());
    }

    /// A one-shot command settles its session and asks for nothing
    /// else: reading the list would only be to discard it.
    #[test]
    fn resuming_opens_the_session_without_listing_the_rest() {
        let service = SessionService::new(FakeStore::new());
        let opened = service.resume(Some(KNOWN)).unwrap();
        assert!(opened.resumed);
        assert_eq!(opened.id.as_str(), KNOWN);
        assert!(service.resume(Some("not-a-guid")).is_err());
    }

    #[test]
    fn a_store_refusal_is_passed_through() {
        let service = SessionService::new(FakeStore::new());
        let error = service
            .start(Some("00000000-0000-4000-8000-00000000dead"))
            .unwrap_err();
        assert!(error.0.starts_with("No session "), "{}", error.0);
    }
}

//! A disk-backed response cache decorating any [`LlmConversation`]:
//! identical histories within the TTL are answered from `.bdd-cache/`
//! without calling the model. A cache error is never fatal.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::domain::config_report::DEFAULT_LLM_CACHE_TTL_SECONDS;
use crate::domain::tools::{ChatMessage, ChatTurn, ToolDefinition};
use crate::ports::{LlmConversation, LlmError};

pub const SCHEMA_VERSION: &str = "ollama-chat:v1";
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(DEFAULT_LLM_CACHE_TTL_SECONDS);

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    content: String,
    created_at: u64,
}

pub struct CachedConversation<C> {
    inner: C,
    dir: PathBuf,
    ttl: Duration,
    context: String,
}

impl<C> CachedConversation<C> {
    pub fn new(inner: C, dir: PathBuf, ttl: Duration, context: String) -> Self {
        Self {
            inner,
            dir,
            ttl,
            context,
        }
    }

    fn key(&self, model: &str, messages: &[ChatMessage], tools: &[ToolDefinition]) -> String {
        let mut hasher = Sha256::new();
        for part in [SCHEMA_VERSION, self.context.as_str(), model] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        let history = serde_json::to_string(messages).unwrap_or_default();
        hasher.update((history.len() as u64).to_be_bytes());
        hasher.update(history.as_bytes());
        let offered: Vec<(&str, &serde_json::Value)> = tools
            .iter()
            .map(|tool| (tool.name.as_str(), &tool.schema))
            .collect();
        let offered = serde_json::to_string(&offered).unwrap_or_default();
        hasher.update((offered.len() as u64).to_be_bytes());
        hasher.update(offered.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn lookup(&self, key: &str) -> Option<ChatTurn> {
        let path = self.entry_path(key);
        let text = fs::read_to_string(&path).ok()?;
        let Ok(entry) = serde_json::from_str::<CacheEntry>(&text) else {
            debug!(key = &key[..12], "chat cache entry corrupt, removed");
            let _ = fs::remove_file(&path);
            return None;
        };
        if expired(&entry, self.ttl) {
            debug!(key = &key[..12], "chat cache entry expired, removed");
            let _ = fs::remove_file(&path);
            return None;
        }
        serde_json::from_str(&entry.content).ok()
    }

    fn store(&self, key: &str, turn: &ChatTurn) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        self.prune_expired();
        let Ok(content) = serde_json::to_string(turn) else {
            return;
        };
        let entry = CacheEntry {
            content,
            created_at: now(),
        };
        let rendered =
            serde_json::to_string(&entry).expect("a string-and-integer struct always renders");
        let _ = fs::write(self.entry_path(key), rendered);
    }

    fn prune_expired(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut pruned = 0usize;
        for file in entries.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let stale = match fs::read_to_string(&path) {
                Ok(text) => serde_json::from_str::<CacheEntry>(&text)
                    .map(|entry| expired(&entry, self.ttl))
                    .unwrap_or(true),
                Err(_) => continue,
            };
            if stale && fs::remove_file(&path).is_ok() {
                pruned += 1;
            }
        }
        if pruned > 0 {
            debug!(pruned, "chat cache swept expired entries");
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn expired(entry: &CacheEntry, ttl: Duration) -> bool {
    now() >= entry.created_at.saturating_add(ttl.as_secs())
}

impl<C: LlmConversation> LlmConversation for CachedConversation<C> {
    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatTurn, LlmError> {
        if self.ttl.is_zero() {
            debug!(model, "chat cache disabled (zero TTL)");
            return self.inner.chat(model, messages, tools);
        }
        let key = self.key(model, messages, tools);
        if let Some(turn) = self.lookup(&key) {
            debug!(model, key = &key[..12], "chat cache hit");
            return Ok(turn);
        }
        debug!(model, key = &key[..12], "chat cache miss");
        let turn = self.inner.chat(model, messages, tools)?;
        self.store(&key, &turn);
        Ok(turn)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::domain::tools::{ChatMessage, ToolOrigin};

    struct CountingChat {
        calls: Cell<usize>,
        script: RefCell<Vec<Result<ChatTurn, LlmError>>>,
    }

    impl CountingChat {
        fn answering(results: Vec<Result<ChatTurn, LlmError>>) -> Self {
            Self {
                calls: Cell::new(0),
                script: RefCell::new(results),
            }
        }
    }

    impl LlmConversation for CountingChat {
        fn chat(
            &self,
            _model: &str,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatTurn, LlmError> {
            self.calls.set(self.calls.get() + 1);
            self.script.borrow_mut().remove(0)
        }
    }

    fn turn(content: &str) -> ChatTurn {
        ChatTurn {
            content: content.into(),
            tool_calls: Vec::new(),
        }
    }

    fn cached_in(
        dir: &tempfile::TempDir,
        results: Vec<Result<ChatTurn, LlmError>>,
    ) -> CachedConversation<CountingChat> {
        CachedConversation::new(
            CountingChat::answering(results),
            dir.path().join("cache"),
            DEFAULT_CACHE_TTL,
            "http://localhost:11434".into(),
        )
    }

    fn messages() -> Vec<ChatMessage> {
        vec![ChatMessage::user("u")]
    }

    fn tools() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "get_tdd_state".into(),
            description: String::new(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Builtin,
        }]
    }

    #[test]
    fn an_identical_repeat_is_served_from_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok(turn("answer"))]);
        assert_eq!(
            cache.chat("m", &messages(), &tools()).unwrap().content,
            "answer"
        );
        assert_eq!(
            cache.chat("m", &messages(), &tools()).unwrap().content,
            "answer"
        );
        assert_eq!(cache.inner.calls.get(), 1);
    }

    #[test]
    fn a_differing_tool_list_is_a_different_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok(turn("a")), Ok(turn("b"))]);
        cache.chat("m", &messages(), &tools()).unwrap();
        cache.chat("m", &messages(), &[]).unwrap();
        assert_eq!(cache.inner.calls.get(), 2);
    }

    #[test]
    fn an_expired_entry_is_a_miss_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok(turn("fresh"))]);
        let key = cache.key("m", &messages(), &tools());
        fs::create_dir_all(&cache.dir).unwrap();
        let old = CacheEntry {
            content: serde_json::to_string(&turn("stale")).unwrap(),
            created_at: now() - DEFAULT_CACHE_TTL.as_secs() - 1,
        };
        fs::write(cache.entry_path(&key), serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(
            cache.chat("m", &messages(), &tools()).unwrap().content,
            "fresh"
        );
        assert_eq!(cache.inner.calls.get(), 1);
    }

    #[test]
    fn a_corrupt_entry_is_a_miss_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok(turn("regenerated"))]);
        let key = cache.key("m", &messages(), &tools());
        fs::create_dir_all(&cache.dir).unwrap();
        fs::write(cache.entry_path(&key), "not json").unwrap();
        assert_eq!(
            cache.chat("m", &messages(), &tools()).unwrap().content,
            "regenerated"
        );
    }

    #[test]
    fn a_zero_ttl_disables_caching() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CachedConversation::new(
            CountingChat::answering(vec![Ok(turn("a")), Ok(turn("b"))]),
            dir.path().join("cache"),
            Duration::ZERO,
            String::new(),
        );
        assert_eq!(cache.chat("m", &messages(), &[]).unwrap().content, "a");
        assert_eq!(cache.chat("m", &messages(), &[]).unwrap().content, "b");
        assert_eq!(cache.inner.calls.get(), 2);
        assert!(!cache.dir.exists());
    }

    #[test]
    fn prune_leaves_a_tools_subdirectory_intact() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok(turn("answer"))]);
        let tools_dir = cache.dir.join("tools");
        fs::create_dir_all(&tools_dir).unwrap();
        let catalog = tools_dir.join("catalog.json");
        fs::write(&catalog, r#"{"keep":true}"#).unwrap();
        cache.chat("m", &messages(), &tools()).unwrap();
        assert!(
            catalog.exists(),
            "the tools/ subdirectory must survive prune_expired"
        );
    }

    #[test]
    fn a_model_error_is_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(
            &dir,
            vec![Err(LlmError("boom".into())), Ok(turn("recovered"))],
        );
        assert_eq!(
            cache.chat("m", &messages(), &[]).unwrap_err(),
            LlmError("boom".into())
        );
        assert_eq!(
            cache.chat("m", &messages(), &[]).unwrap().content,
            "recovered"
        );
        assert_eq!(cache.inner.calls.get(), 2);
    }
}

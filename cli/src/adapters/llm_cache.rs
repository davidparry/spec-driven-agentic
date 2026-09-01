//! A disk-backed response cache decorating any [`LlmGenerator`]: identical
//! requests within the TTL are answered from `.bdd-cache/` without calling
//! the model at all. Disk rather than memory because every `bdd` command
//! runs in a fresh process - an in-process map would never see a repeat.
//! Cache trouble never fails a generation; the worst case is a miss.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::ports::{LlmError, LlmGenerator};

/// Part of every cache key. Bump it whenever the key recipe or entry
/// layout changes, or after pulling new model data behind an unchanged
/// tag such as `:latest` - old entries then simply stop matching.
pub const SCHEMA_VERSION: &str = "ollama-response:v1";

/// `cache_ttl_seconds` under `[llm]` in `.bdd-mcp.toml` overrides this.
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(600);

/// One completed answer on disk. `created_at` is wall-clock unix seconds,
/// not a monotonic instant, because entries outlive the process.
#[derive(Serialize, Deserialize)]
struct CacheEntry {
    content: String,
    created_at: u64,
}

/// Decorates an inner generator with a look-aside cache. A zero TTL
/// disables caching entirely - every call goes straight through.
pub struct CachedGenerator<G> {
    inner: G,
    dir: PathBuf,
    ttl: Duration,
    /// Everything besides (model, system, user) that can change the
    /// answer and so must be part of the key: the provider endpoint and
    /// the generation options.
    context: String,
}

impl<G> CachedGenerator<G> {
    pub fn new(inner: G, dir: PathBuf, ttl: Duration, context: String) -> Self {
        Self {
            inner,
            dir,
            ttl,
            context,
        }
    }

    /// SHA-256 over the length-prefixed inputs, so `("ab", "c")` and
    /// `("a", "bc")` cannot collide.
    fn key(&self, model: &str, system: &str, user: &str) -> String {
        let mut hasher = Sha256::new();
        for part in [SCHEMA_VERSION, &self.context, model, system, user] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// The cached answer for `key`, when a fresh one exists. Expired and
    /// unreadable entries are removed and reported as misses.
    fn lookup(&self, key: &str) -> Option<String> {
        let path = self.entry_path(key);
        let text = fs::read_to_string(&path).ok()?;
        let Ok(entry) = serde_json::from_str::<CacheEntry>(&text) else {
            debug!(key = &key[..12], "llm cache entry corrupt, removed");
            let _ = fs::remove_file(&path);
            return None;
        };
        if expired(&entry, self.ttl) {
            debug!(key = &key[..12], "llm cache entry expired, removed");
            let _ = fs::remove_file(&path);
            return None;
        }
        Some(entry.content)
    }

    /// Best effort: a full disk or unwritable directory costs the next
    /// call a model round-trip, never the current answer.
    fn store(&self, key: &str, content: &str) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        self.prune_expired();
        let entry = CacheEntry {
            content: content.to_string(),
            created_at: now(),
        };
        let rendered =
            serde_json::to_string(&entry).expect("a string-and-integer struct always renders");
        let _ = fs::write(self.entry_path(key), rendered);
    }

    /// Keeps the directory bounded: every write sweeps entries that can
    /// never be served again.
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
            debug!(pruned, "llm cache swept expired entries");
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

impl<G: LlmGenerator> LlmGenerator for CachedGenerator<G> {
    fn generate(&self, model: &str, system: &str, user: &str) -> Result<String, LlmError> {
        if self.ttl.is_zero() {
            debug!(model, "llm cache disabled (zero TTL)");
            return self.inner.generate(model, system, user);
        }
        let key = self.key(model, system, user);
        if let Some(content) = self.lookup(&key) {
            debug!(model, key = &key[..12], "llm cache hit");
            return Ok(content);
        }
        debug!(model, key = &key[..12], "llm cache miss");
        let content = self.inner.generate(model, system, user)?;
        self.store(&key, &content);
        debug!(key = &key[..12], chars = content.len(), "llm cache stored");
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// Counts how often the model is really called; caching is exactly
    /// the claim that this number stays lower than the call count.
    struct CountingLlm {
        calls: Cell<usize>,
        script: RefCell<Vec<Result<String, LlmError>>>,
    }

    impl CountingLlm {
        fn answering(results: Vec<Result<String, LlmError>>) -> Self {
            Self {
                calls: Cell::new(0),
                script: RefCell::new(results),
            }
        }
    }

    impl LlmGenerator for CountingLlm {
        fn generate(&self, _model: &str, _system: &str, _user: &str) -> Result<String, LlmError> {
            self.calls.set(self.calls.get() + 1);
            self.script.borrow_mut().remove(0)
        }
    }

    fn cached_in(
        dir: &tempfile::TempDir,
        results: Vec<Result<String, LlmError>>,
    ) -> CachedGenerator<CountingLlm> {
        CachedGenerator::new(
            CountingLlm::answering(results),
            dir.path().join("cache"),
            DEFAULT_CACHE_TTL,
            "http://localhost:11434".into(),
        )
    }

    #[test]
    fn an_identical_repeat_is_served_from_the_cache_without_the_model() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok("answer".into())]);
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "answer");
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "answer");
        assert_eq!(cache.inner.calls.get(), 1, "second call must be a hit");
    }

    #[test]
    fn the_cache_survives_a_new_process_over_the_same_directory() {
        let dir = tempfile::tempdir().unwrap();
        cached_in(&dir, vec![Ok("answer".into())])
            .generate("m", "s", "u")
            .unwrap();
        // A fresh decorator models a fresh CLI invocation.
        let second = cached_in(&dir, vec![]);
        assert_eq!(second.generate("m", "s", "u").unwrap(), "answer");
        assert_eq!(second.inner.calls.get(), 0);
    }

    #[test]
    fn changing_any_input_misses_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(
            &dir,
            vec![
                Ok("a".into()),
                Ok("b".into()),
                Ok("c".into()),
                Ok("d".into()),
            ],
        );
        cache.generate("m", "s", "u").unwrap();
        cache.generate("other-model", "s", "u").unwrap();
        cache.generate("m", "other system", "u").unwrap();
        cache.generate("m", "s", "other user").unwrap();
        assert_eq!(cache.inner.calls.get(), 4, "each variation is its own key");
    }

    #[test]
    fn a_changed_context_or_schema_version_is_a_different_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![]);
        let key = cache.key("m", "s", "u");
        assert_eq!(key, cache.key("m", "s", "u"), "the key is deterministic");
        let other_context = CachedGenerator::new(
            CountingLlm::answering(vec![]),
            dir.path().join("cache"),
            DEFAULT_CACHE_TTL,
            "http://box:11434".into(),
        );
        assert_ne!(key, other_context.key("m", "s", "u"));
        // Length-prefixing means shifting a character between fields
        // cannot produce the same key.
        assert_ne!(cache.key("ms", "", "u"), cache.key("m", "s", "u"));
    }

    #[test]
    fn an_expired_entry_is_a_miss_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok("fresh".into())]);
        let key = cache.key("m", "s", "u");
        fs::create_dir_all(&cache.dir).unwrap();
        let old = CacheEntry {
            content: "stale".into(),
            created_at: now() - DEFAULT_CACHE_TTL.as_secs() - 1,
        };
        fs::write(cache.entry_path(&key), serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "fresh");
        assert_eq!(cache.inner.calls.get(), 1, "expiry means the model runs");
    }

    #[test]
    fn a_model_error_is_not_cached_and_the_next_call_retries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(
            &dir,
            vec![Err(LlmError("boom".into())), Ok("recovered".into())],
        );
        assert_eq!(
            cache.generate("m", "s", "u").unwrap_err(),
            LlmError("boom".into())
        );
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "recovered");
        assert_eq!(cache.inner.calls.get(), 2);
    }

    #[test]
    fn a_corrupt_entry_is_a_miss_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok("regenerated".into())]);
        let key = cache.key("m", "s", "u");
        fs::create_dir_all(&cache.dir).unwrap();
        fs::write(cache.entry_path(&key), "not json").unwrap();
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "regenerated");
        assert_eq!(cache.inner.calls.get(), 1);
    }

    #[test]
    fn a_zero_ttl_disables_caching_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CachedGenerator::new(
            CountingLlm::answering(vec![Ok("a".into()), Ok("b".into())]),
            dir.path().join("cache"),
            Duration::ZERO,
            String::new(),
        );
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "a");
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "b");
        assert_eq!(cache.inner.calls.get(), 2);
        assert!(!cache.dir.exists(), "nothing is written when disabled");
    }

    #[test]
    fn writes_prune_entries_that_can_never_be_served_again() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok("answer".into())]);
        fs::create_dir_all(&cache.dir).unwrap();
        let stale_path = cache.dir.join("deadbeef.json");
        let stale = CacheEntry {
            content: "stale".into(),
            created_at: now() - DEFAULT_CACHE_TTL.as_secs() - 1,
        };
        fs::write(&stale_path, serde_json::to_string(&stale).unwrap()).unwrap();
        cache.generate("m", "s", "u").unwrap();
        assert!(!stale_path.exists(), "the expired entry was swept");
        assert!(cache.entry_path(&cache.key("m", "s", "u")).exists());
    }

    #[test]
    fn store_swallows_an_unwritable_cache_directory() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        fs::write(&blocker, "x").unwrap();
        let cache = CachedGenerator::new(
            CountingLlm::answering(vec![Ok("a".into()), Ok("b".into())]),
            blocker.join("cache"),
            DEFAULT_CACHE_TTL,
            String::new(),
        );
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "a");
        assert_eq!(cache.generate("m", "s", "u").unwrap(), "b");
        assert_eq!(cache.inner.calls.get(), 2);
    }

    #[test]
    fn prune_skips_non_json_and_unreadable_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cached_in(&dir, vec![Ok("answer".into())]);
        fs::create_dir_all(&cache.dir).unwrap();
        fs::write(cache.dir.join("notes.txt"), "ignore me").unwrap();
        fs::create_dir_all(cache.dir.join("nested.json")).unwrap();
        cache.generate("m", "s", "u").unwrap();
        assert!(cache.entry_path(&cache.key("m", "s", "u")).exists());
    }
}

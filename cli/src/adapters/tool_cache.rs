//! Disk look-aside for the external MCP tool catalog. Lives in
//! `.bdd-cache/tools/` so the response-cache prune cannot delete it.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::domain::mcp_registry::ServerSpec;
use crate::domain::tools::ToolDefinition;
use crate::ports::{ToolDiscovery, ToolError};

pub const SCHEMA_VERSION: &str = "mcp-tools:v1";
pub const DEFAULT_TTL: Duration = Duration::from_secs(86_400);

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    tools: Vec<ToolDefinition>,
    created_at: u64,
}

pub struct CachedDiscovery<D> {
    inner: D,
    dir: PathBuf,
    ttl: Duration,
}

impl<D> CachedDiscovery<D> {
    pub fn new(inner: D, dir: PathBuf, ttl: Duration) -> Self {
        Self { inner, dir, ttl }
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn key(&self, server: &ServerSpec) -> String {
        let mut hasher = Sha256::new();
        for part in [
            SCHEMA_VERSION,
            server.name.as_str(),
            server.program.as_str(),
        ] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        let args = serde_json::to_string(&server.args).unwrap_or_default();
        hasher.update((args.len() as u64).to_be_bytes());
        hasher.update(args.as_bytes());
        let env = serde_json::to_string(&server.env).unwrap_or_default();
        hasher.update((env.len() as u64).to_be_bytes());
        hasher.update(env.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn lookup(&self, key: &str) -> Option<Vec<ToolDefinition>> {
        if self.ttl.is_zero() {
            return None;
        }
        let path = self.entry_path(key);
        let text = fs::read_to_string(&path).ok()?;
        let Ok(entry) = serde_json::from_str::<CacheEntry>(&text) else {
            let _ = fs::remove_file(&path);
            return None;
        };
        if now() >= entry.created_at.saturating_add(self.ttl.as_secs()) {
            let _ = fs::remove_file(&path);
            return None;
        }
        Some(entry.tools)
    }

    fn store(&self, key: &str, tools: &[ToolDefinition]) {
        if self.ttl.is_zero() {
            return;
        }
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let entry = CacheEntry {
            tools: tools.to_vec(),
            created_at: now(),
        };
        let rendered = serde_json::to_string(&entry).expect("entry serializes");
        let _ = fs::write(self.entry_path(key), rendered);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl<D: ToolDiscovery> ToolDiscovery for CachedDiscovery<D> {
    fn discover(&self, server: &ServerSpec) -> Result<Vec<ToolDefinition>, ToolError> {
        let key = self.key(server);
        if let Some(tools) = self.lookup(&key) {
            debug!(server = %server.name, "tool catalog cache hit");
            return Ok(tools);
        }
        debug!(server = %server.name, "tool catalog cache miss");
        let tools = self.inner.discover(server)?;
        self.store(&key, &tools);
        Ok(tools)
    }

    fn discover_fresh(&self, server: &ServerSpec) -> Result<Vec<ToolDefinition>, ToolError> {
        let tools = self.inner.discover(server)?;
        self.store(&self.key(server), &tools);
        Ok(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolDefinition, ToolOrigin};
    use std::cell::Cell;

    struct CountingDiscovery {
        calls: Cell<usize>,
        tools: Vec<ToolDefinition>,
    }

    impl ToolDiscovery for CountingDiscovery {
        fn discover(&self, _server: &ServerSpec) -> Result<Vec<ToolDefinition>, ToolError> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.tools.clone())
        }
    }

    fn server() -> ServerSpec {
        ServerSpec {
            name: "self".into(),
            program: "bdd".into(),
            args: vec!["mcp".into(), "serve".into()],
            env: vec![],
        }
    }

    fn sample() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "self__list_requirements".into(),
            description: "list".into(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Server("self".into()),
        }]
    }

    #[test]
    fn a_repeat_discover_is_a_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CachedDiscovery::new(
            CountingDiscovery {
                calls: Cell::new(0),
                tools: sample(),
            },
            dir.path().join("tools"),
            DEFAULT_TTL,
        );
        assert_eq!(cache.discover(&server()).unwrap().len(), 1);
        assert_eq!(cache.discover(&server()).unwrap().len(), 1);
        assert_eq!(cache.inner.calls.get(), 1);
    }

    #[test]
    fn zero_ttl_disables_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CachedDiscovery::new(
            CountingDiscovery {
                calls: Cell::new(0),
                tools: sample(),
            },
            dir.path().join("tools"),
            Duration::ZERO,
        );
        cache.discover(&server()).unwrap();
        cache.discover(&server()).unwrap();
        assert_eq!(cache.inner.calls.get(), 2);
    }

    #[test]
    fn discover_fresh_rewrites_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CachedDiscovery::new(
            CountingDiscovery {
                calls: Cell::new(0),
                tools: sample(),
            },
            dir.path().join("tools"),
            DEFAULT_TTL,
        );
        cache.discover(&server()).unwrap();
        cache.discover_fresh(&server()).unwrap();
        assert_eq!(cache.inner.calls.get(), 2);
    }

    #[test]
    fn prune_of_the_response_cache_leaves_tools_intact() {
        let dir = tempfile::tempdir().unwrap();
        let cache_root = dir.path().join(".bdd-cache");
        fs::create_dir_all(cache_root.join("tools")).unwrap();
        let catalog = cache_root.join("tools").join("keep.json");
        fs::write(&catalog, "{}").unwrap();
        // Mimic CachedConversation::prune_expired: non-recursive *.json only.
        for file in fs::read_dir(&cache_root).unwrap().flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let _ = fs::remove_file(&path);
        }
        assert!(catalog.exists());
    }
}

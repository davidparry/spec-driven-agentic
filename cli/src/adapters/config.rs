//! TOML configuration adapter: the `[llm]` block of `.bdd-mcp.toml`
//! holds the persisted model choice and the provider endpoint.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::tool_profile::ProfileOverrides;
use crate::ports::{LlmError, ModelStore, ToolError, ToolStore};

pub struct TomlModelStore {
    config_file: PathBuf,
}

/// Shared TOML read so model and tool stores keep unrelated tables.
pub fn read_table(path: &Path) -> Option<toml::Table> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            tracing::debug!(error = %e, "config file not readable, using defaults");
            return None;
        }
    };
    match text.parse::<toml::Table>() {
        Ok(table) => Some(table),
        Err(e) => {
            tracing::debug!(error = %e, "config file is not valid TOML, using defaults");
            None
        }
    }
}

pub struct ToolsSettings {
    pub max_rounds: u32,
    pub confirm: Vec<String>,
    pub discovery_timeout: Duration,
    pub call_timeout: Duration,
    pub cache_ttl: Duration,
    pub mcp_config: Option<String>,
}

impl Default for ToolsSettings {
    fn default() -> Self {
        Self {
            max_rounds: 12,
            confirm: vec!["command_run".into()],
            discovery_timeout: Duration::from_secs(10),
            call_timeout: Duration::from_secs(300),
            cache_ttl: Duration::from_secs(86_400),
            mcp_config: None,
        }
    }
}

pub fn tools_settings(path: &Path) -> ToolsSettings {
    let Some(table) = read_table(path) else {
        return ToolsSettings::default();
    };
    let Some(tools) = table.get("tools").and_then(|v| v.as_table()) else {
        return ToolsSettings::default();
    };
    let mut settings = ToolsSettings::default();
    if let Some(rounds) = tools
        .get("max_rounds")
        .and_then(toml::Value::as_integer)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
    {
        settings.max_rounds = rounds;
    }
    if let Some(names) = tools.get("confirm").and_then(|v| v.as_array()) {
        settings.confirm = names
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(seconds) = tools
        .get("discovery_timeout_seconds")
        .and_then(toml::Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
    {
        settings.discovery_timeout = Duration::from_secs(seconds);
    }
    if let Some(seconds) = tools
        .get("call_timeout_seconds")
        .and_then(toml::Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
    {
        settings.call_timeout = Duration::from_secs(seconds);
    }
    if let Some(seconds) = tools
        .get("cache_ttl_seconds")
        .and_then(toml::Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
    {
        settings.cache_ttl = Duration::from_secs(seconds);
    }
    if let Some(path) = tools.get("mcp_config").and_then(|v| v.as_str()) {
        settings.mcp_config = Some(path.to_string());
    }
    settings
}

pub struct TomlToolStore {
    config_file: PathBuf,
}

impl TomlToolStore {
    pub fn new(config_file: PathBuf) -> Self {
        Self { config_file }
    }
}

fn string_list_table(
    table: &toml::Table,
    key: &str,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let Some(nested) = table.get(key).and_then(|v| v.as_table()) else {
        return std::collections::BTreeMap::new();
    };
    nested
        .iter()
        .filter_map(|(caller, value)| {
            let names = value
                .as_array()?
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect();
            Some((caller.clone(), names))
        })
        .collect()
}

impl ToolStore for TomlToolStore {
    fn overrides(&self) -> ProfileOverrides {
        let Some(table) = read_table(&self.config_file) else {
            return ProfileOverrides::default();
        };
        let Some(tools) = table.get("tools").and_then(|v| v.as_table()) else {
            return ProfileOverrides::default();
        };
        ProfileOverrides {
            replace: string_list_table(tools, "profiles"),
            attached: string_list_table(tools, "enabled"),
            removed: string_list_table(tools, "disabled"),
        }
    }

    fn attach(&self, caller: &str, tool: &str) -> Result<(), ToolError> {
        self.mutate_list("enabled", caller, |names| {
            if !names.iter().any(|existing| existing == tool) {
                names.push(tool.to_string());
            }
        })
    }

    fn detach(&self, caller: &str, tool: &str) -> Result<(), ToolError> {
        self.mutate_list("disabled", caller, |names| {
            if !names.iter().any(|existing| existing == tool) {
                names.push(tool.to_string());
            }
        })?;
        // Also drop from enabled so a later enable isn't immediately cancelled
        // only by the disabled table — attach already appends; disable records
        // removal which beats attachment at resolve time.
        Ok(())
    }
}

impl TomlToolStore {
    fn mutate_list(
        &self,
        table_name: &str,
        caller: &str,
        mutate: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), ToolError> {
        let mut table = read_table(&self.config_file).unwrap_or_default();
        let tools = table
            .entry("tools".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let tools = tools
            .as_table_mut()
            .ok_or_else(|| ToolError("config: [tools] is not a table".into()))?;
        let nested = tools
            .entry(table_name.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let nested = nested
            .as_table_mut()
            .ok_or_else(|| ToolError(format!("config: [tools.{table_name}] is not a table")))?;
        let mut names: Vec<String> = nested
            .get(caller)
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        mutate(&mut names);
        nested.insert(
            caller.to_string(),
            toml::Value::Array(names.into_iter().map(toml::Value::String).collect()),
        );
        let rendered = toml::to_string_pretty(&table).expect("a plain TOML table always renders");
        fs::write(&self.config_file, rendered).map_err(|e| {
            ToolError(format!(
                "config: cannot write {} - {e}",
                self.config_file.display()
            ))
        })
    }
}

impl TomlModelStore {
    pub fn new(config_file: PathBuf) -> Self {
        Self { config_file }
    }

    /// The configured provider endpoint, when one is set.
    pub fn endpoint(&self) -> Option<String> {
        let endpoint = self.llm_key("endpoint");
        tracing::debug!(endpoint = ?endpoint, "config: llm endpoint");
        endpoint
    }

    /// The configured generation timeout in seconds, when one is set.
    /// Large prompts on local models can outlast the default.
    pub fn timeout_seconds(&self) -> Option<u64> {
        let seconds = self.llm_seconds("timeout_seconds");
        tracing::debug!(timeout_seconds = ?seconds, "config: llm timeout");
        seconds
    }

    /// How many times a model call is tried when the reply fails
    /// validation. `0` is treated as missing so the default applies.
    pub fn retry(&self) -> Option<u64> {
        let retries = self.llm_seconds("retry");
        tracing::debug!(retry = ?retries, "config: llm retry attempts");
        retries.filter(|n| *n > 0)
    }

    /// How long cached model responses stay valid, when configured.
    /// `0` disables the response cache entirely.
    pub fn cache_ttl_seconds(&self) -> Option<u64> {
        let seconds = self.llm_seconds("cache_ttl_seconds");
        tracing::debug!(cache_ttl_seconds = ?seconds, "config: llm cache TTL");
        seconds
    }

    fn llm_seconds(&self, key: &str) -> Option<u64> {
        let table = self.read_table()?;
        table
            .get("llm")
            .and_then(|llm| llm.get(key))
            .and_then(toml::Value::as_integer)
            .and_then(|seconds| u64::try_from(seconds).ok())
    }

    fn llm_key(&self, key: &str) -> Option<String> {
        let table = self.read_table()?;
        table
            .get("llm")
            .and_then(|llm| llm.get(key))
            .and_then(|value| value.as_str())
            .map(String::from)
    }

    fn read_table(&self) -> Option<toml::Table> {
        read_table(&self.config_file)
    }
}

impl ModelStore for TomlModelStore {
    fn configured(&self) -> Option<String> {
        let model = self.llm_key("model");
        tracing::debug!(model = ?model, "config: configured model");
        model
    }

    fn persist(&self, model: &str) -> Result<(), LlmError> {
        tracing::debug!(model, file = %self.config_file.display(), "config: persisting model");
        let mut table = self.read_table().unwrap_or_default();
        let llm = table
            .entry("llm".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let llm_table = llm
            .as_table_mut()
            .ok_or_else(|| LlmError("config: [llm] is not a table".into()))?;
        llm_table.insert("model".to_string(), toml::Value::String(model.to_string()));
        let rendered = toml::to_string_pretty(&table).expect("a plain TOML table always renders");
        fs::write(&self.config_file, rendered).map_err(|e| {
            LlmError(format!(
                "config: cannot write {} - {e}",
                self.config_file.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &tempfile::TempDir) -> TomlModelStore {
        TomlModelStore::new(dir.path().join(".bdd-mcp.toml"))
    }

    #[test]
    fn a_missing_config_file_means_nothing_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        assert_eq!(store.configured(), None);
        assert_eq!(store.endpoint(), None);
        assert_eq!(store.timeout_seconds(), None);
        assert_eq!(store.cache_ttl_seconds(), None);
        assert_eq!(store.retry(), None);
    }

    #[test]
    fn an_unparseable_config_file_means_nothing_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(&path, "not = = toml").unwrap();
        let store = TomlModelStore::new(path);
        assert_eq!(store.configured(), None);
        assert_eq!(store.endpoint(), None);
    }

    #[test]
    fn a_configured_cache_ttl_is_read_including_the_disabling_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(&path, "[llm]\ncache_ttl_seconds = 3600\n").unwrap();
        assert_eq!(
            TomlModelStore::new(path.clone()).cache_ttl_seconds(),
            Some(3600)
        );
        fs::write(&path, "[llm]\ncache_ttl_seconds = 0\n").unwrap();
        assert_eq!(
            TomlModelStore::new(path.clone()).cache_ttl_seconds(),
            Some(0)
        );
        fs::write(&path, "[llm]\ncache_ttl_seconds = -1\n").unwrap();
        assert_eq!(TomlModelStore::new(path).cache_ttl_seconds(), None);
    }

    #[test]
    fn a_configured_timeout_is_read_and_junk_values_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(&path, "[llm]\ntimeout_seconds = 600\n").unwrap();
        assert_eq!(
            TomlModelStore::new(path.clone()).timeout_seconds(),
            Some(600)
        );
        fs::write(&path, "[llm]\ntimeout_seconds = \"soon\"\n").unwrap();
        assert_eq!(TomlModelStore::new(path.clone()).timeout_seconds(), None);
        fs::write(&path, "[llm]\ntimeout_seconds = -5\n").unwrap();
        assert_eq!(TomlModelStore::new(path).timeout_seconds(), None);
    }

    #[test]
    fn a_configured_retry_is_read_and_zero_or_junk_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(&path, "[llm]\nretry = 5\n").unwrap();
        assert_eq!(TomlModelStore::new(path.clone()).retry(), Some(5));
        fs::write(&path, "[llm]\nretry = 0\n").unwrap();
        assert_eq!(TomlModelStore::new(path.clone()).retry(), None);
        fs::write(&path, "[llm]\nretry = -1\n").unwrap();
        assert_eq!(TomlModelStore::new(path).retry(), None);
    }

    #[test]
    fn persist_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        store.persist("llama3:latest").unwrap();
        assert_eq!(store.configured(), Some("llama3:latest".to_string()));
    }

    #[test]
    fn persist_rejects_a_config_where_llm_is_not_a_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(&path, "llm = \"not a table\"\n").unwrap();
        let error = TomlModelStore::new(path).persist("qwen3:8b").unwrap_err();
        assert_eq!(error, LlmError("config: [llm] is not a table".into()));
    }

    #[test]
    fn persist_reports_an_unwritable_location() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-dir").join(".bdd-mcp.toml");
        let error = TomlModelStore::new(path).persist("qwen3:8b").unwrap_err();
        assert!(
            error.0.starts_with("config: cannot write"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn persist_preserves_unrelated_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(
            &path,
            "[llm]\nendpoint = \"http://box:11434\"\n\n[policy]\nstrict = true\n",
        )
        .unwrap();
        let store = TomlModelStore::new(path.clone());
        store.persist("qwen3:8b").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("endpoint"), "endpoint kept: {content}");
        assert!(content.contains("strict"), "policy kept: {content}");
        assert_eq!(store.configured(), Some("qwen3:8b".to_string()));
        assert_eq!(store.endpoint(), Some("http://box:11434".to_string()));
    }

    #[test]
    fn tools_tables_round_trip_without_touching_llm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(&path, "[llm]\nmodel = \"keep-me\"\n").unwrap();
        let store = TomlToolStore::new(path.clone());
        store
            .attach("implement", "playwright__browser_navigate")
            .unwrap();
        store.detach("implement", "command_run").unwrap();
        let overrides = store.overrides();
        assert_eq!(
            overrides.attached.get("implement").unwrap(),
            &vec!["playwright__browser_navigate".to_string()]
        );
        assert_eq!(
            overrides.removed.get("implement").unwrap(),
            &vec!["command_run".to_string()]
        );
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("keep-me"), "{content}");
        let settings = tools_settings(&path);
        assert_eq!(settings.max_rounds, 12);
        assert_eq!(settings.confirm, vec!["command_run"]);
    }

    #[test]
    fn tools_settings_read_scalars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bdd-mcp.toml");
        fs::write(
            &path,
            "[tools]\nmax_rounds = 4\nconfirm = [\"run_tests\"]\ndiscovery_timeout_seconds = 2\ncall_timeout_seconds = 9\ncache_ttl_seconds = 0\nmcp_config = \"mine.json\"\n",
        )
        .unwrap();
        let settings = tools_settings(&path);
        assert_eq!(settings.max_rounds, 4);
        assert_eq!(settings.confirm, vec!["run_tests"]);
        assert_eq!(settings.discovery_timeout, Duration::from_secs(2));
        assert_eq!(settings.call_timeout, Duration::from_secs(9));
        assert_eq!(settings.cache_ttl, Duration::from_secs(0));
        assert_eq!(settings.mcp_config.as_deref(), Some("mine.json"));
    }
}

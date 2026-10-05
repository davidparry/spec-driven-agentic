//! TOML configuration adapter: the `[llm]` and `[tools]` blocks of
//! `.spec/config.toml` hold the persisted model choice, provider endpoint, and
//! per-command tool profiles.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapters::spec_home::{ensure_parent, spec_file};
use crate::domain::CONFIG_FILE;
use crate::domain::config_report::{
    ConfigFileStatus, ConfigReport, DEFAULT_DECISION_TIMEOUT_SECONDS, DEFAULT_LLM_ENDPOINT,
    DEFAULT_REFACTOR_ATTEMPTS, DEFAULT_TOOLS_CACHE_TTL_SECONDS, DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS,
    DEFAULT_TOOLS_CONFIRM, DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS, DEFAULT_TOOLS_MAX_ROUNDS,
    PresentValues, build_report,
};
use crate::domain::decision::{DEFAULT_MIN_CONFIDENCE, Mode, Policy};
use crate::domain::tool_profile::ProfileOverrides;
use crate::ports::{LlmError, ModelStore, ToolError, ToolStore};

/// `.spec/config.toml` under this project root. Missing files stay this
/// path so first writes create the current name.
pub fn config_path(root: &Path) -> PathBuf {
    spec_file(root, CONFIG_FILE)
}

enum ConfigLoad {
    Missing,
    Unreadable { path: String },
    Invalid { path: String },
    Table { path: String, table: toml::Table },
}

fn load_config(path: &Path) -> ConfigLoad {
    let shown = path.display().to_string();
    match fs::read_to_string(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => ConfigLoad::Missing,
        Err(error) => {
            tracing::debug!(
                error = %error,
                path = %shown,
                "config file not readable, using defaults"
            );
            ConfigLoad::Unreadable { path: shown }
        }
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(table) => ConfigLoad::Table { path: shown, table },
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    path = %shown,
                    "config file is not valid TOML, using defaults"
                );
                ConfigLoad::Invalid { path: shown }
            }
        },
    }
}

/// Shared TOML read so model and tool stores keep unrelated tables.
pub fn read_table(path: &Path) -> Option<toml::Table> {
    match load_config(path) {
        ConfigLoad::Table { table, .. } => Some(table),
        _ => None,
    }
}

/// Effective configuration for `spec config`: each key and whether it is
/// a code default or was read from this path.
pub fn inspect_config(path: &Path) -> ConfigReport {
    match load_config(path) {
        ConfigLoad::Missing => build_report(ConfigFileStatus::Missing, PresentValues::default()),
        ConfigLoad::Unreadable { path } => build_report(
            ConfigFileStatus::Unreadable { path },
            PresentValues::default(),
        ),
        ConfigLoad::Invalid { path } => {
            build_report(ConfigFileStatus::Invalid { path }, PresentValues::default())
        }
        ConfigLoad::Table { path, table } => build_report(
            ConfigFileStatus::Present { path },
            present_from_table(&table),
        ),
    }
}

fn present_from_table(table: &toml::Table) -> PresentValues {
    let mut present = PresentValues::default();
    if let Some(llm) = table.get("llm").and_then(|v| v.as_table()) {
        present.model = toml_string(llm, "model");
        present.endpoint = toml_string(llm, "endpoint");
        present.timeout_seconds = toml_u64(llm, "timeout_seconds");
        present.cache_ttl_seconds = toml_u64(llm, "cache_ttl_seconds");
        present.retry = toml_u64(llm, "retry").filter(|n| *n > 0);
    }
    if let Some(tools) = table.get("tools").and_then(|v| v.as_table()) {
        present.max_rounds = toml_u32(tools, "max_rounds").filter(|n| *n > 0);
        present.confirm = toml_string_array(tools, "confirm");
        present.discovery_timeout_seconds = toml_u64(tools, "discovery_timeout_seconds");
        present.call_timeout_seconds = toml_u64(tools, "call_timeout_seconds");
        present.tools_cache_ttl_seconds = toml_u64(tools, "cache_ttl_seconds");
        present.mcp_config = toml_string(tools, "mcp_config");
        present.profiles = string_list_table(tools, "profiles");
        present.enabled = string_list_table(tools, "enabled");
        present.disabled = string_list_table(tools, "disabled");
    }
    if let Some(refactor) = table.get("refactor").and_then(|v| v.as_table()) {
        present.refactor_attempts = toml_u32(refactor, "attempts").filter(|n| *n > 0);
    }
    if let Some(decision) = table.get("decision").and_then(|v| v.as_table()) {
        present.decision_model = toml_string(decision, "model");
        present.decision_endpoint = toml_string(decision, "endpoint");
        present.decision_timeout_seconds = toml_u64(decision, "timeout_seconds");
        // An unspellable mode is no mode, so a typo takes the default -
        // which is `enforce`. That is the safe direction: a misspelled
        // `advisory` refuses work until it is noticed, where the old
        // behaviour would have quietly stopped gating.
        present.decision_mode = toml_string(decision, "mode")
            .filter(|raw| crate::domain::decision::Mode::parse(raw).is_some());
        present.decision_min_confidence =
            toml_f64(decision, "min_confidence").filter(|n| (0.0..=1.0).contains(n));
    }
    present
}

fn toml_string(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn toml_string_array(table: &toml::Table, key: &str) -> Option<Vec<String>> {
    table.get(key).and_then(|v| v.as_array()).map(|names| {
        names
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    })
}

fn toml_u64(table: &toml::Table, key: &str) -> Option<u64> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
}

fn toml_u32(table: &toml::Table, key: &str) -> Option<u32> {
    toml_u64(table, key).and_then(|n| u32::try_from(n).ok())
}

/// A threshold reads as a float, and `min_confidence = 1` is a
/// perfectly reasonable thing to write for an integer value.
fn toml_f64(table: &toml::Table, key: &str) -> Option<f64> {
    let value = table.get(key)?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
}

pub struct TomlModelStore {
    config_file: PathBuf,
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
            max_rounds: DEFAULT_TOOLS_MAX_ROUNDS,
            confirm: DEFAULT_TOOLS_CONFIRM
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            discovery_timeout: Duration::from_secs(DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS),
            call_timeout: Duration::from_secs(DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS),
            cache_ttl: Duration::from_secs(DEFAULT_TOOLS_CACHE_TTL_SECONDS),
            mcp_config: None,
        }
    }
}

impl ToolsSettings {
    fn from_present(present: PresentValues) -> Self {
        let mut settings = Self::default();
        if let Some(rounds) = present.max_rounds {
            settings.max_rounds = rounds;
        }
        if let Some(confirm) = present.confirm {
            settings.confirm = confirm;
        }
        if let Some(seconds) = present.discovery_timeout_seconds {
            settings.discovery_timeout = Duration::from_secs(seconds);
        }
        if let Some(seconds) = present.call_timeout_seconds {
            settings.call_timeout = Duration::from_secs(seconds);
        }
        if let Some(seconds) = present.tools_cache_ttl_seconds {
            settings.cache_ttl = Duration::from_secs(seconds);
        }
        if let Some(path) = present.mcp_config {
            settings.mcp_config = Some(path);
        }
        settings
    }
}

pub fn tools_settings(path: &Path) -> ToolsSettings {
    ToolsSettings::from_present(present_values(path))
}

/// The resolved `[decision]` block: which local model judges, where it
/// lives, how long it gets, and the policy its answers are read against.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionSettings {
    /// `None` when no decision model is configured, which is the
    /// shipped state. Nothing asks a question until this is set.
    pub model: Option<String>,
    pub endpoint: String,
    pub timeout: Duration,
    pub policy: Policy,
}

/// The `[decision]` block as the harness will act on it.
///
/// The endpoint falls back to `[llm] endpoint` before the built-in
/// default, so a project that moved its Ollama says so once.
pub fn decision_settings(path: &Path) -> DecisionSettings {
    let present = present_values(path);
    let endpoint = present
        .decision_endpoint
        .clone()
        .or_else(|| present.endpoint.clone())
        .unwrap_or_else(|| DEFAULT_LLM_ENDPOINT.to_string());
    let mode = present
        .decision_mode
        .as_deref()
        .and_then(Mode::parse)
        .unwrap_or_default();
    let threshold = present
        .decision_min_confidence
        .unwrap_or(DEFAULT_MIN_CONFIDENCE);
    let settings = DecisionSettings {
        model: present.decision_model.clone(),
        endpoint,
        timeout: Duration::from_secs(
            present
                .decision_timeout_seconds
                .unwrap_or(DEFAULT_DECISION_TIMEOUT_SECONDS),
        ),
        policy: Policy::new(mode, threshold),
    };
    tracing::debug!(
        model = ?settings.model,
        endpoint = %settings.endpoint,
        mode = %settings.policy.mode,
        threshold = settings.policy.threshold,
        "config: decision settings"
    );
    settings
}

/// Persists the decision model choice under `[decision]`.
///
/// Its own store rather than a second method on [`TomlModelStore`]: the
/// two roles are different keys, and the one mistake worth making
/// impossible is a decision model landing in `llm.model`.
pub struct TomlDecisionStore {
    config_file: PathBuf,
}

impl TomlDecisionStore {
    pub fn new(config_file: PathBuf) -> Self {
        Self { config_file }
    }

    pub fn configured(&self) -> Option<String> {
        decision_settings(&self.config_file).model
    }

    /// Edit `[decision] model` and nothing else, on the same terms as
    /// `spec model use`: every comment in the file survives.
    pub fn persist(&self, model: &str) -> Result<(), LlmError> {
        tracing::debug!(model, file = %self.config_file.display(), "config: persisting decision model");
        if let Some(table) = read_table(&self.config_file)
            && let Some(decision) = table.get("decision")
            && !decision.is_table()
        {
            return Err(LlmError("config: [decision] is not a table".into()));
        }
        let current = fs::read_to_string(&self.config_file).unwrap_or_default();
        write_config(
            &self.config_file,
            &set_in_section(&current, "decision", "model", model),
        )
        .map_err(|e| {
            LlmError(format!(
                "config: cannot write {} - {e}",
                self.config_file.display()
            ))
        })
    }
}

/// `[refactor] attempts`, the write-then-test budget `spec refactor`
/// spends before restoring the code it started from. A missing, zero, or
/// junk value is the default - a budget of nothing would mean the loop
/// reverts without ever having tried.
pub fn refactor_attempts(path: &Path) -> u32 {
    present_values(path)
        .refactor_attempts
        .unwrap_or(DEFAULT_REFACTOR_ATTEMPTS)
}

fn present_values(path: &Path) -> PresentValues {
    match load_config(path) {
        ConfigLoad::Table { table, .. } => present_from_table(&table),
        _ => PresentValues::default(),
    }
}

pub struct TomlToolStore {
    config_file: PathBuf,
}

impl TomlToolStore {
    pub fn new(config_file: PathBuf) -> Self {
        Self { config_file }
    }
}

fn string_list_table(table: &toml::Table, key: &str) -> BTreeMap<String, Vec<String>> {
    let Some(nested) = table.get(key).and_then(|v| v.as_table()) else {
        return BTreeMap::new();
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
        let present = present_values(&self.config_file);
        ProfileOverrides {
            replace: present.profiles,
            attached: present.enabled,
            removed: present.disabled,
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
        // Recorded under [tools.disabled]; resolve treats removal as
        // beating both defaults and [tools.enabled].
        self.mutate_list("disabled", caller, |names| {
            if !names.iter().any(|existing| existing == tool) {
                names.push(tool.to_string());
            }
        })
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
        write_config(&self.config_file, &rendered).map_err(|e| {
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
        toml_u64(table.get("llm")?.as_table()?, key)
    }

    fn llm_key(&self, key: &str) -> Option<String> {
        let table = self.read_table()?;
        toml_string(table.get("llm")?.as_table()?, key)
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

    /// Edit the `model` key and nothing else.
    ///
    /// Re-rendering the parsed table used to rewrite the whole file:
    /// `spec model use` turned a 3322-byte `.spec/config.toml` into 1319 bytes
    /// and threw away every comment in it, including the block that
    /// documents the `server:tool` naming scheme and all the
    /// commented-out defaults. That is the first command of the
    /// workshop, so the first thing a student saw `spec` do was delete
    /// their configuration's documentation.
    fn persist(&self, model: &str) -> Result<(), LlmError> {
        tracing::debug!(model, file = %self.config_file.display(), "config: persisting model");
        // Parsed only to refuse a file the edit would corrupt; the
        // parse result is deliberately not what gets written back.
        if let Some(table) = self.read_table()
            && let Some(llm) = table.get("llm")
            && !llm.is_table()
        {
            return Err(LlmError("config: [llm] is not a table".into()));
        }
        let current = fs::read_to_string(&self.config_file).unwrap_or_default();
        write_config(&self.config_file, &set_model(&current, model)).map_err(|e| {
            LlmError(format!(
                "config: cannot write {} - {e}",
                self.config_file.display()
            ))
        })
    }
}

fn write_config(path: &Path, contents: &str) -> std::io::Result<()> {
    ensure_parent(path)?;
    fs::write(path, contents)
}

/// The file with its `[llm] model` line replaced, and every other byte
/// of it left exactly as the author wrote it.
fn set_model(current: &str, model: &str) -> String {
    set_in_section(current, "llm", "model", model)
}

/// The file with `[section] key` set to `value`, and every other byte of
/// it left exactly as the author wrote it - comments, key order, blank
/// lines and all.
///
/// A file with no `key` under the section gains one directly below the
/// header; a file with no section gains the section at the end. A
/// commented-out `# key = ...` is a comment, not the key, and is left
/// alone.
fn set_in_section(current: &str, section: &str, key: &str, value: &str) -> String {
    let assignment = format!("{key} = {}", toml::Value::String(value.to_string()));
    let mut lines: Vec<String> = current.lines().map(str::to_string).collect();
    match find_key_line(&lines, section, key) {
        Some(at) => {
            let indent = leading_space(&lines[at]).to_string();
            lines[at] = format!("{indent}{assignment}");
        }
        None => match find_header(&lines, section) {
            Some(at) => lines.insert(at + 1, assignment),
            None => {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!("[{section}]"));
                lines.push(assignment);
            }
        },
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The index of the line assigning `key` inside `[section]`, if the file
/// has one.
fn find_key_line(lines: &[String], section: &str, key: &str) -> Option<usize> {
    let header = format!("[{section}]");
    let mut inside = false;
    lines.iter().position(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == header;
        }
        inside && is_key_assignment(trimmed, key)
    })
}

/// The index of the `[section]` header line, if the file has one.
fn find_header(lines: &[String], section: &str) -> Option<usize> {
    let header = format!("[{section}]");
    lines.iter().position(|line| line.trim() == header)
}

fn leading_space(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Whether a line assigns `key`, rather than mentioning it in a comment
/// or being a longer key that starts with the same letters.
fn is_key_assignment(trimmed: &str, key: &str) -> bool {
    trimmed
        .strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CONFIG_FILE;

    fn store_in(dir: &tempfile::TempDir) -> TomlModelStore {
        TomlModelStore::new(dir.path().join(CONFIG_FILE))
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
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "not = = toml").unwrap();
        let store = TomlModelStore::new(path);
        assert_eq!(store.configured(), None);
        assert_eq!(store.endpoint(), None);
    }

    #[test]
    fn a_configured_cache_ttl_is_read_including_the_disabling_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
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
        let path = dir.path().join(CONFIG_FILE);
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
        let path = dir.path().join(CONFIG_FILE);
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
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "llm = \"not a table\"\n").unwrap();
        let error = TomlModelStore::new(path).persist("qwen3:8b").unwrap_err();
        assert_eq!(error, LlmError("config: [llm] is not a table".into()));
    }

    #[test]
    fn persist_reports_an_unwritable_location() {
        let path = PathBuf::from("/dev/null/nowhere").join(CONFIG_FILE);
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
        let path = dir.path().join(CONFIG_FILE);
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

    /// The shipped `.spec/config.toml` is mostly prose: a comment block
    /// explaining `server:tool`, and defaults left commented out so a
    /// student can see what is available. Choosing a model must not
    /// cost them that.
    #[test]
    fn choosing_a_model_changes_one_line_and_leaves_the_rest_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        let original = "# The model the harness talks to.\n\
                        [llm]\n\
                        model = \"old-model\"\n\
                        # endpoint = \"http://localhost:11434\"\n\
                        endpoint = \"http://box:11434\"\n\
                        \n\
                        # Tools are named server:tool.\n\
                        [tools]\n\
                        implement = [\"fs:read\"]\n";
        fs::write(&path, original).unwrap();
        TomlModelStore::new(path.clone())
            .persist("qwen3:8b")
            .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original.replace("model = \"old-model\"", "model = \"qwen3:8b\""),
        );
    }

    #[test]
    fn a_config_without_a_model_key_gains_one_under_the_existing_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "[llm]\n# model = \"commented-out\"\nretry = 2\n").unwrap();
        let store = TomlModelStore::new(path.clone());
        store.persist("qwen3:8b").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[llm]\nmodel = \"qwen3:8b\"\n# model = \"commented-out\"\nretry = 2\n"
        );
        assert_eq!(store.retry(), Some(2));
    }

    #[test]
    fn a_config_with_no_llm_section_gains_one_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "[policy]\nstrict = true\n").unwrap();
        let store = TomlModelStore::new(path.clone());
        store.persist("qwen3:8b").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[policy]\nstrict = true\n\n[llm]\nmodel = \"qwen3:8b\"\n"
        );
        assert_eq!(store.configured(), Some("qwen3:8b".to_string()));
    }

    /// A `model` key in some other section is not the one being set.
    #[test]
    fn a_model_key_in_another_section_is_left_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "[report]\nmodel = \"mine\"\n\n[llm]\nretry = 1\n").unwrap();
        let store = TomlModelStore::new(path.clone());
        store.persist("qwen3:8b").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[report]\nmodel = \"mine\"\n\n[llm]\nmodel = \"qwen3:8b\"\nretry = 1\n"
        );
        assert_eq!(store.configured(), Some("qwen3:8b".to_string()));
    }

    #[test]
    fn a_model_name_needing_quoting_is_written_as_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "[llm]\nmodel = \"old\"\n").unwrap();
        let store = TomlModelStore::new(path.clone());
        store.persist("weird\"name\\here").unwrap();
        assert_eq!(store.configured(), Some("weird\"name\\here".to_string()));
    }

    #[test]
    fn persisting_twice_does_not_accumulate_model_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "[llm]\nretry = 1\n").unwrap();
        let store = TomlModelStore::new(path.clone());
        store.persist("first").unwrap();
        store.persist("second").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("model =").count(), 1, "got: {content}");
        assert_eq!(store.configured(), Some("second".to_string()));
    }

    #[test]
    fn tools_tables_round_trip_without_touching_llm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
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
        assert_eq!(settings.max_rounds, DEFAULT_TOOLS_MAX_ROUNDS);
        assert_eq!(
            settings.confirm,
            DEFAULT_TOOLS_CONFIRM
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn tools_settings_read_scalars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
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

    #[test]
    fn a_configured_refactor_budget_is_read_and_zero_or_junk_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "[refactor]\nattempts = 3\n").unwrap();
        assert_eq!(refactor_attempts(&path), 3);
        // A budget of nothing would revert without ever having tried.
        fs::write(&path, "[refactor]\nattempts = 0\n").unwrap();
        assert_eq!(refactor_attempts(&path), DEFAULT_REFACTOR_ATTEMPTS);
        fs::write(&path, "[refactor]\nattempts = \"ten\"\n").unwrap();
        assert_eq!(refactor_attempts(&path), DEFAULT_REFACTOR_ATTEMPTS);
        fs::write(&path, "[llm]\nmodel = \"m\"\n").unwrap();
        assert_eq!(refactor_attempts(&path), DEFAULT_REFACTOR_ATTEMPTS);
        assert_eq!(
            refactor_attempts(&dir.path().join("no-such-file.toml")),
            DEFAULT_REFACTOR_ATTEMPTS
        );
    }

    #[test]
    fn config_path_is_always_inside_spec_home() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(config_path(root), spec_file(root, CONFIG_FILE));
        fs::write(root.join(".spec.toml"), "[llm]\nmodel = \"legacy\"\n").unwrap();
        assert_eq!(config_path(root), spec_file(root, CONFIG_FILE));
        fs::create_dir_all(spec_file(root, CONFIG_FILE).parent().unwrap()).unwrap();
        fs::write(spec_file(root, CONFIG_FILE), "[llm]\nmodel = \"new\"\n").unwrap();
        assert_eq!(config_path(root), spec_file(root, CONFIG_FILE));
    }

    #[test]
    fn a_profiles_list_replaces_defaults_including_qualified_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(
            &path,
            "[tools.profiles]\nstatus = [\"builtin:get_tdd_state\", \"self:validate_spec\"]\n",
        )
        .unwrap();
        let overrides = TomlToolStore::new(path).overrides();
        assert_eq!(
            overrides.replace.get("status").unwrap(),
            &vec![
                "builtin:get_tdd_state".to_string(),
                "self:validate_spec".to_string()
            ]
        );
    }

    #[test]
    fn inspect_config_attributes_file_keys_and_leaves_the_rest_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(
            &path,
            "[llm]\nmodel = \"mine\"\ntimeout_seconds = 900\n\n[tools]\nmax_rounds = 4\n[tools.profiles]\nstatus = [\"get_tdd_state\"]\n",
        )
        .unwrap();
        let report = inspect_config(&path);
        assert_eq!(report.file.path(), Some(path.to_str().unwrap()));
        let model = report.setting("llm.model").unwrap();
        assert_eq!(model.value, "mine");
        assert!(matches!(
            model.source,
            crate::domain::config_report::ConfigSource::File(_)
        ));
        assert_eq!(
            report.setting("llm.endpoint").unwrap().source,
            crate::domain::config_report::ConfigSource::Default
        );
        assert_eq!(report.setting("llm.timeout_seconds").unwrap().value, "900");
        assert_eq!(report.setting("tools.max_rounds").unwrap().value, "4");
        assert_eq!(
            report.setting("tools.profiles.status").unwrap().value,
            "get_tdd_state"
        );
        assert_eq!(
            report.setting("tools.profiles.implement").unwrap().source,
            crate::domain::config_report::ConfigSource::Default
        );
    }

    #[test]
    fn inspect_config_missing_file_is_all_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let report = inspect_config(&dir.path().join(CONFIG_FILE));
        assert_eq!(report.file, ConfigFileStatus::Missing);
        assert_eq!(
            report.setting("llm.endpoint").unwrap().value,
            crate::domain::config_report::DEFAULT_LLM_ENDPOINT
        );
    }

    #[test]
    fn inspect_config_invalid_toml_is_flagged_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, "not = = toml").unwrap();
        let report = inspect_config(&path);
        assert!(matches!(report.file, ConfigFileStatus::Invalid { .. }));
        assert_eq!(
            report.setting("llm.model").unwrap().source,
            crate::domain::config_report::ConfigSource::Default
        );
    }

    #[test]
    fn inspect_config_unreadable_path_is_flagged_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-a-file");
        fs::create_dir(&path).unwrap();
        let report = inspect_config(&path);
        assert!(matches!(report.file, ConfigFileStatus::Unreadable { .. }));
        assert_eq!(
            report.setting("llm.model").unwrap().source,
            crate::domain::config_report::ConfigSource::Default
        );
    }

    // ---- [decision] ----------------------------------------------------

    fn decision_store_in(dir: &tempfile::TempDir) -> TomlDecisionStore {
        TomlDecisionStore::new(dir.path().join(CONFIG_FILE))
    }

    #[test]
    fn a_persisted_decision_model_reads_back_and_the_generative_one_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[llm]\nmodel = \"coder:test\"\n").unwrap();
        let store = decision_store_in(&dir);

        assert_eq!(store.configured(), None, "nothing is configured yet");
        store.persist("nimble:test").unwrap();

        assert_eq!(store.configured(), Some("nimble:test".into()));
        assert_eq!(
            TomlModelStore::new(path).configured(),
            Some("coder:test".into()),
            "writing the decision model must not touch llm.model"
        );
    }

    #[test]
    fn a_decision_table_that_is_not_a_table_is_refused_rather_than_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "decision = \"nonsense\"\n").unwrap();
        let error = decision_store_in(&dir).persist("nimble:test").unwrap_err();
        assert!(error.0.contains("[decision] is not a table"), "{error:?}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "decision = \"nonsense\"\n",
            "the file is left exactly as it was"
        );
    }

    #[test]
    fn a_decision_model_that_cannot_be_written_reports_the_path() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: the write has to fail.
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(&path).unwrap();
        let error = TomlDecisionStore::new(path.clone())
            .persist("nimble:test")
            .unwrap_err();
        assert!(error.0.contains("cannot write"), "{error:?}");
        assert!(error.0.contains(&path.display().to_string()), "{error:?}");
    }

    #[test]
    fn decision_settings_default_when_the_table_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let settings = decision_settings(&dir.path().join(CONFIG_FILE));
        assert_eq!(settings.model, None);
        assert_eq!(settings.endpoint, DEFAULT_LLM_ENDPOINT);
        assert_eq!(settings.policy.mode, crate::domain::decision::Mode::Enforce);
        assert_eq!(settings.policy.threshold, DEFAULT_MIN_CONFIDENCE);
    }

    #[test]
    fn the_decision_endpoint_falls_back_to_the_llm_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[llm]\nendpoint = \"http://ollama.internal:11434\"\n",
        )
        .unwrap();
        assert_eq!(
            decision_settings(&path).endpoint,
            "http://ollama.internal:11434",
            "one host, stated once"
        );

        fs::write(
            &path,
            "[llm]\nendpoint = \"http://a:1\"\n[decision]\nendpoint = \"http://b:2\"\n",
        )
        .unwrap();
        assert_eq!(
            decision_settings(&path).endpoint,
            "http://b:2",
            "an explicit decision endpoint wins"
        );
    }

    /// A mode nobody can spell is no mode at all, so it takes the
    /// default. That means a misspelled `advisory` keeps gating rather
    /// than quietly stopping — the direction a typo should fail in.
    #[test]
    fn an_unspellable_mode_and_an_out_of_range_threshold_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[decision]\nmode = \"advsiory\"\nmin_confidence = 4.2\n",
        )
        .unwrap();
        let settings = decision_settings(&path);
        assert_eq!(settings.policy.mode, crate::domain::decision::Mode::Enforce);
        assert_eq!(settings.policy.threshold, DEFAULT_MIN_CONFIDENCE);
    }

    #[test]
    fn a_stated_mode_threshold_and_timeout_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[decision]\nmodel = \"nimble:test\"\nmode = \"enforce\"\n\
             min_confidence = 0.85\ntimeout_seconds = 30\n",
        )
        .unwrap();
        let settings = decision_settings(&path);
        assert_eq!(settings.model, Some("nimble:test".into()));
        assert_eq!(settings.policy.mode, crate::domain::decision::Mode::Enforce);
        assert_eq!(settings.policy.threshold, 0.85);
        assert_eq!(settings.timeout, std::time::Duration::from_secs(30));
    }

    /// `min_confidence = 1` is a reasonable thing to write, and TOML
    /// reads it as an integer.
    #[test]
    fn a_whole_number_threshold_is_read_as_a_float() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[decision]\nmin_confidence = 1\n").unwrap();
        assert_eq!(decision_settings(&path).policy.threshold, 1.0);
    }
}

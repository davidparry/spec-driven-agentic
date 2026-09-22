//! Resolved project configuration: each key's effective value and
//! whether it is a code default or came from the config file.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::domain::tool_profile::{Caller, default_profile};

pub const DEFAULT_LLM_ENDPOINT: &str = "http://localhost:11434";
pub const DEFAULT_LLM_TIMEOUT_SECONDS: u64 = 300;
pub const DEFAULT_LLM_CACHE_TTL_SECONDS: u64 = 600;
pub const DEFAULT_LLM_RETRY: u64 = 3;
pub const DEFAULT_TOOLS_MAX_ROUNDS: u32 = 12;
pub const DEFAULT_TOOLS_CONFIRM: &[&str] = &["command_run"];
pub const DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS: u64 = 10;
pub const DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS: u64 = 300;
pub const DEFAULT_TOOLS_CACHE_TTL_SECONDS: u64 = 86_400;
/// How many write-then-test rounds `spec refactor` is given to land a
/// green refactor before it restores the code it started from. High
/// enough that a model which is converging gets to finish, low enough
/// that one which is not stops wasting a laptop's evening.
pub const DEFAULT_REFACTOR_ATTEMPTS: u32 = 10;

/// The key whose value the provider resolves when the file is silent.
pub const LLM_MODEL_KEY: &str = "llm.model";

const UNSET: &str = "(unset)";
const NONE: &str = "(none)";
const DEFAULT: &str = "(default)";
const DISCOVERED: &str = "(discovered)";

/// Where a value came from: the built-in default, a file, or - for
/// `llm.model` alone - the provider's installed list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    Discovered,
    File(String),
}

impl ConfigSource {
    /// A sentinel in parentheses, like the `(unset)` / `(none)` values,
    /// so a code default never reads as a path.
    pub fn display(&self) -> &str {
        match self {
            Self::Default => DEFAULT,
            Self::Discovered => DISCOVERED,
            Self::File(path) => path,
        }
    }
}

impl Serialize for ConfigSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.display())
    }
}

/// What happened when the configuration file was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFileStatus {
    Missing,
    Unreadable { path: String },
    Invalid { path: String },
    Present { path: String },
}

impl ConfigFileStatus {
    pub fn display(&self) -> String {
        match self {
            Self::Missing => NONE.into(),
            Self::Unreadable { path } => format!("{path} (unreadable)"),
            Self::Invalid { path } => format!("{path} (invalid TOML)"),
            Self::Present { path } => path.clone(),
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Present { path } => Some(path.as_str()),
            _ => None,
        }
    }
}

impl Serialize for ConfigFileStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.display())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigSetting {
    pub key: String,
    pub value: String,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigReport {
    pub file: ConfigFileStatus,
    pub settings: Vec<ConfigSetting>,
}

impl ConfigReport {
    pub fn setting(&self, key: &str) -> Option<&ConfigSetting> {
        self.settings.iter().find(|setting| setting.key == key)
    }

    /// Show the model the provider would supply for a run. Ignored once
    /// the file names one, so a configured choice always wins.
    pub fn apply_discovered_model(&mut self, model: &str) {
        if let Some(setting) = self
            .settings
            .iter_mut()
            .find(|setting| setting.key == LLM_MODEL_KEY)
            && setting.source == ConfigSource::Default
        {
            setting.value = model.to_string();
            setting.source = ConfigSource::Discovered;
        }
    }
}

impl fmt::Display for ConfigReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "file\t{}", self.file.display())?;
        for setting in &self.settings {
            writeln!(
                f,
                "{}\t{}\t{}",
                setting.key,
                setting.value,
                setting.source.display()
            )?;
        }
        Ok(())
    }
}

/// Keys that were actually present and valid in the TOML file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PresentValues {
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub cache_ttl_seconds: Option<u64>,
    pub retry: Option<u64>,
    pub max_rounds: Option<u32>,
    pub confirm: Option<Vec<String>>,
    pub discovery_timeout_seconds: Option<u64>,
    pub call_timeout_seconds: Option<u64>,
    pub tools_cache_ttl_seconds: Option<u64>,
    pub refactor_attempts: Option<u32>,
    pub mcp_config: Option<String>,
    pub profiles: BTreeMap<String, Vec<String>>,
    pub enabled: BTreeMap<String, Vec<String>>,
    pub disabled: BTreeMap<String, Vec<String>>,
}

/// Merge file-present keys with code defaults. A listed
/// `[tools.profiles]` command replaces that caller's default set.
pub(crate) fn build_report(file: ConfigFileStatus, present: PresentValues) -> ConfigReport {
    let mut settings = Vec::new();
    let (value, set) = optional_or(present.model, UNSET);
    push(&mut settings, &file, LLM_MODEL_KEY, value, set);
    let (value, set) = optional_or(present.endpoint, DEFAULT_LLM_ENDPOINT);
    push(&mut settings, &file, "llm.endpoint", value, set);
    let (value, set) = number_or(present.timeout_seconds, DEFAULT_LLM_TIMEOUT_SECONDS);
    push(&mut settings, &file, "llm.timeout_seconds", value, set);
    let (value, set) = number_or(present.cache_ttl_seconds, DEFAULT_LLM_CACHE_TTL_SECONDS);
    push(&mut settings, &file, "llm.cache_ttl_seconds", value, set);
    let (value, set) = number_or(present.retry, DEFAULT_LLM_RETRY);
    push(&mut settings, &file, "llm.retry", value, set);
    let (value, set) = number_or(present.max_rounds, DEFAULT_TOOLS_MAX_ROUNDS);
    push(&mut settings, &file, "tools.max_rounds", value, set);
    let (value, set) = match present.confirm {
        Some(names) => (join_names(&names), true),
        None => (join_names(DEFAULT_TOOLS_CONFIRM), false),
    };
    push(&mut settings, &file, "tools.confirm", value, set);
    let (value, set) = number_or(
        present.discovery_timeout_seconds,
        DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS,
    );
    push(
        &mut settings,
        &file,
        "tools.discovery_timeout_seconds",
        value,
        set,
    );
    let (value, set) = number_or(
        present.call_timeout_seconds,
        DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS,
    );
    push(
        &mut settings,
        &file,
        "tools.call_timeout_seconds",
        value,
        set,
    );
    let (value, set) = number_or(
        present.tools_cache_ttl_seconds,
        DEFAULT_TOOLS_CACHE_TTL_SECONDS,
    );
    push(&mut settings, &file, "tools.cache_ttl_seconds", value, set);
    let (value, set) = optional_or(present.mcp_config, UNSET);
    push(&mut settings, &file, "tools.mcp_config", value, set);
    let (value, set) = number_or(present.refactor_attempts, DEFAULT_REFACTOR_ATTEMPTS);
    push(&mut settings, &file, "refactor.attempts", value, set);
    for caller in Caller::ALL {
        let key = format!("tools.profiles.{}", caller.key());
        match present.profiles.get(caller.key()) {
            Some(names) => push(&mut settings, &file, &key, join_names(names), true),
            None => push(
                &mut settings,
                &file,
                &key,
                join_names(default_profile(caller)),
                false,
            ),
        }
    }
    for (caller, names) in &present.enabled {
        push(
            &mut settings,
            &file,
            &format!("tools.enabled.{caller}"),
            join_names(names),
            true,
        );
    }
    for (caller, names) in &present.disabled {
        push(
            &mut settings,
            &file,
            &format!("tools.disabled.{caller}"),
            join_names(names),
            true,
        );
    }
    ConfigReport { file, settings }
}

fn optional_or(value: Option<String>, default: &str) -> (String, bool) {
    match value {
        Some(value) => (value, true),
        None => (default.to_string(), false),
    }
}

fn number_or<T: ToString>(value: Option<T>, default: T) -> (String, bool) {
    match value {
        Some(value) => (value.to_string(), true),
        None => (default.to_string(), false),
    }
}

fn join_names(names: &[impl AsRef<str>]) -> String {
    if names.is_empty() {
        NONE.into()
    } else {
        names
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn push(
    into: &mut Vec<ConfigSetting>,
    file: &ConfigFileStatus,
    key: &str,
    value: String,
    set: bool,
) {
    let source = match (set, file.path()) {
        (true, Some(path)) => ConfigSource::File(path.to_string()),
        _ => ConfigSource::Default,
    };
    into.push(ConfigSetting {
        key: key.into(),
        value,
        source,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_default_renders_as_a_parenthesized_sentinel_not_a_path() {
        assert_eq!(ConfigSource::Default.display(), "(default)");
        assert_eq!(ConfigSource::Discovered.display(), "(discovered)");
        assert_eq!(
            ConfigSource::File("/p/.spec/config.toml".into()).display(),
            "/p/.spec/config.toml"
        );
    }

    #[test]
    fn an_unset_model_is_filled_in_with_the_one_the_provider_offers() {
        let mut report = build_report(ConfigFileStatus::Missing, PresentValues::default());
        assert_eq!(report.setting(LLM_MODEL_KEY).unwrap().value, UNSET);
        report.apply_discovered_model("qwen3:8b");
        let model = report.setting(LLM_MODEL_KEY).unwrap();
        assert_eq!(model.value, "qwen3:8b");
        assert_eq!(model.source, ConfigSource::Discovered);
    }

    #[test]
    fn a_configured_model_is_never_replaced_by_discovery() {
        let path = "/p/.spec/config.toml";
        let present = PresentValues {
            model: Some("mine".into()),
            ..PresentValues::default()
        };
        let mut report = build_report(ConfigFileStatus::Present { path: path.into() }, present);
        report.apply_discovered_model("qwen3:8b");
        let model = report.setting(LLM_MODEL_KEY).unwrap();
        assert_eq!(model.value, "mine");
        assert_eq!(model.source, ConfigSource::File(path.into()));
    }

    #[test]
    fn missing_file_reports_every_scalar_as_default() {
        let report = build_report(ConfigFileStatus::Missing, PresentValues::default());
        assert_eq!(report.file.display(), NONE);
        let model = report.setting("llm.model").unwrap();
        assert_eq!(model.value, UNSET);
        assert_eq!(model.source, ConfigSource::Default);
        let endpoint = report.setting("llm.endpoint").unwrap();
        assert_eq!(endpoint.value, DEFAULT_LLM_ENDPOINT);
        assert_eq!(endpoint.source, ConfigSource::Default);
        let implement = report.setting("tools.profiles.implement").unwrap();
        assert!(implement.value.contains("command_run"));
        assert_eq!(implement.source, ConfigSource::Default);
        assert!(report.setting("tools.enabled.implement").is_none());
    }

    #[test]
    fn present_keys_are_attributed_to_the_file_path() {
        let path = "/tmp/project/.spec/config.toml";
        let mut present = PresentValues {
            model: Some("mine".into()),
            timeout_seconds: Some(900),
            max_rounds: Some(4),
            ..PresentValues::default()
        };
        present
            .profiles
            .insert("status".into(), vec!["get_tdd_state".into()]);
        present.enabled.insert(
            "implement".into(),
            vec!["playwright:browser_navigate".into()],
        );
        let report = build_report(ConfigFileStatus::Present { path: path.into() }, present);
        assert_eq!(report.file.display(), path);
        let model = report.setting("llm.model").unwrap();
        assert_eq!(model.value, "mine");
        assert_eq!(model.source, ConfigSource::File(path.into()));
        let endpoint = report.setting("llm.endpoint").unwrap();
        assert_eq!(endpoint.source, ConfigSource::Default);
        let timeout = report.setting("llm.timeout_seconds").unwrap();
        assert_eq!(timeout.value, "900");
        assert_eq!(timeout.source, ConfigSource::File(path.into()));
        let rounds = report.setting("tools.max_rounds").unwrap();
        assert_eq!(rounds.value, "4");
        let status = report.setting("tools.profiles.status").unwrap();
        assert_eq!(status.value, "get_tdd_state");
        assert_eq!(status.source, ConfigSource::File(path.into()));
        let implement = report.setting("tools.profiles.implement").unwrap();
        assert_eq!(implement.source, ConfigSource::Default);
        let extra = report.setting("tools.enabled.implement").unwrap();
        assert_eq!(extra.value, "playwright:browser_navigate");
        assert_eq!(extra.source, ConfigSource::File(path.into()));
    }

    #[test]
    fn invalid_file_still_uses_defaults() {
        let report = build_report(
            ConfigFileStatus::Invalid {
                path: "/tmp/.spec/config.toml".into(),
            },
            PresentValues::default(),
        );
        assert!(report.file.display().contains("invalid TOML"));
        assert_eq!(
            report.setting("llm.retry").unwrap().source,
            ConfigSource::Default
        );
    }

    #[test]
    fn unreadable_file_still_uses_defaults() {
        let report = build_report(
            ConfigFileStatus::Unreadable {
                path: "/tmp/.spec/config.toml".into(),
            },
            PresentValues::default(),
        );
        assert!(report.file.display().contains("unreadable"));
        assert_eq!(
            report.setting("llm.model").unwrap().source,
            ConfigSource::Default
        );
    }

    #[test]
    fn text_render_is_tab_separated_key_value_source() {
        let present = PresentValues {
            model: Some("qwen".into()),
            ..PresentValues::default()
        };
        let text = build_report(
            ConfigFileStatus::Present {
                path: "/p/.spec/config.toml".into(),
            },
            present,
        )
        .to_string();
        assert!(text.starts_with("file\t/p/.spec/config.toml\n"));
        assert!(text.contains("llm.model\tqwen\t/p/.spec/config.toml\n"));
        assert!(text.contains(&format!(
            "llm.endpoint\t{DEFAULT_LLM_ENDPOINT}\t{DEFAULT}\n"
        )));
    }
}

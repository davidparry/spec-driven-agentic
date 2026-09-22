//! Pure business logic. Nothing in this module performs IO; anything the
//! logic needs from the outside world arrives through [`crate::ports`].

/// The Ollama model this harness, talk, and workshop are developed and run
/// against. Named in pull hints and the `spec init` scaffold.
pub const RECOMMENDED_MODEL: &str = "qwen3.8-flash-next:125b-mlx";

/// Hidden parent for configuration, state, cache, logs, and staging.
pub const SPEC_DIR: &str = ".spec";

/// Project configuration written by `spec init` and `spec model use`.
pub const CONFIG_FILE: &str = "config.toml";

/// The TDD phase machine.
pub const STATE_FILE: &str = "state.json";

/// Durable project identity.
pub const MEMORY_FILE: &str = "memory.json";

/// Staging area holding mutations until `spec changes commit`.
pub const STAGED_DIR: &str = "staged";

/// Cached LLM responses and discovered tool catalogs; safe to delete.
pub const CACHE_DIR: &str = "cache";

/// Daily-rolling diagnostic logs; safe to delete.
pub const LOG_DIR: &str = "log";

/// Interactive-shell command history.
pub const HISTORY_FILE: &str = "history";

/// Project-relative path shown to humans, e.g. `.spec/state.json`.
pub fn spec_rel(name: &str) -> String {
    format!("{SPEC_DIR}/{name}")
}

pub mod command_policy;
pub mod config_report;
pub mod coverage;
pub mod feature;
pub mod generation;
pub mod language;
pub mod layout;
pub mod mcp_registry;
pub mod memory;
pub mod model;
pub mod paths;
pub mod prompts;
pub mod proposal;
pub mod refactor;
pub mod refiner;
pub mod scaffold;
pub mod scenario;
pub mod spec_validator;
pub mod steps;
pub mod tdd;
pub mod tool_profile;
pub mod tools;
pub mod workflow;

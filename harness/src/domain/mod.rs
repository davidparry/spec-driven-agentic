//! Pure business logic. Nothing in this module performs IO; anything the
//! logic needs from the outside world arrives through [`crate::ports`].

/// The Ollama model this harness, talk, and workshop are developed and run
/// against. Named in pull hints and the `spec init` scaffold.
pub const RECOMMENDED_MODEL: &str = "qwen3.8-flash-next:125b-mlx";

/// Project configuration written by `spec init` and `spec model use`.
pub const CONFIG_FILE: &str = ".spec.toml";

/// The TDD phase machine, in the project root.
pub const STATE_FILE: &str = ".spec-state.json";

/// Durable project identity, in the project root.
pub const MEMORY_FILE: &str = ".spec-memory.json";

/// Staging area holding mutations until `spec changes commit`.
pub const STAGED_DIR: &str = ".spec-staged";

/// Cached LLM responses and discovered tool catalogs; safe to delete.
pub const CACHE_DIR: &str = ".spec-cache";

/// Daily-rolling diagnostic logs; safe to delete.
pub const LOG_DIR: &str = ".spec-log";

/// Interactive-shell command history.
pub const HISTORY_FILE: &str = ".spec-history";

pub mod command_policy;
pub mod config_report;
pub mod feature;
pub mod generation;
pub mod language;
pub mod mcp_registry;
pub mod memory;
pub mod model;
pub mod paths;
pub mod prompts;
pub mod proposal;
pub mod refiner;
pub mod scaffold;
pub mod spec_validator;
pub mod steps;
pub mod tdd;
pub mod tool_profile;
pub mod tools;
pub mod workflow;

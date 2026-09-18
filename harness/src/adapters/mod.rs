//! The outermost ring: concrete implementations of [`crate::ports`] that
//! touch the filesystem, the network, and configuration files. Only the
//! composition roots — `main.rs`, `mcp.rs`, `greenfield.rs`, and
//! `wiring.rs` — may name these types.

pub mod chat_cache;
pub mod config;
pub mod console_prompt;
pub mod fs_memory;
pub mod fs_project;
pub mod fs_scaffold;
pub mod fs_sources;
pub mod fs_spec;
pub mod fs_staging;
pub mod fs_state;
pub mod gherkin_features;
pub mod mcp_client;
pub mod mcp_config;
pub mod noop_llm;
pub mod ollama;
pub mod ollama_chat;
pub mod overlay;
pub mod process_exec;
pub mod process_runtime;
pub mod prompt_end;
pub mod readline_prompt;
pub mod readline_shell;
pub mod runners;
pub mod spinner;
pub mod staging_lock;
pub mod tool_cache;

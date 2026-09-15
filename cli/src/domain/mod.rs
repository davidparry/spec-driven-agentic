//! Pure business logic. Nothing in this module performs IO; anything the
//! logic needs from the outside world arrives through [`crate::ports`].

/// The Ollama model this CLI, talk, and workshop are developed and run
/// against. Named in pull hints and the `bdd init` scaffold.
pub const RECOMMENDED_MODEL: &str = "qwen3.8-flash-next:125b-mlx";

pub mod command_policy;
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

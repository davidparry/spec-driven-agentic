//! HARNESS-023: Explain a specific command by name.
//!
//! Provides a [`CommandTree`] of named [`CommandEntry`] records and an
//! [`explain`] function that looks a command up by name and renders its
//! description, options, and tool dependencies — or reports "Unknown
//! command" when the name is absent.

use std::fmt;

/// Error message returned when a command name is not found in the tree.
const UNKNOWN_COMMAND_MSG: &str = "Unknown command";

/// A single command's documentation record: its human-readable
/// description, its command-line options, and the tools it calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    /// The name used to invoke this command (e.g. "run").
    pub name: String,
    /// A short description of what the command does.
    pub description: String,
    /// The options accepted by this command (e.g. `["--config", "--verbose"]`).
    pub options: Vec<String>,
    /// The tools this command depends on / calls (e.g. `["cargo", "java"]`).
    pub tools: Vec<String>,
}

/// A collection of named [`CommandEntry`] values that supports lookup by
/// name and rendering documentation.
#[derive(Debug, Clone, Default)]
pub struct CommandTree {
    entries: Vec<CommandEntry>,
}

impl CommandTree {
    /// Create an empty command tree.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a command entry in the tree. Overwrites any existing
    /// entry with the same name.
    pub fn register(&mut self, entry: CommandEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.name == entry.name) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Look up a command by name. Returns `Some(&CommandEntry)` if found.
    pub fn find(&self, name: &str) -> Option<&CommandEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Explain a command by name: returns a formatted string containing
    /// the description, options, and tools, or `Err` if the name is not
    /// in the tree.
    pub fn explain(&self, name: &str) -> Result<String, CommandExplainError> {
        let entry = self
            .find(name)
            .ok_or_else(|| CommandExplainError(UNKNOWN_COMMAND_MSG.into()))?;
        Ok(format_entry(entry))
    }
}

/// Error returned when [`CommandTree::explain`] cannot find the named
/// command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExplainError(pub String);

impl fmt::Display for CommandExplainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CommandExplainError {}

/// Format a [`CommandEntry`] into a human-readable documentation string.
fn format_entry(entry: &CommandEntry) -> String {
    let mut output = String::new();
    output.push_str(&format!("Command: {}\n", entry.name));
    output.push_str(&format!("Description: {}\n", entry.description));

    output.push_str(&format_section("Options", &entry.options));
    output.push_str(&format_section("Tools", &entry.tools));

    output
}

/// Render a labelled section with bulleted items, or `(none)` when empty.
fn format_section(label: &str, items: &[String]) -> String {
    if items.is_empty() {
        format!("{label}: (none)\n")
    } else {
        let mut section = format!("{label}:\n");
        for item in items {
            section.push_str(&format!("  - {item}\n"));
        }
        section
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_found_command_shows_description_options_and_tools() {
        let mut tree = CommandTree::new();
        tree.register(CommandEntry {
            name: "run".into(),
            description: "Run the test suite".into(),
            options: vec!["--config".into(), "--verbose".into()],
            tools: vec!["cargo".into(), "java".into()],
        });

        let result = tree.explain("run").unwrap();
        assert!(result.contains("Run the test suite"));
        assert!(result.contains("--config"));
        assert!(result.contains("--verbose"));
        assert!(result.contains("cargo"));
        assert!(result.contains("java"));
    }

    #[test]
    fn explain_unknown_command_returns_error() {
        let tree = CommandTree::new();
        let err = tree.explain("unknown-cmd").unwrap_err();
        assert_eq!(err.to_string(), "Unknown command");
    }

    #[test]
    fn register_overwrites_same_name() {
        let mut tree = CommandTree::new();
        tree.register(CommandEntry {
            name: "run".into(),
            description: "first".into(),
            options: vec![],
            tools: vec![],
        });
        tree.register(CommandEntry {
            name: "run".into(),
            description: "second".into(),
            options: vec![],
            tools: vec![],
        });
        let entry = tree.find("run").unwrap();
        assert_eq!(entry.description, "second");
    }

    #[test]
    fn find_returns_none_for_absent_name() {
        let tree = CommandTree::new();
        assert!(tree.find("missing").is_none());
    }

    #[test]
    fn format_entry_with_no_options_or_tools() {
        let entry = CommandEntry {
            name: "empty".into(),
            description: "Nothing here".into(),
            options: vec![],
            tools: vec![],
        };
        let out = format_entry(&entry);
        assert!(out.contains("Options: (none)"));
        assert!(out.contains("Tools: (none)"));
    }
}

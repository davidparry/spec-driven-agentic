//! The trait boundary (ports) between the inner rings and the outside
//! world. Application services depend on these abstractions; adapters
//! implement them; `main.rs` injects them. This is the inversion-of-control
//! seam of the whole crate.

use crate::domain::model::{ROOT_SPEC_FILE, Spec, SpecCatalog};

macro_rules! string_error {
    ($name:ident) => {
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl std::error::Error for $name {}
    };
}

/// Loads the requirements spec. The error string is already formatted the
/// way the workshop server reports unreadable specs, so it can be surfaced
/// directly as a validation issue.
pub trait SpecRepository {
    /// The spec merged across the whole include tree.
    fn load(&self) -> Result<Spec, SpecError>;

    /// The spec tree file by file: the root document first, then every
    /// include depth-first. Paths are relative to the root document's
    /// directory. Single-document sources (in-memory fakes) hold
    /// everything in the root file.
    fn load_catalog(&self) -> Result<SpecCatalog, SpecError> {
        self.load().map(SpecCatalog::single_root)
    }

    /// Raw JSON of one spec file by catalog-relative path, so overlay
    /// readers can resolve include trees that mix staged and working-tree
    /// files. Errors are fully formatted `spec: ...` messages.
    fn read_raw(&self, path: &str) -> Result<String, SpecError> {
        if path == ROOT_SPEC_FILE {
            self.load().map(|spec| {
                serde_json::to_string_pretty(&spec).expect("spec is always serializable")
            })
        } else {
            Err(SpecError(format!(
                "spec: {path} is not readable - the file does not exist"
            )))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError(pub String);
string_error!(SpecError);

/// Read-only queries about Gherkin feature files, used by spec validation
/// (does the file exist, does it carry the requirement's tag).
pub trait FeatureFiles {
    fn exists(&self, path: &str) -> bool;
    fn has_tag(&self, path: &str, tag: &str) -> bool;
}

/// Parsed access to the project's Gherkin feature files.
pub trait FeatureCatalog {
    fn list(&self) -> Result<Vec<crate::domain::feature::FeatureSummary>, FeatureError>;
    fn read(&self, path: &str) -> Result<crate::domain::feature::FeatureDoc, FeatureError>;
    fn exists(&self, path: &str) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureError(pub String);
string_error!(FeatureError);

/// Read-only questions about a project's marker files, used by language
/// detection (`pom.xml`, `package.json`, `*.csproj`, ...).
pub trait ProjectFiles {
    fn exists(&self, name: &str) -> bool;
    fn any_with_extension(&self, extension: &str) -> bool;
}

/// File bytes and a directory outline, used to scan project memory
/// without the domain talking to the filesystem.
pub trait ProjectInventory {
    fn exists(&self, path: &str) -> bool;
    fn read(&self, path: &str) -> Option<String>;
    /// Relative paths of files and directories (directories end with `/`),
    /// skipping build output, dependencies, and hidden directories.
    fn list_tree(&self) -> Vec<String>;
}

/// Persists [`.spec-memory.json`](crate::domain::memory::ProjectMemory)
/// between harness invocations.
pub trait MemoryStore {
    fn load(&self) -> Result<Option<crate::domain::memory::ProjectMemory>, MemoryError>;
    fn save(&self, memory: &crate::domain::memory::ProjectMemory) -> Result<(), MemoryError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryError(pub String);
string_error!(MemoryError);

/// Probes whether a runtime command is installed. `None` means the
/// command is not available; `Some` carries its version line.
pub trait RuntimeProbe {
    fn version(&self, command: &str) -> Option<String>;
}

/// One installed LLM model as reported by the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub name: String,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
}

/// Discovers which models the local LLM provider (Ollama by default) has
/// installed.
pub trait ModelCatalog {
    fn models(&self) -> Result<Vec<ModelInfo>, LlmError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmError(pub String);
string_error!(LlmError);

/// Reads and persists the configured model choice.
pub trait ModelStore {
    fn configured(&self) -> Option<String>;
    fn persist(&self, model: &str) -> Result<(), LlmError>;
}

/// One tool-using round trip. Stateless in the adapter — the caller
/// owns the message history. Every production model call goes through
/// this port (`Agent::ask` over Ollama `/api/chat`).
pub trait LlmConversation {
    fn chat(
        &self,
        model: &str,
        messages: &[crate::domain::tools::ChatMessage],
        tools: &[crate::domain::tools::ToolDefinition],
    ) -> Result<crate::domain::tools::ChatTurn, LlmError>;
}

impl<T: LlmConversation + ?Sized> LlmConversation for &T {
    fn chat(
        &self,
        model: &str,
        messages: &[crate::domain::tools::ChatMessage],
        tools: &[crate::domain::tools::ToolDefinition],
    ) -> Result<crate::domain::tools::ChatTurn, LlmError> {
        (**self).chat(model, messages, tools)
    }
}

impl<T: LlmConversation + ?Sized> LlmConversation for std::sync::Arc<T> {
    fn chat(
        &self,
        model: &str,
        messages: &[crate::domain::tools::ChatMessage],
        tools: &[crate::domain::tools::ToolDefinition],
    ) -> Result<crate::domain::tools::ChatTurn, LlmError> {
        self.as_ref().chat(model, messages, tools)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);
string_error!(ToolError);

/// Invokes one tool by its catalog name. Built-in tools loop back
/// through the embedded server; registered tools reach their own.
pub trait ToolBroker {
    fn call(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<crate::domain::tools::ToolOutcome, ToolError>;
}

impl<T: ToolBroker + ?Sized> ToolBroker for &T {
    fn call(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<crate::domain::tools::ToolOutcome, ToolError> {
        (**self).call(name, arguments)
    }
}

/// Connects to one registered MCP server and lists its tools.
pub trait ToolDiscovery {
    fn discover(
        &self,
        server: &crate::domain::mcp_registry::ServerSpec,
    ) -> Result<Vec<crate::domain::tools::ToolDefinition>, ToolError>;

    /// Bypass any catalog cache (`spec tools refresh`). Defaults to [`Self::discover`].
    fn discover_fresh(
        &self,
        server: &crate::domain::mcp_registry::ServerSpec,
    ) -> Result<Vec<crate::domain::tools::ToolDefinition>, ToolError> {
        self.discover(server)
    }
}

/// Reads the registered external MCP servers. Never fails: an absent or
/// broken registration file is reported as `problems`.
pub trait McpRegistrySource {
    fn load(&self) -> crate::domain::mcp_registry::RegistryLoad;
}

/// Reads and persists the per-command tool attachments and overrides.
pub trait ToolStore {
    fn overrides(&self) -> crate::domain::tool_profile::ProfileOverrides;
    fn attach(&self, caller: &str, tool: &str) -> Result<(), ToolError>;
    fn detach(&self, caller: &str, tool: &str) -> Result<(), ToolError>;
}

/// One source file, read for step-definition discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

/// Read access to the project's source files by extension, used to scan
/// step definitions per framework.
///
/// Implementations return the sources of the module the build compiles,
/// with project-root-relative paths. Callers may treat every file they
/// get back as on the compile path; a file outside the module would make
/// a discovery gate pass over code the runner never sees.
pub trait SourceFiles {
    fn sources(&self, extension: &str) -> Result<Vec<SourceFile>, SourceError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError(pub String);
string_error!(SourceError);

/// Writes scaffold files during `spec init`. Never overwrites: existing
/// files are reported as skipped so re-running init is always safe.
pub trait ScaffoldWriter {
    /// Returns `true` when the file was created, `false` when it already
    /// existed and was left alone.
    fn write_new(&self, path: &str, content: &str) -> Result<bool, ScaffoldError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldError(pub String);
string_error!(ScaffoldError);

/// One file change waiting in the staging area.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StagedChange {
    pub path: String,
    /// "create" (file does not exist in the working tree) or "modify".
    pub action: String,
    pub summary: String,
}

/// The staging area: every mutation the harness authors lands here first,
/// never directly in working files. The human reviews with
/// `changes show` and applies with `changes commit`.
pub trait ChangeStore {
    /// Stage `content` for `path`; re-staging the same path replaces it.
    fn stage(&self, path: &str, content: &str, summary: &str) -> Result<StagedChange, StageError>;
    fn changes(&self) -> Result<Vec<StagedChange>, StageError>;
    /// The staged content for a path, if that path is staged.
    fn content(&self, path: &str) -> Result<Option<String>, StageError>;
    /// Apply every staged change to the working tree and clear the area.
    fn commit(&self) -> Result<Vec<StagedChange>, StageError>;
    /// Drop every staged change without applying it.
    fn discard(&self) -> Result<Vec<StagedChange>, StageError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageError(pub String);
string_error!(StageError);

impl From<StageError> for FeatureError {
    fn from(error: StageError) -> Self {
        Self(error.0)
    }
}

impl From<StageError> for SourceError {
    fn from(error: StageError) -> Self {
        Self(error.0)
    }
}

/// A long-running step in progress. Hold it while the work runs and
/// drop it when the work is done; an interactive implementation
/// animates until then. The inert form does nothing on drop.
pub trait Working {}

/// The [`Working`] returned by the default `Prompter::working`: the
/// message was already told once, nothing to animate or clean up.
pub struct ToldOnce;

impl Working for ToldOnce {}

/// Interactive questions to the human developer. Behind a port so the
/// drafting and greenfield flows are testable with a scripted fake.
pub trait Prompter {
    fn tell(&mut self, message: &str);
    /// A dead end the developer must act on - a model failure, a missing
    /// runtime, a hand-off back to manual work. The console renders it
    /// in red; by default it is an ordinary `tell`, so fakes and
    /// transcripts see the same words either way.
    fn warn(&mut self, message: &str) {
        self.tell(message);
    }
    /// Announce a long-running step ("Running the tests - working").
    /// The returned guard lives for the duration of the work; an
    /// interactive console animates the trailing dots until it drops.
    /// By default the message is told once with " ..." appended, so
    /// fakes and piped runs see the familiar single line.
    fn working(&mut self, message: &str) -> Box<dyn Working> {
        self.tell(&format!("{message} ..."));
        Box::new(ToldOnce)
    }
    fn ask(&mut self, question: &str) -> Result<String, PromptError>;
    fn confirm(&mut self, question: &str) -> Result<bool, PromptError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptError(pub String);
string_error!(PromptError);

/// One read from the interactive `spec` shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellLine {
    Line(String),
    /// Ctrl+C - the session ends.
    Interrupted,
    /// Ctrl+D / end of input - the session ends.
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellError(pub String);
string_error!(ShellError);

/// Driving port for the interactive shell: line-edited input with a
/// session history that persists between shells. Behind a port so the
/// shell loop is testable with a scripted fake.
pub trait InteractiveShell {
    fn read_line(&mut self, prompt: &str) -> Result<ShellLine, ShellError>;
    fn tell(&mut self, message: &str);
    /// Persist the session history for the next shell to load.
    fn save_session(&mut self) -> Result<(), ShellError>;
}

/// Persists the TDD state log between harness invocations
/// (`.spec-state.json`): timestamped entries plus interpretation
/// instructions, so `test`, `state`, and `refactor` share one machine
/// across harness invocations.
pub trait StateStore {
    fn load(&self) -> Result<crate::domain::tdd::TddSnapshot, StateError>;
    fn save(&self, snapshot: &crate::domain::tdd::TddSnapshot) -> Result<(), StateError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateError(pub String);
string_error!(StateError);

/// Narrows a test run to one feature file and/or one scenario name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestFilter {
    pub feature: Option<String>,
    pub scenario: Option<String>,
}

/// Runs the project's test suite and summarizes the outcome. One
/// implementation per supported build tool (Maven, cucumber-js,
/// `dotnet test`, `cargo test`).
pub trait TestRunner {
    fn run(&self, filter: &TestFilter)
    -> Result<crate::domain::model::TestRunSummary, RunnerError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerError {
    /// The language's runtime is not installed. The harness never installs
    /// runtimes; it reports what is missing and how to get it.
    RuntimeMissing {
        runtime: String,
        hint: String,
    },
    Failed(String),
}

/// The captured result of one guarded command execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutcome {
    /// `None` when the process was killed (by the timeout or a signal).
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u64,
}

/// Spawns one already-validated argv with a pinned working directory and
/// a hard timeout. Policy lives in
/// [`crate::domain::command_policy`]; this port only executes.
pub trait CommandExecutor {
    fn run(
        &self,
        argv: &[String],
        dir: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Result<ExecOutcome, ExecError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecError(pub String);
string_error!(ExecError);

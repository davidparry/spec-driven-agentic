//! `spec` — spec-driven BDD/TDD harness with an embedded MCP server.
//!
//! This binary is a composition root: it names concrete adapters and
//! wires them into application services, and nothing else.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use spec_harness::adapters::auto_prompt::AutoPrompter;
use spec_harness::adapters::chat_cache::{CachedConversation, DEFAULT_CACHE_TTL};
use spec_harness::adapters::config::{
    TomlModelStore, TomlToolStore, config_path, inspect_config, refactor_attempts, tools_settings,
};
use spec_harness::adapters::console_prompt::ConsolePrompter;
use spec_harness::adapters::fs_project::FsProjectFiles;
use spec_harness::adapters::fs_scaffold::FsScaffoldWriter;
use spec_harness::adapters::fs_spec::FsSpecRepository;
use spec_harness::adapters::fs_staging::FsChangeStore;
use spec_harness::adapters::mcp_client::McpToolBroker;
use spec_harness::adapters::mcp_config::FsMcpRegistry;
use spec_harness::adapters::ollama::{DEFAULT_ENDPOINT, DEFAULT_GENERATION_TIMEOUT, OllamaCatalog};
use spec_harness::adapters::ollama_chat::OllamaChat;
use spec_harness::adapters::process_runtime::ProcessRuntimeProbe;
use spec_harness::adapters::prompt_end::AbortOnEndOfInput;
use spec_harness::adapters::readline_prompt::ReadlinePrompter;
use spec_harness::adapters::readline_shell::ReadlineShell;
use spec_harness::adapters::runners::detect_runner;
use spec_harness::adapters::spec_home::{ensure_spec_home, spec_file};
use spec_harness::adapters::spinner::{HushingPrompter, Spinner};
use spec_harness::adapters::tool_cache::CachedDiscovery;
use spec_harness::application::DEFAULT_LLM_ATTEMPTS;
use spec_harness::application::agent_service::AgentConfig;
use spec_harness::application::generation_service::{GenerationService, ResolvedLlm};
use spec_harness::application::implement_service::ImplementService;
use spec_harness::application::init_service::InitService;
use spec_harness::application::inspect_service::InspectService;
use spec_harness::application::memory_service::MemoryAwareConversation;
use spec_harness::application::model_service::{
    ModelResolution, ModelService, ModelSource, SessionModel,
};
use spec_harness::application::refactor_service::{RefactorReport, RefactorService};
use spec_harness::application::spec_mutation_service::SpecMutationService;
use spec_harness::application::spec_service::STAGED;
use spec_harness::application::status_service::StatusService;
use spec_harness::application::tdd_service::TddError;
use spec_harness::application::tool_call_service::ToolCallService;
use spec_harness::application::tool_service::ToolService;
use spec_harness::bootstrap::{prompt_language, refresh_project_memory, settle_project_memory};
use spec_harness::deliver::{DEFAULT_ATTEMPTS, Deliver, DeliverOptions, parse_target};
use spec_harness::domain::config_report::{ConfigSource, LLM_MODEL_KEY};
use spec_harness::domain::language::Language;
use spec_harness::domain::mcp_registry::ServerSpec;
use spec_harness::domain::prompts::ask_prompt;
use spec_harness::domain::spec_validator::{is_structural_issue, structural_repair};
use spec_harness::domain::tdd::ImplementAttempt;
use spec_harness::domain::tool_profile::{Caller, resolve};
use spec_harness::domain::{CACHE_DIR, HISTORY_FILE, LOG_DIR, RECOMMENDED_MODEL, spec_rel};
use spec_harness::greenfield::Greenfield;
use spec_harness::mcp::{WorkflowServer, builtin_tool_definitions};
use spec_harness::ports::{
    FeatureCatalog as _, McpRegistrySource as _, Prompter, TestFilter, ToolStore as _,
};
use spec_harness::repl::{Ending, is_greenfield_start, offer_greenfield, run_shell};
use spec_harness::wiring::{self, DynLlm};
use spec_harness::workspace::{SPEC_PATH, detect_project_layout, project_layout};

#[derive(Parser)]
#[command(
    name = "spec",
    version,
    about = "Spec-driven BDD/TDD authoring, validation, and execution"
)]
struct Cli {
    /// Override the configured LLM model for this invocation
    #[arg(long, global = true)]
    model: Option<String>,

    /// Project root (where requirements/ and .spec/config.toml live)
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    /// Verbose diagnostic logging on stderr (prompts, responses, cache, MCP)
    #[arg(long, global = true)]
    debug: bool,

    /// Max attempts when a model reply fails validation (default 3)
    #[arg(long, global = true)]
    retry: Option<u32>,

    /// Replace this command's tool profile for one run (comma-separated names)
    #[arg(long, global = true)]
    tools: Option<String>,

    /// Override [tools] max_rounds for one run
    #[arg(long, global = true)]
    max_rounds: Option<u32>,

    /// Omitted entirely: print help and open the interactive shell
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold build files, Cucumber runner, empty spec, and config
    Init(InitArgs),
    /// Run the full orchestrated loop from zero (two human gates)
    Greenfield,
    /// Drive a requirement, a description, or the whole backlog to implemented
    Deliver(DeliverArgs),
    // Flattened, so the requirements-spec verbs are `spec list`, `spec draft`,
    // `spec validate`, … rather than `spec spec …`.
    #[command(flatten)]
    Spec(SpecCommand),
    /// Detect project languages, build system, runtimes, and roots
    Inspect,
    /// Feature discovery and creation
    #[command(subcommand)]
    Feature(FeatureCommand),
    /// Scenario mutations
    #[command(subcommand)]
    Scenario(ScenarioCommand),
    /// Step-definition discovery and generation
    #[command(subcommand)]
    Steps(StepsCommand),
    /// Unit-test generation (the TDD altitude)
    #[command(subcommand)]
    Unittest(UnittestCommand),
    /// Ask the model to make the failing tests pass (stages the files)
    Implement { req_id: String },
    /// Run tests and update the RED/GREEN/REFACTOR phase (run_tests)
    Test(TestArgs),
    /// Show the current TDD phase and last run (get_tdd_state)
    State,
    /// Where every requirement stands on the road to implemented, and the next step
    Status,
    /// Begin a refactor step; only allowed on GREEN (start_refactor)
    Refactor(RefactorArgs),
    /// Staged-transaction management
    #[command(subcommand)]
    Changes(ChangesCommand),
    /// LLM model discovery and selection (Ollama)
    #[command(subcommand)]
    Model(ModelCommand),
    /// Print resolved configuration and where each value came from
    Config {
        /// Print JSON instead of the tab-separated table
        #[arg(long)]
        json: bool,
    },
    /// MCP server
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Per-command tool profiles and the external MCP registry
    #[command(subcommand)]
    Tools(ToolsCommand),
    /// Free-form question with the read-only tool profile
    Ask {
        /// The question. Omit on a tty for a multi-turn prompt.
        task: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SpecCommand {
    /// List every requirement with id, title, and status (list_requirements)
    List,
    /// Show one requirement, enriched with locations and a workflow hint (get_requirement)
    Show { req_id: String },
    /// Draft a requirement. Flags skip the interactive wizard.
    Draft {
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        story: Option<String>,
        /// Repeatable Given/When/Then acceptance criterion
        #[arg(long)]
        criterion: Vec<String>,
        /// Draft into this included spec file instead of the root
        /// catalog (e.g. requirements/core/math.json)
        #[arg(long)]
        file: Option<String>,
    },
    /// Validate the requirements spec on disk (validate_spec)
    Validate,
    /// Review one requirement's wording for quality (refine_requirement)
    Refine { req_id: String },
    /// Reword an existing requirement (staged). Flags skip the wizard.
    Reword {
        req_id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        story: Option<String>,
        #[arg(long)]
        criterion: Vec<String>,
    },
    /// Point a requirement at a feature file (staged; for extracted specs)
    SetFeature {
        req_id: String,
        #[arg(long)]
        file: String,
    },
    /// Flip a requirement's status to implemented (requirement_mark_implemented)
    MarkImplemented { req_id: String },
    /// Manage the spec catalog's included files
    #[command(subcommand)]
    Include(IncludeCommand),
}

#[derive(Subcommand)]
enum IncludeCommand {
    /// Include a spec file in the catalog (staged); created empty when missing
    Add {
        /// The spec file to include, e.g. requirements/core/math.json
        path: String,
        /// The catalog document declaring the include (default: the root
        /// requirements.json)
        #[arg(long)]
        from: Option<String>,
    },
}

#[derive(Subcommand)]
enum FeatureCommand {
    /// List every feature file with its name and scenario count
    List,
    /// Show one parsed feature file (path relative to --root)
    Show { path: String },
    /// Create a feature file (staged)
    Create {
        /// Feature file path relative to --root
        #[arg(long)]
        path: String,
        /// Feature name (the text after "Feature:")
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum ScenarioCommand {
    /// Append a tagged scenario to a feature file (staged)
    Add {
        /// Feature file path relative to --root
        #[arg(long)]
        feature: String,
        /// Requirement id the scenario implements (tagged @REQ-...)
        #[arg(long)]
        req: String,
        /// Scenario name
        #[arg(long)]
        name: String,
        /// One full Gherkin step per flag, e.g. --step "Given a calculator"
        #[arg(long = "step")]
        steps: Vec<String>,
    },
    /// Write a requirement's scenarios from its acceptance criteria (staged)
    Generate {
        req_id: String,
        /// Feature file to append to; defaults to the requirement's own
        #[arg(long)]
        feature: Option<String>,
    },
    /// Replace a scenario's steps and/or requirement tag (staged)
    Update {
        #[arg(long)]
        feature: String,
        #[arg(long)]
        name: String,
        /// New requirement id for the tag; omit to keep the current tag
        #[arg(long)]
        req: Option<String>,
        /// New steps; omit to keep the current steps
        #[arg(long = "step")]
        steps: Vec<String>,
    },
    /// Remove a scenario from a feature file (staged)
    Delete {
        #[arg(long)]
        feature: String,
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum StepsCommand {
    /// Report undefined and ambiguous steps (step_definitions_find)
    Missing,
    /// Generate step definitions for undefined steps (step_definition_create)
    Generate,
}

#[derive(Subcommand)]
enum UnittestCommand {
    /// Generate a unit test from a requirement's acceptance criteria (unit_test_create)
    Generate { req_id: String },
}

#[derive(Args)]
struct InitArgs {
    /// Target language (java, javascript, typescript, dotnet, rust); prompted when omitted
    #[arg(long)]
    language: Option<String>,
    /// Project name; defaults to the root directory's name
    #[arg(long)]
    name: Option<String>,
}

#[derive(Args)]
struct DeliverArgs {
    /// A requirement id (REQ-003) to see through to implemented, or a
    /// plain-words requirement to break down and deliver. Omit for every
    /// pending requirement in the spec.
    target: Vec<String>,
    /// RED-to-GREEN attempts per requirement (default 3)
    #[arg(long)]
    attempts: Option<u32>,
    /// Stop at the first requirement that falls short instead of
    /// carrying on to the next
    #[arg(long)]
    fail_fast: bool,
    /// Skip the refactor step even on a green bar
    #[arg(long)]
    no_refactor: bool,
    /// Draft new requirements into this included spec file instead of
    /// the root catalog (e.g. requirements/core/math.json)
    #[arg(long)]
    file: Option<String>,
}

#[derive(Args)]
struct TestArgs {
    /// Run only one feature
    #[arg(long)]
    feature: Option<String>,
    /// Run only one scenario
    #[arg(long)]
    scenario: Option<String>,
}

#[derive(Args)]
struct RefactorArgs {
    /// What you intend to refactor and why. With a model resolved this
    /// is also the brief it is given
    #[arg(long)]
    note: Option<String>,
    /// The requirement whose behaviour must not change, shown to the
    /// model as the specification it is preserving
    #[arg(long = "req")]
    req_id: Option<String>,
    /// Mark the phase and stop - the cleanup stays in your hands
    #[arg(long)]
    manual: bool,
}

#[derive(Subcommand)]
enum ChangesCommand {
    Show,
    Commit,
    Discard,
    /// Validate Gherkin on disk and in the stage (changes_validate)
    Validate,
}

#[derive(Subcommand)]
enum ModelCommand {
    /// List models available in Ollama
    List,
    /// Show the resolved model and where it came from (flag, config, discovery)
    Current,
    /// Persist a model choice in configuration
    Use { model_name: String },
}

#[derive(Subcommand)]
enum McpCommand {
    /// Serve the MCP tools over stdio
    Serve,
    /// List tools over one throwaway MCP session
    Tools {
        /// Spawn `spec mcp serve` as a child instead of the in-process loopback
        #[arg(long)]
        stdio: bool,
        #[arg(long)]
        json: bool,
    },
    /// Call one tool over a throwaway MCP session
    Call {
        tool: String,
        /// Repeatable key=value (always a string unless the value parses as JSON)
        #[arg(long = "arg", value_parser = parse_arg_pair)]
        arg: Vec<(String, String)>,
        /// JSON object merged under --arg
        #[arg(long = "args")]
        args: Option<String>,
        /// Spawn `spec mcp serve` as a child instead of the in-process loopback
        #[arg(long)]
        stdio: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ToolsCommand {
    /// List tools (optionally one caller's resolved profile)
    List {
        #[arg(long = "for")]
        for_caller: Option<String>,
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        json: bool,
    },
    /// Every caller and its resolved tool set
    Profiles {
        #[arg(long)]
        json: bool,
    },
    /// Description, JSON Schema, and origin of one tool
    Show { name: String },
    /// Attach a tool to one command's profile
    Enable {
        name: String,
        #[arg(long = "for")]
        for_caller: Option<String>,
    },
    /// Remove a tool from one command's profile
    Disable {
        name: String,
        #[arg(long = "for")]
        for_caller: Option<String>,
    },
    /// Rediscover every registered server and rewrite the catalog cache
    Refresh,
    /// Registered servers, config path, and problems
    Servers {
        #[arg(long)]
        json: bool,
    },
}

fn parse_arg_pair(raw: &str) -> Result<(String, String), String> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("--arg expects key=value, got {raw}"))?;
    if key.is_empty() {
        return Err("--arg key is empty".into());
    }
    Ok((key.to_string(), value.to_string()))
}

/// A command that already printed its (JSON) reply but must exit
/// nonzero — the structured `runtime_missing` refusal. A sentinel
/// instead of `process::exit` so the interactive shell survives it.
#[derive(Debug)]
struct NonzeroExit;

impl std::fmt::Display for NonzeroExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "nonzero exit")
    }
}

impl std::error::Error for NonzeroExit {}

/// The directory `spec` operates on.
///
/// A concrete `--root` wins. An empty value, or an unexpanded template
/// such as `${CLAUDE_PROJECT_DIR:-.}` (Claude Code stores that string
/// as-is; it does not apply bash defaults), is not a directory. In that
/// case `SPEC_PROJECT_DIR` is used when it names a real path. Otherwise
/// the process keeps the directory it was launched in.
fn resolve_project_root(flag: PathBuf, spec_project_dir: Option<&str>) -> PathBuf {
    if usable_root(&flag) {
        return flag;
    }
    if let Some(from_env) = spec_project_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(from_env);
        if usable_root(&path) {
            return path;
        }
    }
    PathBuf::from(".")
}

fn usable_root(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let trimmed = text.trim();
    !trimmed.is_empty() && !is_unexpanded_template(trimmed)
}

fn is_unexpanded_template(value: &str) -> bool {
    value.contains("${")
        || value.contains("$CLAUDE_PROJECT_DIR")
        || value.contains("$SPEC_PROJECT_DIR")
}

fn main() -> anyhow::Result<()> {
    let mut cli = Cli::parse();
    let from_env = std::env::var("SPEC_PROJECT_DIR").ok();
    cli.root = resolve_project_root(cli.root, from_env.as_deref());
    ensure_spec_home(&cli.root)?;
    // Held until exit so the logging worker thread drains its queue.
    let log_guard = init_logging(cli.debug, &cli.root);
    match cli.command {
        Some(ref command) => {
            let resumes = resumes_the_shell(command);
            match execute(
                &cli.root,
                cli.model.as_deref(),
                cli.retry,
                cli.tools.as_deref(),
                cli.max_rounds,
                command,
            ) {
                Err(error) if error.is::<NonzeroExit>() => {
                    // process::exit skips destructors; flush the log queue first.
                    drop(log_guard);
                    std::process::exit(1)
                }
                Ok(()) if resumes => {
                    resume_shell_after_orchestrator(&cli.root, cli.model.as_deref(), cli.retry)
                }
                other => other,
            }
        }
        None => run_shell_mode(&cli.root, cli.model.as_deref(), cli.retry),
    }
}

/// Whether a finished command hands the terminal to the interactive
/// shell instead of exiting. The orchestrators do: a run that just
/// closed a loop is nearly always followed by another command, and
/// relaunching to type it is friction for nothing.
///
/// A delivery asks nothing *during* the run, but it has finished by the
/// time this is reached, so handing back a prompt is the convenience and
/// not a contradiction. `resume_shell_after_orchestrator` still ends the
/// process when there is no terminal, so a piped or CI run is unaffected.
fn resumes_the_shell(command: &Command) -> bool {
    matches!(command, Command::Greenfield | Command::Deliver(_))
}

/// Diagnostics go to daily-rolling files under `<root>/.spec/log/`, so
/// stdout stays clean for JSON output and the MCP stdio protocol, and
/// stderr stays clean for user-facing messages. Writes go through an
/// in-memory queue drained by a dedicated worker thread; the returned
/// guard flushes the queue on exit and must live until then. When the
/// log directory cannot be created, diagnostics fall back to stderr.
/// `RUST_LOG` overrides both the default level and `--debug`.
fn init_logging(debug: bool, root: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;

    let default_directives = if debug {
        "spec=debug,spec_harness=debug"
    } else {
        "spec=info,spec_harness=info"
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directives));
    let log_dir = spec_file(root, LOG_DIR);
    if std::fs::create_dir_all(&log_dir).is_err() {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_target(debug)
            .compact()
            .init();
        return None;
    }
    let appender = tracing_appender::rolling::daily(log_dir, "spec.log");
    // lossy(false): a full queue blocks the caller instead of dropping
    // lines, so prompts and responses are never missing from the file.
    let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .lossy(false)
        .finish(appender);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(debug)
        .compact()
        .init();
    Some(guard)
}

fn execute(
    root: &Path,
    model: Option<&str>,
    retry: Option<u32>,
    tools: Option<&str>,
    max_rounds: Option<u32>,
    command: &Command,
) -> anyhow::Result<()> {
    let attempts = resolve_llm_attempts(root, retry);
    match command {
        Command::Spec(command) => run_spec(root, model, attempts, tools, max_rounds, command),
        Command::Model(command) => run_model(root, model, command),
        Command::Config { json } => {
            let report = config_report(root);
            if *json {
                print_json(&report)
            } else {
                print!("{report}");
                Ok(())
            }
        }
        Command::Inspect => {
            let service =
                InspectService::new(FsProjectFiles::new(root.to_path_buf()), ProcessRuntimeProbe);
            print_json(&service.inspect())
        }
        Command::Feature(command) => run_feature(root, command),
        Command::Scenario(command) => {
            run_scenario(root, model, attempts, tools, max_rounds, command)
        }
        Command::Changes(command) => run_changes(root, command),
        Command::Init(args) => run_init(root, args),
        Command::Greenfield => run_greenfield(root, model, attempts),
        Command::Deliver(args) => run_deliver(root, model, attempts, args),
        Command::Mcp(command) => run_mcp(root, command),
        Command::Tools(command) => run_tools(root, command),
        Command::Ask { task, json } => {
            run_ask(root, model, attempts, tools, max_rounds, task, *json)
        }
        Command::Test(args) => run_test(root, args),
        Command::State => tdd_reply(tdd_service(root).state()),
        Command::Status => {
            let state = tdd_service(root)
                .state()
                .map_err(|e| anyhow::anyhow!(tdd_error_message(e)))?;
            let service = status_service(root, model, attempts, tools, max_rounds)?;
            let report = service.status(&state.phase)?;
            print_json(&report)?;
            // Deterministic nextStep already names the command for asset
            // gaps. Do not let the model override that with spec draft.
            if service.has_model() && !deterministic_status_gap(&report.next_step) {
                const RED: &str = "\x1b[31m";
                const RESET: &str = "\x1b[0m";
                let work = Spinner::start("Asking the model for the next step - working");
                let mut prompter = interactive_prompter(Prompts::Incidental);
                let advice = service.advice(prompter.as_mut(), &report, &state.last_run);
                drop(work);
                match advice {
                    Ok(Some(advice)) => println!("Model advice: {advice}"),
                    Ok(None) => {}
                    Err(e) => println!("{RED}{}{RESET}", e.0),
                }
            }
            Ok(())
        }
        Command::Refactor(args) => {
            const RED: &str = "\x1b[31m";
            const RESET: &str = "\x1b[0m";
            // The phase transition is the gate: it refuses off GREEN and
            // logs the note, exactly as it did when marking the phase was
            // all this command could do. The loop only runs once it has
            // agreed a refactor is legitimate here.
            let marked = tdd_service(root).refactor(args.note.as_deref());
            if args.manual {
                return tdd_reply(marked);
            }
            let phase = match marked {
                Ok(phase) => phase,
                Err(error) => return tdd_reply(Err::<RefactorReport, _>(error)),
            };
            let service = refactor_service(root, model, attempts, tools, max_rounds)?;
            if !service.has_model() {
                // No model, no loop: the phase is marked and the cleanup
                // is the developer's, which is the whole behaviour this
                // command used to have.
                return print_json(&phase);
            }
            let runner = detect_runner(root).map_err(|message| anyhow::anyhow!(message))?;
            let mut prompter = interactive_prompter(Prompts::Incidental);
            let report = service.run(
                prompter.as_mut(),
                runner.as_ref(),
                args.note.as_deref(),
                args.req_id.as_deref(),
            )?;
            if let Some(warning) = &report.warning {
                println!("{RED}{warning}{RESET}");
            }
            print_json(&report)
        }
        Command::Steps(command) => {
            let service = generation_service(
                root,
                model,
                attempts,
                tools,
                max_rounds,
                Caller::StepsGenerate,
            )?;
            match command {
                StepsCommand::Missing => print_json(&service.steps_missing()?),
                StepsCommand::Generate => {
                    let mut prompter = interactive_prompter(Prompts::Incidental);
                    print_json(&service.steps_generate(prompter.as_mut())?)
                }
            }
        }
        Command::Implement { req_id } => {
            const RED: &str = "\x1b[31m";
            const GREEN: &str = "\x1b[32m";
            const RESET: &str = "\x1b[0m";
            let service =
                implement_service(root, model, attempts, tools, max_rounds, Caller::Implement)?;
            let tdd = tdd_service(root);
            let phase = tdd
                .state()
                .map_err(|e| anyhow::anyhow!(tdd_error_message(e)))?
                .phase;
            // The last run's failure details (stack traces included) are
            // the model's brief, together with every prior attempt on
            // this requirement; a fresh RED bar comes from spec test
            // right before this. The attempt is logged so the next one
            // learns from it.
            let brief = tdd
                .implementation_brief(req_id)
                .map_err(|e| anyhow::anyhow!(tdd_error_message(e)))?;
            println!(
                "{req_id}: checking prerequisites - phase {phase}, \
                 {} recorded failure(s), {} prior attempt(s).",
                brief.failures.len(),
                brief.history.len()
            );
            let readiness = service.readiness(req_id, &phase, &brief.failures)?;
            for asset in &readiness.assets {
                let mark = if asset.present {
                    format!("{GREEN}present{RESET}")
                } else {
                    format!("{RED}missing{RESET}")
                };
                println!("  {}: {} - {mark}", asset.role, asset.path);
            }
            let mut prompter = interactive_prompter(Prompts::Incidental);
            if !readiness.ready {
                for finding in &readiness.findings {
                    println!("{RED}{finding}{RESET}");
                }
                if service.has_model() {
                    let work =
                        Spinner::start("Asking the model whether implement can run - working");
                    let advice_service = implement_service(
                        root,
                        model,
                        attempts,
                        tools,
                        max_rounds,
                        Caller::ImplementAdvice,
                    )?;
                    let advice = advice_service.advice(
                        prompter.as_mut(),
                        req_id,
                        &readiness,
                        &brief.failures,
                    );
                    drop(work);
                    match advice {
                        Ok(Some(advice)) => println!("Model advice: {advice}"),
                        Ok(None) => {}
                        Err(e) => println!("{RED}{}{RESET}", e.0),
                    }
                }
                return print_json(&readiness);
            }
            let work = Spinner::start(
                "Sending the sources, the failures, and the attempt history \
                 to the model - working",
            );
            let report = service
                .generate(
                    prompter.as_mut(),
                    req_id,
                    &brief.failures,
                    &brief.history,
                    &brief.states,
                )
                .map_err(anyhow::Error::from);
            drop(work);
            let report = report?;
            for target in &report.targets {
                println!("  staged: {target}");
            }
            if let Some(warning) = &report.warning {
                println!("{RED}{warning}{RESET}");
            }
            // The outcome stays empty until the next test run attaches
            // what these changes actually caused.
            tdd.record_attempt(ImplementAttempt {
                requirement: req_id.clone(),
                targets: report.targets.clone(),
                failures: brief.failures,
                ..Default::default()
            })
            .map_err(|e| anyhow::anyhow!(tdd_error_message(e)))?;
            print_json(&report)?;
            drop(prompter);
            if report.staged && !report.targets.is_empty() {
                implement_follow_up(root, req_id)?;
            }
            Ok(())
        }
        Command::Unittest(UnittestCommand::Generate { req_id }) => {
            let service = generation_service(
                root,
                model,
                attempts,
                tools,
                max_rounds,
                Caller::UnittestGenerate,
            )?;
            let mut prompter = interactive_prompter(Prompts::Incidental);
            print_json(&service.unittest_generate(prompter.as_mut(), req_id)?)
        }
    }
}

/// Bare `spec`: print the help, then hand the terminal to the
/// interactive shell. Each line is parsed exactly like a one-shot
/// invocation, inheriting the shell's --root, --model, and --retry unless the
/// line sets its own; errors are printed and the shell keeps going.
/// Without a terminal (pipes, CI) the help alone is the whole reply.
/// The shell banner: the harness mark as ASCII art - the red-to-green
/// cycle looping around the prompt - with the compiled-in version.
fn print_banner() {
    const R: &str = "\x1b[31m"; // the red arc
    const G: &str = "\x1b[32m"; // the green arc
    const B: &str = "\x1b[1m"; // bold
    const D: &str = "\x1b[2m"; // dim
    const X: &str = "\x1b[0m"; // reset
    // Interior width of the loop, in display columns.
    const W: usize = 34;
    let version = env!("CARGO_PKG_VERSION");
    let title = format!("> spec  v{version}");
    let title_pad = " ".repeat(W - 4 - title.len());
    let top = "─".repeat(W);
    let gap = " ".repeat(W);
    println!();
    println!("  {R}╭{top}╮{X}");
    println!("  {R}│{X}{gap}{R}▼{X}");
    println!("  {R}│{X}    {B}> spec{X}  {D}v{version}{X}{title_pad}{G}│{X}");
    println!("  {R}│{X}    {D}spec →{X} {R}RED{X} {D}→{X} {G}GREEN{X} {D}→ REFACTOR{X} {G}│{X}");
    println!("  {G}▲{X}{gap}{G}│{X}");
    println!("  {G}╰{top}╯{X}");
    println!();
}

/// The startup model line: what this session will use, or exactly what
/// to install to make generation work. Discovery is session-only - the
/// configuration is never touched until the user runs `spec model use`.
/// Returns whether a model is ready, which gates the greenfield nudge.
fn announce_session_model(root: &Path, flag: Option<&str>) -> bool {
    let session = model_service(root).session_model(flag);
    let ready = matches!(session, SessionModel::Ready { .. });
    match session {
        SessionModel::Ready { model, source } => match source {
            ModelSource::Flag => println!("Model set: {model} (from the --model flag)."),
            ModelSource::Config => println!("Model set: {model} (from configuration)."),
            ModelSource::OnlyInstalled | ModelSource::FirstInstalled => println!(
                "Model set for this session: {model} (not saved - keep it with: \
                 spec model use {model})."
            ),
        },
        SessionModel::NoModels => println!(
            "Ollama is running but has no models - generation will use \
             deterministic templates. For optimal results pull a coding \
             model, e.g.: ollama pull {RECOMMENDED_MODEL} \
             (mileage varies with models not trained for development)"
        ),
        SessionModel::ProviderDown(_) => println!(
            "Ollama is not reachable - generation will use deterministic \
             templates. Install it from https://ollama.com, start it, and \
             pull a coding model, e.g.: ollama pull {RECOMMENDED_MODEL} \
             (mileage varies with models not trained for development)"
        ),
    }
    ready
}

fn run_shell_mode(root: &Path, model: Option<&str>, retry: Option<u32>) -> anyhow::Result<()> {
    use clap::CommandFactory as _;
    use std::io::IsTerminal as _;
    Cli::command().print_help()?;
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    print_banner();
    let model_ready = announce_session_model(root, model);
    // Session start is where the layout question belongs: a human is
    // attached, and the answer is recorded for every later command.
    let llm =
        cached_chat(root, model).map(|(model, chat)| (model, std::sync::Arc::new(chat) as DynLlm));
    let mut prompter = interactive_prompter(Prompts::Incidental);
    settle_project_memory(
        root,
        llm.as_ref(),
        resolve_llm_attempts(root, retry),
        prompter.as_mut(),
    );
    drop(prompter);
    println!(
        "Interactive shell - type commands without the spec prefix \
         (e.g. list). exit, quit, or Ctrl+C leaves. The session \
         history lives in {}.",
        spec_rel(HISTORY_FILE)
    );
    interactive_shell_loop(root, model, retry, true, model_ready)
}

/// After a one-shot orchestrator run (`spec greenfield`, `spec deliver`)
/// on a real terminal, keep the session open at the `spec>` prompt so the
/// next requirement (or any other command) can be typed without
/// relaunching. Without a terminal - a pipe, CI - the run simply ends.
fn resume_shell_after_orchestrator(
    root: &Path,
    model: Option<&str>,
    retry: Option<u32>,
) -> anyhow::Result<()> {
    use std::io::IsTerminal as _;
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    println!(
        "Interactive shell - type commands without the spec prefix \
         (e.g. list, deliver, greenfield). exit, quit, or Ctrl+C leaves."
    );
    interactive_shell_loop(root, model, retry, false, false)
}

fn interactive_shell_loop(
    root: &Path,
    model: Option<&str>,
    retry: Option<u32>,
    offer: bool,
    model_ready: bool,
) -> anyhow::Result<()> {
    use clap::CommandFactory as _;
    let history = spec_file(root, HISTORY_FILE);
    let first_session = !history.exists();
    let mut shell = ReadlineShell::open(history).map_err(|error| anyhow::anyhow!(error.0))?;
    let mut dispatch = |tokens: Vec<String>| {
        let explicit_root = tokens
            .iter()
            .any(|t| t == "--root" || t.starts_with("--root="));
        let argv = std::iter::once("spec".to_string()).chain(tokens);
        match Cli::try_parse_from(argv) {
            Err(error) => {
                let _ = error.print();
            }
            Ok(cli) => match cli.command {
                None => {
                    let _ = Cli::command().print_help();
                }
                Some(command) => {
                    let line_root = if explicit_root { &cli.root } else { root };
                    let line_model = cli.model.as_deref().or(model);
                    let line_retry = cli.retry.or(retry);
                    match execute(
                        line_root,
                        line_model,
                        line_retry,
                        cli.tools.as_deref(),
                        cli.max_rounds,
                        &command,
                    ) {
                        Ok(()) => {}
                        // The refusal already printed its JSON reply.
                        Err(error) if error.is::<NonzeroExit>() => {}
                        Err(error) => eprintln!("\x1b[31merror: {error}\x1b[0m"),
                    }
                }
            },
        }
    };
    if offer {
        let spec_exists = root.join(SPEC_PATH).exists();
        if is_greenfield_start(first_session, model_ready, spec_exists) {
            offer_greenfield(&mut shell, &mut dispatch);
        }
    }
    let summary = run_shell(&mut shell, &mut dispatch);
    if let Ending::Failed(reason) = summary.ending {
        anyhow::bail!(reason);
    }
    println!(
        "Session over - {} command{} run.",
        summary.commands,
        if summary.commands == 1 { "" } else { "s" }
    );
    Ok(())
}

/// Detect the project's primary language. Memory (a greenfield choice)
/// wins over marker detection so a polyglot tree keeps the chosen stack.
fn primary_language(root: &Path) -> anyhow::Result<Language> {
    spec_harness::workspace::primary_language(root).map_err(anyhow::Error::msg)
}

type OverlayFeatures = wiring::OverlayFeatures;
type OverlayTree = wiring::OverlayTree;

fn overlay_catalog(root: &Path) -> OverlayFeatures {
    wiring::overlay_catalog(root)
}

fn deterministic_status_gap(next_step: &str) -> bool {
    next_step.contains("spec scenario add")
        || next_step.contains("spec unittest generate")
        || next_step.contains("spec steps generate")
        || next_step.contains("spec mark-implemented")
        || next_step.contains("spec implement")
}

fn generation_service(
    root: &Path,
    model_flag: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
    caller: Caller,
) -> anyhow::Result<
    GenerationService<
        OverlayFeatures,
        OverlayTree,
        FsChangeStore,
        FsSpecRepository,
        ChatLlm,
        LiveBroker,
    >,
> {
    let language = primary_language(root)?;
    let layout = project_layout(root);
    Ok(GenerationService::new(
        overlay_catalog(root),
        wiring::overlay_sources(root, layout.module_root.as_deref()),
        wiring::change_store(root),
        wiring::spec_repository(root),
        language,
        layout,
        connected_llm(root, model_flag, caller, attempts, tools, max_rounds),
    ))
}

fn implement_service(
    root: &Path,
    model_flag: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
    caller: Caller,
) -> anyhow::Result<
    ImplementService<
        OverlayFeatures,
        OverlayTree,
        FsChangeStore,
        FsSpecRepository,
        ChatLlm,
        LiveBroker,
    >,
> {
    let language = primary_language(root)?;
    let layout = project_layout(root);
    Ok(ImplementService::new(
        overlay_catalog(root),
        wiring::overlay_sources(root, layout.module_root.as_deref()),
        wiring::change_store(root),
        wiring::spec_repository(root),
        language,
        layout,
        connected_llm(root, model_flag, caller, attempts, tools, max_rounds),
    ))
}

fn refactor_service(
    root: &Path,
    model_flag: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
) -> anyhow::Result<
    RefactorService<OverlayTree, FsChangeStore, FsSpecRepository, ChatLlm, LiveBroker>,
> {
    let language = primary_language(root)?;
    let layout = project_layout(root);
    let rounds = refactor_attempts(&config_path(root));
    Ok(RefactorService::new(
        wiring::overlay_sources(root, layout.module_root.as_deref()),
        wiring::change_store(root),
        wiring::spec_repository(root),
        language,
        layout,
        connected_llm(
            root,
            model_flag,
            Caller::Refactor,
            attempts,
            tools,
            max_rounds,
        ),
        rounds,
    ))
}

fn status_service(
    root: &Path,
    model_flag: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
) -> anyhow::Result<
    StatusService<
        OverlayFeatures,
        OverlayTree,
        FsChangeStore,
        FsSpecRepository,
        ChatLlm,
        LiveBroker,
    >,
> {
    let layout = project_layout(root);
    Ok(StatusService::new(
        overlay_catalog(root),
        wiring::overlay_sources(root, layout.module_root.as_deref()),
        wiring::change_store(root),
        wiring::spec_repository(root),
        primary_language(root)?,
        layout,
        connected_llm(
            root,
            model_flag,
            Caller::Status,
            attempts,
            tools,
            max_rounds,
        ),
    ))
}

fn spec_service(
    root: &Path,
) -> spec_harness::application::spec_service::SpecService<
    FsSpecRepository,
    spec_harness::adapters::fs_spec::FsFeatureFiles,
    spec_harness::adapters::fs_staging::FsChangeStore,
> {
    wiring::spec_service(root, detect_project_layout(root))
}

fn change_service(
    root: &Path,
) -> spec_harness::application::change_service::ChangeService<
    spec_harness::adapters::fs_staging::FsChangeStore,
    FsSpecRepository,
    OverlayFeatures,
> {
    wiring::change_service(root)
}

fn mutation_service(
    root: &Path,
    attempts: u32,
) -> SpecMutationService<
    FsSpecRepository,
    OverlayFeatures,
    spec_harness::adapters::fs_staging::FsChangeStore,
    spec_harness::adapters::fs_state::FsStateStore,
> {
    wiring::mutation_service(root, attempts)
}

fn scenario_service(
    root: &Path,
) -> spec_harness::application::scenario_service::ScenarioService<
    spec_harness::adapters::fs_staging::FsChangeStore,
    OverlayFeatures,
> {
    wiring::scenario_service(root)
}

fn tdd_service(
    root: &Path,
) -> spec_harness::application::tdd_service::TddService<
    spec_harness::adapters::fs_state::FsStateStore,
> {
    wiring::tdd_service(root)
}

fn run_test(root: &Path, args: &TestArgs) -> anyhow::Result<()> {
    // One shared language→runner dispatch (adapters::runners). The harness
    // only executes when the language's runtime is present; the runner
    // enforces that with the structured `runtime_missing` refusal.
    let runner = detect_runner(root).map_err(|message| anyhow::anyhow!(message))?;
    let filter = TestFilter {
        feature: args.feature.clone(),
        scenario: args.scenario.clone(),
    };
    tdd_reply(tdd_service(root).run_tests(runner.as_ref(), &filter))
}

/// After `spec implement` stages files, close the loop. On a terminal
/// the command offers to apply the staged changes and run the tests
/// right away; a decline - or piped stdin - still says the next
/// command in plain words instead of leaving it inside the JSON.
fn implement_follow_up(root: &Path, req_id: &str) -> anyhow::Result<()> {
    const RED: &str = "\x1b[31m";
    const GREEN: &str = "\x1b[32m";
    const RESET: &str = "\x1b[0m";
    use std::io::IsTerminal as _;
    let accepted = std::io::stdin().is_terminal()
        && HushingPrompter::new(ConsolePrompter::new(
            std::io::BufReader::new(std::io::stdin()),
            std::io::stdout(),
        ))
        .confirm("Apply the staged files and run the tests now?")
        .unwrap_or(false);
    if !accepted {
        println!(
            "Next: {GREEN}changes commit && test{RESET} - then \
             {GREEN}implement {req_id}{RESET} again if the bar stays RED."
        );
        return Ok(());
    }
    let changes = change_service(root).commit()?;
    print_json(&changes)?;
    let runner = detect_runner(root).map_err(|message| anyhow::anyhow!(message))?;
    let filter = TestFilter {
        feature: None,
        scenario: None,
    };
    match tdd_service(root).run_tests(runner.as_ref(), &filter) {
        Ok(run) => {
            let green = run.phase == "GREEN";
            print_json(&run)?;
            if green {
                println!(
                    "{GREEN}GREEN{RESET} - next: refactor (optional), then \
                     {GREEN}mark-implemented {req_id} && changes commit{RESET}."
                );
            } else {
                println!(
                    "{RED}Still RED{RESET} - the fresh failures are recorded; \
                     run {GREEN}implement {req_id}{RESET} for another model \
                     attempt, or implement by hand and rerun test."
                );
            }
            Ok(())
        }
        Err(e) => tdd_reply::<serde_json::Value>(Err(e)),
    }
}

/// Print a TDD reply, turning a missing runtime into the structured
/// `runtime_missing` refusal with a nonzero exit (signalled, not
/// `process::exit`, so the interactive shell survives it).
fn tdd_reply<T: serde::Serialize>(result: Result<T, TddError>) -> anyhow::Result<()> {
    match result {
        Ok(report) => print_json(&report),
        Err(TddError::RuntimeMissing { runtime, hint }) => {
            print_json(&serde_json::json!({
                "error": "runtime_missing",
                "runtime": runtime,
                "hint": hint,
            }))?;
            Err(NonzeroExit.into())
        }
        Err(TddError::Other(message)) => anyhow::bail!(message),
    }
}

/// Flatten a TDD error into its human message, for commands whose reply
/// is not a TDD report.
fn tdd_error_message(error: TddError) -> String {
    match error {
        TddError::Other(message) => message,
        TddError::RuntimeMissing { hint, .. } => hint,
    }
}

fn run_changes(root: &Path, command: &ChangesCommand) -> anyhow::Result<()> {
    let service = change_service(root);
    let report = match command {
        ChangesCommand::Validate => return print_json(&service.validate()?),
        ChangesCommand::Show => service.show(),
        ChangesCommand::Commit => service.commit(),
        ChangesCommand::Discard => service.discard(),
    }?;
    print_json(&report)
}

fn run_scenario(
    root: &Path,
    model: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
    command: &ScenarioCommand,
) -> anyhow::Result<()> {
    let service = scenario_service(root);
    if let ScenarioCommand::Generate { req_id, feature } = command {
        let generation = generation_service(
            root,
            model,
            attempts,
            tools,
            max_rounds,
            Caller::ScenarioGenerate,
        )?;
        let mut prompter = interactive_prompter(Prompts::Incidental);
        let report = generation.scenario_generate(
            prompter.as_mut(),
            &service,
            req_id,
            feature.as_deref(),
        )?;
        drop(prompter);
        let _ = mutation_service(root, DEFAULT_LLM_ATTEMPTS).set_feature(req_id, &report.feature);
        return print_json(&report);
    }
    let report = match command {
        ScenarioCommand::Generate { .. } => unreachable!("handled above"),
        ScenarioCommand::Add {
            feature,
            req,
            name,
            steps,
        } => {
            let report = service.add_scenario(feature, req, name, steps.clone())?;
            let _ = mutation_service(root, DEFAULT_LLM_ATTEMPTS).set_feature(req, feature);
            report
        }
        ScenarioCommand::Update {
            feature,
            name,
            req,
            steps,
        } => service.update_scenario(feature, name, steps.clone(), req.as_deref())?,
        ScenarioCommand::Delete { feature, name } => service.delete_scenario(feature, name)?,
    };
    print_json(&report)
}

fn run_spec(
    root: &Path,
    model: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
    command: &SpecCommand,
) -> anyhow::Result<()> {
    let service = spec_service(root);
    match command {
        SpecCommand::List => {
            let requirements = mutation_service(root, attempts).list_requirements()?;
            print_json(&requirements)
        }
        SpecCommand::Show { req_id } => {
            let requirement = service.get_requirement(req_id)?;
            print_json(&requirement)
        }
        SpecCommand::Validate => {
            let mut report = service.validate_spec();
            // The service words its next step for the MCP agent; the
            // shell gets the same advice naming commands. Catalog
            // structure is the exception - no command repairs it, so the
            // service's remedy stands as written.
            report.next_step = match structural_repair(&report.issues) {
                Some(repair) if report.issues.iter().all(|i| is_structural_issue(i)) => repair,
                Some(repair) => {
                    format!("{repair} Run spec reword for the remaining wording issues.")
                }
                None if report.valid => {
                    "The spec is valid. Run spec list, pick a pending requirement, and write \
                     its Gherkin scenario (spec scenario add)."
                        .into()
                }
                None => "Run spec reword to fix the issues, then run spec validate again.".into(),
            };
            let valid = report.valid;
            print_json(&report)?;
            // A gate scripted on this used to pass on an invalid spec:
            // the report said "valid": false and the exit status said
            // 0. `spec list` already exits 1 on a circular include, so
            // the binary disagreed with itself as well.
            if valid {
                Ok(())
            } else {
                Err(NonzeroExit.into())
            }
        }
        SpecCommand::Refine { req_id } => {
            let mut report = service.refine_requirement(req_id)?;
            // Same advice as the service gives the agent, naming commands
            // instead of tools.
            report.next_step = match (report.clean, report.source) {
                (false, _) => format!(
                    "Run spec reword {req_id} to address each finding, then run \
                     spec refine {req_id} again - it reviews your staged edit, so \
                     there is no need to commit between passes. Iterate until there \
                     are no findings."
                ),
                (true, STAGED) => "The staged wording reads clean. Review it with spec changes \
                     show, apply it with spec changes commit, then add the scenario \
                     with spec scenario add."
                    .into(),
                (true, _) => report.next_step,
            };
            print_json(&report)
        }
        SpecCommand::Draft {
            title,
            story,
            criterion,
            file,
        } => {
            let mutations = mutation_service(root, attempts);
            let file = file.as_deref();
            if title.is_some() || story.is_some() || !criterion.is_empty() {
                if let Some(complaint) =
                    missing_draft_flags(title.is_some(), story.is_some(), !criterion.is_empty())
                {
                    anyhow::bail!(complaint);
                }
                let (title, story) = (
                    title.as_deref().expect("checked above"),
                    story.as_deref().expect("checked above"),
                );
                let report = mutations.draft_direct_in(title, story, criterion.clone(), file)?;
                return print_json(&report);
            }
            let mut prompter = interactive_prompter(Prompts::Wizard);
            let report = match resolved_chat(root, model) {
                Some((name, chat)) => {
                    let mutations =
                        mutation_with_tools(root, attempts, Caller::SpecDraft, tools, max_rounds);
                    mutations.draft_assisted_in(prompter.as_mut(), &name, &chat, file)
                }
                None => mutations.draft_in(prompter.as_mut(), file),
            }?;
            print_json(&report)
        }
        SpecCommand::Reword {
            req_id,
            title,
            story,
            criterion,
        } => {
            let mutations = mutation_service(root, attempts);
            if title.is_some() || story.is_some() || !criterion.is_empty() {
                let report = mutations.reword_direct(
                    req_id,
                    title.clone(),
                    story.clone(),
                    criterion.clone(),
                )?;
                return print_json(&report);
            }
            let mut prompter = interactive_prompter(Prompts::Wizard);
            let report = match resolved_chat(root, model) {
                Some((name, chat)) => {
                    let mutations =
                        mutation_with_tools(root, attempts, Caller::SpecReword, tools, max_rounds);
                    mutations.reword_assisted(prompter.as_mut(), req_id, &name, &chat)
                }
                None => mutations.reword(prompter.as_mut(), req_id),
            }?;
            print_json(&report)
        }
        SpecCommand::SetFeature { req_id, file } => {
            let report = mutation_service(root, attempts).set_feature(req_id, file)?;
            print_json(&report)
        }
        SpecCommand::MarkImplemented { req_id } => {
            let report = mutation_service(root, attempts).mark_implemented(req_id)?;
            print_json(&report)
        }
        SpecCommand::Include(IncludeCommand::Add { path, from }) => {
            let report = mutation_service(root, attempts).include_add(path, from.as_deref())?;
            print_json(&report)
        }
    }
}

fn config_file(root: &Path) -> PathBuf {
    config_path(root)
}

/// The configuration dump. When the file names no model, Ollama is
/// asked which one a run would actually use, so `llm.model` shows that
/// instead of `(unset)`. A configured model skips the call entirely;
/// an unreachable or empty provider leaves the key unset.
fn config_report(root: &Path) -> spec_harness::domain::config_report::ConfigReport {
    let mut report = inspect_config(&config_file(root));
    let configured = report
        .setting(LLM_MODEL_KEY)
        .is_some_and(|setting| setting.source != ConfigSource::Default);
    if !configured
        && let SessionModel::Ready { model, .. } = model_service(root).session_model(None)
    {
        report.apply_discovered_model(&model);
    }
    report
}

fn run_feature(root: &Path, command: &FeatureCommand) -> anyhow::Result<()> {
    let catalog = overlay_catalog(root);
    match command {
        FeatureCommand::List => {
            let summaries = catalog.list()?;
            print_json(&summaries)
        }
        FeatureCommand::Show { path } => {
            let doc = catalog.read(path)?;
            print_json(&doc)
        }
        FeatureCommand::Create { path, name } => {
            let report = scenario_service(root).create_feature(path, name)?;
            print_json(&report)
        }
    }
}

fn model_service(root: &Path) -> ModelService<OllamaCatalog, TomlModelStore> {
    let store = TomlModelStore::new(config_file(root));
    let endpoint = store
        .endpoint()
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    ModelService::new(OllamaCatalog::new(endpoint), store)
}

/// Cached Ollama chat without project-memory wrapping. Greenfield wraps
/// after the scaffold scan so a brand-new project still briefs the
/// model with the files that were just written.
type CachedChat = CachedConversation<OllamaChat>;

fn cached_chat(root: &Path, model_flag: Option<&str>) -> Option<(String, CachedChat)> {
    match model_service(root).resolve(model_flag) {
        ModelResolution::Resolved { model, .. } => {
            let store = TomlModelStore::new(config_file(root));
            let endpoint = store
                .endpoint()
                .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
            let timeout = store
                .timeout_seconds()
                .map_or(DEFAULT_GENERATION_TIMEOUT, std::time::Duration::from_secs);
            let ttl = store
                .cache_ttl_seconds()
                .map_or(DEFAULT_CACHE_TTL, std::time::Duration::from_secs);
            let context = endpoint.clone();
            let chat = CachedConversation::new(
                OllamaChat::with_timeout(endpoint, timeout),
                spec_file(root, CACHE_DIR),
                ttl,
                context,
            );
            Some((model, chat))
        }
        ModelResolution::Unavailable(_) => None,
    }
}

fn run_model(root: &Path, flag: Option<&str>, command: &ModelCommand) -> anyhow::Result<()> {
    let service = model_service(root);
    match command {
        ModelCommand::List => {
            let models = service.list()?;
            if models.is_empty() {
                println!(
                    "No models installed - pull one first (e.g. `ollama pull {RECOMMENDED_MODEL}`)."
                );
                return Ok(());
            }
            for model in models {
                let size = model
                    .size_bytes
                    .map(|b| format!("{:.1} GB", b as f64 / 1_000_000_000.0))
                    .unwrap_or_else(|| "-".to_string());
                let modified = model.modified_at.as_deref().unwrap_or("-");
                println!("{}\t{}\t{}", model.name, size, modified);
            }
            Ok(())
        }
        ModelCommand::Current => match service.resolve(flag) {
            ModelResolution::Resolved { model, source } => {
                let source = match source {
                    ModelSource::Flag => "--model flag",
                    ModelSource::Config => "configuration",
                    ModelSource::OnlyInstalled => "the only installed model",
                    ModelSource::FirstInstalled => {
                        "the first installed model, this session only - \
                         persist it with: spec model use <model-name>"
                    }
                };
                println!("{model} (from {source})");
                Ok(())
            }
            ModelResolution::Unavailable(message) => anyhow::bail!(message),
        },
        ModelCommand::Use { model_name } => {
            service.choose(model_name)?;
            let file = config_file(root);
            // Canonicalize after the write so the user sees the real
            // absolute location, not the raw --root-relative path.
            let shown = file.canonicalize().unwrap_or(file);
            println!("Configured model: {model_name}");
            println!("Written to: {}", shown.display());
            Ok(())
        }
    }
}

fn run_init(root: &Path, args: &InitArgs) -> anyhow::Result<()> {
    let language = match &args.language {
        Some(answer) => Language::parse(answer).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown language {answer:?} - pick java, javascript, typescript, dotnet, or rust"
            )
        })?,
        None => {
            let mut prompter = interactive_prompter(Prompts::Incidental);
            prompt_language(prompter.as_mut())?
        }
    };
    let name = match &args.name {
        Some(name) => name.clone(),
        None => root
            .canonicalize()
            .ok()
            .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".into()),
    };
    let report =
        InitService::new(FsScaffoldWriter::new(root.to_path_buf())).init(language, &name)?;
    refresh_project_memory(root, Some(language));
    print_json(&report)
}

fn run_greenfield(root: &Path, model_flag: Option<&str>, attempts: u32) -> anyhow::Result<()> {
    let llm = cached_chat(root, model_flag)
        .map(|(model, chat)| (model, std::sync::Arc::new(chat) as DynLlm));
    let mut prompter = interactive_prompter(Prompts::Wizard);
    let report = Greenfield::new(root.to_path_buf(), llm)
        .with_llm_attempts(attempts)
        .run(prompter.as_mut())
        .map_err(|message| anyhow::anyhow!(message))?;
    print_json(&report)
}

/// `spec deliver`: resolve the target, work the plan, and let the exit
/// status carry the verdict. A plan the run did not finish exits nonzero
/// through [`NonzeroExit`], so the same command works as a gate - the
/// JSON reply has already been printed by then.
fn run_deliver(
    root: &Path,
    model_flag: Option<&str>,
    attempts: u32,
    args: &DeliverArgs,
) -> anyhow::Result<()> {
    let target = parse_target(Some(args.target.join(" ").as_str()))
        .map_err(|message| anyhow::anyhow!(message))?;
    let options = DeliverOptions {
        attempts: args.attempts.unwrap_or(DEFAULT_ATTEMPTS).max(1),
        fail_fast: args.fail_fast,
        refactor: !args.no_refactor,
        file: args.file.clone(),
    };
    let llm = cached_chat(root, model_flag)
        .map(|(model, chat)| (model, std::sync::Arc::new(chat) as DynLlm));
    // The auto-answering prompter is wrapped here, at the one place that
    // decides who answers the questions, so nothing further down has to
    // know that nobody is attached.
    let mut prompter = deliver_prompter();
    let report = Deliver::new(root.to_path_buf(), llm, options)
        .with_llm_attempts(attempts)
        .run(prompter.as_mut(), &target)
        .map_err(|message| anyhow::anyhow!(message))?;
    drop(prompter);
    let completed = report.completed;
    print_json(&report)?;
    if completed {
        Ok(())
    } else {
        Err(NonzeroExit.into())
    }
}

/// Who answers `spec deliver`'s questions: nobody. Every proposal is
/// accepted and every gate approved, so the run drives through instead of
/// stopping at a prompt. Narration still reaches the terminal.
fn deliver_prompter() -> Box<dyn Prompter> {
    Box::new(AutoPrompter::new(
        HushingPrompter::new(ConsolePrompter::new(
            std::io::BufReader::new(std::io::stdin()),
            std::io::stdout(),
        )),
        "spec deliver never stops to ask",
    ))
}

fn resolve_llm_attempts(root: &Path, flag: Option<u32>) -> u32 {
    if let Some(n) = flag {
        return n.max(1);
    }
    TomlModelStore::new(config_file(root))
        .retry()
        .and_then(|n| u32::try_from(n).ok())
        .map(|n| n.max(1))
        .unwrap_or(DEFAULT_LLM_ATTEMPTS)
}

/// Warned once when a command that prompts starts on piped stdin. A read
/// past the end of the pipe returns an empty line, which the prompter
/// cannot tell apart from pressing Enter, so every remaining prompt takes
/// its default and every confirmation declines. True of every command
/// that prompts, so every command that prompts says it.
const PIPED_STDIN_WARNING: &str = "stdin is not a terminal: prompts are read from the pipe, and \
     once it runs out every remaining prompt takes its default and every confirmation declines.";

/// What the warning adds for a wizard. The draft/reword wizard does its
/// model work and then loses it: the last question is "Stage this?" and
/// an exhausted pipe declines it, so the command stages nothing and
/// still exits 0. `spec implement`, `spec unittest generate` and
/// `spec steps generate` have no wizard and stage regardless - they used
/// to print this too, which told a scripted run its staged work had been
/// thrown away when it had not.
const PIPED_STDIN_WIZARD_WARNING: &str =
    "A wizard that ends in \"Stage this?\" therefore stages nothing.";

const PIPED_STDIN_REMEDY: &str = "Run this in a terminal to answer the prompts.";

/// Whether the command about to prompt is a wizard whose last question
/// decides if anything is kept.
#[derive(Clone, Copy, PartialEq)]
enum Prompts {
    /// Ends in "Stage this?": a declined prompt throws the work away.
    Wizard,
    /// Prompts along the way - an optional tool call, a setup question -
    /// and finishes either way.
    Incidental,
}

/// The wizard prompter. On a real terminal, rustyline gives the answers
/// full line editing - arrow keys move the cursor anywhere in the typed
/// text, Home/End jump, up-arrow recalls this session's answers. Piped
/// input (scripts, CI) falls back to plain buffered reads.
/// Both prompters are decorated here rather than inside themselves:
/// this is the one place the CLI decides who asks the questions, so it
/// is the one place that can promise every question survives the
/// spinner running above it ([`HushingPrompter`]) and that no question
/// is put to an input that has already ended
/// ([`AbortOnEndOfInput`]). A wizard written later inherits both
/// without naming either.
fn interactive_prompter(prompts: Prompts) -> Box<dyn Prompter> {
    use std::io::IsTerminal as _;
    if std::io::stdin().is_terminal()
        && let Ok(prompter) = ReadlinePrompter::new()
    {
        return Box::new(AbortOnEndOfInput::new(HushingPrompter::new(prompter)));
    }
    // stderr, so a caller parsing the JSON on stdout still can.
    eprintln!("{}", piped_stdin_warning(prompts));
    Box::new(AbortOnEndOfInput::new(HushingPrompter::new(
        ConsolePrompter::new(std::io::BufReader::new(std::io::stdin()), std::io::stdout()),
    )))
}

fn piped_stdin_warning(prompts: Prompts) -> String {
    match prompts {
        Prompts::Wizard => {
            format!("{PIPED_STDIN_WARNING} {PIPED_STDIN_WIZARD_WARNING} {PIPED_STDIN_REMEDY}")
        }
        Prompts::Incidental => format!("{PIPED_STDIN_WARNING} {PIPED_STDIN_REMEDY}"),
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", speak_cli(&serde_json::to_string_pretty(value)?));
    Ok(())
}

/// What is wrong with a half-given `spec draft`, said in terms of the
/// flags actually typed.
///
/// The old message picked whichever flag it happened to check first,
/// so `--title X` on its own was answered with "spec draft --story
/// requires --title and --criterion" - naming a flag the user had not
/// typed and demanding one they had.
fn missing_draft_flags(title: bool, story: bool, criterion: bool) -> Option<String> {
    let named = |present: bool, flag: &'static str| present.then_some(flag);
    let supplied: Vec<&str> = [
        named(title, "--title"),
        named(story, "--story"),
        named(criterion, "--criterion"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let missing: Vec<&str> = [
        named(!title, "--title"),
        named(!story, "--story"),
        named(!criterion, "--criterion"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "spec draft with {} also needs {}. Give all three, or none of them \
         to be asked question by question.",
        list_flags(&supplied),
        list_flags(&missing),
    ))
}

fn list_flags(flags: &[&str]) -> String {
    match flags {
        [] => "nothing".into(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Every MCP tool the shell has a command for.
///
/// The services word their advice for the agent, which calls tools.
/// None of those names exist on the shell, so a student who follows
/// the advice literally types something that does not run. Tools with
/// no CLI equivalent are deliberately absent - leaving the tool name
/// visible is better than inventing a command.
const CLI_FOR_TOOL: [(&str, &str); 23] = [
    ("requirement_mark_implemented", "spec mark-implemented"),
    ("step_definitions_find", "spec steps missing"),
    ("step_definition_create", "spec steps generate"),
    ("requirement_reword", "spec reword"),
    ("refine_requirement", "spec refine"),
    ("list_requirements", "spec list"),
    ("unit_test_create", "spec unittest generate"),
    ("changes_validate", "spec changes validate"),
    ("scenario_update", "spec scenario update"),
    ("scenario_delete", "spec scenario delete"),
    ("get_requirement", "spec show"),
    ("changes_discard", "spec changes discard"),
    ("project_inspect", "spec inspect"),
    ("start_refactor", "spec refactor"),
    ("changes_commit", "spec changes commit"),
    ("feature_create", "spec feature create"),
    ("get_tdd_state", "spec state"),
    ("validate_spec", "spec validate"),
    ("feature_list", "spec feature list"),
    ("feature_read", "spec feature show"),
    ("scenario_add", "spec scenario add"),
    ("changes_show", "spec changes show"),
    ("run_tests", "spec test"),
];

/// Command phrases the services write without their `spec ` prefix, so
/// "Review with changes show" and "Review with spec changes show" do
/// not both ship. Only unambiguous multi-word phrases are listed:
/// bare words like "validate" and "list" are ordinary English as often
/// as they are commands, and guessing wrong reads worse than leaving
/// them be.
const BARE_CLI_PHRASES: [&str; 6] = [
    "changes validate",
    "changes discard",
    "changes commit",
    "changes show",
    "run validate",
    "run list",
];

/// Rewrite every `nextStep` in a reply into the shell's dialect.
///
/// Done here, at the one place the CLI prints a reply, rather than in
/// each command handler: there are two dozen replies carrying advice
/// and they should all speak the same way. The MCP server builds its
/// own payloads and never passes through here, so the tool-facing
/// wording it promises is untouched.
///
/// Works on the rendered text rather than a parsed `serde_json::Value`
/// because that type sorts its keys, and the order the reports declare
/// their fields in is how they are meant to be read.
fn speak_cli(rendered: &str) -> String {
    rendered
        .lines()
        .map(translated_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// One rendered line. Pretty JSON puts each key and its value on a
/// line of their own, and a string value is escaped onto that one
/// line, so a `nextStep` can be swapped without reparsing the reply.
fn translated_line(line: &str) -> Cow<'_, str> {
    const KEY: &str = "\"nextStep\": ";
    let Some((indent, rest)) = line.split_once(KEY) else {
        return Cow::Borrowed(line);
    };
    // Only a key sits alone at the start of its line; an occurrence
    // inside some other value has text in front of it.
    if !indent.chars().all(char::is_whitespace) {
        return Cow::Borrowed(line);
    }
    let (literal, tail) = match rest.strip_suffix(',') {
        Some(literal) => (literal, ","),
        None => (rest, ""),
    };
    let Ok(text) = serde_json::from_str::<String>(literal) else {
        return Cow::Borrowed(line);
    };
    let spoken = serde_json::to_string(&cli_next_step(&text)).expect("a string always renders");
    Cow::Owned(format!("{indent}{KEY}{spoken}{tail}"))
}

/// One `nextStep`, in commands a reader can paste.
fn cli_next_step(text: &str) -> String {
    let mut out = text.to_string();
    for (tool, command) in CLI_FOR_TOOL {
        if !out.contains(tool) {
            continue;
        }
        out = out.replace(&format!("`{tool}`"), command);
        out = out.replace(tool, command);
        // "Call spec test" is not how anyone says it.
        out = out.replace(&format!("Call {command}"), &format!("Run {command}"));
        out = out.replace(&format!("call {command}"), &format!("run {command}"));
    }
    for phrase in BARE_CLI_PHRASES {
        out = prefix_with_spec(&out, phrase);
    }
    out
}

/// `phrase` given its `spec ` prefix wherever it does not already have
/// one, so no reply says "spec spec changes show".
fn prefix_with_spec(text: &str, phrase: &str) -> String {
    let bare = phrase.strip_prefix("run ").unwrap_or(phrase);
    let lead = &phrase[..phrase.len() - bare.len()];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(phrase) {
        let end = at + phrase.len();
        let (before, after) = (&rest[..at], &rest[end..]);
        // "the changes shown by" does not name the changes show command.
        if after.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-') {
            out.push_str(&rest[..end]);
            rest = after;
            continue;
        }
        out.push_str(before);
        out.push_str(lead);
        if !(lead.is_empty() && before.ends_with("spec ")) {
            out.push_str("spec ");
        }
        out.push_str(bare);
        rest = after;
    }
    out.push_str(rest);
    out
}

type ChatLlm = MemoryAwareConversation<CachedConversation<OllamaChat>>;
type LiveBroker = McpToolBroker<WorkflowServer>;
type LiveTools = ToolService<TomlToolStore, CachedDiscovery<LiveBroker>, FsMcpRegistry>;

fn live_broker(root: &Path) -> LiveBroker {
    let settings = tools_settings(&config_file(root));
    let servers = mcp_registry(root).load().servers;
    McpToolBroker::with_timeouts(
        WorkflowServer::new(root.to_path_buf()),
        servers,
        settings.discovery_timeout,
        settings.call_timeout,
    )
}

fn mcp_registry(root: &Path) -> FsMcpRegistry {
    FsMcpRegistry::new(
        root.to_path_buf(),
        tools_settings(&config_file(root)).mcp_config.clone(),
    )
}

fn tool_service(root: &Path) -> LiveTools {
    let settings = tools_settings(&config_file(root));
    ToolService::new(
        TomlToolStore::new(config_file(root)),
        CachedDiscovery::new(
            live_broker(root),
            spec_file(root, CACHE_DIR).join("tools"),
            settings.cache_ttl,
        ),
        mcp_registry(root),
        builtin_tool_definitions(),
    )
}

fn self_stdio_spec(root: &Path) -> anyhow::Result<ServerSpec> {
    let program = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("cannot locate this spec binary: {e}"))?;
    Ok(ServerSpec {
        name: "spec".into(),
        program: program.display().to_string(),
        args: vec![
            "mcp".into(),
            "serve".into(),
            "--root".into(),
            root.display().to_string(),
        ],
        env: vec![],
    })
}

fn call_broker(root: &Path, stdio: bool) -> anyhow::Result<LiveBroker> {
    let broker = live_broker(root);
    if stdio {
        Ok(broker.with_self_stdio(self_stdio_spec(root)?))
    } else {
        Ok(broker)
    }
}

fn parse_caller_arg(raw: Option<&str>) -> anyhow::Result<Caller> {
    spec_harness::application::tool_service::parse_caller(raw).map_err(anyhow::Error::from)
}

fn warn_unknown(unknown: &[String]) {
    for name in unknown {
        eprintln!("warning: tool {name} is named in config but not in the catalog");
    }
}

fn one_shot_overrides(
    root: &Path,
    caller: Caller,
    tools_flag: Option<&str>,
) -> spec_harness::domain::tool_profile::ProfileOverrides {
    let mut overrides = TomlToolStore::new(config_file(root)).overrides();
    if let Some(flag) = tools_flag {
        let names: Vec<String> = flag
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        overrides.replace.insert(caller.key().into(), names);
    }
    overrides
}

fn connected_llm(
    root: &Path,
    model_flag: Option<&str>,
    caller: Caller,
    attempts: u32,
    tools_flag: Option<&str>,
    max_rounds_flag: Option<u32>,
) -> Option<ResolvedLlm<ChatLlm, LiveBroker>> {
    let (model, chat) = resolved_chat(root, model_flag)?;
    let settings = tools_settings(&config_file(root));
    let catalog = tool_service(root).catalog(false, false);
    warn_unknown(&catalog.problems);
    let resolved = resolve(
        caller,
        &one_shot_overrides(root, caller, tools_flag),
        &catalog.tools,
    );
    warn_unknown(&resolved.unknown);
    Some(ResolvedLlm::connected(
        model,
        chat,
        live_broker(root),
        resolved.tools,
        AgentConfig::new(
            caller.section(),
            attempts,
            max_rounds_flag.unwrap_or(settings.max_rounds),
            settings.confirm,
        ),
    ))
}

fn resolved_chat(root: &Path, model_flag: Option<&str>) -> Option<(String, ChatLlm)> {
    cached_chat(root, model_flag).map(|(model, chat)| {
        (
            model,
            MemoryAwareConversation::new(chat, refresh_project_memory(root, None).brief()),
        )
    })
}

fn mutation_with_tools(
    root: &Path,
    attempts: u32,
    caller: Caller,
    tools_flag: Option<&str>,
    max_rounds_flag: Option<u32>,
) -> SpecMutationService<
    FsSpecRepository,
    OverlayFeatures,
    spec_harness::adapters::fs_staging::FsChangeStore,
    spec_harness::adapters::fs_state::FsStateStore,
> {
    let settings = tools_settings(&config_file(root));
    let catalog = tool_service(root).catalog(false, false);
    warn_unknown(&catalog.problems);
    let resolved = resolve(
        caller,
        &one_shot_overrides(root, caller, tools_flag),
        &catalog.tools,
    );
    warn_unknown(&resolved.unknown);
    mutation_service(root, attempts).with_tool_loop(
        resolved.tools,
        max_rounds_flag.unwrap_or(settings.max_rounds),
        settings.confirm,
        Box::new(live_broker(root)),
    )
}

fn run_tools(root: &Path, command: &ToolsCommand) -> anyhow::Result<()> {
    let service = tool_service(root);
    match command {
        ToolsCommand::List {
            for_caller,
            offline,
            refresh,
            json,
        } => {
            if let Some(raw) = for_caller {
                let caller = parse_caller_arg(Some(raw))?;
                let (resolved, problems) = service.list_for(caller, *refresh, *offline);
                for problem in &problems {
                    eprintln!("warning: {problem}");
                }
                warn_unknown(&resolved.unknown);
                if *json {
                    return print_json(&resolved.tools.iter().map(|t| &t.name).collect::<Vec<_>>());
                }
                for tool in &resolved.tools {
                    println!(
                        "{}\t{}",
                        tool.name,
                        ToolService::<TomlToolStore, CachedDiscovery<LiveBroker>, FsMcpRegistry>::origin_label(
                            &tool.origin
                        )
                    );
                }
                return Ok(());
            }
            let list = service.catalog(*refresh, *offline);
            for problem in &list.problems {
                eprintln!("warning: {problem}");
            }
            if *json {
                return print_json(&list.tools.iter().map(|t| &t.name).collect::<Vec<_>>());
            }
            for tool in &list.tools {
                println!(
                    "{}\t{}",
                    tool.name,
                    ToolService::<TomlToolStore, CachedDiscovery<LiveBroker>, FsMcpRegistry>::origin_label(
                        &tool.origin
                    )
                );
            }
            if *offline {
                for name in &list.undiscovered {
                    eprintln!("warning: {name} (not discovered — run spec tools refresh)");
                }
            }
            Ok(())
        }
        ToolsCommand::Profiles { json } => {
            let (views, problems) = service.profiles(false);
            for problem in &problems {
                eprintln!("warning: {problem}");
            }
            if *json {
                return print_json(
                    &views
                        .iter()
                        .map(|v| {
                            serde_json::json!({
                                "caller": v.caller,
                                "tools": v.tools,
                                "unknown": v.unknown,
                            })
                        })
                        .collect::<Vec<_>>(),
                );
            }
            for view in views {
                warn_unknown(&view.unknown);
                println!(
                    "{}\t{}\t{}",
                    view.caller,
                    view.tools.len(),
                    view.tools.join(", ")
                );
            }
            Ok(())
        }
        ToolsCommand::Show { name } => {
            let shown = service.show(name)?;
            println!(
                "{}\t{}",
                shown.name,
                ToolService::<TomlToolStore, CachedDiscovery<LiveBroker>, FsMcpRegistry>::origin_label(
                    &shown.origin
                )
            );
            println!("{}", shown.description);
            println!("{}", serde_json::to_string_pretty(&shown.schema)?);
            Ok(())
        }
        ToolsCommand::Enable { name, for_caller } => {
            let caller = parse_caller_arg(for_caller.as_deref())?;
            service.enable(name, caller)?;
            println!("Enabled {name} for {}", caller.key());
            Ok(())
        }
        ToolsCommand::Disable { name, for_caller } => {
            let caller = parse_caller_arg(for_caller.as_deref())?;
            service.disable(name, caller)?;
            println!("Disabled {name} for {}", caller.key());
            Ok(())
        }
        ToolsCommand::Refresh => {
            let list = service.catalog(true, false);
            for problem in &list.problems {
                eprintln!("warning: {problem}");
            }
            println!("Refreshed {} tool(s).", list.tools.len());
            Ok(())
        }
        ToolsCommand::Servers { json } => {
            let load = service.registry();
            if *json {
                return print_json(&serde_json::json!({
                    "path": load.path,
                    "servers": load.servers.iter().map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "program": s.program,
                            "args": s.args,
                        })
                    }).collect::<Vec<_>>(),
                    "problems": load.problems,
                }));
            }
            match &load.path {
                Some(path) => println!("{path}"),
                None => println!("(no mcp.json found)"),
            }
            for server in &load.servers {
                println!(
                    "{}\t{}\t{}",
                    server.name,
                    server.program,
                    server.args.join(" ")
                );
            }
            for problem in &load.problems {
                eprintln!("warning: {problem}");
            }
            Ok(())
        }
    }
}

fn run_mcp(root: &Path, command: &McpCommand) -> anyhow::Result<()> {
    match command {
        McpCommand::Serve => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(spec_harness::mcp::serve_stdio(root.to_path_buf()))
        }
        McpCommand::Tools { stdio, json } => {
            let broker = call_broker(root, *stdio)?;
            let tools = broker.list_builtin_tools()?;
            if *json {
                return print_json(&tools.iter().map(|t| &t.name).collect::<Vec<_>>());
            }
            for tool in tools {
                println!("{}", tool.name);
            }
            Ok(())
        }
        McpCommand::Call {
            tool,
            arg,
            args,
            stdio,
            json,
        } => {
            let mut catalog = builtin_tool_definitions();
            if spec_harness::domain::tools::find(&catalog, tool).is_err() {
                catalog = tool_service(root).catalog(false, false).tools;
            }
            let arguments = ToolCallService::merge_arguments(args.as_deref(), arg)?;
            ToolCallService::prepare(&catalog, tool, &arguments)?;
            let broker = call_broker(root, *stdio)?;
            let envelope = ToolCallService::call(&broker, &catalog, tool, &arguments)?;
            if *json {
                print_json(&envelope)?;
            } else {
                println!("{}", envelope.content);
            }
            if envelope.is_error {
                return Err(NonzeroExit.into());
            }
            Ok(())
        }
    }
}

fn run_ask(
    root: &Path,
    model: Option<&str>,
    attempts: u32,
    tools: Option<&str>,
    max_rounds: Option<u32>,
    task: &[String],
    json: bool,
) -> anyhow::Result<()> {
    let Some(llm) = connected_llm(root, model, Caller::Ask, attempts, tools, max_rounds) else {
        anyhow::bail!("no model resolved - pull one with ollama and run spec model use <name>");
    };
    let mut prompter = interactive_prompter(Prompts::Incidental);
    let joined = task.join(" ").trim().to_string();
    if joined.is_empty() {
        use std::io::IsTerminal as _;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("spec ask needs a task (or a tty for the multi-turn prompt)");
        }
        loop {
            let line = prompter.ask("ask> ")?;
            let line = line.trim().to_string();
            if line.is_empty() || line == "exit" || line == "quit" {
                break;
            }
            ask_once(&llm, prompter.as_mut(), &line, json)?;
        }
        return Ok(());
    }
    ask_once(&llm, prompter.as_mut(), &joined, json)
}

fn ask_once(
    llm: &ResolvedLlm<ChatLlm, LiveBroker>,
    prompter: &mut dyn Prompter,
    task: &str,
    json: bool,
) -> anyhow::Result<()> {
    let prompt = ask_prompt(task);
    let answer = llm
        .ask(
            prompter,
            &prompt,
            |text| {
                let body = text.trim();
                if body.is_empty() {
                    Err("the answer was empty".into())
                } else {
                    Ok(body.to_string())
                }
            },
            |_, _, _| {},
        )
        .map_err(|e| match e {
            spec_harness::application::LlmReplyError::Call(error) => anyhow::anyhow!(error.0),
            spec_harness::application::LlmReplyError::Invalid { reason } => anyhow::anyhow!(reason),
        })?;
    if json {
        print_json(&serde_json::json!({ "answer": answer }))
    } else {
        println!("{answer}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wording the student guides quote verbatim, for the command
    /// they quote it about.
    #[test]
    fn a_wizard_on_a_pipe_is_told_its_work_will_be_thrown_away() {
        assert_eq!(
            piped_stdin_warning(Prompts::Wizard),
            "stdin is not a terminal: prompts are read from the pipe, and once it runs out \
             every remaining prompt takes its default and every confirmation declines. \
             A wizard that ends in \"Stage this?\" therefore stages nothing. \
             Run this in a terminal to answer the prompts."
        );
    }

    /// `spec unittest generate` and `spec steps generate` have no
    /// wizard and stage either way. Telling a scripted run they staged
    /// nothing was simply false.
    #[test]
    fn a_command_that_stages_regardless_does_not_claim_it_stages_nothing() {
        let warning = piped_stdin_warning(Prompts::Incidental);
        assert!(warning.starts_with("stdin is not a terminal:"), "{warning}");
        assert!(warning.ends_with("Run this in a terminal to answer the prompts."));
        assert!(!warning.contains("stages nothing"), "{warning}");
    }

    #[test]
    fn an_unexpanded_root_uses_spec_project_dir_or_the_launch_directory() {
        let placeholder = PathBuf::from("${CLAUDE_PROJECT_DIR:-.}");
        assert_eq!(
            resolve_project_root(placeholder.clone(), Some("/work/kata")),
            PathBuf::from("/work/kata")
        );
        assert_eq!(resolve_project_root(placeholder, None), PathBuf::from("."));
        assert_eq!(
            resolve_project_root(PathBuf::from("${SPEC_PROJECT_DIR}"), Some("  ")),
            PathBuf::from(".")
        );
        assert_eq!(
            resolve_project_root(PathBuf::from("."), Some("/work/kata")),
            PathBuf::from(".")
        );
        assert_eq!(
            resolve_project_root(PathBuf::from("/explicit"), Some("/work/kata")),
            PathBuf::from("/explicit")
        );
    }

    #[test]
    fn the_tdd_phase_advice_names_commands_rather_than_tools() {
        assert_eq!(
            cli_next_step("No tests have been run yet. Call run_tests to establish a baseline."),
            "No tests have been run yet. Run spec test to establish a baseline."
        );
        assert_eq!(
            cli_next_step(
                "All tests pass. Either call start_refactor to clean up, or call \
                 get_requirement for the next pending requirement and write a failing \
                 test for it."
            ),
            "All tests pass. Either run spec refactor to clean up, or run spec show \
             for the next pending requirement and write a failing test for it."
        );
    }

    #[test]
    fn a_tool_name_in_backticks_is_replaced_along_with_its_backticks() {
        assert_eq!(
            cli_next_step("Call `validate_spec`, then `get_requirement`."),
            "Run spec validate, then spec show."
        );
    }

    /// `spec draft` said "Review with spec changes show" and `spec
    /// reword` said "Review with changes show". Only one of them can
    /// be pasted.
    #[test]
    fn a_command_named_without_its_prefix_gains_one() {
        assert_eq!(
            cli_next_step(
                "Review with changes show, run validate, then apply with changes commit."
            ),
            "Review with spec changes show, run spec validate, then apply with spec changes commit."
        );
    }

    #[test]
    fn a_command_that_already_has_its_prefix_does_not_get_a_second() {
        let already = "Review with spec changes show, apply with spec changes commit.";
        assert_eq!(cli_next_step(already), already);
    }

    /// The prefixer matches a phrase, not the letters that start one.
    #[test]
    fn an_english_word_that_starts_like_a_command_is_left_alone() {
        let prose = "Read the changes shown above before you decide.";
        assert_eq!(cli_next_step(prose), prose);
    }

    #[test]
    fn a_reply_without_advice_passes_through_untouched() {
        let reply = "{\n  \"valid\": true\n}";
        assert_eq!(speak_cli(reply), reply);
    }

    #[test]
    fn only_the_next_step_field_is_rewritten_and_the_rest_keeps_its_order() {
        let rendered = "{\n  \"phase\": \"START\",\n  \
                        \"nextStep\": \"Call run_tests to start.\",\n  \
                        \"note\": \"Call run_tests to start.\"\n}";
        assert_eq!(
            speak_cli(rendered),
            "{\n  \"phase\": \"START\",\n  \
             \"nextStep\": \"Run spec test to start.\",\n  \
             \"note\": \"Call run_tests to start.\"\n}"
        );
    }

    #[test]
    fn a_next_step_holding_quotes_survives_the_rewrite() {
        let rendered = "{\n  \"nextStep\": \"Answer \\\"Stage this?\\\", then call run_tests.\"\n}";
        assert_eq!(
            speak_cli(rendered),
            "{\n  \"nextStep\": \"Answer \\\"Stage this?\\\", then run spec test.\"\n}"
        );
    }

    /// The old message answered `--title X` by naming `--story` as the
    /// flag that was supplied and `--title` as the one missing.
    #[test]
    fn a_half_given_draft_is_told_what_it_typed_and_what_is_missing() {
        let complaint = missing_draft_flags(true, false, false).expect("a complaint");
        assert!(
            complaint.starts_with("spec draft with --title also needs --story and --criterion."),
            "{complaint}"
        );
        assert_eq!(
            missing_draft_flags(false, false, true).expect("a complaint"),
            "spec draft with --criterion also needs --title and --story. Give all three, \
             or none of them to be asked question by question."
        );
        assert!(
            missing_draft_flags(true, true, false)
                .expect("a complaint")
                .starts_with("spec draft with --title and --story also needs --criterion.")
        );
    }

    #[test]
    fn a_fully_given_draft_has_no_complaint() {
        assert_eq!(missing_draft_flags(true, true, true), None);
    }

    /// Advice is only worth rewriting if what it names can be run.
    /// Caught `spec project inspect`, which does not exist - the
    /// command is `spec inspect`.
    #[test]
    fn every_command_the_table_names_is_a_command_that_exists() {
        use clap::CommandFactory as _;
        for (tool, command) in CLI_FOR_TOOL {
            let mut node = Cli::command();
            let words = command.strip_prefix("spec ").expect("a spec command");
            for word in words.split(' ') {
                node.build();
                node = node
                    .find_subcommand(word)
                    .unwrap_or_else(|| panic!("{tool} -> {command:?}: no subcommand {word:?}"))
                    .clone();
            }
        }
    }

    /// Two tools mapping to the same command, or a tool name that is
    /// part of another, would make the rewrite order matter.
    #[test]
    fn no_tool_name_in_the_table_contains_another() {
        for (tool, _) in CLI_FOR_TOOL {
            for (other, _) in CLI_FOR_TOOL {
                assert!(
                    tool == other || !tool.contains(other),
                    "{other} hides inside {tool}"
                );
            }
        }
    }
}

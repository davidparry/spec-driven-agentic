# Global flags

These flags are accepted by `bdd` itself and by every command. Place
them anywhere on the command line.

## `--root <ROOT>`

The project root — the directory where `requirements/requirements.json`
(the root of the [spec catalog](spec-format.md)) and `.bdd.toml`
live, and the base for every relative path the harness reads or writes.
Defaults to the current directory. MCP clients that cannot see argv can
call `project_root` to read the same directory as an absolute path.

```bash
bdd --root ~/code/calculator spec list
bdd spec list --root ~/code/calculator   # same thing
```

In the [interactive shell](interactive-shell.md), commands inherit the
shell's `--root` unless a line supplies its own.

## `--model <MODEL>`

Override the LLM model for this invocation only. This wins over the
configured model and over discovery, and is never written to
configuration:

```bash
bdd --model qwen3.8-flash-next:125b-mlx steps generate
bdd --model llama3:8b greenfield   # another model; mileage will vary
```

Model resolution order (see [bdd model](commands/model.md) for the
full story):

1. `--model` flag — this invocation only.
2. `model` in `.bdd.toml` — the persisted project choice.
3. Discovery — the first installed Ollama model, session-only.

## `--debug`

Verbose diagnostic logging: the full prompts sent to the model and its
responses, cache hits and misses, resolved configuration, MCP tool
calls, and test-runner activity. Without the flag only high-level
lifecycle events (info and above) are logged.

Diagnostics are written to daily-rolling files under `.bdd-log/` in
the project root (gitignored; safe to delete at any time) — stdout and
stderr are never touched, so JSON output, the MCP stdio protocol, and
user-facing messages stay clean either way. Log writes go through an
in-memory queue drained by a worker thread, so logging never blocks
the command. If the log directory cannot be created, diagnostics fall
back to stderr.

```bash
bdd --debug implement REQ-003             # full prompts and replies in the log
tail -f .bdd-log/bdd.log.$(date +%F)      # watch the diagnostics live
```

The `RUST_LOG` environment variable overrides both the default and
`--debug` with per-module directives, e.g.
`RUST_LOG=bdd_harness::adapters=trace bdd test`.

## `--retry <N>`

How many times to try a model call when the reply fails validation
(not a JSON array of requirements, not a complete rewording object,
not a usable file update, empty advice, and so on). Default is **3**.
Each retry keeps the original prompt and appends the invalid reply
plus the reason, so the model can correct itself. Transport failures
(Ollama unreachable) are not retried.

```bash
bdd --retry 5 greenfield
bdd spec draft --retry 1   # one attempt, then the usual fallback
```

`--retry` wins over `retry` under `[llm]` in `.bdd.toml`. A missing
or zero config value uses the default of 3. In the interactive shell,
commands inherit the shell's `--retry` unless a line supplies its own.

## `--tools <NAMES>`

Replace this command's tool profile for one run with a comma-separated
list of catalog names (`validate_spec`, `playwright:browser_navigate`,
`builtin:run_tests`). Default profiles live in code and in
`.bdd.toml`; see [`bdd tools`](commands/tools.md). [`bdd config`](commands/config.md)
prints the resolved keys and their source.

```bash
bdd --tools list_requirements,get_requirement status
```

## `--max-rounds <N>`

Override `[tools] max_rounds` for one run: how many tool-call rounds
`Agent::ask` may take before it must return a parsed reply.

## `-V`, `--version`

Prints the version compiled into the binary (from `Cargo.toml` at
build time).

## `-h`, `--help`

Every command and subcommand answers `--help` with its synopsis,
arguments, and flags.

## Exit status

- `0` — success (including replies whose JSON reports e.g. an invalid
  spec: the *command* succeeded).
- `1` — the command itself failed or refused: unknown requirement id,
  missing runtime (`runtime_missing`), unreachable Ollama for a
  model-required operation, unparseable arguments, and similar.

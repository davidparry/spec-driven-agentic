# Introduction

`bdd` is one native binary for the whole spec-driven loop:

```text
spec → Gherkin scenario → RED → implement → GREEN → REFACTOR
```

The requirements spec (`requirements/requirements.json`) is the source
of truth, and the discipline is enforced by tooling, not by
convention. The root file is a [catalog](spec-format.md) that may
include child spec files, so the backlog scales past one file while
one entry point stays fixed. The CLI validates the spec's structure,
quality-gates its wording, turns approved requirements into tagged
Gherkin scenarios, runs the tests through your project's own build
tool, and tracks the persistent RED/GREEN/REFACTOR phase between
invocations. It also
embeds an MCP server (`bdd mcp serve`) so AI agents can drive the same
workflow through typed tools — with no filesystem or shell escape
hatches.

## How to read this manual

- **Using bdd** covers the concepts that span commands: the global
  flags, the interactive shell, the workflow phases, and the staged
  changes model that protects your working tree.
- **Command reference** documents every command, subcommand, and flag,
  with realistic examples and the exact JSON reply shapes.

Use the search icon (or press <kbd>S</kbd>) to search the whole manual.

## Conventions

- Commands are shown as you would type them in a shell. Inside the
  [interactive shell](interactive-shell.md) the leading `bdd` is
  optional.
- Replies are JSON on stdout unless a command is inherently
  interactive. Every reply carries a `nextStep` field that says what
  to do next — the same guidance an AI agent receives over MCP.
- Names in parentheses in help text, like `(run_tests)`, are the
  matching MCP tool names — frozen contracts kept by
  `cli/tests/mcp_conformance.rs` and mcp-client `ToolPlan`.

## Supported languages

| Language | Build tool | BDD framework | Runtime probed |
| --- | --- | --- | --- |
| Java | Maven | Cucumber-JVM | `mvn` |
| JavaScript | npm | Cucumber-JS | `node` |
| TypeScript | npm + ts-node | Cucumber-JS | `node` |
| .NET | dotnet | Reqnroll | `dotnet` |
| Rust | Cargo | cucumber-rs | `cargo` |

The CLI only ever *executes* when the language's runtime is present;
it reports a structured `runtime_missing` refusal otherwise and never
installs anything.

LLM-backed generation uses local [Ollama](https://ollama.com). The
model this CLI is developed and run against is
`qwen3.8-flash-next:125b-mlx` — see [Getting started](getting-started.md)
and [`bdd model`](commands/model.md). Your mileage will vary with
other models, especially those not trained for development work.

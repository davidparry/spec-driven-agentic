# bdd mcp

The workshop MCP server. This is the same workflow the CLI offers a
human, exposed to AI agents as typed tools over the Model Context
Protocol. Cursor, Claude, the bundled `smoke-test.jar`, and `bdd mcp
call` all talk to this process.

```text
Usage: bdd mcp [OPTIONS] <COMMAND>

Commands:
  serve  Serve the MCP tools over stdio
  tools  List tools over one throwaway session
  call   Call one tool over one throwaway session
```

Wire identity is `spec-driven-server` / `1.0.0` (title `Spec Driven`:
serves spec-driven TDD and BDD tools; the requirements spec is the
source of truth; website
https://davidparry.github.io/spec-driven-agentic/; icon
https://davidparry.github.io/spec-driven-agentic/assets/bdd-cli-mark.png). Frozen seven-tool
**reply shapes** are owned by
`cli/tests/mcp_conformance.rs` and smoke-test's `ToolPlan` — not by a
separate Java server.

The server is stdio only: JSON-RPC on stdin/stdout. Cursor, Claude,
Inspector, and `smoke-test.jar` launch it as a child process. It prefers
protocol `2026-07-28`: no `initialize` handshake. `bdd mcp call` and
`bdd mcp tools` open a session with `server/discover` and per-request
`_meta`. The Java smoke walkthrough starts at `tools/list` (it does not
call `initialize`). A host that still sends `initialize` is answered by
rmcp for compatibility; this project's own Rust clients do not.

---

## bdd mcp serve

Serve the MCP tools over stdio. The process reads JSON-RPC on stdin
and writes replies on stdout, so an MCP client (Cursor, Claude
Desktop, `smoke-test.jar`) launches it as a child process — you
normally never run it by hand.

```bash
bdd mcp serve --root /path/to/project
```

Client configuration (Cursor's `mcp.json` shown; others are
equivalent):

```json
{
  "mcpServers": {
    "spec-driven-server": {
      "command": "bdd",
      "args": ["mcp", "serve", "--root", "${workspaceFolder}"]
    }
  }
}
```

Cursor sees **all 25 tools**, including staging. CLI commands that
call a model attach a **narrower profile** (`bdd tools profiles`) —
typically 3–7 tools — so a local model is not offered commit or
mark-implemented.

## bdd mcp tools

List the built-in tools over one throwaway session. Default is an
in-process loopback; `--stdio` spawns `bdd mcp serve` as a child
(the same bytes Cursor would read).

```bash
bdd mcp tools
bdd mcp tools --stdio --json
```

## bdd mcp call

Invoke one tool, print the result, exit. No narration, no tokens.

```bash
bdd mcp call get_tdd_state
bdd mcp call get_requirement --arg id=REQ-003
bdd mcp call list_requirements --json
bdd mcp call run_tests --stdio
```

`--arg key=value` is always a string unless the value parses as JSON.
`--args '{"id":"REQ-003"}'` merges a JSON object.

## The tools served

Twenty-five tools in three groups. There is no `spec_draft` or
`implement` MCP tool: a **new** requirement is still drafted by the
human (`bdd spec draft`) and Cursor writes production Java. Rewording an
existing requirement, Gherkin, steps, unit-test scaffolds,
mark-implemented, and staging all go through tools. Generation over MCP
is **template-only** (`source: "template"`).

### Frozen seven (reply shapes stay)

| MCP tool | CLI equivalent |
| --- | --- |
| `list_requirements` | [`bdd spec list`](spec.md#bdd-spec-list) |
| `get_requirement` | [`bdd spec show`](spec.md#bdd-spec-show) |
| `validate_spec` | [`bdd spec validate`](spec.md#bdd-spec-validate) |
| `refine_requirement` | [`bdd spec refine`](spec.md#bdd-spec-refine) |
| `run_tests` | [`bdd test`](test.md) |
| `get_tdd_state` | [`bdd state`](state.md) |
| `start_refactor` | [`bdd refactor`](refactor.md) |

### Authoring and staging

| MCP tool | CLI equivalent |
| --- | --- |
| `feature_list` / `feature_read` / `feature_create` | [`bdd feature`](feature.md) |
| `scenario_add` / `scenario_update` / `scenario_delete` | [`bdd scenario`](scenario.md) |
| `changes_show` / `changes_commit` / `changes_discard` | [`bdd changes`](changes.md) |
| `changes_validate` | [`bdd validate`](validate.md) (staged-wins; frozen `validate_spec` stays on disk) |
| `requirement_reword` | [`bdd spec reword`](spec.md#bdd-spec-reword) (the repair path for `validate_spec` and `refine_requirement` findings; never hand-edit the spec file) |
| `requirement_mark_implemented` | [`bdd spec mark-implemented`](spec.md#bdd-spec-mark-implemented) |
| `step_definitions_find` | [`bdd steps missing`](steps.md#bdd-steps-missing) |
| `step_definition_create` | [`bdd steps generate`](steps.md#bdd-steps-generate) (template only) |
| `unit_test_create` | [`bdd unittest generate`](unittest.md) (template only; arg is `req_id`) |

### Inspect

| MCP tool | CLI equivalent |
| --- | --- |
| `project_root` | `--root` (the absolute directory this process was started with) |
| `project_inspect` | [`bdd inspect`](inspect.md) |
| `command_run` | — (MCP and the `implement` profile; see below) |

## command_run: the guarded command line

`command_run` lets an agent run one dev-tool command during the
implementation phase — building, compiling, or installing what the
failing tests need. It is not a shell. Every call passes these
guardrails, checked before anything spawns:

- **Allowlist.** The program must be one of `cargo`, `mvn`, `npm`,
  `npx`, `node`, `dotnet`, `java`, `javac`, `tsc`, given as a bare
  name (never a path). `rm`, `sudo`, `sh`, `curl`, `git`, and
  everything else is refused.
- **No shell.** The command executes directly as argv, so `;`, `&&`,
  `|`, globs, and redirection are inert text — chaining a destructive
  command onto an allowed one is unexpressible.
- **Eval escapes refused.** Flags that turn an allowed tool into
  arbitrary code execution (`node -e/--eval/-p/--print`,
  `npx -c/--call`, `npm exec`/`npm x`, Maven `exec:*` goals) are
  refused.
- **Root jail.** The process runs with `--root` as its working
  directory, and no argument may be an absolute path or contain
  `..` — the command cannot name anything outside the root.
- **RED bar only.** Commands run only during the implementation
  phase. Off a RED bar the tool refuses and points at `run_tests`.
- **Human confirm on the CLI.** When `bdd implement` offers
  `command_run`, the CLI asks before spawning. Piped/CI stdin
  declines; it never hangs.
- **Timeout and output cap.** A hard timeout (default and maximum
  300 seconds) kills a hung process; each output stream is truncated
  to its last 200 lines.

This is policy-level guardrailing, not an OS sandbox: an allowed
build tool can still run build scripts. What the policy makes
unexpressible is running destructive binaries and reaching outside
the project root.

`run_tests` during `bdd implement` sees the **working tree**, not an
unstaged patch. Commit (or apply staged files) before you trust the
bar.

## Why serve tools instead of letting the agent edit files?

- **No escape hatches.** The agent gets exactly these tools — no
  open-ended shell, no arbitrary file writes. The one command tool is
  allowlisted, jailed to the root, and phase-gated; mutations go
  through the [staging area](../staged-changes.md) for human review.
- **The discipline is in the server.** An agent cannot skip RED,
  refactor while failing, or invent requirements: the tools refuse,
  with a `nextStep` that teaches the correct move.
  `requirement_mark_implemented` is GREEN-gated and needs a tagged
  scenario.
- **State survives.** The phase machine lives on disk, so a
  reconnecting agent (or a human taking over in the CLI) continues
  from the same place.

## Flags

| Flag | Description |
| --- | --- |
| `--root <ROOT>` | Project root the served tools operate on. Defaults to the process's working directory. |
| `--model <MODEL>` | Model override for the serving session. MCP generation tools do not call Ollama; they stage templates. |

## Notes

- The server logs nothing to stdout except protocol traffic (stdout
  is the wire). Diagnostics go to stderr.
- One server serves one project root. Point different projects at
  different server entries.
- Backup Inspector: `npx @modelcontextprotocol/inspector bdd mcp serve --root $PWD`.
- See also [`bdd tools`](tools.md) and [`bdd ask`](ask.md).

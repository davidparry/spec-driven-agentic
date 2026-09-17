# spec mcp

The workshop MCP server. This is the same workflow the harness offers a
human, exposed to AI agents as typed tools over the Model Context
Protocol. Cursor, Claude, the bundled `smoke-test.jar`, and `spec mcp
call` all talk to this process.

```text
Usage: spec mcp [OPTIONS] <COMMAND>

Commands:
  serve  Serve the MCP tools over stdio
  tools  List tools over one throwaway session
  call   Call one tool over one throwaway session
```

Wire identity is `spec-driven-server` / `1.0.0` (title `Spec Driven`:
serves spec-driven TDD and BDD tools; the requirements spec is the
source of truth; website
https://davidparry.github.io/spec-driven-agentic/; icon
https://davidparry.github.io/spec-driven-agentic/assets/spec-harness-mark.png). Frozen seven-tool
**reply shapes** are owned by
`harness/tests/mcp_conformance.rs` and smoke-test's `ToolPlan` — not by a
separate Java server.

The server is stdio only: JSON-RPC on stdin/stdout. Cursor, Claude,
Inspector, `pi` (through `pi-mcp-extension`), and `smoke-test.jar` launch it
as a child process.

**The lifecycle is dual-era, and both eras work.** The server prefers
`2026-07-28`, where there is no `initialize` handshake: `spec mcp call` and
`spec mcp tools` open a session with `server/discover` and per-request
`_meta`, and the Java smoke walkthrough starts straight at `tools/list`. A
host that still sends the classic `initialize` with protocol `2025-11-25`
gets a normal handshake reply — that path is live, not a fallback stub,
which is why `.cursor/mcp.json` can set `"protocolEra": "auto"` and let the
host pick. This project's own Rust clients use the newer era.

---

## spec mcp serve

Serve the MCP tools over stdio. The process reads JSON-RPC on stdin
and writes replies on stdout, so an MCP client (Cursor, Claude
Desktop, `smoke-test.jar`) launches it as a child process — you
normally never run it by hand.

```bash
spec mcp serve --root /path/to/project
```

Client configuration (Cursor's `mcp.json` shown; others are
equivalent):

```json
{
  "mcpServers": {
    "spec-driven-server": {
      "command": "spec",
      "args": ["mcp", "serve", "--root", "${workspaceFolder}"]
    }
  }
}
```

Any MCP host can drive this server. [pi](https://pi.dev) has no MCP in core,
so it needs the `pi-mcp-extension` package; the repo ships a ready
[`.pi/mcp.json`](https://github.com/davidparry/spec-driven-agentic/blob/trunk/.pi/mcp.json),
and `pi -nbt` disables pi's own `bash`/`write`/`edit` so these tools are all
the model gets. The bridge registers them as `mcp_<server>_<tool>`, so
`run_tests` arrives as `mcp_spec_driven_server_run_tests`.

Cursor sees **all 25 tools**, including staging, and so does `pi -nbt`.
Harness commands that call a model attach a **narrower profile**
(`spec tools profiles`) — 3–7 tools for a generating command, 12 for the
read-only `spec ask` — so a local model is not offered commit or
mark-implemented.

## spec mcp tools

List the built-in tools over one throwaway session. Default is an
in-process loopback; `--stdio` spawns `spec mcp serve` as a child
(the same bytes Cursor would read).

```bash
spec mcp tools
spec mcp tools --stdio --json
```

## spec mcp call

Invoke one tool, print the result, exit. No narration, no tokens.

```bash
spec mcp call get_tdd_state
spec mcp call get_requirement --arg id=REQ-003
spec mcp call list_requirements --json
spec mcp call run_tests --stdio
```

`--arg key=value` is always a string unless the value parses as JSON.
`--args '{"id":"REQ-003"}'` merges a JSON object.

## The tools served

Twenty-five tools in three groups. There is no `spec_draft` or
`implement` MCP tool: a **new** requirement is still drafted by the
human (`spec draft`) and Cursor writes production Java. Rewording an
existing requirement, Gherkin, steps, unit-test scaffolds,
mark-implemented, and staging all go through tools. Generation over MCP
is **template-only** (`source: "template"`).

### Frozen seven (reply shapes stay)

| MCP tool | Harness equivalent |
| --- | --- |
| `list_requirements` | [`spec list`](spec.md#spec-list) |
| `get_requirement` | [`spec show`](spec.md#spec-show) |
| `validate_spec` | [`spec validate`](spec.md#spec-validate) |
| `refine_requirement` | [`spec refine`](spec.md#spec-refine) |
| `run_tests` | [`spec test`](test.md) |
| `get_tdd_state` | [`spec state`](state.md) |
| `start_refactor` | [`spec refactor`](refactor.md) |

### Authoring and staging

| MCP tool | Harness equivalent |
| --- | --- |
| `feature_list` / `feature_read` / `feature_create` | [`spec feature`](feature.md) |
| `scenario_add` / `scenario_update` / `scenario_delete` | [`spec scenario`](scenario.md) |
| `changes_show` / `changes_commit` / `changes_discard` | [`spec changes`](changes.md) |
| `changes_validate` | [`spec changes validate`](changes.md#spec-changes-validate) (staged-wins; frozen `validate_spec` stays on disk) |
| `requirement_reword` | [`spec reword`](spec.md#spec-reword) (the repair path for `validate_spec` and `refine_requirement` findings; never hand-edit the spec file) |
| `requirement_mark_implemented` | [`spec mark-implemented`](spec.md#spec-mark-implemented) |
| `step_definitions_find` | [`spec steps missing`](steps.md#spec-steps-missing) |
| `step_definition_create` | [`spec steps generate`](steps.md#spec-steps-generate) (template only) |
| `unit_test_create` | [`spec unittest generate`](unittest.md) (template only; arg is `req_id`) |

### Inspect

| MCP tool | Harness equivalent |
| --- | --- |
| `project_root` | `--root` (the absolute directory this process was started with) |
| `project_inspect` | [`spec inspect`](inspect.md) |
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
- **Human confirm on the harness.** When `spec implement` offers
  `command_run`, the harness asks before spawning. Piped/CI stdin
  declines; it never hangs.
- **Timeout and output cap.** A hard timeout (default and maximum
  300 seconds) kills a hung process; each output stream is truncated
  to its last 200 lines.

This is policy-level guardrailing, not an OS sandbox: an allowed
build tool can still run build scripts. What the policy makes
unexpressible is running destructive binaries and reaching outside
the project root.

`run_tests` during `spec implement` sees the **working tree**, not an
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
  reconnecting agent (or a human taking over in the harness) continues
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
- Backup Inspector: `npx @modelcontextprotocol/inspector spec mcp serve --root $PWD`.
- See also [`spec tools`](tools.md) and [`spec ask`](ask.md).

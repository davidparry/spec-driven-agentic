# The free path: `pi` on a local model

This is the cheapest way into the workshop. [pi](https://pi.dev) is a
minimal, MIT-licensed coding agent; [Ollama](https://ollama.com) runs the
model on your laptop. No account, no API key, no network — and the same
`spec mcp serve` the Cursor hour uses.

Read it as two steps. **Step A** is pi as it ships: a general-purpose agent
with a shell. **Step B** takes the shell away and hands it this repo's 25
MCP tools instead. Step B is where the workshop actually starts.

> The MCP server is the constant. Cursor, `pi`, and the `spec` runner all
> call the same tools. What differs is how much of the workflow you supply
> yourself — see [harness-path.md](harness-path.md) for the other end of
> that spectrum.

## Install

```bash
brew install pi-coding-agent          # or: npm install -g @earendil-works/pi-coding-agent
pi --version
```

Point pi at Ollama by creating `~/.pi/agent/models.json`:

```json
{
  "providers": {
    "ollama": {
      "baseUrl": "http://127.0.0.1:11434/v1",
      "api": "openai-completions",
      "apiKey": "ollama",
      "models": [{ "id": "qwen3.8-flash-next:125b-mlx" }]
    }
  }
}
```

The `apiKey` is a placeholder — Ollama ignores it, but pi will not offer a
model it thinks is unauthenticated. Then make it the default in
`~/.pi/agent/settings.json`:

```json
{
  "defaultProvider": "ollama",
  "defaultModel": "qwen3.8-flash-next:125b-mlx"
}
```

Pull the model — this is the big download, do it before the class:

```bash
ollama pull qwen3.8-flash-next:125b-mlx     # the model this workshop runs against
pi --offline                                # or PI_OFFLINE=1: no startup network calls
```

A smaller tool-capable model works too; the workflow is identical and the
output quality is not. See [Ollama model](../harness/README.md#ollama-model).

## Step A — pi as it ships

Run `pi` in the repo and ask it to do something. It works, and it is
genuinely pleasant. Know what you have:

- **Eight built-in tools**: `read`, `bash`, `powershell` (Windows), `edit`,
  `write`, `grep`, `find`, `ls` — the list `pi --help` prints under
  **Built-in Tool Names**. The last three ship *off by default*, so a
  session that has not asked for them has five.
- **No permission popups.** That is pi's documented philosophy, not an
  oversight — it will run `bash` without asking. Run it in a container if
  that matters to you.
- **No MCP in core** and no plan mode in core. Everything beyond the eight
  is an extension, a skill, or a package — so the tool list in front of you
  is pi's built-ins plus whatever you have installed, and counting it is
  the only way to know what the model can reach.

This is a deliberate trade: pi stays small and does not dictate a workflow.
The consequence is that the workflow is yours to supply. On a frontier model
you are the review and that is usually fine. On a local model it is not: give
`qwen3.8-flash-next:125b-mlx` a shell, a writable test file, and a red bar,
and it will eventually make the bar green by editing the test.

You can close that gap inside pi — with a careful system prompt, `--skill`
files, prompt templates, and extensions. That works, and then you maintain
it. The rest of this page takes the other route: keep pi, remove the escape
hatches, and let a server enforce the workflow.

## Step B — same model, no shell

pi has no MCP in core, so add it:

```bash
pi install npm:pi-mcp-extension
```

This repo already ships the server registration at
[`.pi/mcp.json`](../.pi/mcp.json):

```json
{
  "mcpServers": {
    "spec-driven-server": {
      "transport": "stdio",
      "command": "spec",
      "args": ["mcp", "serve"],
      "lifecycle": "eager"
    }
  }
}
```

There is no `--root` in those args, so `spec` uses the current directory.
**Launch pi from the repository root** or the server will serve the wrong
project. `spec` must be on PATH (`cargo install --path harness`) and must
report **0.5.4 or newer** — check with `spec --version`.

Now start pi with its own tools switched off:

```bash
cd /path/to/tdd-bdd-agentic
pi -nbt          # --no-builtin-tools: built-ins off, extension tools stay on
```

Inside the session, `/mcp` shows connected servers. On first run pi asks
whether to trust this project folder — it has to, before it will load
`.pi/mcp.json`. Answer yes, or pass `--approve`.

**The tools arrive prefixed.** The bridge registers them as
`mcp_<server>_<tool>`, so `run_tests` is
`mcp_spec_driven_server_run_tests`. Use the prefixed names in prompts, or
just describe the tool and let the model match it.

What the model now has: the tools that read the spec, stage Gherkin and unit
tests, run Cucumber and JUnit as one bar, and refuse to refactor on red. What
it does not have: `bash`, `write`, `edit`. Nothing about the model changed.

**Count the tools in the session, not from this page.** 25 is the *server's*
contract: `spec mcp serve` registers exactly that many, and
`harness/tests/mcp_conformance.rs` fails the build if the number moves. You
can confirm all 25 really cross the wire with `spec mcp tools`, which opens
one throwaway MCP session and prints what it is offered — none are dropped in
the bridge. A session total is a larger number, because pi adds its own
built-ins on top of the server's; a `-xt bash,powershell` run listed **27**.
When the two disagree, the session is the one that decides what reaches the
model.

### Try the workshop loop

```text
Using the spec-driven-server tools only: call
mcp_spec_driven_server_validate_spec, then
mcp_spec_driven_server_get_requirement for REQ-003. Add its Gherkin with
scenario_add, tagged with the requirement id. Show me changes_show and wait
for my approval before changes_commit. Then run_tests — I expect RED.
```

From there the loop is the one in
[student-follow-along.md](../student-follow-along.md): review the staged
Gherkin, commit, RED, implement, GREEN, refactor, mark implemented.

### Two steps need an editor, so plan for them

The server's tools stage Gherkin, unit tests, and step definitions, but none
of them writes a *new* requirement and none writes production code —
`requirement_reword` only edits a requirement that already exists, and the
follow-along says it outright for the implement step ("a file edit — there
is no `implement` MCP tool"). So under a strict `-nbt` two steps cannot
happen at all: **Exercise 1's draft** of REQ-007, and **the implement step**
of every Red/Green cycle.

Give those two steps an editor while still keeping the shell away:

```bash
pi -xt bash,powershell          # read/write/edit on, no shell
```

`-xt` is a denylist: it takes `bash` and `powershell` away and leaves on
whatever was already on, which is `read`, `write`, and `edit`. It does
**not** hand you `grep`, `find`, or `ls` — those three ship off by default,
and only an allowlist switches them on (`-t read,write,edit,grep,find,ls`).
Shell commands used to cover that ground and now nothing does. That is fine
for these two steps, which each write one file rather than search the tree.

Run the tool-driven steps under `-nbt` and switch to `-xt bash,powershell`
for the draft and the implementation. The point of the exercise survives —
the model still cannot run commands, so the bar is whatever `run_tests`
says — and you avoid watching a capable agent insist it has no way to write
the file. The alternative is to hand-write REQ-007 yourself and let the
agent critique it with `validate_spec` and `refine_requirement`, which is
closer to what the [harness path](harness-path.md) does with `spec draft`.

**Expect `edit` to miss, and `write` to rescue it.** pi's `edit` tool is a
find/replace, and it fails often enough to notice when the match is
whitespace-sensitive — an indented Java block, a step under a `Scenario:`.
The agent recovers on its own by rewriting the whole file with `write`.
Nothing is broken; let it.

**Do not plan a scripted run.** `pi -p` is real — it takes one prompt,
processes it, and exits — but it cannot carry this hour. Under a strict
`-nbt` there is no MCP tool that adds a requirement, so Exercise 1 has no
path to completion however you invoke it; and the rest of the loop is built
on you reading `changes_show` before you allow `changes_commit`, which is
the exercise rather than an obstacle to it. Drive it interactively and
change flags between steps, which is what the rest of this page assumes.
(The flag is `--print` / `-p`. There is no `--prompt`, and
`--prompt-template` is a different thing.)

### The catch, and why the runner exists

`-nbt` is a flag on one run. Forget it and the shell is back. More to the
point, nothing sequences the work: the model still decides which tool to call
when, and "validate the spec before writing a scenario" is a sentence in your
prompt rather than a property of the system.

That is the difference between a **general agent** and a **spec-specific
runner**. pi can do anything, so the discipline is your prompt's job. `spec`
does exactly one job — spec → Gherkin → RED → GREEN → REFACTOR — and because
it only knows that one job, it can hand the model just the 3–7 tools the
current step allows, stage every write, and refuse the illegal transitions
outright. Same server, same tools, less rope.

Continue with [harness-path.md](harness-path.md).

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `/mcp` shows no servers | pi was started outside the repo root, or the project was not trusted — restart with `--approve` |
| Tools listed but every call errors | `spec` is not on PATH; check `spec --version` reports 0.5.4 or newer |
| Model missing from `/model` | No auth configured for the provider — keep the placeholder `apiKey` in `models.json` |
| Tool calls vanish mid-stream | Ollama's OpenAI-compat shim drops `tool_calls` when streaming; use a tool-capable model and a current Ollama |
| Server serves the wrong project | No `--root` in `.pi/mcp.json`; `cd` to the repository root first |
| `edit` reports it could not find the text | Whitespace-sensitive match; the agent will fall back to `write` on its own |
| `grep` / `find` / `ls` not offered under `-xt bash,powershell` | They are off by default and `-xt` only denies; use `-t` to allowlist them |

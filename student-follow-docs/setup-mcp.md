# Setting up the `spec-driven-server` MCP server in your agent

The ready-to-run server entry lives in [config/mcp.json](../config/mcp.json).
Put **`spec` on PATH** first (`cargo install --path harness`, a GitHub release
binary, or `harness/target/release/spec`), then register the server with your
client of choice below. `${workspaceFolder}` is a Cursor variable.
Claude Code passes `${SPEC_PROJECT_DIR}`. When that variable is unset,
the server uses the directory it was launched in. pi ships with no
`--root`, so start it from the repo root. Other clients need the
**absolute path** to your repo clone.

The server itself is always the same command, whatever the client:

```bash
spec mcp serve --root /absolute/path/to/tdd-bdd-agentic
```

Cursor, `pi -nbt`, and the bundled `smoke-test.jar` all speak to this process
over stdio and see **all 25 tools**, including staging.

---

## Cursor

Copy [`config/mcp.json`](../config/mcp.json) to `.cursor/mcp.json` in the
project (this repo already ships that file). Cursor picks it up when you
open the folder. To register it in another project, copy the same entry
into that project's `.cursor/mcp.json` or merge it into `~/.cursor/mcp.json`
(global).

To find the MCP settings: open **Cursor Settings** (gear icon in the top
right, or `Cmd+Shift+J` on macOS / `Ctrl+Shift+J` on Windows/Linux), go to
**Customize**, then the **MCP** tab. Each configured server is listed there
with its status — `spec-driven-server` should show green, and toggling it off/on
restarts it.

If it stays red, `spec` is not on PATH for GUI apps. Launch Cursor from a
terminal where `spec --version` works, or put the absolute path to the
binary in `command`.

- Docs: [Cursor — Model Context Protocol](https://cursor.com/docs/mcp)

## pi

[pi](https://pi.dev) has no MCP client in core — that is a stated design
choice — so add one, then start pi with its own `bash`/`write`/`edit`
switched off:

```bash
pi install npm:pi-mcp-extension
cd /path/to/tdd-bdd-agentic && pi -nbt
```

This repo ships [`.pi/mcp.json`](../.pi/mcp.json) with the server already
registered. Its `args` carry no `--root`, so **launch pi from the repository
root**. Trust the project when pi asks (or pass `--approve`), then check
`/mcp`. The bridge prefixes tool names: `run_tests` arrives as
`mcp_spec_driven_server_run_tests`.

`-nbt` is right for the tool-driven steps, but no MCP tool writes a *new*
requirement or production code, so two steps of the workshop need an editor.
Swap to `pi -xt bash,powershell` for those and back afterwards — [the pi
path](pi-path.md) says which two and why.

Full walkthrough, including the local-model setup: [the pi path](pi-path.md).

- Docs: [pi.dev](https://pi.dev) · [pi-mcp-extension](https://github.com/irahardianto/pi-mcp-extension)

## Claude Desktop

Open **Settings → Developer → Edit Config** and merge the `mcpServers` entry
from `config/mcp.json` into `claude_desktop_config.json`, replacing
`${workspaceFolder}` with your absolute repo path. Fully restart the app.

- Config file: `~/Library/Application Support/Claude/claude_desktop_config.json`
  (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows)
- Docs: [MCP — Connect to local servers](https://modelcontextprotocol.io/docs/develop/connect-local-servers)

## Claude Code

This repo already ships [`.mcp.json`](../.mcp.json) (the project-scoped
server list Claude Code reads) and [`.claude/settings.json`](../.claude/settings.json)
(approves `spec-driven-server` after you trust the folder). `spec` must be
on PATH. In a Claude Code session, trust the workspace if prompted, then
confirm with `/mcp`.

[`.mcp.json`](../.mcp.json) passes `--root ${SPEC_PROJECT_DIR}`. Set that
variable to the project directory when Claude Code starts the server. If
it is unset, `spec` uses the directory the process was launched in. An
unexpanded `${...}` value is not turned into a directory.

To register the same server in another clone without those files:

```bash
claude mcp add spec-driven-server --scope project -- spec mcp serve --root "${SPEC_PROJECT_DIR:-.}"
```

- Docs: [Claude Code — MCP](https://code.claude.com/docs/en/mcp)

## OpenAI Codex (CLI / IDE extension)

Codex uses TOML, not JSON. Add this to `~/.codex/config.toml` (or a trusted
project's `.codex/config.toml`):

```toml
[mcp_servers.spec-driven-server]
command = "spec"
args = ["mcp", "serve", "--root", "/absolute/path/to/tdd-bdd-agentic"]
```

Or use the harness: `codex mcp add spec-driven-server -- spec mcp serve --root "$PWD"`.

- Docs: [Codex — Model Context Protocol](https://developers.openai.com/codex/mcp)

## VS Code (GitHub Copilot)

Create `.vscode/mcp.json` in the project. Note VS Code's top-level key is
`servers` (not `mcpServers`) and each server takes a `type`:

```json
{
  "servers": {
    "spec-driven-server": {
      "type": "stdio",
      "command": "spec",
      "args": ["mcp", "serve", "--root", "${workspaceFolder}"]
    }
  }
}
```

- Docs: [VS Code — Add and manage MCP servers](https://code.visualstudio.com/docs/agent-customization/mcp-servers)

## Windsurf

Copy the `mcpServers` block from `config/mcp.json` into Windsurf's MCP
config, with an absolute `--root`.

## Gemini CLI

```bash
gemini mcp add spec-driven-server spec -- mcp serve --root "$PWD"
```

- Docs: [Gemini CLI — MCP servers](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)

## MCP Inspector (backup)

```bash
npx @modelcontextprotocol/inspector spec mcp serve --root "$PWD"
```

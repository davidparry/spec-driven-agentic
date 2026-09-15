# Setting up the `tdd-workflow` MCP server in your agent

The ready-to-run server entry lives in [config/mcp.json](../config/mcp.json).
Put **`bdd` on PATH** first (`cargo install --path cli`, a GitHub release
binary, or `cli/target/release/bdd`), then register the server with your
client of choice below. One thing to know before you copy:
`${workspaceFolder}` is a Cursor variable — every other client needs it
replaced with the **absolute path** to your repo clone.

The server itself is always the same command, whatever the client:

```bash
bdd mcp serve --root /absolute/path/to/tdd-bdd-agentic
```

Cursor and the bundled `tdd-agent.jar` both speak to this process over
stdio and see **all 23 tools**, including staging.

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
with its status — `tdd-workflow` should show green, and toggling it off/on
restarts it.

If it stays red, `bdd` is not on PATH for GUI apps. Launch Cursor from a
terminal where `bdd --version` works, or put the absolute path to the
binary in `command`.

- Docs: [Cursor — Model Context Protocol](https://cursor.com/docs/mcp)

## Claude Desktop

Open **Settings → Developer → Edit Config** and merge the `mcpServers` entry
from `config/mcp.json` into `claude_desktop_config.json`, replacing
`${workspaceFolder}` with your absolute repo path. Fully restart the app.

- Config file: `~/Library/Application Support/Claude/claude_desktop_config.json`
  (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows)
- Docs: [MCP — Connect to local servers](https://modelcontextprotocol.io/docs/develop/connect-local-servers)

## Claude Code

One command from the repo root registers the server for this project:

```bash
claude mcp add tdd-workflow -- bdd mcp serve --root "$PWD"
```

Or create `.mcp.json` in the project root with the `mcpServers` block from
`config/mcp.json` (absolute paths). Verify with `/mcp` inside a session.

- Docs: [Claude Code — MCP](https://code.claude.com/docs/en/mcp)

## OpenAI Codex (CLI / IDE extension)

Codex uses TOML, not JSON. Add this to `~/.codex/config.toml` (or a trusted
project's `.codex/config.toml`):

```toml
[mcp_servers.tdd-workflow]
command = "bdd"
args = ["mcp", "serve", "--root", "/absolute/path/to/tdd-bdd-agentic"]
```

Or use the CLI: `codex mcp add tdd-workflow -- bdd mcp serve --root "$PWD"`.

- Docs: [Codex — Model Context Protocol](https://developers.openai.com/codex/mcp)

## VS Code (GitHub Copilot)

Create `.vscode/mcp.json` in the project. Note VS Code's top-level key is
`servers` (not `mcpServers`) and each server takes a `type`:

```json
{
  "servers": {
    "tdd-workflow": {
      "type": "stdio",
      "command": "bdd",
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
gemini mcp add tdd-workflow bdd -- mcp serve --root "$PWD"
```

- Docs: [Gemini CLI — MCP servers](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)

## MCP Inspector (backup)

```bash
npx @modelcontextprotocol/inspector bdd mcp serve --root "$PWD"
```

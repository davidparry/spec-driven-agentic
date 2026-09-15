# Changelog

## Unreleased

- MCP `scenario_update` (and other optional tool fields) emit portable
  `anyOf` schemas instead of `type: ["string","null"]` arrays that some
  MCP clients drop or reject.
- The workshop MCP server stays stdio-only (`bdd mcp serve`) on rmcp 3.3.

- One MCP server: `bdd mcp serve` (23 tools, including `changes_validate`
  for staged-wins spec+Gherkin checks). Workshop Cursor config and
  `tdd-agent.jar` launch that binary; the Java `mcp-server/` module is gone.
  Frozen seven-tool reply shapes stay (`cli/tests/mcp_conformance.rs` +
  mcp-client `ToolPlan`). CLI LLM calls use Ollama `/api/chat` with
  per-command tool profiles (`bdd tools`, `bdd mcp call`, `bdd ask`).
- Switch the recommended Ollama model this CLI, talk, and workshop run
  against to `qwen3.8-flash-next:125b-mlx`.

## 0.2.5

- Document `qwen3-coder-next:latest` as the Ollama model this CLI is
  developed and run against. Your mileage will vary with other models,
  especially those not trained for development work. The session pull
  hint, empty-catalog `bdd model list` message, `llm_unavailable`
  reply, and the commented model in the `bdd init` scaffold now name
  that model.

## 0.2.4

- Implementation attempts now record `outcome`: the first test run after
  the attempt, so the next model brief sees what that try actually
  caused. An empty `outcome` means no run followed. State files from
  0.2.3 still load; a missing `outcome` is treated as empty.
- Relicensed the project to AGPL-3.0.

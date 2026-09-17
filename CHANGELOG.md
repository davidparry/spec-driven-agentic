# Changelog

## Unreleased

- The binary is now `spec` and the crate is `spec-harness`. What the tool
  does is author, gate, and drive a requirements spec; `bdd` named the
  altitude of one of the two test loops it runs, which was always the
  narrower half of the story. This is a hard cutover — there is no `bdd`
  shim and no alias. Installing `spec` leaves an older `bdd` on PATH
  untouched, so uninstall that separately.

  The `bdd spec …` group is promoted to the top level, because
  `spec spec draft` is absurd: `spec list`, `spec show`, `spec draft`,
  `spec validate`, `spec refine`, `spec reword`, `spec set-feature`,
  `spec mark-implemented`, and `spec include add`. Only `validate`
  collided, and the requirements spec won it, so the Gherkin gate that
  was `bdd validate` is now `spec changes validate` — which also finally
  matches its MCP name, `changes_validate`. Every other command keeps its
  own name behind the new one.

  On-disk state is renamed with the tool: `.spec.toml`,
  `.spec-state.json`, `.spec-memory.json`, `.spec-staged/`,
  `.spec-cache/`, `.spec-log/` (holding `spec.log`), `.spec-history`, and
  the home-directory MCP registry at `~/.spec/mcp.json`. Those names were
  scattered string literals and are now constants in `domain`, declared
  once. Nothing migrates a project carrying the old names: rename them,
  or let the harness recreate what it needs.

  Also renamed: `BDD_MCP_CONFIG` to `SPEC_MCP_CONFIG` and the `BDD_E2E_*`
  knobs to `SPEC_E2E_*`; release assets to `spec-harness-*` with the
  receipt at `~/.config/spec-harness/spec-harness-receipt.json`;
  `RUST_LOG` filters on `spec_harness::…`; the interactive shell prompt to
  `spec>`, forgiving a pasted leading `spec` where it used to forgive
  `bdd`; and the smoke test's `-Dbdd.binary` to `-Dspec.binary`.

  Two things deliberately did not move. The 25 MCP tool names and the
  `spec-driven-server` key are unchanged, so an MCP client needs nothing
  but the new `"command": "spec"`. And `bddFramework` stays `bddFramework`
  in `project_inspect` output and `.spec-memory.json`, because that field
  names the project's BDD framework — Cucumber-JVM, cucumber-rs — and has
  never referred to this tool.

- The `cli/` directory is now `harness/` and the crate is `bdd-harness`:
  what ships is a harness for the whole spec-driven loop — commands and
  the embedded MCP server — not only a command line. The binary is still
  `bdd` and no command, flag, or tool name changed. What does change:
  release assets are `bdd-harness-*` (installer, archives, and
  `bdd-harness-uninstaller.sh`), the install receipt is
  `~/.config/bdd-harness/bdd-harness-receipt.json`, `RUST_LOG` filters on
  `bdd_harness::…`, the CI job is `harness`, and the site serves the
  harness page at `/harness/`. An existing install keeps working, but a
  receipt written by an older installer is only understood by the old
  `bdd-cli-uninstaller.sh`.

- `scripts/verify-workshop-run.sh check` grades Exercise 1 on its own
  terms. It used to demand that REQ-007 match the `complete` branch word
  for word, which no correct run could satisfy: the wording is authored
  live by the student and their agent, and the recorded one predates the
  custom-delimiter prompt. It now asks `bdd spec validate` and
  `bdd spec refine` — the same deterministic checks Exercise 1 runs — and
  that a criterion covers the `//` declaration.

- Exercise 2 is graded the same way, so the verifier no longer reads the
  `complete` branch at all. It used to require REQ-003's scenarios to
  match that branch character for character and the unit test to contain a
  method named `twoCommaSeparatedNumbersAreSummed`, which a harness-driven
  run cannot produce: `bdd unittest generate` names one method per
  acceptance criterion. A green run was failed for naming. Both checks now
  ask whether every one of REQ-003's acceptance criteria is covered by a
  scenario tagged `@REQ-003` and asserted by a `@Test` that names the
  requirement — the criteria are the bar, the wording is the run's. A
  missing scenario is still caught, and named: `bdd validate` only
  requires that *one* tagged scenario exist, so a run that wrote a
  scenario for one of two criteria used to pass every gate.

- The deck can change cuts from inside the deck: a switch in the
  bottom-left corner and the <kbd>t</kbd> key move between the 60- and
  30-minute tracks, and `?60` now forces the long cut so the switch also
  works from the published `/talk30/` path. Until now the cut was decided
  by the URL alone, so opening `slides/index.html` — what the README tells
  you to do — left no way to reach the short track but to retype the
  address. The links that were supposed to offer it were broken in the
  same direction: `README.md` and `speaking.md` each wrote `?30` in the
  link text and left it out of the target, so every route into the deck,
  including the published `/speaking/` page, landed on the 60-minute cut.

- Exercise 2 names REQ-003 in its prompt — in the follow-along, the
  README, and the slide deck, the three places attendees paste it from.
  "the next pending id" was ambiguous once Exercise 1 succeeded, because
  the REQ-007 just drafted is pending too and freshest in context, so
  agents took it to green and left REQ-003 untouched. Every phase gate
  passed while it happened, which is the point: the gates police how an
  agent works, never what it works on. That is now a documented outcome
  in Step 6, a presenter note on the deck, and a row in the *Where This
  Breaks* catalog.

- MCP `requirement_reword` rewords one requirement's title, story, or
  acceptance criteria into the staging area, the same mutation
  `bdd spec reword` performs. `validate_spec` and `refine_requirement` now
  name it in their `nextStep` instead of telling an agent to edit the
  requirements file: the spec file's JSON escaping and indentation differ
  from what the read tools return, so a hand-written string replacement
  against `requirements.json` does not match. The catalog is 25 tools.

- The chat cache keeps only terminal model turns. A turn carrying tool
  calls is neither stored nor served, so an identical later request asks
  the model again rather than replaying calls the agent loop would
  execute a second time. An entry written by an earlier build is swept
  when it is read.

- `bdd config` prints every LLM and tools key with `(default)` or the
  path of the `.bdd.toml` it was read from. With no `llm.model` in the
  file, Ollama is asked which model a run would use and it prints as
  `(discovered)`. The file is read from `--root` only; parent
  directories are never searched.

- Project configuration is `.bdd.toml` only. `bdd init` writes
  `[tools.profiles]` with the tools each LLM-backed command offers the
  model (the code defaults, listed for reference). Other keys stay
  commented. A listed command replaces that caller's built-in tools.
  MCP tools from `mcp.json` are `server:tool` (or `server__tool`);
  `builtin:name` pins the harness tool when short names collide.

- MCP `project_root` returns the absolute `--root` this `bdd mcp serve`
  process uses for every other tool.
- Claude Code can use the workshop MCP server from the committed
  `.mcp.json` (enabled in `.claude/settings.json`).
- MCP server identity is `spec-driven-server` / `1.0.0`, title `Spec Driven`,
  description that the requirements spec is the source of truth, website
  `https://davidparry.github.io/spec-driven-agentic/`, and icon
  `https://davidparry.github.io/spec-driven-agentic/assets/bdd-harness-mark.png`.
- The smoke jar launches the `bdd` on `PATH` (the same binary `bdd --version`
  uses). It no longer looks under `harness/target`. If `bdd` is missing: install
  it (`cargo install --path harness`) or add its directory to `PATH`.
- Default smoke walkthrough now calls the remaining read-only MCP tools
  (`validate_spec`, `refine_requirement`, `project_root`, `project_inspect`, `feature_list`,
  `feature_read` of the workshop kata feature, `changes_show`,
  `changes_validate`, `step_definitions_find`). Mutating tools stay behind
  `--sweep --include-mutating`.
- Renamed the Java module from `mcp-client` to `smoke-test`
  (`smoke-test.jar`, package `com.davidparry.workshop.smoke`). It is a
  smoke test of `bdd mcp serve`, not a general MCP client product.
- MCP sessions from `bdd mcp call` / `bdd mcp tools` use the 2026-07-28
  discover lifecycle: they do not send `initialize`. Conformance asserts
  `tools/list` succeeds as the first stdio request, with per-request
  `_meta`. The Java smoke walkthrough no longer calls `initialize`; STEP 1
  is `tools/list`.
- MCP `scenario_update` (and other optional tool fields) emit portable
  `anyOf` schemas instead of `type: ["string","null"]` arrays that some
  MCP clients drop or reject.
- MCP `get_info` returns rmcp 3.4 `ServerConfig` (the `ServerInfo` alias is
  deprecated).
- The workshop MCP server stays stdio-only (`bdd mcp serve`) on rmcp 3.4.

- One MCP server: `bdd mcp serve` (25 tools, including `project_root`
  and `changes_validate`
  for staged-wins spec+Gherkin checks). Workshop Cursor config and
  `smoke-test.jar` launch that binary; the Java `mcp-server/` module is gone.
  Frozen seven-tool reply shapes stay (`harness/tests/mcp_conformance.rs` +
  smoke-test `ToolPlan`). Harness LLM calls use Ollama `/api/chat` with
  per-command tool profiles (`bdd tools`, `bdd mcp call`, `bdd ask`).
- Switch the recommended Ollama model this harness, talk, and workshop run
  against to `qwen3.8-flash-next:125b-mlx`.

## 0.2.5

- Document `qwen3-coder-next:latest` as the Ollama model this harness is
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

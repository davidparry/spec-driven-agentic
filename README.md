# Spec-Driven with Harness — TDD & BDD in the Agentic Era

[![CI](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/ci.yml/badge.svg?branch=trunk)](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/ci.yml)
[![Release](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/release.yml/badge.svg)](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/release.yml)
[![spec harness](https://img.shields.io/github/v/release/davidparry/tdd-bdd-agentic?label=spec%20harness)](https://github.com/davidparry/tdd-bdd-agentic/releases/latest)
[![Harness coverage](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fdavidparry%2Ftdd-bdd-agentic%2Fbadges%2Fcoverage.json)](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/ci.yml)
[![Java coverage gate](https://img.shields.io/badge/JaCoCo-100%25%20gate-brightgreen)](pom.xml)
[![Quality gates](https://img.shields.io/badge/SpotBugs%20%7C%20PMD%20%7C%20clippy-enforced-blue)](.github/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/github/license/davidparry/tdd-bdd-agentic)](LICENSE)

> **Site:** [https://davidparry.github.io/spec-driven-agentic/](https://davidparry.github.io/spec-driven-agentic/)
> Install `spec`, download binaries, open the talk, and read the
> [write-up](https://davidparry.com/blog/2026/08/07/spec-first-was-always-right-agents-just-made-it-fast/).

> **Conference organizers:** the session abstract, formats, and stage
> requirements are in [`speaking.md`](speaking.md) —
> *Turn Off the Wi-Fi: Spec-Driven Development That Delivers on a Local Model.*

> **Students: start here → [`student-follow-along.md`](student-follow-along.md)**
> Your step-by-step companion for the hour — the exact commands, the exact
> agent prompts, what you should see at every step, and a self-check that
> grades your run against the spec's own acceptance criteria.

A 60-minute hands-on workshop. There is **one MCP implementation**: `spec mcp
serve` (25 tools). Cursor, [pi](https://pi.dev) with its MCP extension, and the
bundled `smoke-test.jar` all drive that binary over stdio. What you drive is a
**spec-driven development workflow spanning SDD, BDD, and TDD**: you and an AI
agent draft requirements together and iterate them through two server feedback
loops — `validate_spec` for structure, `refine_requirement` for wording quality
— until the spec is valid and clean; requirements become executable Gherkin
scenarios and unit tests (via staging tools); and the agent collaborates with
you through the Red/Green/Refactor cycle — with the human in control of every
engineering decision. The conference / Wi-Fi-off path is the **same server**
with narrower per-command tool profiles (`spec tools profiles`), not a second
implementation.

**One server, two kinds of client.** A *general* agent — Cursor, pi, whatever
your team uses — can call all 25 tools, and you supply the workflow through
prompting, skills, and review. The `spec` commands are a *spec-specific runner*
for the same tools: the sequence, the tool profile for each step, and the phase
gates are already encoded, so there is less to re-type and less to get wrong.
The free end of that spectrum is [the pi path](student-follow-docs/pi-path.md)
(MIT agent, local Ollama model, no network); the strict end is
[the harness path](student-follow-docs/harness-path.md).

## Three altitudes, one discipline

This workshop is not "just TDD." It composes the three spec-first
methodologies, each at its own altitude, and the agent works across all of
them:

| Methodology | Pins | Canonical artifact | In this repo |
| --- | --- | --- | --- |
| **SDD** (spec-driven) | the feature | versioned spec + acceptance criteria | `requirements/requirements.json` — the catalog of requirements the agent implements from (it can include child spec files, nested N deep, merged into one backlog) |
| **BDD** (behavior-driven) | one behavior | Gherkin scenario (Given/When/Then) | `kata/src/test/resources/features/string_calculator.feature`, executed by Cucumber |
| **TDD** (test-driven) | one unit | failing unit test | `kata/src/test/java/.../StringCalculatorTest.java`, JUnit 5 |

The flow is spec-down: the agent reads a requirement (SDD), turns its
acceptance criteria into a tagged Gherkin scenario (BDD), adds unit tests
where useful (TDD), and the `run_tests` tool runs Cucumber and JUnit
together — one bar, one color. Tests are generated *from* the spec, not the
other way around, which is exactly the spec-driven claim.

**Slides:** open [`slides/index.html`](slides/index.html) in a browser for the
60-minute workshop, or [`slides/index.html?30`](slides/index.html?30) for the
30-minute demo-driven cut. One file, two tracks — see
[`slides/index.html`](slides/index.html) `data-track`. The deck carries a
switch in the bottom-left corner and toggles on <kbd>t</kbd>, so you can
change cuts without retyping the URL.

**Attending this workshop?** Follow
[`student-follow-along.md`](student-follow-along.md) step by step.

**Presenting this workshop?** Run `scripts/preflight.sh` before going on
stage, and rehearse through `scripts/verify-workshop-run.sh` — it cuts a
fresh branch from `trunk` for every run and grades the end state against
the spec's own acceptance criteria. (The minute-by-minute run-of-show is
kept with the presenter, not in the repo.)

## What's in the box

| Module / folder | What it is |
| --- | --- |
| `kata/` | A **standalone** Maven project — the String Calculator kata. It has its own `pom.xml` (no parent). Two requirements are implemented; the rest are driven agentically during the workshop. Gherkin feature files (`src/test/resources/features/`) are the executable behavior spec, run by Cucumber alongside the JUnit tests. Copy the folder and it still builds: `mvn -f kata/pom.xml test`. |
| [`harness/`](harness/README.md) | The `spec` harness **and** the workshop MCP server. `spec mcp serve` exposes 25 tools over stdio (wire identity `spec-driven-server` / `1.0.0`, title `Spec Driven`, website [spec-driven-agentic](https://davidparry.github.io/spec-driven-agentic/)). Frozen seven-tool reply shapes are gated by `harness/tests/mcp_conformance.rs`. The same binary automates the spec-driven loop with per-command tool profiles (3–7 tools for a generating command, 12 for the read-only `ask`) and a local Ollama model (`qwen3.8-flash-next:125b-mlx`). See [`harness/README.md`](harness/README.md) and the searchable [command manual](https://davidparry.github.io/spec-driven-agentic/manual/). |
| `smoke-test/` | A narrated **smoke test** of `spec mcp serve` (`smoke-test.jar`) plus an automated 25-tool sweep. It launches **only** that server as a child process — discovery, baseline `run_tests`, then remaining read-only tools (`validate_spec`, `project_root`, `project_inspect`, `feature_list` / `feature_read`, `changes_show` / `changes_validate`, `step_definitions_find`; no `initialize` handshake). Mutating tools stay behind `--sweep --include-mutating`. Own spec (`smoke-test/requirements/requirements.json`), tagged Cucumber scenarios, `SpecCompletenessTest`, 100% instruction/branch coverage (JaCoCo-enforced; excludes `TddAgent` and `SdkToolClient` only), SpotBugs + PMD gating `mvn -pl smoke-test verify`. |
| `requirements/requirements.json` | The SDD spec: the requirements backlog, and the root of the **spec catalog** — it holds requirements of its own and may `include` child spec files (which may include further files, N levels deep); the tooling merges the tree into one backlog. Each requirement carries acceptance criteria (already phrased Given/When/Then) that agents turn into executable Gherkin scenarios and failing tests, plus a `featureFile` pointer to where its scenarios live. Full field-by-field reference: [The requirements format](https://davidparry.github.io/spec-driven-agentic/manual/spec-format.html). |
| `slides/index.html` | The reveal.js slide deck (self-contained, CDN-based). **One file, two cuts:** every top-level `<section>` carries `data-track="60"`, `"30"`, or `"both"`, and a script strips the other track before reveal initializes. Plain URL gives the 60-minute workshop (30 slides); `?30` — or the published `/talk30/` path — gives the 30-minute demo-driven session (22 slides). Facts live in one place, so the two cuts cannot drift apart. |
| `student-follow-along.md` | The attendee's step-by-step companion: commands, prompts, expected output, self-check, homework. |
| [`student-follow-docs/pi-path.md`](student-follow-docs/pi-path.md) | The free, offline on-ramp: pi (MIT) on a local Ollama model, first as it ships and then with `-nbt` so the same 25 MCP tools are all the model gets. |
| [`speaking.md`](speaking.md) | The conference session built on this repo — abstract, what attendees leave with, the *Where This Breaks* catalog of local-model failure modes, formats, and stage requirements. Published at [/speaking/](https://davidparry.github.io/spec-driven-agentic/speaking/). |
| `scripts/` | `preflight.sh` (presenter readiness), `verify-workshop-run.sh` (fresh run branch + end-state check against the spec's acceptance criteria), `check-workshop-start.sh` / `check-class-complete.sh` (the two CI branch guards). |
| `.cursor/mcp.json` / [`.mcp.json`](.mcp.json) / [`.pi/mcp.json`](.pi/mcp.json) / [`config/mcp.json`](config/mcp.json) | Registers `spec mcp serve` with Cursor (`.cursor/mcp.json`), Claude Code (`.mcp.json` + `.claude/settings.json`), [pi](https://pi.dev) (`.pi/mcp.json`, read by `pi-mcp-extension`; no `--root`, so launch pi from the repo root), and other MCP hosts. |

## Branches and CI

| Branch | What it is |
| --- | --- |
| `trunk` | The workshop starting point — what you clone before the class. The kata has REQ-001 and REQ-002 implemented (the green baseline the exercises build on); REQ-003–006 are pending and REQ-007 does not exist yet. |
| `complete` | The finished loop: the end-of-class state — REQ-007 drafted and refined through the `validate_spec` / `refine_requirement` loops (Exercise 1), REQ-003 taken through Red/Green/Refactor to green (Exercise 2) — plus the homework done. Every requirement in the backlog (REQ-001–007) is implemented, with every scenario tagged and green. |

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs on every push and pull request:

- **build-and-test** — `mvn -pl smoke-test verify` (JUnit + Cucumber, JaCoCo 100%, SpotBugs, PMD) and `mvn -f kata/pom.xml test` for the standalone kata. This job does **not** require the `spec` binary, so a missing Rust build cannot look like a JaCoCo failure.
- **harness** — `spec` tests, clippy, fmt, coverage (≥ 97% library lines, `main.rs` ignored), and the release binary.
- **smoke-test-live** — needs `harness`; drives `smoke-test.jar` against that binary (`-Dspec.binary=…`) so the child's process is proven to be real `spec mcp serve`.
- **class-completeness** — runs [scripts/check-class-complete.sh](scripts/check-class-complete.sh), which asserts the class deliverables (REQ-003 implemented, REQ-007 in the spec). It **fails on `trunk` by design** — the red X is the reminder that trunk is the starting line — and passes on `complete`.
- **workshop-start** — the inverse gate: runs [scripts/check-workshop-start.sh](scripts/check-workshop-start.sh), which asserts the starting state is intact (REQ-003–006 pending, no REQ-007, no scenarios beyond REQ-001/002, `StringCalculator` unimplemented past REQ-002). It **passes on `trunk`** and **fails on `complete` by design**, so completed work can never silently leak into the branch attendees clone.

**Both branches build green on purpose.** "Incomplete" lives in the spec
(pending statuses), not in a failing build: your setup check (`spec --version`
and `mvn -q -f kata/pom.xml test`) must pass before the class, and the RED bar is created *live*
during Exercise 2 when the agent writes the `@REQ-003` scenario. Also note
that `mvn clean validate` runs no tests at all — `validate` only checks the
POMs. Use `mvn -pl smoke-test verify` and `mvn -f kata/pom.xml test`
to actually run the Java suites, and the two guard scripts above to tell
the branches apart.

## Verify everything is ready for production

Before pushing to `trunk` (which deploys the website) or tagging a
release, run this from the repository root. It builds and gates
everything that ships — the harness, the command manual, and the site:

```bash
(cd harness && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release) \
  && mdbook build manual \
  && scripts/build-pages.sh
```

1. **Harness** — format check, clippy with warnings as errors, the full
   unit + Cucumber suite, and the release binary
   (`harness/target/release/spec`). The same gates CI enforces.
2. **Manual** — regenerates [`docs/manual/`](docs/manual/) from
   [`manual/src`](manual/src). The output is committed, so any
   changes it produces belong in your commit. Must run before the site
   build, which copies the built book.
3. **Website** — assembles `_site/` exactly as the
   [Pages workflow](.github/workflows/pages.yml) does on push to
   `trunk`; a clean local run means a clean deploy.

One-time tools: `cargo install mdbook` and `pip install markdown`.

If you touched the Java smoke test, also run `mvn -pl smoke-test verify` and
`mvn -f kata/pom.xml test` (the standalone kata). The multi-platform
release binaries are built by the release workflow when a `v*` tag is
pushed (`scripts/release.sh`), not locally.

## Prerequisites

- **`spec` on PATH** (GitHub release, `cargo install --path harness`, or `harness/target/release/spec`)
- Java 21+
- Maven 3.9+
- An MCP host for the exercises (Cursor, Claude Desktop, [pi](https://pi.dev) with `pi install npm:pi-mcp-extension`, or the bundled `smoke-test.jar`)
- Optional, and the fully offline route: [Ollama](https://ollama.com) with `qwen3.8-flash-next:125b-mlx` — see [the pi path](student-follow-docs/pi-path.md)
- Optional: Node.js for the MCP Inspector (`npx @modelcontextprotocol/inspector spec mcp serve --root $PWD`)

`cargo clean` then `cargo build --all-targets` only rebuilds into
`harness/target/debug/`. It does **not** copy `spec` onto PATH. Cursor's
`.cursor/mcp.json` runs `"command": "spec"`, which is
`~/.cargo/bin/spec` after install, so a rebuild alone leaves the old
server in place. After changing the harness, install from the **repository
root** (not from inside `harness/`):

```bash
cargo install --path harness
```

From `harness/` itself use `cargo install --path .` — `--path harness` from
there looks for `harness/harness` and fails.

Then reload the MCP server in Cursor (toggle it off/on). To try a
debug binary without installing: `harness/target/debug/spec mcp serve --root .`.

## Setup (do this before the workshop)

```bash
git clone <this repo> && cd tdd-bdd-agentic
spec --version                     # must succeed before Cursor will connect
mvn -q -pl smoke-test package     # MCP-server smoke-test jar
mvn -q -f kata/pom.xml test       # standalone kata: JUnit + Cucumber
```

A green pair of builds means you're ready. The kata is **not** a Maven
module of the workshop reactor — it is its own project, so a copy of
`kata/` builds without this repo's parent POM. During the workshop you'll work on a
branch cut from `trunk` (`git checkout -b workshop trunk`) — the exercises
rewrite the spec and the kata, and `trunk` stays pristine so you can always
reset by re-branching. Details in
[`student-follow-along.md`](student-follow-along.md).

## The server's tools

`spec mcp serve` exposes **25 tools**. Cursor sees all of them, and so does
`pi --no-builtin-tools`. Harness commands that call a model attach a scoped
profile (`spec tools profiles`): 3–7 tools for a generating command, 12 for the
read-only `spec ask`.

There is no `spec_draft` or `implement` MCP tool: a **new** requirement is
still drafted by the human (`spec draft`), and Cursor writes production
Java. Rewording an existing requirement, Gherkin, steps, unit-test
scaffolds, mark-implemented, and staging all go through tools. Generation
over MCP is template-only.

**Frozen seven** (reply shapes stay; conformance is `harness/tests/mcp_conformance.rs`
plus smoke-test `ToolPlan`):

| Tool | Purpose |
| --- | --- |
| `list_requirements` | Every requirement with its id, title, and status — find pending work. Re-reads the spec fresh on every call, so requirements an agent just drafted show up immediately. |
| `get_requirement` | One requirement's user story, acceptance criteria, and `featureLocation` — the raw material for Gherkin scenarios and failing tests, plus a `workflowHint` telling the agent what to do next. |
| `validate_spec` | Validates the requirements file **on disk**: well-formed unique ids, stories, Given/When/Then acceptance criteria, and tagged scenarios for implemented requirements. During `spec draft` these lookups do not critique the in-flight proposal; `parse_proposals_checked` is that gate. |
| `refine_requirement` | Deterministic quality feedback on one requirement's wording: ambiguous words ("should", "handle", "quickly"), stories missing their actor or their why, outcomes with no concrete expected value, criteria covering more than one action, and happy-path-only coverage. |
| `run_tests` | Runs the project tests (Maven on this kata), aggregating Cucumber (BDD) and JUnit (TDD) into one bar color: failures → **RED**, all passing → **GREEN**. On `spec implement` this sees the **working tree**, not an unstaged patch. |
| `get_tdd_state` | Current Red/Green/Refactor phase, last run summary, and a suggested next step. |
| `start_refactor` | Begins a refactor. Refuses unless the bar is GREEN — never refactor on a red bar. |

**Authoring / staging:** `feature_list`, `feature_read`, `feature_create`,
`scenario_add`, `scenario_update`, `scenario_delete`, `changes_show`,
`changes_validate`, `changes_commit`, `changes_discard`, `requirement_reword`
(the repair path `validate_spec` and `refine_requirement` point at — agents
must never hand-edit `requirements.json`, whose JSON escaping and indentation
differ from what the read tools return), `requirement_mark_implemented`
(GREEN-gated, tagged scenario required), `step_definitions_find`,
`step_definition_create`, `unit_test_create` (arg `req_id`).

**Inspect:** `project_root` (the absolute `--root` this process was started with), `project_inspect`, `command_run` (allowlisted, path-jailed,
RED-gated; the harness `implement` profile also asks the human to confirm).

The workflow rules live in `harness/src/domain/tdd.rs` (`TddStateMachine`) and
the MCP handlers in `harness/src/mcp.rs`. They were built test-first.

The Java **smoke test** practices what it preaches: `smoke-test/requirements/requirements.json`
with `@CLI-XXX` tags, tagged Gherkin scenarios, a `SpecCompletenessTest`, and
100% instruction/branch coverage with SpotBugs and PMD gating
`mvn -pl smoke-test verify`. The Rust server's frozen contracts are the
conformance suite plus that module's `ToolPlan` (exactly 25 names; a 26th
tool fails the Java build).

## The workshop

### The plumbing, briefly (13–20 min)

The server is `spec mcp serve` — this segment is a quick tour, not an
exercise. Composition root: `harness/src/mcp.rs` plus the harness TDD
services. A stdio server must never write to stdout — that corrupts the
JSON-RPC stream.
Prove the plumbing with the bundled **smoke test**, which does exactly what an
IDE does: launch `spec mcp serve`, `tools/list` (25 tools),
then `tools/call` — narrating each step. No `initialize` handshake.
Default smoke is read-only plus the baseline `run_tests`: after
`get_requirement` it also calls `validate_spec`, `refine_requirement`,
`project_root`, `project_inspect`, `feature_list`, `feature_read` (workshop kata path),
`changes_show`, `changes_validate`, and `step_definitions_find`.
Mutating tools stay behind `--sweep --include-mutating`.

```bash
spec --version
mvn -q -pl smoke-test package && mvn -q -f kata/pom.xml test
java -jar smoke-test/target/smoke-test.jar
# optional: java -jar smoke-test/target/smoke-test.jar --sweep
# optional: java -jar smoke-test/target/smoke-test.jar --sweep --include-mutating
```

The quiet kata test run prints the String Calculator Cucumber narration —
the full expected output is still captured in
[student-follow-docs/pre-step.log](student-follow-docs/pre-step.log).
The smoke test then narrates the whole protocol exchange, starting like this:

```text
========================================================================
  STEP 0 — Launch the server
========================================================================
```

…through discovery and tool calls — the full expected output is
captured in [student-follow-docs/step2.log](student-follow-docs/step2.log).

### Exercise 1 — Draft and refine the spec with your agent (20–32 min)

The spec comes first, and the agent helps write it — then refine it against
the server's feedback. `.cursor/mcp.json` (same as [`config/mcp.json`](config/mcp.json))
already registers `spec mcp serve` with Cursor — `spec` must be on PATH. Prompt
your agent:

> Add a new requirement to requirements/requirements.json: a custom delimiter
> may be declared on the first line, so "//+\n1+2" adds up to 3. Follow the
> existing format — unique id, title, user story, acceptance criteria phrased
> Given/When/Then, status pending. Then call validate_spec and fix every
> issue until the spec is valid. Then call refine_requirement on the new
> requirement and reword it from the findings until there are none. Do not
> write scenarios or code yet — we are only agreeing on the spec.

You'll watch spec iteration in two stages. **Structure:** the agent drafts
the requirement → `validate_spec` arbitrates → `"valid": true` (a
format-following draft usually passes on the first call; if it doesn't — a
criterion missing its Then, a duplicate id, broken JSON — the agent fixes
and validates again). **Wording:** `refine_requirement` critiques the draft
("'quickly' is ambiguous", "story is missing its why", "only happy paths —
add an edge case") → the LLM rewords, re-validates, re-refines →
`"clean": true` → **you read the story and criteria and approve the
wording**. The human owns intent, the agent owns wording and iteration speed,
the server owns the critique.

### Exercise 2 — The end-to-end agentic spec-to-green loop (32–52 min)

With a valid spec, prompt your agent to **use MCP tools** (not hand-edits of
Gherkin, tests, or spec status):

> Using the spec-driven-server tools: validate the spec first, then `get_requirement` for REQ-003 — not the REQ-007 you just drafted, which stays pending until homework. Add its Gherkin with `scenario_add` (tag the requirement id), add missing steps with `step_definition_create` if `step_definitions_find` reports any, add a unit test with `unit_test_create`. Show `changes_show` and ask me before `changes_commit`. Then `run_tests` (expect RED). Implement the simplest production code in `StringCalculator`. `run_tests` (GREEN). `start_refactor` if I agree. On GREEN, `requirement_mark_implemented`. Ask me before each phase change.

Name the id. "The next pending id" reads fine until Exercise 1 succeeds —
then REQ-007 is pending too, and it is the freshest thing in the agent's
context, so that is what gets taken to green. Correct discipline, wrong
requirement: the phase gates police *how* an agent works, never *what it
works on*.

You'll watch the loop: `validate_spec` → `get_requirement` →
`scenario_add` / `unit_test_create` (staged) → **you review with
`changes_show` before `changes_commit`** → `run_tests` (RED) → you review
production `add` (a file edit) → `run_tests` (GREEN) → `start_refactor` if
you agree → **`requirement_mark_implemented`** (the tool refuses off GREEN
or without a tagged scenario). Requirements REQ-003 through REQ-006 are
waiting — plus the one you drafted in Exercise 1. When you're done,
`scripts/verify-workshop-run.sh check` grades your end state: every
acceptance criterion of REQ-003 covered by a scenario and asserted by a
test, and the REQ-007 you drafted against `spec validate` and
`spec refine` rather than anyone else's wording.

### Backup — Inspect the protocol (if time allows)

Option A, the Inspector UI with a full message log:

```bash
npx @modelcontextprotocol/inspector spec mcp serve --root $PWD
```

Option B, be the client yourself — start the server and paste one line at a time:

```bash
spec mcp serve --root $PWD
```

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"me","version":"0"},"io.modelcontextprotocol/clientCapabilities":{}}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"run_tests","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"me","version":"0"},"io.modelcontextprotocol/clientCapabilities":{}}}}
```

## Ideas to keep building

- Expose `requirements.json` as an MCP **resource** and add per-phase **prompts**.
- Point `--root` at a real project instead of the kata.

## License

AGPL-3.0 — see [LICENSE](LICENSE).

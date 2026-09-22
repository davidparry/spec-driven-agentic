# Workshop — Spec-Driven with Harness

> **Students: start here → [`student-follow-along.md`](../student-follow-docs/student-follow-along.md)**
> Your step-by-step companion for the hour — the exact commands, the exact
> agent prompts, what you should see at every step, and a self-check that
> grades your run against the spec's own acceptance criteria.

> **Conference organizers:** the session abstract, formats, and stage
> requirements are in [`talks/speaking.md`](speaking.md) —
> *Turn Off the Wi-Fi: Spec-Driven Development That Delivers on a Local Model.*

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
The free end of that spectrum is [the pi path](../student-follow-docs/pi-path.md)
(MIT agent, local Ollama model, no network); the strict end is
[the harness path](../student-follow-docs/harness-path.md).

Install `spec` from the [main README](../README.md#install-spec) before the
class. The harness command manual is
[searchable online](https://davidparry.github.io/spec-driven-agentic/manual/).

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

**Slides:** open [`talks/slides/index.html`](slides/index.html) in a browser for the
60-minute workshop, or [`talks/slides/index.html?30`](slides/index.html?30) for the
30-minute demo-driven cut. One file, two tracks — see
[`talks/slides/index.html`](slides/index.html) `data-track`. The deck carries a
switch in the bottom-left corner and toggles on <kbd>t</kbd>, so you can
change cuts without retyping the URL.

**Attending this workshop?** Follow
[`student-follow-along.md`](../student-follow-docs/student-follow-along.md) step by step.

**Presenting this workshop?** Run `scripts/preflight.sh` before going on
stage, and rehearse through `scripts/verify-workshop-run.sh` — it cuts a
fresh branch from `trunk` for every run and grades the end state against
the spec's own acceptance criteria. (The minute-by-minute run-of-show is
kept with the presenter, not in the repo.)

## What's in the box

| Module / folder | What it is |
| --- | --- |
| `kata/` | A **standalone** Maven project — the String Calculator kata. It has its own `pom.xml` (no parent). Two requirements are implemented; the rest are driven agentically during the workshop. Gherkin feature files (`src/test/resources/features/`) are the executable behavior spec, run by Cucumber alongside the JUnit tests. Copy the folder and it still builds: `mvn -f kata/pom.xml test`. |
| [`harness/`](../harness/README.md) | The `spec` harness **and** the workshop MCP server. `spec mcp serve` exposes 25 tools over stdio (wire identity `spec-driven-server` / `1.0.0`, title `Spec Driven`, website [spec-driven-agentic](https://davidparry.github.io/spec-driven-agentic/)). Frozen seven-tool reply shapes are gated by `harness/tests/mcp_conformance.rs`. The same binary automates the spec-driven loop with per-command tool profiles (3–7 tools for a generating command, 12 for the read-only `ask`) and a local Ollama model (`qwen3.8-flash-next:125b-mlx`). See [`harness/README.md`](../harness/README.md) and the searchable [command manual](https://davidparry.github.io/spec-driven-agentic/manual/). |
| `smoke-test/` | A narrated **smoke test** of `spec mcp serve` (`smoke-test.jar`) plus an automated 25-tool sweep. It launches **only** that server as a child process — discovery, baseline `run_tests`, then remaining read-only tools (`validate_spec`, `project_root`, `project_inspect`, `feature_list` / `feature_read`, `changes_show` / `changes_validate`, `step_definitions_find`; the narration starts at `tools/list`, since the server does not require an `initialize` handshake). Mutating tools stay behind `--sweep --include-mutating`. Own spec (`smoke-test/requirements/requirements.json`), tagged Cucumber scenarios, `SpecCompletenessTest`, 100% instruction/branch coverage (JaCoCo-enforced; excludes `TddAgent` and `SdkToolClient` only), SpotBugs + PMD gating `mvn -pl smoke-test verify`. |
| `requirements/requirements.json` | The SDD spec: the requirements backlog, and the root of the **spec catalog** — it holds requirements of its own and may `include` child spec files (which may include further files, N levels deep); the tooling merges the tree into one backlog. Each requirement carries acceptance criteria (already phrased Given/When/Then) that agents turn into executable Gherkin scenarios and failing tests, plus a `featureFile` pointer to where its scenarios live. Full field-by-field reference: [The requirements format](https://davidparry.github.io/spec-driven-agentic/manual/spec-format.html). |
| `talks/slides/index.html` | The reveal.js slide deck (self-contained, CDN-based). **One file, two cuts:** every top-level `<section>` carries `data-track="60"`, `"30"`, or `"both"`, and a script strips the other track before reveal initializes. Plain URL gives the 60-minute workshop (28 slides); `?30` — or the published `/talk30/` path — gives the 30-minute demo-driven session (21 slides). Facts live in one place, so the two cuts cannot drift apart. |
| [`student-follow-docs/student-follow-along.md`](../student-follow-docs/student-follow-along.md) | The attendee's step-by-step companion: commands, prompts, expected output, self-check, homework. |
| [`student-follow-docs/pi-path.md`](../student-follow-docs/pi-path.md) | The free, offline on-ramp: pi (MIT) on a local Ollama model, first as it ships and then with `-nbt` so the same 25 MCP tools are all the model gets. |
| [`talks/speaking.md`](speaking.md) | The conference session built on this repo — abstract, what attendees leave with, the *Where This Breaks* catalog of local-model failure modes, formats, and stage requirements. Published at [/speaking/](https://davidparry.github.io/spec-driven-agentic/speaking/). |
| `scripts/` | `preflight.sh` (presenter readiness), `verify-workshop-run.sh` (fresh run branch + end-state check against the spec's acceptance criteria), `check-workshop-start.sh` / `check-class-complete.sh` (the two CI branch guards). |
| `.cursor/mcp.json` / [`.mcp.json`](../.mcp.json) / [`.pi/mcp.json`](../.pi/mcp.json) / [`config/mcp.json`](../config/mcp.json) | Registers `spec mcp serve`. Cursor (`.cursor/mcp.json`, and [`config/mcp.json`](../config/mcp.json)) passes `--root ${workspaceFolder}`. Claude Code ([`.mcp.json`](../.mcp.json)) passes `--root ${SPEC_PROJECT_DIR}` and, when that variable is unset, `spec` uses the directory the process was launched in. [pi](https://pi.dev) (`.pi/mcp.json`, read by `pi-mcp-extension`) has no `--root`, so launch pi from the repo root. |

## Branches and CI

| Branch | What it is |
| --- | --- |
| `trunk` | The workshop starting point — what you clone before the class. The kata has REQ-001 and REQ-002 implemented (the green baseline the exercises build on); REQ-003–006 are pending and REQ-007 does not exist yet. |
| `complete` | The finished loop: the end-of-class state — REQ-007 drafted and refined through the `validate_spec` / `refine_requirement` loops (Exercise 1), REQ-003 taken through Red/Green/Refactor to green (Exercise 2) — plus the homework done. Every requirement in the backlog (REQ-001–007) is implemented, with every scenario tagged and green. |

CI ([.github/workflows/ci.yml](../.github/workflows/ci.yml)) runs on every push and pull request:

- **build-and-test** — `mvn -pl smoke-test verify` (JUnit + Cucumber, JaCoCo 100%, SpotBugs, PMD) and `mvn -f kata/pom.xml test` for the standalone kata. This job does **not** require the `spec` binary, so a missing Rust build cannot look like a JaCoCo failure.
- **harness** — `spec` tests, clippy, fmt, coverage (≥ 97% library lines, `main.rs` ignored), and the release binary.
- **smoke-test-live** — needs `harness`; drives `smoke-test.jar` against that binary (`-Dspec.binary=…`) so the child's process is proven to be real `spec mcp serve`.
- **class-completeness** — runs [scripts/check-class-complete.sh](../scripts/check-class-complete.sh), which asserts the class deliverables (REQ-003 implemented, REQ-007 in the spec). It **fails on `trunk` by design** — the red X is the reminder that trunk is the starting line — and passes on `complete`.
- **workshop-start** — the inverse gate: runs [scripts/check-workshop-start.sh](../scripts/check-workshop-start.sh), which asserts the starting state is intact (REQ-003–006 pending, no REQ-007, no scenarios beyond REQ-001/002, `StringCalculator` unimplemented past REQ-002). It **passes on `trunk`** and **fails on `complete` by design**, so completed work can never silently leak into the branch attendees clone.

**Both branches build green on purpose.** "Incomplete" lives in the spec
(pending statuses), not in a failing build: your setup check (`spec --version`
and `mvn -q -f kata/pom.xml test`) must pass before the class, and the RED bar is created *live*
during Exercise 2 when the agent writes the `@REQ-003` scenario. Also note
that `mvn clean validate` runs no tests at all — `validate` only checks the
POMs. Use `mvn -pl smoke-test verify` and `mvn -f kata/pom.xml test`
to actually run the Java suites, and the two guard scripts above to tell
the branches apart.

## Prerequisites

Install `spec` first — [published installer](../README.md#install-spec), **0.5.4 or newer**.

- Java 21+
- Maven 3.9+
- An MCP host for the exercises (Cursor, Claude Desktop, [pi](https://pi.dev) with `pi install npm:pi-mcp-extension`, or the bundled `smoke-test.jar`)
- Optional, and the fully offline route: [Ollama](https://ollama.com) with `qwen3.8-flash-next:125b-mlx` — see [the pi path](../student-follow-docs/pi-path.md)
- Optional: Node.js for the MCP Inspector (`npx @modelcontextprotocol/inspector spec mcp serve --root $PWD`)

## Setup (do this before the workshop)

```bash
git clone <this repo> && cd tdd-bdd-agentic
spec --version                     # must succeed, and report 0.5.4 or newer
mvn -q -pl smoke-test package     # MCP-server smoke-test jar
mvn -q -f kata/pom.xml test       # standalone kata: JUnit + Cucumber
```

A green pair of builds means you're ready. The kata is **not** a Maven
module of the workshop reactor — it is its own project, so a copy of
`kata/` builds without this repo's parent POM. During the workshop you'll work on a
branch cut from `trunk` (`git checkout -b workshop trunk`) — the exercises
rewrite the spec and the kata, and `trunk` stays pristine so you can always
reset by re-branching. Details in
[`student-follow-along.md`](../student-follow-docs/student-follow-along.md).

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
| `list_requirements` | Every requirement with its id, title, status, and the spec `file` it lives in — find pending work, and know which document holds it once the catalog is split across includes. Re-reads the spec fresh on every call, so requirements an agent just drafted show up immediately. |
| `get_requirement` | One requirement's user story, acceptance criteria, and `featureLocation` — the raw material for Gherkin scenarios and failing tests, plus a `workflowHint` telling the agent what to do next. |
| `validate_spec` | Validates the requirements file **on disk**: well-formed unique ids, stories, Given/When/Then acceptance criteria, and tagged scenarios for implemented requirements. It reads the committed spec, so when a spec edit is waiting in staging its `nextStep` says so and names `changes_validate`, the staged-aware twin. During `spec draft` these lookups do not critique the in-flight proposal; `parse_proposals_checked` is that gate. `spec validate` exits non-zero on an invalid spec, so a CI gate can be scripted on it. |
| `refine_requirement` | Deterministic quality feedback on one requirement's wording: ambiguous words ("should", "handle", "quickly"), stories missing their actor or their why, outcomes with no concrete expected value, criteria covering more than one action, and happy-path-only coverage. Reads the staged edit when there is one and names which copy it judged in a `source` field, so the reword/refine loop converges without a `changes_commit` between passes. |
| `run_tests` | Runs the project tests (Maven on this kata), aggregating Cucumber (BDD) and JUnit (TDD) into one bar color: failures → **RED**, all passing → **GREEN**. On `spec implement` this sees the **working tree**, not an unstaged patch. |
| `get_tdd_state` | Current Red/Green/Refactor phase, last run summary, and a suggested next step. The reply leads with `phase`; the `instructions` guide to reading the phase log comes last. |
| `start_refactor` | Begins a refactor. Refuses unless the bar is GREEN, and words the refusal for the phase you are in — "never refactor on a red bar" on RED, "no tests have been run yet" at START, "a refactor is already in progress" in REFACTOR. |

**Authoring / staging:** `feature_list`, `feature_read`, `feature_create`,
`scenario_add`, `scenario_update`, `scenario_delete`, `changes_show`,
`changes_validate`, `changes_commit`, `changes_discard`, `requirement_reword`
(the repair path `validate_spec` and `refine_requirement` point at for
**wording** — agents must never hand-edit `requirements.json`, whose JSON
escaping and indentation differ from what the read tools return),
`requirement_mark_implemented`
(GREEN-gated, tagged scenario required), `step_definitions_find`,
`step_definition_create`, `unit_test_create` (arg `req_id`).

Catalog **structure** is the written-down exception to that prohibition. A
duplicate id needs a requirement object deleted and a repeated `includes`
entry needs removing, and no tool performs either edit, so `validate_spec`
and `changes_validate` answer those two classes by naming the file edit and
saying that the hand-editing rule covers wording, not structure.

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

## The hour

### The plumbing, briefly (16–20 min)

The server is `spec mcp serve` — this segment is a quick tour, not an
exercise. Composition root: `harness/src/mcp.rs` plus the harness TDD
services. A stdio server must never write to stdout — that corrupts the
JSON-RPC stream.
Prove the plumbing with the bundled **smoke test**, which does exactly what an
IDE does: launch `spec mcp serve`, `tools/list` (25 tools),
then `tools/call` — narrating each step. The walkthrough starts straight at
`tools/list`; the server's `2026-07-28` lifecycle does not require an
`initialize` handshake, though the Java MCP SDK still opens the stdio
session with the classic one (protocol `2025-11-25`) on first use, and the
server answers it normally.
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
[student-follow-docs/pre-step.log](../student-follow-docs/pre-step.log).
The smoke test then narrates the whole protocol exchange, starting like this:

```text
========================================================================
  STEP 0 — Launch the server
========================================================================
```

…through discovery and tool calls — the full expected output is
captured in [student-follow-docs/step2.log](../student-follow-docs/step2.log).

### Exercise 1 — Draft and refine the spec with your agent (20–32 min)

The spec comes first, and the agent helps write it — then refine it against
the server's feedback. `.cursor/mcp.json` (the same entry as
[`config/mcp.json`](../config/mcp.json), plus a `"protocolEra": "auto"` hint
for the host) already registers `spec mcp serve` with Cursor — `spec` must
be on PATH. Prompt your agent:

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

### Exercise 2 — The end-to-end agentic spec-to-green loop (32–50 min)

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

**Eighteen minutes buys one requirement, not two.** Measured across the
validation runs on the workshop's local model, `spec implement` takes
66–125 seconds per requirement, and 284.6 seconds in the worst case
observed — which is what happens when the model asks to run shell commands
and each one waits on your confirmation. Add the two Maven runs that
bracket it and the review you owe the staged Gherkin, and one requirement
is a comfortable fit in this window while two are not. Plan the room's time
around the worst case, not the median: nearly five minutes of a spinner is
within normal range and looks exactly like a hang. Per-command timings are
in [`notes/workshop-validation-runs.md`](../notes/workshop-validation-runs.md).

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

AGPL-3.0 — see [LICENSE](../LICENSE).

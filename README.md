# TDD, BDD & Spec-Driven Development in the Agentic Era

[![CI](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/ci.yml/badge.svg?branch=trunk)](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/ci.yml)
[![Release](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/release.yml/badge.svg)](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/release.yml)
[![bdd CLI](https://img.shields.io/github/v/release/davidparry/tdd-bdd-agentic?label=bdd%20CLI)](https://github.com/davidparry/tdd-bdd-agentic/releases/latest)
[![CLI coverage](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fdavidparry%2Ftdd-bdd-agentic%2Fbadges%2Fcoverage.json)](https://github.com/davidparry/tdd-bdd-agentic/actions/workflows/ci.yml)
[![Java coverage gate](https://img.shields.io/badge/JaCoCo-100%25%20gate-brightgreen)](pom.xml)
[![Quality gates](https://img.shields.io/badge/SpotBugs%20%7C%20PMD%20%7C%20clippy-enforced-blue)](.github/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/github/license/davidparry/tdd-bdd-agentic)](LICENSE)

> **Site:** [https://davidparry.github.io/tdd-bdd-agentic/](https://davidparry.github.io/tdd-bdd-agentic/)
> Install `bdd`, download binaries, open the talk, and read the
> [write-up](https://davidparry.com/blog/2026/08/07/spec-first-was-always-right-agents-just-made-it-fast/).

> **Conference organizers:** the session abstract, formats, and stage
> requirements are in [`speaking.md`](speaking.md) —
> *Turn Off the Wi-Fi: Spec-Driven Development That Delivers on a Local Model.*

> **Students: start here → [`student-follow-along.md`](student-follow-along.md)**
> Your step-by-step companion for the hour — the exact commands, the exact
> agent prompts, what you should see at every step, and a self-check that
> grades your run against the `complete` branch.

A 60-minute hands-on workshop. The MCP server and client in this repo are
**completed infrastructure — you use them, you don't build them**. What you
drive is a **spec-driven development workflow spanning SDD, BDD, and TDD**:
you and an AI agent draft requirements together and iterate them through two
server feedback loops — `validate_spec` for structure, `refine_requirement`
for wording quality — until the spec is valid and clean; requirements become
executable Gherkin scenarios and unit tests; and the agent collaborates with
you through the Red/Green/Refactor cycle — with the human in control of every
engineering decision. MCP is the plumbing that feeds these tools into
whatever agent you use (Cursor, Claude, or the bundled CLI client).

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

**Slides:** open [`slides/index.html`](slides/index.html) in a browser.

**Attending this workshop?** Follow
[`student-follow-along.md`](student-follow-along.md) step by step.

**Presenting this workshop?** Run `scripts/preflight.sh` before going on
stage, and rehearse through `scripts/verify-workshop-run.sh` — it cuts a
fresh branch from `trunk` for every run and verifies the end state against
the `complete` branch. (The minute-by-minute run-of-show is kept with the
presenter, not in the repo.)

## What's in the box

| Module / folder | What it is |
| --- | --- |
| `kata/` | A **standalone** Maven project — the String Calculator kata. It has its own `pom.xml` (no parent). Two requirements are implemented; the rest are driven agentically during the workshop. Gherkin feature files (`src/test/resources/features/`) are the executable behavior spec, run by Cucumber alongside the JUnit tests. Copy the folder and it still builds: `mvn -f kata/pom.xml test`. |
| `mcp-server/` | The MCP server (stdio transport, Java MCP SDK 2.0), complete and ready to use. Exposes seven workflow tools that drive the SDD/BDD/TDD loop — and is itself built with the same triangle: its own spec (`mcp-server/requirements/server-requirements.json`), tagged Cucumber scenarios (`src/test/resources/features/`), JUnit unit tests at 100% instruction/branch coverage (JaCoCo-enforced), and a `SpecCompletenessTest` that fails the build if spec and scenarios drift apart. SpotBugs and PMD gate `mvn verify`. Packages as `tdd-mcp-server.jar`. |
| `mcp-client/` | A minimal MCP client/agent harness, complete and ready to use. Launches the server as a child process, performs the handshake, discovers tools, and narrates the protocol exchange. Built to the same standard as the server: its own spec (`mcp-client/requirements/client-requirements.json`), tagged Cucumber scenarios (`src/test/resources/features/`), a `SpecCompletenessTest`, 100% instruction/branch coverage (JaCoCo-enforced), and SpotBugs + PMD gating `mvn verify`. Packages as `tdd-agent.jar`. |
| [`cli/`](cli/README.md) | The `bdd` CLI (Rust): one native binary that automates the same spec-driven loop the workshop teaches — spec validation and wording refinement, Ollama model selection (`qwen3-coder-next:latest` is the model used to develop the CLI; mileage varies with other models, especially those not trained for development), project/runtime inspection across Java, JavaScript/TypeScript, .NET, and Rust — with an embedded MCP server keeping the seven tool contracts above, byte-identical. Clean architecture, coverage-gated tests, its own spec-driven Cucumber suite. See [`cli/README.md`](cli/README.md) and the searchable [command manual](https://davidparry.github.io/tdd-bdd-agentic/manual/). |
| `requirements/requirements.json` | The SDD spec: the requirements backlog, and the root of the **spec catalog** — it holds requirements of its own and may `include` child spec files (which may include further files, N levels deep); the tooling merges the tree into one backlog. Each requirement carries acceptance criteria (already phrased Given/When/Then) that agents turn into executable Gherkin scenarios and failing tests, plus a `featureFile` pointer to where its scenarios live. Full field-by-field reference: [The requirements format](https://davidparry.github.io/tdd-bdd-agentic/manual/spec-format.html). |
| `slides/index.html` | The reveal.js slide deck for the 60-minute talk (self-contained, CDN-based). |
| `student-follow-along.md` | The attendee's step-by-step companion: commands, prompts, expected output, self-check, homework. |
| [`speaking.md`](speaking.md) | The conference session built on this repo — abstract, what attendees leave with, the *Where This Breaks* catalog of local-model failure modes, formats, and stage requirements. Published at [/speaking/](https://davidparry.github.io/tdd-bdd-agentic/speaking/). |
| `scripts/` | `preflight.sh` (presenter readiness), `verify-workshop-run.sh` (fresh run branch + end-state check against `complete`), `check-workshop-start.sh` / `check-class-complete.sh` (the two CI branch guards). |
| `.cursor/mcp.json` | Registers the server with Cursor so a real LLM agent can drive the loop. |

## Branches and CI

| Branch | What it is |
| --- | --- |
| `trunk` | The workshop starting point — what you clone before the class. The kata has REQ-001 and REQ-002 implemented (the green baseline the exercises build on); REQ-003–006 are pending and REQ-007 does not exist yet. |
| `complete` | The finished loop: the end-of-class state — REQ-007 drafted and refined through the `validate_spec` / `refine_requirement` loops (Exercise 1), REQ-003 taken through Red/Green/Refactor to green (Exercise 2) — plus the homework done. Every requirement in the backlog (REQ-001–007) is implemented, with every scenario tagged and green. |

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs three jobs on every push and pull request:

- **build-and-test** — `mvn verify` for mcp-server and mcp-client (JUnit + Cucumber, JaCoCo 100%, SpotBugs, PMD) and `mvn -f kata/pom.xml test` for the standalone kata. Green on both branches, and it uploads every report (Surefire, Cucumber HTML/JSON, JaCoCo, SpotBugs, PMD) as a build artifact.
- **class-completeness** — runs [scripts/check-class-complete.sh](scripts/check-class-complete.sh), which asserts the class deliverables (REQ-003 implemented, REQ-007 in the spec). It **fails on `trunk` by design** — the red X is the reminder that trunk is the starting line — and passes on `complete`.
- **workshop-start** — the inverse gate: runs [scripts/check-workshop-start.sh](scripts/check-workshop-start.sh), which asserts the starting state is intact (REQ-003–006 pending, no REQ-007, no scenarios beyond REQ-001/002, `StringCalculator` unimplemented past REQ-002). It **passes on `trunk`** and **fails on `complete` by design**, so completed work can never silently leak into the branch attendees clone.

**Both branches build green on purpose.** "Incomplete" lives in the spec
(pending statuses), not in a failing build: your setup check (`mvn -q
package && mvn -q -f kata/pom.xml test`) must pass before the class, and the RED bar is created *live*
during Exercise 2 when the agent writes the `@REQ-003` scenario. Also note
that `mvn clean validate` runs no tests at all — `validate` only checks the
POMs. Use `mvn clean verify` (MCP modules) and `mvn -f kata/pom.xml test`
(the kata) to actually run the suites, and the two guard
scripts above to tell the branches apart.

## Verify everything is ready for production

Before pushing to `trunk` (which deploys the website) or tagging a
release, run this from the repository root. It builds and gates
everything that ships — the CLI, the command manual, and the site:

```bash
(cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release) \
  && mdbook build manual \
  && scripts/build-pages.sh
```

1. **CLI** — format check, clippy with warnings as errors, the full
   unit + Cucumber suite, and the release binary
   (`cli/target/release/bdd`). The same gates CI enforces.
2. **Manual** — regenerates [`docs/manual/`](docs/manual/) from
   [`manual/src`](manual/src). The output is committed, so any
   changes it produces belong in your commit. Must run before the site
   build, which copies the built book.
3. **Website** — assembles `_site/` exactly as the
   [Pages workflow](.github/workflows/pages.yml) does on push to
   `trunk`; a clean local run means a clean deploy.

One-time tools: `cargo install mdbook` and `pip install markdown`.

If you touched the Java modules, also run `mvn clean verify` (MCP server
and client) and `mvn -f kata/pom.xml test` (the standalone kata). The
multi-platform release binaries are built by the release
workflow when a `v*` tag is pushed (`scripts/release.sh`), not
locally.

## Prerequisites

- Java 21+
- Maven 3.9+
- An MCP host for the exercises (Cursor, Claude Desktop, or the bundled client)
- Optional: Node.js for the MCP Inspector (`npx @modelcontextprotocol/inspector`)

## Setup (do this before the workshop)

```bash
git clone <this repo> && cd tdd-bdd-agentic
mvn -q package                    # MCP server + client: tests, both jars
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

| Tool | Purpose |
| --- | --- |
| `list_requirements` | Every requirement with its id, title, and status — find pending work. Re-reads the spec fresh on every call, so requirements an agent just drafted show up immediately. |
| `get_requirement` | One requirement's user story, acceptance criteria, and `featureLocation` — the raw material for Gherkin scenarios and failing tests, plus a `workflowHint` telling the agent to write the scenario first. |
| `validate_spec` | Validates the requirements file on disk: well-formed unique ids, stories, Given/When/Then acceptance criteria, and tagged scenarios for implemented requirements. The agent drafts, `validate_spec` arbitrates, and the loop repeats until `valid` is `true` — a valid spec is the entry ticket to writing any scenario or code. |
| `refine_requirement` | Deterministic quality feedback on one requirement's wording: ambiguous words ("should", "handle", "quickly"), stories missing their actor or their why, outcomes with no concrete expected value, criteria covering more than one action, and happy-path-only coverage. The LLM rewords from the findings and re-checks until `clean` is `true` — spec refinement as a feedback loop, not a vibe. |
| `run_tests` | Runs `mvn -f kata/pom.xml test`, aggregating the Cucumber scenarios (BDD) and JUnit tests (TDD) from the Surefire reports into one bar color: failures → **RED**, all passing → **GREEN**. |
| `get_tdd_state` | Current Red/Green/Refactor phase, last run summary, and a suggested next step. |
| `start_refactor` | Begins a refactor. Refuses unless the bar is GREEN — never refactor on a red bar. |

The workflow rules live in `TddStateMachine`, which was built test-first;
its tests are in `mcp-server/src/test/java`.

The server and the client both practice what they preach. Each module has
its own requirements spec (SDD) — `mcp-server/requirements/server-requirements.json`
with `@SRV-XXX` tags and `mcp-client/requirements/client-requirements.json`
with `@CLI-XXX` tags; each requirement has tagged Gherkin scenarios in the
module's own feature file, executed by Cucumber alongside the JUnit suite
(BDD + TDD, one bar); each module's `SpecCompletenessTest` fails the build if
any requirement lacks a tagged scenario or any scenario tag lacks a
requirement; and both modules enforce 100% instruction/branch coverage with
SpotBugs and PMD gating `mvn verify`.

## The workshop

### The plumbing, briefly (13–20 min)

The server and client are already built — this segment is a quick tour, not
an exercise. See the composition root in
`mcp-server/src/main/java/com/davidparry/workshop/mcp/server/TddMcpServer.java`
(one transport, seven tools via `McpServerFactory`; a stdio server must never
write to stdout — that corrupts the JSON-RPC stream), then prove the plumbing
works by running the bundled client, which does exactly what an IDE does:
launch the server, `initialize`, `tools/list`, then `tools/call` — narrating
each step.

```bash
mvn -q package && mvn -q -f kata/pom.xml test
java -jar mcp-client/target/tdd-agent.jar
```

The quiet kata test run prints the String Calculator Cucumber narration —
the full expected output of a combined reactor build is still captured in
[student-follow-docs/pre-step.log](student-follow-docs/pre-step.log)
(kata scenarios first, then the server and client suites). The
client then narrates the whole protocol exchange, starting like this:

```text
========================================================================
  STEP 0 — Launch the server
========================================================================
```

…through handshake, discovery, and tool calls — the full expected output is
captured in [student-follow-docs/step2.log](student-follow-docs/step2.log).

### Exercise 1 — Draft and refine the spec with your agent (20–32 min)

The spec comes first, and the agent helps write it — then refine it against
the server's feedback. `.cursor/mcp.json` already registers the server with
Cursor (build the jar first; for Claude Desktop copy the same JSON into its
config and replace `${workspaceFolder}` with the absolute repo path). Prompt
your agent:

> Add a new requirement to requirements/requirements.json: newlines may
> separate numbers in addition to commas. Follow the existing format — unique
> id, title, user story, acceptance criteria phrased Given/When/Then, status
> pending. Then call validate_spec and fix every issue until the spec is
> valid. Then call refine_requirement on the new requirement and reword it
> from the findings until there are none. Do not write scenarios or code yet
> — we are only agreeing on the spec.

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

With a valid spec, prompt your agent:

> Using the tdd-workflow tools: validate the spec first, then find the next
> pending requirement, add a Gherkin scenario for its acceptance criteria to
> the feature file (tag it with the requirement id), reuse or add step
> definitions, add a matching JUnit unit test, run the tests to show RED,
> then implement the simplest code to reach GREEN, then refactor, then mark
> the requirement implemented in requirements/requirements.json. Ask me
> before each phase change.

You'll watch the loop: `validate_spec` → `get_requirement` → agent writes the
Gherkin scenario (the executable spec) plus the unit test → **you review the
scenario** → `run_tests` (RED) → agent implements → `run_tests` (GREEN) →
`start_refactor` → cleanup → `run_tests` (still GREEN) → **you approve** →
the agent records the requirement as `implemented` in the spec. Requirements
REQ-003 through REQ-006 are waiting — plus the one you drafted in Exercise 1.
When you're done, `scripts/verify-workshop-run.sh check` grades your end
state against the `complete` branch.

### Backup — Inspect the protocol (if time allows)

Option A, the Inspector UI with a full message log:

```bash
npx @modelcontextprotocol/inspector \
  java -Dworkshop.root=$PWD -jar mcp-server/target/tdd-mcp-server.jar
```

Option B, be the client yourself — start the server and paste one line at a time:

```bash
java -Dworkshop.root=$PWD -jar mcp-server/target/tdd-mcp-server.jar
```

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"me","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"run_tests","arguments":{}}}
```

## Ideas to keep building

- Add a `write_test` tool that scaffolds test files from acceptance criteria.
- Expose `requirements.json` as an MCP **resource** and add per-phase **prompts**.
- Swap stdio for Streamable HTTP and share one server across a team.
- Point `workshop.root` at a real project and generalize `MavenTestRunner`.

## License

AGPL-3.0 — see [LICENSE](LICENSE).

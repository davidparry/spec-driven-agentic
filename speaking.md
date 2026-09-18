# Turn Off the Wi-Fi: Spec-Driven Development That Delivers on a Local Model

A conference session built on this repository. Everything it demonstrates runs
locally and is open source: this repository under AGPL-3.0, and the two pieces
it starts from — [pi](https://pi.dev) and its MCP extension — under MIT.

**Speaker:** David Parry ·
[davidparry.com](https://davidparry.com) ·
[github.com/davidparry](https://github.com/davidparry)

---

## Abstract

Your agent just produced 400 lines that compile, pass tests nobody asked for,
and encode a design you never approved. The industry's answer is "use a bigger
model." That answer costs you your budget, your code's confidentiality, and
your ability to reproduce a result six months from now. It also misreads the
problem: a bigger model is a better System 1, and System 2 has to come from
somewhere else.

There is a better answer, and Java developers have had it for twenty years:
write the specification first, and make the tests the contract. This session
builds up to it in three moves, each one starting from something you can
install for free this afternoon.

**We start with [pi](https://pi.dev)** — a minimal, MIT-licensed coding agent
that talks to [Ollama](https://ollama.com) out of the box. `ollama pull`, point
pi at the model, and you have an agent writing code on your laptop with no
account, no key, and no network. That is the whole on-ramp, and it is genuinely
good. It is also, by its author's explicit design, *loose*: pi ships eight
built-in tools — including `bash`, `write`, and `edit` — and its README states
the philosophy in four words, **"No permission popups."** There is no plan
mode, no phase gate, no staging area. On a frontier model you can live with
that, because you are the review. On `qwen3.8-flash-next:125b-mlx` you watch it
fix a failing build by editing the test.

**Second move: keep pi, take the tools away.** pi deliberately has no MCP in
core — a third-party extension adds it — so we install that extension, register
this repo's server (`spec mcp serve`, 25 tools), and launch pi with
`--no-builtin-tools`. Now the same local model, in the same loose host, has no
shell and no file writes: only a set of typed tools that make it validate a
requirement's structure, survive a wording review that rejects ambiguity like
*should*, *handles*, and *properly*, turn the accepted criteria into a tagged
Gherkin scenario and a JUnit test **through those tools**, watch it go red, then
make it green. A state machine refuses to let it refactor on a red bar. Every
write lands in a staging area a human reviews. `requirement_mark_implemented`
is GREEN-gated. The behavior change is dramatic, and nothing about the model
changed.

**Third move, and the point of the hour: that is still not enough.** `-nbt` is
a flag on one run. Nothing persists it, nothing sequences the work, and the
host still decides when to call what. So the same tools get a harness around
them — per-command profiles that hand a generating command three to seven tools
and nothing else, a requirements catalog rather than a chat transcript as the
source of truth, and Cucumber as the executable spec, because Gherkin has been
an agreed contract between business and code for twenty years and did not need
inventing. Once the discipline lives in the tooling instead of in a prompt,
model capability stops being the variable that decides quality. We run the
identical **contracts** twice — once with a frontier agent in Cursor (all 25
tools), once with `qwen3.8-flash-next:125b-mlx` on the laptop on stage — and
compare the diffs. Then we look at what makes the local model hold up:
JSON-only response contracts, one-finding-at-a-time correction,
validate-and-retry that feeds the invalid reply back, and deterministic
templates as the floor when generation fails.

Then comes the segment most talks skip: **Where This Breaks.** Real failures
this project hit — format drift, fixing the wrong file, silently dropping a
criterion, looping on an attempt that already failed — each with the
deterministic check that now catches it, and an honest account of where a
frontier model is still the right call.

The harness and its server were built this way themselves: every tool has a
numbered requirement, a Cucumber scenario, and a test that fails the build
when spec and scenarios drift apart.

Every layer is free and open source, and every layer runs on your hardware. To
prove the point, the demo runs with the Wi-Fi switched off.

## What attendees leave with

- A free, offline agent setup they can install the same afternoon: pi plus
  Ollama, no account and no key, and a clear-eyed read on what that gets them
  and what it does not.
- The one-flag demonstration that tool surface, not model size, is the lever:
  the same local model in the same host, with and without `--no-builtin-tools`.
- A pattern for MCP servers that **enforce a workflow** instead of handing
  agents filesystem access.
- The deterministic validation layer that lets a local model produce output you
  would sign your name to.
- A spec format that maps onto Cucumber-JVM and JUnit 5, and the build gate that
  keeps the two from drifting apart.
- A named catalog of local-model failure modes, each with the guardrail that
  catches it.
- Three optional breakage demos they can run themselves, where the *human*
  breaks the spec and the tool catches it: a criterion that is not phrased
  Given/When/Then (`validate_spec` rejects it), a story full of *should*,
  *handle*, and *quickly* (`refine_requirement` returns one finding per
  offence), and a spec split across included files that still merges into one
  backlog.
- A repository you can run on your own machine, behind your own firewall.

## Where This Breaks

The honest segment, and the reason this is not a demo reel. Each of these is a
failure the project actually hit; each is now caught by a deterministic check.

| Failure mode | What the model did | What catches it now |
| --- | --- | --- |
| Format drift | Wrapped required JSON in markdown fences, or emitted `<think>` blocks | Prompts forbid it *and* the parser rejects it; the invalid reply is fed back with a correction prompt |
| Fixing the wrong file | Rewrote the production class repeatedly while the real bug was a Cucumber step expression that did not match the scenario line, down to a trailing period | The failure output is scanned for implicated files, and the reply must include them |
| Silent scope loss | Dropped an acceptance criterion during a rewording pass | Rewording addresses **one** finding at a time and must return every criterion |
| Looping | Repeated an attempt that had already failed | Attempt history — targets written and what the following test run reported — is fed into the next prompt |
| Ambiguity leaking into the spec | Wrote "handles negatives properly" | A deterministic wording review rejects a fixed list of ambiguous words before any code is written |
| Refactoring on red | Offered to "clean up" while tests were failing | The TDD state machine refuses the transition from any phase but GREEN |
| Premature completion | Marked a requirement implemented with nothing proving it | `requirement_mark_implemented` requires GREEN plus a scenario tagged with the requirement ID |
| Right process, wrong requirement | Asked for "the next pending id", took the requirement it had just drafted to green instead — correct discipline, every gate satisfied, an hour spent on work nobody asked for | Nothing in the loop, and that is the point: the phase gates police *how* the agent works, never *what it works on*. The prompt names the id, and the end-of-run verifier grades that id by name |
| Tool-calling drift | Local model invented a tool, skipped staging, or called `command_run` without waiting | Per-command profiles (`spec tools profiles`) hand the generating commands 3–7 tools and the read-only `spec ask` 12; `[tool_rules]` in `harness/prompts/prompts.toml`; the harness's `command_run` asks the human to confirm |
| Fixing the test instead of the code | Given a shell and a writable test file, the local model made the bar green by deleting the assertion | Nothing in a loose host — pi has no permission popups by design. In the harness the model never gets a shell or a free-hand write: an implementation attempt lands in staging as a reviewable diff, and `run_tests` is the only thing that can report a bar |
| Implementing more than was asked | Asked to implement REQ-006, the local model also quietly implemented REQ-005 — the bar went green, both behaviors worked, and the spec still called REQ-005 pending | The mirror image of premature completion: the code runs ahead of the spec instead of behind it. `spec implement` now matches the staged diff against the other pending requirements and warns when it satisfies one. Literal matching — same quoted inputs, same expected value — so it warns and never blocks; reading the staged diff is still the real defense |

Where a frontier model is still the better call, and where a human still has to
be on the review, is stated plainly rather than skipped.

## Who it is for

Java developers, tech leads, and architects who are adopting AI coding agents
and are accountable for what those agents produce — especially in regulated,
air-gapped, or cost-constrained environments. Level: intermediate. No prior MCP
or LLM experience required; comfort with JUnit and Cucumber is assumed.

## Formats

One deck, two cuts — [`slides/index.html`](slides/index.html) selects by
query string, and <kbd>t</kbd> switches between them live.

| Format | Deck cut | What it covers |
| --- | --- | --- |
| **Conference session** (50 minutes) | [`?60`](slides/index.html?60) | The full narrative above — pi on Ollama, pi with its built-ins taken away, then the harness — with the live frontier-versus-local comparison and the *Where This Breaks* segment. The two exercise slides are delivered as demos from the stage rather than as hands-on time, which is what makes the 60-minute cut fit in 50. |
| **Short session** (30 minutes) | [`?30`](slides/index.html?30) | The same three acts, demo-driven, with no hands-on segment: pi free and offline, the one-flag change to `pi --no-builtin-tools`, then the spec-specific runner taken from requirement to green — plus *Where This Breaks*. Published at [/talk30/](https://davidparry.github.io/spec-driven-agentic/talk30/). |
| **Hands-on workshop** (60 minutes) | [`?60`](slides/index.html?60) | Attendees run the loop on their own machines against a local model: draft a requirement, refine it until the wording review is clean, take it through RED, GREEN, and REFACTOR, then grade the run with `scripts/verify-workshop-run.sh check`. Companion material is the [workshop follow-along](student-follow-along.md); the [pi path](student-follow-docs/pi-path.md) is the free, no-IDE on-ramp, and the [harness path](student-follow-docs/harness-path.md) covers attendees who prefer the terminal to an IDE. |

## Technical requirements

- Stage projector and my laptop.
- **No conference network needed.** The entire demo runs locally against
  Ollama, which is the thesis rather than a convenience.
- Stack on stage: Java 21, Maven, the `spec` binary (`spec mcp serve`), Cucumber-JVM 7, JUnit 5,
  Ollama running `qwen3.8-flash-next:125b-mlx`, and [pi](https://pi.dev) with the
  `pi-mcp-extension` package for the first two acts. The bundled `smoke-test.jar` is an
  MCP-server **smoke test**.
- For the workshop format: attendees need **`spec` on PATH**, Java 21, Maven, git, and an MCP
  host — Cursor, Claude, or pi with `pi-mcp-extension` — and, to run fully offline, Ollama with
  `qwen3.8-flash-next:125b-mlx` pulled ahead of time. The [pi path](student-follow-docs/pi-path.md)
  and the [harness path](student-follow-docs/harness-path.md) are the Wi-Fi-off alternatives.

## What is on stage, in this repo

| Shown live | Where it lives |
| --- | --- |
| The free on-ramp: pi on Ollama, then pi with `--no-builtin-tools` against this server | [`.pi/mcp.json`](.pi/mcp.json), [`student-follow-docs/pi-path.md`](student-follow-docs/pi-path.md) |
| The 25-tool MCP server enforcing the loop | `harness/src/mcp.rs` (`spec mcp serve`) |
| The deterministic structure and wording reviews | `harness/src/domain/` (spec validator, requirement refiner) |
| The state machine that refuses a red-bar refactor | `harness/src/domain/tdd.rs` (`TddStateMachine`) |
| The requirements catalog that drives everything | `requirements/requirements.json` |
| Gherkin and JUnit generated from the spec | `kata/` |
| The offline harness: same server, scoped profiles on `qwen3.8-flash-next:125b-mlx` | [`harness/README.md`](harness/README.md), [`student-follow-docs/harness-path.md`](student-follow-docs/harness-path.md) |
| Every prompt sent to the model, in one auditable file | `harness/prompts/prompts.toml` |
| The end-of-run grader that names the requirement id — where "right process, wrong requirement" is caught | [`scripts/verify-workshop-run.sh`](scripts/verify-workshop-run.sh) |
| The bundled MCP-server smoke test: launch, discover 25 tools, invoke | `smoke-test/`, captured run in [`student-follow-docs/step2.log`](student-follow-docs/step2.log) |
| The slide deck — one file, two cuts ([`?30`](slides/index.html?30) selects the short one, or press <kbd>t</kbd> in the deck) | [`slides/index.html`](slides/index.html) |

## Booking

Interested in this session or the workshop for your conference or team? Open an
issue on
[github.com/davidparry/spec-driven-agentic](https://github.com/davidparry/spec-driven-agentic/issues)
or reach out through [davidparry.com](https://davidparry.com).

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
write the specification first, and make the tests the contract. The way to make
an agent honor that is not a better model but a **purpose-built harness** —
software that already knows the job, with the model as its smallest component.
This session builds one in three moves, every layer free and installable this
afternoon.

**We start with the harness.** `spec` is a single binary that does one job: take
a requirement from a validated spec to a green bar. Underneath it is an MCP
server with 25 tools, none of which calls a model — the same input gives the
same answer — and on top of it a runner that owns the parts a prompt cannot be
trusted with. The sequence is fixed: a requirement's structure is validated, its
wording survives a review that rejects ambiguity like *should*, *handles*, and
*properly*, the accepted criteria become a tagged Gherkin scenario and a JUnit
test **through those tools**, the suite goes red, and only then is production
code written. Per-command profiles hand a generating command three to seven
tools and nothing else. Every write lands in a staging area a human reviews as a
diff. On stage that sequence is driven a step at a time, with the room as the
orchestrator, because a command that collapses the steps also collapses the
checks — and the checks are the session. Each step is verified rather than
trusted: the asset survey is re-read between them, so a step that claims to
have written a scenario is answered by a survey that says whether one is
there. A state machine refuses to refactor on a red bar, and
`requirement_mark_implemented` is GREEN-gated and needs a scenario tagged with
the requirement id. A requirements catalog rather than a chat transcript is the
source of truth, and Cucumber is the executable spec, because Gherkin has been
an agreed contract between business and code for twenty years and did not need
inventing.

**Then the claim that costs the industry the most money.** That harness drives
`qwen3.8-flash-next:125b-mlx` on the laptop on stage, Wi-Fi off, and produces
work you would sign your name to — not because the model improved, but because
nothing is asking it to remember the discipline. We run the identical
**contracts** twice — once with a frontier agent in Cursor (all 25 tools), once
locally — and compare the diffs. Then we look at what makes the local model hold
up: JSON-only response contracts, one-finding-at-a-time correction,
validate-and-retry that feeds the invalid reply back, and deterministic
templates as the floor when generation fails. Once the discipline lives in the
tooling instead of in a prompt, model capability stops being the variable that
decides quality — which is the same as saying you stop needing to rent it.

**Second move: the same tools, a general host.** [pi](https://pi.dev) is a
minimal, MIT-licensed coding agent that talks to [Ollama](https://ollama.com)
out of the box. `ollama pull`, point pi at the model, and you have an agent
writing code on your laptop with no account, no key, and no network. That
on-ramp is genuinely good, and it is also, by its author's explicit design,
*loose*: pi ships eight built-in tools — including `bash`, `write`, and `edit` —
and its README states the philosophy in four words, **"No permission popups."**
There is no plan mode, no phase gate, no staging area. On a frontier model you
can live with that, because you are the review. On a local model you watch it
fix a failing build by deleting the assertion. So we take the tools away: pi
deliberately has no MCP in core, so we install the extension, register the same
server, and launch with `--no-builtin-tools`. The same model in the same loose
host now has no shell and no file writes — only the typed tools the runner was
already driving — and the behavior change is dramatic with nothing about the
model changed.

**Third move: pick your altitude.** `-nbt` is a flag on one run. Nothing
persists it, nothing sequences the work, and the host still decides when to call
what. A general agent you specialize is a different bet from a runner that
ships specialized, and the honest comparison is the last thing we draw: whether
the discipline is something you re-type or something that ships.

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

- The shape of a **purpose-built harness**: the sequence, the tool surface, the
  phase gates, and the prompts as versioned software rather than as things a
  developer re-types into a chat box.
- The argument, demonstrated rather than asserted, that a harness of this shape
  **removes the frontier model from the requirements list** for this class of
  work — no key, no bill, no code leaving the building, and a run that still
  reproduces in six months.
- A pattern for MCP servers that **enforce a workflow** instead of handing
  agents filesystem access.
- The deterministic validation layer that lets a local model produce output you
  would sign your name to.
- A free, offline agent setup they can install the same afternoon: pi plus
  Ollama, no account and no key, and a clear-eyed read on what that gets them
  and what it does not.
- The one-flag demonstration that tool surface, not model size, is the lever:
  the same local model in the same host, with and without `--no-builtin-tools`.
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
| **Conference session** (50 minutes) | [`?60`](slides/index.html?60) | The full narrative above — the harness first, taken from requirement to green on a local model, then pi as the free general-purpose on-ramp and the altitude comparison — with the live frontier-versus-local comparison and the *Where This Breaks* segment. The two exercise slides are delivered as demos from the stage rather than as hands-on time, which is what makes the 60-minute cut fit in 50. |
| **Short session** (30 minutes) | [`?30`](slides/index.html?30) | The harness and nothing else, demo-driven, with no hands-on segment: why a general agent gives a general result, what a purpose-built harness owns instead, then the runner taken from requirement to green with the Wi-Fi off — plus *Where This Breaks*. pi is 60-minute material and does not appear. Published at [/talk30/](https://davidparry.github.io/spec-driven-agentic/talk30/). |
| **Hands-on workshop** (60 minutes) | [`?60`](slides/index.html?60) | Attendees run the loop on their own machines against a local model: draft a requirement, refine it until the wording review is clean, take it through RED, GREEN, and REFACTOR, then grade the run with `scripts/verify-workshop-run.sh check`. Companion material is the [workshop follow-along](../student-follow-docs/student-follow-along.md); the [pi path](../student-follow-docs/pi-path.md) is the free, no-IDE on-ramp, and the [harness path](../student-follow-docs/harness-path.md) covers attendees who prefer the terminal to an IDE. |

Every cut runs **the same requirement through the same steps** and ends on the
same verdict: REQ-003 driven a step at a time with the asset survey checked
between steps, then `scripts/verify-workshop-run.sh check` grading seven named
checks. The short cut drops the pi segment, the MCP internals, and the hands-on
time — never the loop, and never the way it is verified.

## Technical requirements

- Stage projector and my laptop.
- **No conference network needed.** The entire demo runs locally against
  Ollama, which is the thesis rather than a convenience.
- Stack on stage: Java 21, Maven, the `spec` binary (`spec mcp serve`), Cucumber-JVM 7, JUnit 5,
  Ollama running `qwen3.8-flash-next:125b-mlx`, and [pi](https://pi.dev) with the
  `pi-mcp-extension` package for the general-agent segment. The bundled `smoke-test.jar` is an
  MCP-server **smoke test**.
- For the workshop format: attendees need **`spec` on PATH**, Java 21, Maven, git, and an MCP
  host — Cursor, Claude, or pi with `pi-mcp-extension` — and, to run fully offline, Ollama with
  `qwen3.8-flash-next:125b-mlx` pulled ahead of time. The [pi path](../student-follow-docs/pi-path.md)
  and the [harness path](../student-follow-docs/harness-path.md) are the Wi-Fi-off alternatives.

## What is on stage, in this repo

| Shown live | Where it lives |
| --- | --- |
| The purpose-built runner: one binary, one job, per-command tool profiles | `harness/src/main.rs` (`spec`), `spec tools profiles` |
| The 25-tool MCP server enforcing the loop | `harness/src/mcp.rs` (`spec mcp serve`) |
| The deterministic structure and wording reviews | `harness/src/domain/` (spec validator, requirement refiner) |
| The state machine that refuses a red-bar refactor | `harness/src/domain/tdd.rs` (`TddStateMachine`) |
| The requirements catalog that drives everything | `requirements/requirements.json` |
| Gherkin and JUnit generated from the spec | `kata/` |
| The offline harness: same server, scoped profiles on `qwen3.8-flash-next:125b-mlx` | [`harness/README.md`](../harness/README.md), [`student-follow-docs/harness-path.md`](../student-follow-docs/harness-path.md) |
| The asset survey each step is checked against — the gap still open, and the one next command | `spec status` |
| Every prompt sent to the model, in one auditable file | `harness/prompts/prompts.toml` |
| The free general-purpose on-ramp: pi on Ollama, then pi with `--no-builtin-tools` against this server | [`.pi/mcp.json`](../.pi/mcp.json), [`student-follow-docs/pi-path.md`](../student-follow-docs/pi-path.md) |
| The end-of-run grader that names the requirement id — where "right process, wrong requirement" is caught | [`scripts/verify-workshop-run.sh`](../scripts/verify-workshop-run.sh) |
| The bundled MCP-server smoke test: launch, discover 25 tools, invoke | `smoke-test/`, captured run in [`student-follow-docs/step2.log`](../student-follow-docs/step2.log) |
| The slide deck — one file, two cuts ([`?30`](slides/index.html?30) selects the short one, or press <kbd>t</kbd> in the deck) | [`slides/index.html`](slides/index.html) |

## Booking

Interested in this session or the workshop for your conference or team? Open an
issue on
[github.com/davidparry/spec-driven-agentic](https://github.com/davidparry/spec-driven-agentic/issues)
or reach out through [davidparry.com](https://davidparry.com).

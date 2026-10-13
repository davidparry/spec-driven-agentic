Two conference sessions are built on this repository, sharing one deck and one
codebase:

1. **Turn Off the Wi-Fi** — spec-driven development on a local model. The
   argument about where the discipline should live.
2. **[A Day in the Life](#a-day-in-the-life)** — a harness is worn, a jig is
   clamped down. One requirement delivered twice under two very different
   amounts of trust, and what that means for AI in your own product.

---

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
somewhere else. A model trained to decide rather than to write is still
System 1 — faster and typed, but holding no ground truth.

There is a better answer, and developers have had it for twenty years:
write the specification first, and make the tests the contract. The way to make
an agent honor that is not a better model but a **purpose-built harness** —
software that already knows the job, with the model as its smallest component.
This session builds one in three moves, every layer free and installable this
afternoon.

**We start with the harness.** `spec` is a single binary that does one job: take
a requirement from a validated spec to a green bar. Underneath it is an MCP
server whose tools are exactly the ones the loop needs, none of which calls a model by default — the same input
gives the same answer — and on top of it a runner that owns the parts a prompt
cannot be trusted with. The sequence is fixed: a requirement's structure is validated, its
wording survives a review that rejects ambiguity like *should*, *handles*, and
*properly*, the accepted criteria become a tagged Gherkin scenario and a JUnit
test **through those tools**, the suite goes red, and only then is production
code written. Per-command profiles hand a generating command three to seven
tools and nothing else. Every write lands in the project's real files, on a
branch the run asks for up front, so `git diff` is the review and `git restore`
is the undo. On stage that sequence is driven a step at a time, with the room as the
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
**contracts** twice — once with a frontier agent in Cursor (every tool on offer), once
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
There is no plan mode, no phase gate, no branch gate. On a frontier model you
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

The harness is held to the discipline it enforces.
[`harness/requirements/requirements.json`](../harness/requirements/requirements.json)
is the spec for the `spec` binary — numbered requirements, each with a Cucumber
scenario carrying its tag — and
[`harness/tests/spec_completeness.rs`](../harness/tests/spec_completeness.rs)
fails the build on drift in either direction: a requirement whose scenario is
missing, a tag left behind by a requirement that is gone, or an implemented
requirement whose wording would not survive the harness's own review. The MCP
surface gets the same treatment from the Java side, where `smoke-test/` carries
its own catalog and an unplanned tool fails that build until someone plans it.

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
| Ambiguity leaking into the spec | Wrote "handles negatives properly" | A deterministic wording review rejects a fixed list of ambiguous words before any code is written. Optionally a decision model answers one typed question beside it — *is this criterion measurable* — which catches wording the fixed list cannot, and is advice rather than a gate |
| Refactoring on red | Offered to "clean up" while tests were failing | The TDD state machine refuses the transition from any phase but GREEN |
| Premature completion | Marked a requirement implemented with nothing proving it | `requirement_mark_implemented` requires GREEN plus a scenario tagged with the requirement ID |
| Right process, wrong requirement | Asked for "the next pending id", took the requirement it had just drafted to green instead — correct discipline, every gate satisfied, an hour spent on work nobody asked for | Nothing in the loop, and that is the point: the phase gates police *how* the agent works, never *what it works on*. The prompt names the id, and the end-of-run verifier grades that id by name |
| Tool-calling drift | Local model invented a tool, skipped the authoring tools, or called `command_run` without waiting | Per-command profiles (`spec tools profiles`) hand the generating commands 3–7 tools and the read-only `spec ask` 12; `[tool_rules]` in `harness/prompts/prompts.toml`; the harness's `command_run` asks the human to confirm |
| Fixing the test instead of the code | Given a shell and a writable test file, the local model made the bar green by deleting the assertion | Nothing in a loose host — pi has no permission popups by design. In the harness the model never gets a shell or a free-hand write: an implementation attempt lands in the named files as a reviewable diff, and `run_tests` is the only thing that can report a bar |
| Implementing more than was asked | Asked to implement REQ-006, the local model also quietly implemented REQ-005 — the bar went green, both behaviors worked, and the spec still called REQ-005 pending | The mirror image of premature completion: the code runs ahead of the spec instead of behind it. `spec implement` now matches the diff it wrote against the other pending requirements and warns when it satisfies one. Literal matching — same quoted inputs, same expected value — so it warns and never blocks; reading the diff is still the real defense |

Where a frontier model is still the better call, and where a human still has to
be on the review, is stated plainly rather than skipped.

## Who it is for

Developers, tech leads, and architects who are adopting AI coding agents
and are accountable for what those agents produce — especially in regulated,
air-gapped, or cost-constrained environments. Level: intermediate. No prior MCP
or LLM experience required; comfort with unit tests and BDD scenarios is
assumed, in whatever language you write them — the examples on stage are
JUnit and Cucumber.

## Formats

One deck, three cuts — [`slides/index.html`](slides/index.html) selects by
query string, and <kbd>t</kbd> cycles through them live.

| Format | Deck cut | What it covers |
| --- | --- | --- |
| **Conference session** (50 minutes) | [`?60`](slides/index.html?60) | The full narrative above — the harness first, taken from requirement to green on a local model, then pi as the free general-purpose on-ramp and the altitude comparison — with the live frontier-versus-local comparison and the *Where This Breaks* segment. The two exercise slides are delivered as demos from the stage rather than as hands-on time, which is what makes the 60-minute cut fit in 50. |
| **A Day in the Life** (60 minutes) | [`?day`](slides/index.html?day) | A separate session, described [below](#a-day-in-the-life): seven slides and two live demos, both delivering the sentence `explain one spec command by name - what it does` — once by a frontier model in an editor sequenced by a skill, once by a compiled command that never puts a skill in front of the model at all. Then the same five pieces on a business decision with no test suite, and what Spring AI and Embabel give you for building the same thing into a product. Published at [/talkday/](https://davidparry.github.io/spec-driven-agentic/talkday/). |
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
| The MCP server enforcing the loop | `harness/src/mcp.rs` (`spec mcp serve`) |
| The deterministic structure and wording reviews | `harness/src/domain/` (spec validator, requirement refiner) |
| The state machine that refuses a red-bar refactor | `harness/src/domain/tdd.rs` (`TddStateMachine`) |
| The requirements catalog that drives everything | `requirements/requirements.json` |
| Gherkin and JUnit generated from the spec | `kata/` |
| The offline harness: same server, scoped profiles on `qwen3.8-flash-next:125b-mlx` | [`harness/README.md`](../harness/README.md), [`student-follow-docs/harness-path.md`](../student-follow-docs/harness-path.md) |
| The asset survey each step is checked against — the gap still open, and the one next command | `spec status` |
| Every prompt sent to the model, in one auditable file | `harness/prompts/prompts.toml` |
| The free general-purpose on-ramp: pi on Ollama, then pi with `--no-builtin-tools` against this server | [`.pi/mcp.json`](../.pi/mcp.json), [`student-follow-docs/pi-path.md`](../student-follow-docs/pi-path.md) |
| The end-of-run grader that names the requirement id — where "right process, wrong requirement" is caught | [`scripts/verify-workshop-run.sh`](../scripts/verify-workshop-run.sh) |
| The bundled MCP-server smoke test: launch, discover the tools, invoke | `smoke-test/`, captured run in [`student-follow-docs/step2.log`](../student-follow-docs/step2.log) |
| The slide deck — one file, three cuts ([`?30`](slides/index.html?30) and [`?day`](slides/index.html?day) select the others, or press <kbd>t</kbd> in the deck) | [`slides/index.html`](slides/index.html) |
| The harness's own spec, and the build gate that fails on drift | [`harness/requirements/requirements.json`](../harness/requirements/requirements.json), [`harness/tests/spec_completeness.rs`](../harness/tests/spec_completeness.rs) |

---

# A Day in the Life

**60 minutes · two live demos · deck cut [`?day`](slides/index.html?day), published at
[/talkday/](https://davidparry.github.io/spec-driven-agentic/talkday/)**

## Abstract

A harness goes on a person. It catches you when you fall, and you still decide
where to walk. Nobody wears a jig. A jig is clamped to the bench, and it holds
the work so the cut comes out the same every time. That distinction is the whole
session, because the thing we are now trying to hold is a language model.

The test it gives you is blunt: can the model **skip** it? Anything a model can
talk its way past is a preference, not a rail. Most of what we currently call
guardrails fails that test.

The session proves it by delivering one requirement twice. The sentence is
`explain one spec command by name - what it does`, and both runs go through the
same MCP server, the same gates, and the same acceptance criteria.

**Demo one** is the loose end of the range: a frontier model in an editor,
sequenced by a skill, reaching the work through 27 typed tools and nothing else —
the one thing it cannot do for itself is create a git branch, which needs a shell
it was never given, so it stops and asks.
It opens by asking the agent what the project *is* — which is worth watching,
because an agent describing a repository from its prose is guessing, and an agent
describing it from `list_requirements` is reading. Then it works the loop:
requirement, structure review, wording review, Gherkin, step definitions, unit
test, red, implementation, green, refactor. Real gates hold throughout. It
cannot refactor on a red bar, it cannot mark work implemented without a green bar
and a scenario tagged with the requirement id, and the one command-running tool
is red-gated, argv-allowlisted, and spawns no shell.

Then the uncomfortable part: a Skill is not a Jig. A skill is a briefing. It
loads when the model decides it is relevant and it is followed as far as the
model chooses to follow it. Activation and compliance are both the model's call,
so a skill fails the skip test by construction — nobody inside a skill has the
job of saying no. The skill is not the jig. The server is. A model can skip an
instruction; it cannot skip a tool that returns an error instead of a result.

**Demo two** is the tight end, and the same sentence goes in: one compiled
command, no skill anywhere on the wire. The sequence is in code, so nothing asks
the model whether to run it, in what order, or whether this step can be skipped
just this once. The room watches three judges work at every generating stage —
deterministic regex and structural rules, type checks that mean a response which
will not parse into the expected shape is not a result at all, and a decision
model asked one bounded question at a time, returning a probability, where
anything under 0.80 comes back unsure and unsure is never read as a yes. Four of
those judgments exist only here, because only this path generates code. The
honest parts get shown too: the measured runs took 20, 36 and 57 minutes, a
second run is faster only because of a cache keyed on the whole conversation,
the step can time out and lose its partial work, and an attempt can come back
with nothing usable. When
it falls short it reports what landed and exits nonzero, which is the one thing a
prompt structurally cannot do: tell you the truth about work nobody watched.

Strip the domain out of both demos and five pieces are left, none of them about
software delivery: a source of truth validated before anything acts on it, a
sequence held in code rather than a prompt, a narrow typed surface that is the
only way to touch the work, evidence collected before anything is called done,
and a refusal when the evidence is not there.

So the session spends its last third somewhere else entirely: an account coming
up for loyalty review, where the tier drives discount band, support SLA, renewal
terms, and who gets a human account manager. We have two ways of building that
today and both are bad. A rules engine, where every exception is a new rule, the
firing order quietly becomes load-bearing, and the tier arrives with no account of
itself. Or a prompt, which answers fluently every time and gives you no way to
know which time it was right. The jig version puts the policy under structural
and wording review, makes the decision a state machine, gives the model five
read-only typed tools, demands evidence that names its source before any tier is
proposed, and refuses when the evidence is missing rather than guessing. The
walked example lands on Silver with arithmetic nobody can argue with, and leaves
one judgement call sitting in the record at 0.64 — under the floor, unused, and
flagged for a human.

The close is the part that should follow people out of the room. All of this
builds a jig for a developer's own working day, and that is the easy half. The
question you are answerable for is how AI enters the product your business sells.
On the JVM the pieces already exist, and the session is specific about which does
what: Embabel derives its plan from types — an action's parameter types are its
preconditions, its return type is its effect — and plans with Goal-Oriented
Action Planning, an A\* search that is deterministic and is not a model. Spring
AI supplies the judges: schema-validated structured output with a correcting
retry, provider-level output constraints, tool scoping and progressive disclosure
as the clamp, and a fact-checking evaluator that asks one bounded question and is
documented running on a small local model rather than a frontier one. Refusal
falls out for free: a condition that stays false leaves no applicable action, so
there is no path to the goal — nothing to be talked past.

Rod Johnson's phrase for the posture is "trust, then verify", which is the
opposite of both things we usually ship: the rules engine that verifies without
trusting anything, and the prompt that trusts without verifying anything.

## What attendees leave with

- A usable distinction between a **harness and a jig**, and the skip test that
  tells them apart in their own codebase.
- Why a **skill, a prompt, or a system message is not a control** — and what the
  difference looks like when you watch the same requirement delivered both ways.
- The **three kinds of judge** a pipeline needs, and why keeping them apart
  matters more than which model you chose: deterministic rules, type checks, and
  a bounded-question decision model with a confidence floor.
- A concrete reason the **frontier model is not the requirement**, from a worked
  decision where the expensive model's remaining share of the job is small enough
  to run on your own hardware.
- The case for **refusal as a first-class outcome**, which is the thing both a
  rules engine and a prompt will never give you.
- A specific, named mapping onto **Spring AI and Embabel** for building the same
  pattern into a product, with the planner on the deterministic side of the line.
- An honest account of the **cost**: what strictness buys, what it costs to
  build, and the kind of work where a jig does not repay the effort at all.

## Who it is for

Developers, tech leads, and architects who are now accountable for what coding
agents produce, and for what AI does inside the products they ship. Level:
intermediate. No MCP or LLM experience required; comfort with a unit test and a
Gherkin scenario is assumed. The demos are Rust and Java, and the framework
mapping at the close is JVM-specific, but nothing in the argument is
language-specific.

## Technical requirements

Stage projector and my laptop. No conference network needed. On stage: the
`spec` binary, Rust and Cargo, Ollama running two local models — one that
writes and a smaller one that judges — and an editor with an MCP host for the
first demo. The two demos run from two git worktrees, one each, because they
drive the same crate and cannot share a build directory or a test bar.
Presenter script and the verified paste-ready wording are in
[`presenter-notes.md`](presenter-notes.md); the attendee companion is
[`a-day-life.md`](../student-follow-docs/a-day-life.md).

## Booking

Interested in this session or the workshop for your conference or team? Open an
issue on
[github.com/davidparry/spec-driven-agentic](https://github.com/davidparry/spec-driven-agentic/issues)
or reach out through [davidparry.com](https://davidparry.com).

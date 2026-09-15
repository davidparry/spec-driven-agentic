# Turn Off the Wi-Fi: Spec-Driven Development That Delivers on a Local Model

A conference session built on this repository. Everything it demonstrates is
here, runs locally, and is open source under AGPL-3.0.

**Speaker:** David Parry ·
[davidparry.com](https://davidparry.com) ·
[github.com/davidparry](https://github.com/davidparry)

---

## Abstract

Your agent just produced 400 lines that compile, pass tests nobody asked for,
and encode a design you never approved. The industry's answer is "use a bigger
model." That answer costs you your budget, your code's confidentiality, and
your ability to reproduce a result six months from now.

There is a better answer, and Java developers have had it for twenty years:
write the specification first, and make the tests the contract.

This session walks through a working, open-source pipeline where a requirements
catalog — not a chat transcript — is the source of truth. One MCP server
(`bdd mcp serve`, 23 tools) exposes a deliberately locked-down set: no
"write this file," no open shell. Cursor sees every tool, including staging.
The agent must validate a requirement's structure, survive a wording
review that rejects ambiguity like *should*, *handles*, and *properly*, turn the
accepted criteria into a tagged Gherkin scenario and a JUnit test **through
those tools**, watch it go red, then make it green. A state machine refuses
to let it refactor on a red bar, and every write lands in a staging area a
human reviews. `requirement_mark_implemented` is GREEN-gated.

Here is the part worth your hour: once that discipline lives in the server
instead of in a prompt, model capability stops being the variable that
decides quality. We run the identical **contracts** twice — once with a
frontier agent in Cursor (all 23 tools), once with
`qwen3.8-flash-next:125b-mlx` on the laptop on stage through Ollama and the
CLI's per-command profiles (3–7 tools) — and compare the diffs. Then we look at
what makes the local model hold up: JSON-only response contracts,
one-finding-at-a-time correction, validate-and-retry that feeds the invalid
reply back, and deterministic templates as the floor when generation fails.

Then comes the segment most talks skip: **Where This Breaks.** Real failures
this project hit — format drift, fixing the wrong file, silently dropping a
criterion, looping on an attempt that already failed — each with the
deterministic check that now catches it, and an honest account of where a
frontier model is still the right call.

The server and CLI were built this way themselves: every tool has a numbered
requirement, a Cucumber scenario, and a test that fails the build when spec and
scenarios drift apart.

To prove the point, the demo runs with the Wi-Fi switched off.

## What attendees leave with

- A pattern for MCP servers that **enforce a workflow** instead of handing
  agents filesystem access.
- The deterministic validation layer that lets a local model produce output you
  would sign your name to.
- A spec format that maps onto Cucumber-JVM and JUnit 5, and the build gate that
  keeps the two from drifting apart.
- A named catalog of local-model failure modes, each with the guardrail that
  catches it.
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
| Tool-calling drift | Local model invented a tool, skipped staging, or called `command_run` without waiting | Per-command profiles (`bdd tools profiles`) offer 3–7 tools; `[tool_rules]` in `cli/prompts/prompts.toml`; CLI `command_run` asks the human to confirm |

Where a frontier model is still the better call, and where a human still has to
be on the review, is stated plainly rather than skipped.

## Who it is for

Java developers, tech leads, and architects who are adopting AI coding agents
and are accountable for what those agents produce — especially in regulated,
air-gapped, or cost-constrained environments. Level: intermediate. No prior MCP
or LLM experience required; comfort with JUnit and Cucumber is assumed.

## Formats

| Format | What it covers |
| --- | --- |
| **Conference session** (50 minutes) | The full narrative above, with the live frontier-versus-local comparison and the *Where This Breaks* segment. |
| **Hands-on workshop** | Attendees run the loop on their own machines against a local model: draft a requirement, refine it until the wording review is clean, take it through RED, GREEN, and REFACTOR. Companion material is the [workshop follow-along](student-follow-along.md); the [CLI path](student-follow-docs/cli-path.md) covers attendees who prefer the terminal to an IDE. |

## Technical requirements

- Stage projector and my laptop.
- **No conference network needed.** The entire demo runs locally against
  Ollama, which is the thesis rather than a convenience.
- Stack on stage: Java 21, Maven, the `bdd` binary (`bdd mcp serve`), Cucumber-JVM 7, JUnit 5,
  Ollama running `qwen3.8-flash-next:125b-mlx`. The bundled `tdd-agent.jar` is an MCP **client**.
- For the workshop format: attendees need **`bdd` on PATH**, Java 21, Maven, git, Cursor (or
  Claude), and — to run fully offline — Ollama with `qwen3.8-flash-next:125b-mlx` pulled ahead of
  time. The [CLI path](student-follow-docs/cli-path.md) is the Wi-Fi-off alternative.

## What is on stage, in this repo

| Shown live | Where it lives |
| --- | --- |
| The 22-tool MCP server enforcing the loop | `cli/src/mcp.rs` (`bdd mcp serve`) |
| The deterministic structure and wording reviews | `cli/src/domain/` (spec validator, requirement refiner) |
| The state machine that refuses a red-bar refactor | `cli/src/domain/tdd.rs` (`TddStateMachine`) |
| The requirements catalog that drives everything | `requirements/requirements.json` |
| Gherkin and JUnit generated from the spec | `kata/` |
| The offline CLI: same server, scoped profiles on `qwen3.8-flash-next:125b-mlx` | [`cli/README.md`](cli/README.md), [`student-follow-docs/cli-path.md`](student-follow-docs/cli-path.md) |
| Every prompt sent to the model, in one auditable file | `cli/prompts/prompts.toml` |
| The slide deck | [`slides/index.html`](slides/index.html) |

## Booking

Interested in this session or the workshop for your conference or team? Open an
issue on
[github.com/davidparry/tdd-bdd-agentic](https://github.com/davidparry/tdd-bdd-agentic/issues)
or reach out through [davidparry.com](https://davidparry.com).

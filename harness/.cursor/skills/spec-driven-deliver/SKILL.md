---
name: spec-driven-deliver
description: Drive a requirement, an ask, a plain-words description, or the whole pending backlog all the way to implemented through the spec-driven-server MCP tools — scenario, step definitions, unit test, RED, implement, GREEN, refactor, mark implemented — and report whether it got there. Use when asked to deliver something using spec-driven development, to deliver a requirement or ask by id, to take a described behaviour to a green bar through spec-driven TDD, or to clear the pending spec backlog.
---

# spec-driven-deliver

Drive the spec-driven delivery pipeline through the
`spec-driven-server` MCP tools. The server owns the state, sequence
constraints, and refusals. You supply the requirement wording, Gherkin,
test assertions, and production code.

The server exposes deterministic primitives only. There is no `deliver`
tool and no `implement` tool, because the client driving the server is
itself the model. Do not look for them.

## Hard rules

These are not preferences. Most are enforced by a tool that will refuse
you; the rest are what make a run readable afterwards.

- **Never hand-edit the requirement catalog.** Every change goes through
  `requirement_create`, `requirement_update`, `requirement_reword`,
  `requirement_decide`, or `requirement_mark_implemented`.
- **Never claim a test passed without `run_tests`.** Tool output is the
  only evidence. Your own reading of the code is not.
- **Never refactor on RED, never mark off GREEN.** `start_refactor` and
  `requirement_mark_implemented` both refuse. Do not attempt either to
  find out.
- **One requirement in flight.** The server holds a single global
  RED/GREEN bar. Work the plan strictly in order, one at a time. Never
  parallelise across requirements and never delegate two at once.
- **`command_run` is RED-only**, argv-only, and limited to
  `cargo mvn npm npx node dotnet java javac tsc`. No shell, no paths in
  the program name, no `..` or absolute paths in arguments, 300s cap.
- **Use only the two planned checkpoints.** Choose the branch before the
  first write. When plain words must be shaped, get approval of the
  resulting requirements before writing tests or production code. After
  that approval, run to the end and report.

## Preflight

Run these before anything else and read the answers:

1. `project_root` — confirm which project you are in.
2. `project_inspect` — discover the language, repository state, test
   conventions, and authoritative write locations. Never assume a path
   or reuse one from another project. See [tool-map.md](tool-map.md).
3. `list_requirements` — the catalog, its id prefix, and what is pending.
4. `validate_spec` — if it is not `valid`, report the issues and refuse
   without changing the catalog.
5. `get_tdd_state` — the phase you are starting from. A bar left RED or
   REFACTOR by an earlier session changes what is safe to do first.

Then refuse, without touching anything, if:

- **No project was detected.** Report it and ask the developer to
  initialize a spec-driven project. Do not scaffold one yourself.
- **The catalog is empty and nothing was described.** Ask for a
  description rather than inventing a backlog.
- **The target is an ask that is not ready.** Report every reason from
  `get_requirement`'s `reasons`, and point at `requirement_decide`.
- **The target is one word holding a digit, or opening with the
  catalog's prefix, and is not a known id.** `req7` is a typo, not a
  description. Refuse and show a real id from `list_requirements`.

Before any mutating tool call or file edit, briefly explain what the
preflight found:

```text
Project: <root and purpose inferred from the inspected assets>
Stack: <language, test framework, and module or package shape>
Spec: <catalog prefix, implemented count, pending count, validity>
TDD: <current phase and whether the bar is safe to begin from>
```

Ground the explanation in `project_inspect`, the requirement catalog,
and the test assets—not in README prose. Keep it brief: this is an
orientation before delivery, not a second project review.

## The branch gate

This is the first checkpoint and happens **before the first write**,
including requirement shaping.

Check `project_inspect`'s `git` field. If the project is under git, ask
for a branch name, offering `spec/<date>-<target>` as the default when
the target has an id and `spec/<date>-delivery` otherwise. Offer "stay
here" as an option.

**You do not create the branch.** Ask the developer to run
`git switch -c <name>` and to tell you when they have, then re-read
`project_inspect` and confirm the branch moved before the first write.
Every effect you have on this project goes through a tool that can
refuse you; a shell is not one, so the one step that needs a shell is
the developer's to take, not yours.

Outside a git repository, say so and carry on.

## Resolve the target into a plan

Four shapes, decided by looking the argument up rather than matching a
pattern you assumed:

- **A known requirement id** — match case-insensitively against
  `list_requirements`, accepting the catalog's own prefix and id format
  rather than assuming either. Plan: that one id.
- **A known ask id (`ASK-nnn`)** — `get_requirement` it. If `ready` is
  false, refuse with its `reasons`. If ready, plan its `derived` ids that
  are still pending. If every derived id is already implemented, mark the
  ask implemented and stop.
- **Plain words** — one or more words that are not an id. Shape them
  into requirements first: follow [shaping.md](shaping.md), then plan the
  accepted ids.
- **Nothing** — plan every pending requirement in catalog order. Asks are
  not planned themselves; their derived requirements are.

State the plan before you start working it:

```text
Plan: 2 requirement(s) - REQ-001, REQ-002.
```

## The loop

For each planned id, in order:

```text
1. scenario            Gherkin scenario tagged @<id>
2. steps               step definitions for every undefined step
3. unit test           a failing test from the acceptance criteria
4. test -> RED         the bar fails honestly
5. implement           production code, then the pending step bodies
6. test -> GREEN
7. refactor            optional; only on GREEN
8. mark implemented    status flipped, then validated
```

Each stage is in [tdd-loop.md](tdd-loop.md) with its exact calls and
parameters. Read it before the first requirement.

Two things govern every stage:

**Verify, do not trust.** After a stage writes, re-read the project and
confirm the gap is actually closed. The four checks are in
[tool-map.md](tool-map.md). A stage whose gap is still open stops that
requirement — do not retry it, because these tools are deterministic and
a second identical call lands in the same place.

**Idempotent, so resumable.** A stage whose work is already in place is
skipped, not redone. A requirement whose scenario you wrote by hand last
week is picked up where it was left:

```text
[1 of 1] REQ-003
A scenario tagged @REQ-003 is already in place.
```

### Carrying on

A requirement that falls short is reported and the run moves to the next
one, so one bad requirement does not strand the backlog. Record it as
outstanding with the reason and the phase it stopped in, then continue.

Stop early only if fail-fast was asked for, and then count the
requirements after it as outstanding — they were never attempted.

### Closing asks

After the loop, any pending ask whose derived requirements are now all
implemented gets `requirement_mark_implemented {id: "ASK-nnn"}`. An ask
that refuses to close is a warning, not a failure: say it and leave the
ask pending.

## Report

Close with this stable machine-readable shape:

```json
{
  "planned": ["REQ-001", "REQ-002"],
  "delivered": ["REQ-002"],
  "outstanding": [
    {
      "id": "REQ-001",
      "reason": "The bar is still RED after 3 attempt(s).",
      "phase": "RED"
    }
  ],
  "completed": false,
  "nextStep": "1 of 2 delivered. Still pending: REQ-001."
}
```

`completed` is true only when every planned id was delivered — a run that
landed one of three is not a success. Omit `outstanding` when it is
empty, and `implementedAsks` unless an ask closed.

Then say in prose what happened, what you wrote, and what the developer
should read. Point at `git diff` for the code: you wrote it, so you are
not the one who should be signing off on it.

## What you own, and what you do not

The gates are about correctness. Not one of them asks whether this was
the right thing to build, because that question has no state machine
behind it. If the wording looks like it will build the wrong thing, say
so before the scenario rather than after the green bar.

## Reference

- [shaping.md](shaping.md) — plain words to approved requirements.
- [tdd-loop.md](tdd-loop.md) — the eight stages, with exact tool calls.
- [tool-map.md](tool-map.md) — stage-to-tool map, the verification
  checks, and the constraints of the MCP surface.

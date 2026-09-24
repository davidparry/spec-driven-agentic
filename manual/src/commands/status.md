# spec status

Where the project stands on the road to every requirement being
implemented — and the one next step that moves it forward. `spec state`
answers "what did the last test run say"; `spec status` answers "where
am I in the whole loop and what do I do now".

```text
Usage: spec status [OPTIONS]
```

```bash
spec status
```

```json
{
  "phase": "RED",
  "staged": [
    { "path": "src/main/java/BddTest.java", "action": "modify", "summary": "implementation attempt for REQ-001 (llm)" }
  ],
  "requirements": [
    {
      "id": "REQ-001",
      "title": "Text Input Calculator",
      "status": "pending",
      "findings": []
    }
  ],
  "nextStep": "1 staged file(s) await review - inspect with spec changes show, apply with spec changes commit, then run spec test."
}
```

## How the next step is chosen

The priority order mirrors the loop itself:

1. **Staged changes wait** — nothing the harness authors touches the
   working tree until you apply it, so an unapplied implementation
   attempt (or scenario, or spec edit) always comes first:
   `spec changes show`, then `spec changes commit`, then `spec test`.
2. **A requirement is in flight** — its scenario, step definitions,
   and unit test all exist. On GREEN the loop closes with the chain
   `spec mark-implemented <id>`, then `spec changes validate`, then
   `spec changes commit`; on any other bar the step is `spec test`,
   and on RED `spec implement <id>` lets the model try.
3. **The earliest asset gap** — a pending requirement is missing its
   tagged scenario (`spec scenario add`), step definitions
   (`spec steps generate`), or unit test
   (`spec unittest generate <id>`); the finding names the command.
4. **Everything is implemented** — draft the next requirement with
   `spec draft`.

Each pending requirement's entry carries its own `findings`, so with
several requirements you see every gap, not just the first.

## Model advice

When a model is resolved (see [`spec model`](model.md)), the
deterministic report is followed by one advice call: the model is
briefed with the whole workflow process — the states, the commands,
the loop, and the invariants — plus the current phase, the last run's
counts, the staging area, and every requirement's position, and it
answers with the next command in plain words:

```text
Model advice: The bar is GREEN and REQ-001 has every asset in place -
close the loop with spec mark-implemented REQ-001, then spec
validate, then spec changes commit.
```

Without a model the report alone is the whole reply, and a model
failure never breaks `spec status`.

## Why a requirement stays pending

`implemented` is never set by a passing run alone. The status flips
only when you run [`spec mark-implemented`](spec.md) — and that
command is GREEN-gated: it refuses unless the last recorded run
passed, and it refuses without a scenario tagged `@<id>` (it records
the tagged feature as the requirement's `featureFile`). The road is
always: staged changes applied → `spec test` GREEN →
`spec mark-implemented <id>` → `spec changes validate` →
`spec changes commit` (the status change is staged too, like every
mutation).

## See also

- [`spec state`](state.md) — the raw TDD state: phase, last run, refactor log.
- [`spec changes`](changes.md) — review and apply what is staged.
- [`spec implement`](implement.md) — the model attempt, with its own preflight.

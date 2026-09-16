# The workflow: spec → RED → GREEN → REFACTOR

The harness enforces a two-altitude test discipline driven by a validated
spec. Understanding the phases makes every command's `nextStep` field
self-explanatory.

## The spec is the entry point

Nothing meaningful happens without `requirements/requirements.json`.
It is a [catalog](spec-format.md): it holds requirements of its own
and may include child spec files (nested as deep as the backlog
needs), and every command works on the merged view of the whole tree.
The iteration loop for the spec itself:

1. Draft or edit a requirement — [`bdd spec draft`](commands/spec.md#bdd-spec-draft)
   or your editor.
2. [`bdd spec validate`](commands/spec.md#bdd-spec-validate) until the
   structure is valid.
3. [`bdd spec refine <id>`](commands/spec.md#bdd-spec-refine) until
   there are no wording findings.
4. A human approves the wording. This is the first human gate.

## The two altitudes

- **BDD altitude** — each requirement becomes a Gherkin scenario
  tagged `@REQ-...` in a feature file, with step definitions binding
  it to real code.
- **TDD altitude** — unit tests
  ([`bdd unittest generate`](commands/unittest.md)) pin down the
  fine-grained behavior beneath the scenario.

## The phase machine

The persistent TDD phase lives in `.bdd-tdd-state.json` under the
project root and survives between invocations and across MCP sessions.

```text
          tests fail                    tests pass
  (start) ──────────► RED ────────────► GREEN ──┐
                       ▲                  │     │ bdd refactor
                       │   tests fail     ▼     ▼
                       └────────────── REFACTOR
                                        (tests pass → GREEN)
```

- [`bdd test`](commands/test.md) runs the suite and moves the phase to
  RED (failures) or GREEN (all passing).
- [`bdd refactor`](commands/refactor.md) is only allowed on GREEN — it
  moves to REFACTOR and records your note in the refactor log.
- [`bdd state`](commands/state.md) shows the phase, the last run's
  counts, and the refactor log at any time.
- [`bdd status`](commands/status.md) zooms out from the phase to the
  spec: where every requirement stands on the road to implemented, and
  the single next step for the one that is furthest along.

## One requirement at a time

The intended rhythm for each pending requirement:

```bash
bdd spec show REQ-002        # locations + workflow hint
bdd scenario add --feature features/calculator.feature \
    --req REQ-002 --name "Two numbers are summed" \
    --step 'Given the input "1,2"' \
    --step 'When add is called' \
    --step 'Then the result is 3'
bdd changes commit           # apply the staged scenario
bdd steps missing            # any undefined steps?
bdd steps generate && bdd changes commit
bdd test                     # RED: the scenario fails honestly
# ...implement the production code...
bdd test                     # GREEN
bdd refactor --note "tidy the parser" && bdd test
bdd status                   # confirm REQ-002 is ready to mark
bdd spec mark-implemented REQ-002   # flips the status, records the featureFile
bdd validate                 # checks the @REQ-002 scenario exists
bdd changes commit
```

[`bdd greenfield`](commands/greenfield.md) automates exactly this
rhythm, pausing only at the two human gates.

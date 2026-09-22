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

1. Draft or edit a requirement — [`spec draft`](commands/spec.md#spec-draft)
   or your editor.
2. [`spec validate`](commands/spec.md#spec-validate) until the
   structure is valid.
3. [`spec refine <id>`](commands/spec.md#spec-refine) until
   there are no wording findings.
4. A human approves the wording. This is the first human gate.

## The two altitudes

- **BDD altitude** — each requirement becomes a Gherkin scenario
  tagged `@REQ-...` in a feature file, with step definitions binding
  it to real code.
- **TDD altitude** — unit tests
  ([`spec unittest generate`](commands/unittest.md)) pin down the
  fine-grained behavior beneath the scenario.

## The phase machine

The persistent TDD phase lives in `.spec/state.json` and survives
between invocations and across MCP sessions. That file, and the rest of
the harness's project files, live in the `.spec/` directory — see
[Where spec keeps its files](getting-started.md#where-spec-keeps-its-files).

```text
          tests fail                    tests pass
  (start) ──────────► RED ────────────► GREEN ──┐
                       ▲                  │     │ spec refactor
                       │   tests fail     ▼     ▼
                       └────────────── REFACTOR
                                        (tests pass → GREEN)
```

- [`spec test`](commands/test.md) runs the suite and moves the phase to
  RED (failures) or GREEN (all passing).
- [`spec refactor`](commands/refactor.md) is only allowed on GREEN. It
  moves to REFACTOR, records your note in the refactor log, and — with a
  model resolved — carries the cleanup out in a loop that never edits a
  test and restores your code if it cannot keep the bar green.
- [`spec state`](commands/state.md) shows the phase, the last run's
  counts, and the refactor log at any time.
- [`spec status`](commands/status.md) zooms out from the phase to the
  spec: where every requirement stands on the road to implemented, and
  the single next step for the one that is furthest along.

## One requirement at a time

The intended rhythm for each pending requirement:

```bash
spec show REQ-002        # locations + workflow hint
spec scenario add --feature features/calculator.feature \
    --req REQ-002 --name "Two numbers are summed" \
    --step 'Given the input "1,2"' \
    --step 'When add is called' \
    --step 'Then the result is 3'
spec changes commit           # apply the staged scenario
spec steps missing            # any undefined steps?
spec steps generate && spec changes commit
spec test                     # RED: the scenario fails honestly
# ...implement the production code...
spec test                     # GREEN
spec refactor --note "tidy the parser" && spec test
spec status                   # confirm REQ-002 is ready to mark
spec mark-implemented REQ-002   # flips the status, records the featureFile
spec changes validate                 # checks the @REQ-002 scenario exists
spec changes commit
```

[`spec greenfield`](commands/greenfield.md) automates exactly this
rhythm, pausing only at the two human gates.

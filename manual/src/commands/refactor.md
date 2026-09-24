# spec refactor

Clean up the production code without changing what it does. Only allowed
on GREEN — the discipline's core rule is that you never restructure code
while tests are failing.

With a model resolved, this carries out the refactor: it reads the code,
rewrites it, runs the suite, and keeps the result only if the bar is
exactly where it was. Without one — or with `--manual` — it marks the
    10|phase and the cleanup stays in your hands.

```text
Usage: spec refactor [OPTIONS]
```

MCP tool equivalent: `start_refactor` (the phase marker only).

## Flags

    20|| Flag | Description |
| --- | --- |
| `--note <NOTE>` | What you intend to refactor and why. Recorded in the refactor log, and with a model resolved it is also the brief the model is given. |
| `--req <REQ-ID>` | The requirement whose behaviour must not change. Its story and acceptance criteria are shown to the model as the specification it is preserving. |
| `--manual` | Mark the phase and stop, without a model call. |
| `--root <ROOT>` | Project root. Defaults to `.`. |
| `--model <MODEL>` | The model to use for the loop. |

## The tests are never touched

    30|A refactor is only a refactor if the thing judging it did not move, so
the suite has to be the same suite afterwards. That is enforced rather
than requested, at four levels:

- The prompt shows the tests, step definitions and feature files as
  read-only context and mandates that they are never rewritten.
- The only paths the model is allowed to name are the production files,
  computed from the project layout — not from what the model claims.
- A reply naming a test path is rejected **in full** and the round is
  asked for again. Nothing is salvaged from it: a refactor the model
    40|  believed came with a test change is not one you want half of.
- After every round the test files are byte-compared with what the loop
  started from. Anything that moved restores the code and abandons the
  run.

Green also means *the same number of tests*. A run that passes at a
different count is treated as a failure, because green stopped meaning
what it meant at the baseline.

## The loop
    50|
Each round is one model call and one full test run:

1. Measure the bar. Not green? The run is refused — a refactor has
   nothing to preserve until it is.
2. Ask for a rewrite of the production files, given the goal, the
   requirement, the read-only tests, the declared dependencies, and what
   every earlier round of this run broke.
3. Apply it and run the suite.
4. Green at the same test count? Keep it and stop. Otherwise brief the
    60|   next round with the failures it caused.

If the budget runs out, **every file it touched is restored to the byte**
from a snapshot taken before the first round. The restore is the harness's
own, so it does not depend on your git state being clean.

The budget is `[refactor] attempts` in `.spec/config.toml`, ten by default:

```toml
[refactor]
attempts = 10
    70|```

A model that answers the same thing twice ends the run early rather than
spending the rest of the budget confirming it.

## What the model is shown

Assembled deterministically, so the same project always produces the same
brief:

| Context | Where it comes from |
    80|| --- | --- |
| The goal | Your `--note` |
| The behaviour to preserve | `--req`'s story and acceptance criteria |
| Writable code | The production file, plus every production file that names its type |
| Read-only behaviour | The unit tests, step definitions and feature files |
| Available libraries | The dependency coordinates declared in `pom.xml`, `build.gradle`, `package.json` or `Cargo.toml` |
| The bar to beat | The test count measured before the first round |

The reference walk is whole-word, so refactoring `Calc` does not drag in
`Calculator`, and an unconnected file is not padding in the prompt.

    90|## Examples

On GREEN, with a model resolved:

```bash
spec refactor --note "extract the single-number parse into a private method" --req REQ-002
```

```json
{
   100|  "phase": "REFACTOR",
  "goal": "extract the single-number parse into a private method",
  "rounds": 1,
  "attempts": 10,
  "targets": ["kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java"],
  "tests": 5,
  "applied": true,
  "reverted": false,
  "source": "llm",
  "nextStep": "The refactor is in your working tree and the bar is where it was. Read it with git diff, then run spec test to record the run."
   110|}
```

A refactor that cannot be had without breaking a test is declined rather
than forced:

```json
{
  "phase": "REFACTOR",
  "rounds": 1,
  "applied": false,
   120|  "reverted": false,
  "warning": "The model found nothing worth refactoring and left the code as it is."
}
```

Marking the phase and doing the work yourself:

```bash
spec refactor --note "collapse duplicate parsing" --manual
```

   130|```json
{
  "phase": "REFACTOR",
  "nextStep": "A refactor is in progress. Run spec test to prove the refactor kept the bar green."
}
```

Attempting it off GREEN is refused with exit status 1:

```text
   140|Error: Refactoring is only allowed from GREEN (current phase: RED). Make the tests pass first.
```

So is starting with work already staged, since the loop applies each
round in order to test it:

```text
Error: 1 change(s) are already staged, and the refactor loop applies each round to run the tests - review them with spec changes show, then spec changes commit or spec changes discard before refactoring.
```

   150|## It writes to your working tree

Every other authoring command stages its work for you to read before it
lands. This one cannot: the only thing that can tell a refactor from a
rewrite is the test suite, and the suite runs against files on disk.

So a successful refactor is already applied when the command returns.
Read it with `git diff`, and run `spec test` to record the run in the
phase log.

   160|## The refactor loop by hand

```bash
spec test                                  # GREEN - safe to restructure
spec refactor --note "collapse duplicate parsing" --manual
# ...restructure, behaviour unchanged...
spec test                                  # GREEN again: refactor complete
```

If that final `spec test` fails, the phase drops to RED: the refactor
changed behaviour, and the failing tests tell you exactly where.
   170|
## Why the note matters

Each `--note` is appended to the `refactorLog` that
[`spec state`](state.md) reports. Over a kata or a workshop, the log
becomes the narrative of deliberate design decisions — which is the
half of TDD that "make it pass" alone never captures. With a model
resolved it does double duty as the brief, so a note worth logging is
also a note worth acting on.

   180|## See also

- [`spec state`](state.md) — the phase and the accumulated log.
- [`spec config`](config.md) — where `refactor.attempts` is reported.
- [The workflow](../workflow.md) — where REFACTOR sits in the machine.

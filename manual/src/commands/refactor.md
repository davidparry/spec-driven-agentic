# spec refactor

Begin a refactor step. Only allowed on GREEN — the discipline's core
rule is that you never restructure code while tests are failing.

```text
Usage: spec refactor [OPTIONS]
```

MCP tool equivalent: `start_refactor`.

## Flags

| Flag | Description |
| --- | --- |
| `--note <NOTE>` | What you intend to refactor and why. Recorded in the refactor log. |
| `--root <ROOT>` | Project root. Defaults to `.`. |
| `--model <MODEL>` | Accepted (global flag) but unused. |

## Examples

On GREEN:

```bash
spec refactor --note "extract the delimiter parser from add()"
```

```json
{
  "phase": "REFACTOR",
  "nextStep": "Refactor with the tests as your safety net, then 'spec test'. Passing returns you to GREEN; a failure means the refactor broke behavior."
}
```

Attempting it on RED is refused with exit status 1:

```text
Error: refactoring is only allowed on GREEN - you are RED. Make the tests pass first.
```

## The refactor loop

```bash
spec test                                  # GREEN - safe to restructure
spec refactor --note "collapse duplicate parsing"
# ...restructure, behavior unchanged...
spec test                                  # GREEN again: refactor complete
```

If that final `spec test` fails, the phase drops to RED: the refactor
changed behavior, and the failing tests tell you exactly where.

## Why the note matters

Each `--note` is appended to the `refactorLog` that
[`spec state`](state.md) reports. Over a kata or a workshop, the log
becomes the narrative of deliberate design decisions — which is the
half of TDD that "make it pass" alone never captures.

## See also

- [`spec state`](state.md) — the phase and the accumulated log.
- [The workflow](../workflow.md) — where REFACTOR sits in the machine.

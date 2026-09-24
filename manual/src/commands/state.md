# spec state

Show the current TDD phase, the last run's counts, the refactor
log, and at most the three latest dated state entries. Read-only —
it never changes anything.

```text
Usage: spec state [OPTIONS]
```

MCP tool equivalent: `get_tdd_state`.

## Flags

Only the [global flags](../global-flags.md) (`--root`, `--model`).

## Examples

```bash
spec state
```

```json
{
  "instructions": "This file is the TDD phase log. ... When briefing a model, include only the three most recent entries. ...",
  "phase": "GREEN",
  "lastRun": {
    "tests": 3,
    "failures": 0,
    "errors": 0,
    "skipped": 0
  },
  "refactorLog": [
    "extract the delimiter parser from add()"
  ],
  "entries": [
    {
      "timestamp": "2026-08-13T21:01:00Z",
      "phase": "RED",
      "lastRun": { "tests": 3, "failures": 1, "errors": 0, "skipped": 0 },
      "refactorLog": [],
      "attemptLog": []
    },
    {
      "timestamp": "2026-08-13T21:02:00Z",
      "phase": "GREEN",
      "lastRun": { "tests": 3, "failures": 0, "errors": 0, "skipped": 0 },
      "refactorLog": [
        "extract the delimiter parser from add()"
      ],
      "attemptLog": []
    }
  ],
  "nextStep": "You are GREEN. Refactor with 'spec refactor', or mark the requirement implemented."
}
```

Before any test has ever run, the phase is the starting state and
`lastRun` is all zeros; `nextStep` points you at `spec test`.

## Where the state lives

`.spec/state.json`, inside the project's `.spec/` directory. It is a chronological log of
timestamped entries — one per test run, refactor, or implementation
attempt — plus `instructions` that explain how to read the schema. The
file keeps the full history so a human can audit the loop; `spec state`
and every model brief include **only the three latest entries**. Delete
the file to reset the phase machine (there is deliberately no `reset`
command — losing the log should be an explicit filesystem act).

The same file also carries `attemptLog` — the record of every model
implementation attempt that [`spec implement`](implement.md) and the
greenfield loop brief the next attempt with: the files it wrote
(`targets`), the failures it was briefed with (`failures`), and the
output of the first test run after it (`outcome`; empty means no run
ever verified it). A file written before this field existed loads with
an empty `outcome` — the attempt is treated as never verified. A GREEN
run clears it: a closed loop leaves no history for the next
requirement to inherit.

## Reading the reply

- `instructions` — how to interpret the log (the same text stored in
  the file).
- `phase` — `RED`, `GREEN`, or `REFACTOR` (see
  [the workflow](../workflow.md)). The current phase; also the last
  entry's `phase`.
- `lastRun` — counts only; the failure details live in the
  [`spec test`](test.md) reply that produced them.
- `refactorLog` — every note passed to
  [`spec refactor --note`](refactor.md), in order. It is the audit
  trail of intentional design work.
- `entries` — at most the three latest dated snapshots (`timestamp`,
  `phase`, `lastRun`, `refactorLog`, `attemptLog`). Older entries stay
  on disk and are not sent to a model.

## See also

- [`spec test`](test.md) — the command that moves the phase.
- [`spec refactor`](refactor.md) — appends to the log shown here.

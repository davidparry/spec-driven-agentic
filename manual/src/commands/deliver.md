# spec deliver

Drive a requirement, a plain-words description, or the whole pending
backlog all the way to implemented — and say at the end whether it got
there.

```text
Usage: spec deliver [OPTIONS] [TARGET]...
```

## Flags

| Flag | Description |
| --- | --- |
| `--root <ROOT>` | Project root. Defaults to `.`. |
| `--model <MODEL>` | LLM model for the generation steps, this run only. |
| `--retry <N>` | Max attempts when a model reply fails validation. Default 3. |
| `--attempts <N>` | RED-to-GREEN rounds per requirement. Default 3. |
| `--fail-fast` | Stop at the first requirement that falls short instead of carrying on. |
| `--no-refactor` | Skip the refactor step even on a green bar. |
| `--file <FILE>` | Draft new requirements into this included spec file instead of the root catalog. |

## What the argument means

The argument decides where the run starts. There are three entries, and
the command tells them apart by looking at what you typed:

| You type | It means |
| --- | --- |
| `spec deliver REQ-003` | That one requirement, already in the catalog. |
| `spec deliver "empty input means zero"` | Plain words: split into requirements first, then deliver each. |
| `spec deliver` | Every pending requirement, in catalog order. |

An argument shaped like `REQ-` followed by digits is a requirement id
(case-insensitively, so `req-3` works). Several words are a description;
the words do not need quoting, since everything after the flags is joined
into one.

A single word that was reaching for an id and missed is refused rather
than drafted, because handing `R-003` to the model as prose invents a
requirement nobody asked for:

```text
$ spec deliver R-003
Error: R-003 is not a requirement id, and it is one word rather than a
requirement to break down, so nothing here can be delivered. Ids look
like REQ-003 - run spec list to see them. To describe new work instead,
use plain words: spec deliver "a custom delimiter on the first line".
```

A description needs a model to break it down. Without one the run hands
the work back rather than walking you through wording it — that is what
[`spec draft`](spec.md) and [`spec greenfield`](greenfield.md) are for.

## What one requirement goes through

```text
1. scenario            Gherkin scenario tagged @REQ-...
2. steps               step definitions for every undefined step
3. test → RED          the scenario fails honestly
4. implement           the model attempts, up to --attempts times
5. test → GREEN
6. refactor            optional; only on GREEN, and only with a model
7. mark implemented    status flipped, validated, committed, read back
```

Every step is **verified** rather than trusted. After the command that
writes an asset returns, the run re-reads the same
[asset survey](status.md) `spec status` reports and asks whether the gap
is actually closed. A step whose gap is still open is reported with the
command that shows you what really landed — it is not retried, because
these commands are deterministic and a second identical run would land
in the same place.

That verification also makes each step idempotent, so a requirement
whose scenario you wrote by hand is picked up where you left it:

```text
[1 of 1] REQ-003
A scenario tagged @REQ-003 is already in place.
The unit test for REQ-003 is already in place.
Running the tests - working ...
```

A step that loops on its own is called once and its loop is trusted:
drafting's validate-and-reword rounds, `spec implement`'s attempts,
and `spec refactor`'s rounds are not wrapped in a second loop here.

## It never stops to ask

A run either completes or tells you why it could not. It does neither
halfway through a question, because an orchestrator that waits at a
prompt is not running.

So every proposal is accepted and every gate approved. That is not a
separate unattended dialect: every wizard prompt in this harness already
treats <kbd>Enter</kbd> as "accept what is in front of you", and this is
that path. Each one is echoed, so the transcript shows what was decided
for you and which wording landed:

```text
REQ-001 title [Empty string returns zero] (Enter keeps it): kept (spec deliver never stops to ask)
The wording reads clean. Stage this requirement? yes (spec deliver never stops to ask)
```

The generated unit test is still printed before it runs — the assertions
are the one thing here you will want to sharpen afterwards — but it is
not put behind a gate, since a run that cannot be answered would only
ever approve it.

Anything that genuinely needs an answer no default can supply is refused
up front, with the reason and the command that settles it:

| The run cannot | It says |
| --- | --- |
| Scaffold a project (no build markers) | Run [`spec init --language`](init.md), or [`spec greenfield`](greenfield.md) to be walked through it |
| Work from an empty catalog with nothing described | Describe it: `spec deliver "<what to build>"`, or [`spec greenfield`](greenfield.md) |
| Break a description down with no model | Point one at the project with [`spec model use`](model.md), or word it with [`spec draft`](spec.md) |
| Guess what `R-003` meant | Ids look like `REQ-003` — [`spec list`](spec.md) shows them |

Each of these exits nonzero without touching the spec, so nothing is
half-written when you come back to it.

If you want to be walked through the decisions instead, that is what
[`spec greenfield`](greenfield.md) is: the same loop with the questions
left in.

## Carrying on, or not

By default a requirement that falls short is reported and the run moves
to the next one, so one bad requirement does not strand the rest of the
backlog:

```text
Plan: 2 requirement(s) - REQ-001, REQ-002.
[1 of 2] REQ-001
...
REQ-001 stopped: The generated tests were declined and the staging area is clear. Author them by hand, then run spec deliver again.
[2 of 2] REQ-002
```

`--fail-fast` stops instead, and the requirements after it are counted
as outstanding because they were never attempted:

```text
Stopping here - the rest of the plan is untouched (--fail-fast).
```

## Reply

```json
{
  "planned": ["REQ-001", "REQ-002"],
  "delivered": ["REQ-002"],
  "outstanding": [
    {
      "id": "REQ-001",
      "reason": "The bar is still RED after 3 attempt(s). The failures and every attempt are recorded - read them with spec state, then run spec deliver REQ-001 again for another 3, or implement by hand.",
      "phase": "RED"
    }
  ],
  "completed": false,
  "nextStep": "1 of 2 delivered. Still pending: REQ-001. Read why with spec status, then run spec deliver REQ-001 again."
}
```

`completed` is true only when every planned requirement was delivered —
a run that landed one of three is not a success, and a plan cut short
by `--fail-fast` counts the ids it never reached. The `outstanding` key
is absent from a clean reply.

A run that did not finish its plan **exits 1**, which is what makes the
command usable as a gate:

```bash
spec deliver --attempts 5 || echo "the backlog is not clear yet"
```

On a real terminal the session stays open at the `spec>` prompt
afterwards, like [`spec greenfield`](greenfield.md) — the run has
finished by then, so handing back a prompt is a convenience rather than a
contradiction. Piped or in CI there is no terminal, so the process ends.

## spec deliver or spec greenfield?

They share the loop; they differ in where they start and what they
promise at the end.

Use [`spec greenfield`](greenfield.md) to *work* — it starts at the
drafting wizard, walks each field through your hands, offers the
implementation attempt at a prompt you control, and hands the refactor
to your editor.

Use `spec deliver` to *finish* something — it starts from a plan (an id,
a description, or the backlog), never stops to ask, uses the
model-driven refactor because there is nobody to hand off to, and reports
a verdict you can act on in a script. Anything it cannot decide alone it
refuses up front rather than pausing for you.

## See also

- [The workflow](../workflow.md) — the same rhythm, step by step.
- [`spec status`](status.md) — the asset survey each step is verified against.
- [`spec state`](state.md) — the recorded failures and attempts behind a RED stop.
- [`spec model`](model.md) — pick which model powers generation.

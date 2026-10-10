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
| `--root <ROOT>` | Project root. Defaults to the nearest enclosing project, [searching upward](../global-flags.md#discovery-what-happens-when-you-say-nothing) from the working directory. |
| `--model <MODEL>` | LLM model for the generation steps, this run only. |
| `--retry <N>` | Max attempts when a model reply fails validation. Default 3. |
| `--attempts <N>` | RED-to-GREEN rounds per requirement. Default 3. |
| `--fail-fast` | Stop at the first requirement that falls short instead of carrying on. |
| `--no-refactor` | Skip the refactor step even on a green bar. |
| `--file <FILE>` | Draft new requirements into this included spec file. Omitted, the drafted requirement lands in the catalog document covering the working directory — see [`spec draft`](spec.md#spec-draft). |
| `--no-branch` | Skip [the branch gate](../branch-gate.md) entirely: git is not consulted and no branch is created. |
| `--judge-draft` | Put each drafted criterion to the [decision model](judge.md) and give the drafting model one round to reword what it cannot read as an assertion. Off by default — see [below](#judge-draft-the-decision-model-during-drafting). |

Requirements drafted from a description are numbered the way
[`spec draft`](spec.md#how-the-id-is-chosen) numbers anything: the
prefix is read off the catalog being drafted into, so delivering a
description against a `HARNESS-` catalog plans `HARNESS-` ids and never
`REQ-`.

## What the argument means

The argument decides where the run starts. There are three entries, and
the command tells them apart by looking at what you typed:

| You type | It means |
| --- | --- |
| `spec deliver REQ-003` | That one requirement, already in the catalog. |
| `spec deliver "empty input means zero"` | Plain words: split into requirements first, then deliver each. |
| `spec deliver` | Every pending requirement, in catalog order. |

An argument that **names a requirement in the catalog** is that
requirement, matched case-insensitively — so `req-3` and `harness-018`
both work, and the prefix can be whatever the catalog uses. There is no
hard-coded `REQ`: the command looks the argument up rather than matching
a shape it was built with. An argument that merely *looks* like an id —
an uppercase prefix, a dash, digits — is also taken as one, so a typo in
the number is reported by name:

```text
$ spec deliver REQ-999
Error: No requirement with id REQ-999. Run spec list to see valid ids, or
describe a new one with spec deliver "<what to build>".
```

Several words are a description; the words do not need quoting, since
everything after the flags is joined into one.

A single word that was reaching for an id and missed is refused rather
than drafted, because handing `req7` to the model as prose invents a
requirement nobody asked for. A lone word counts as a miss when it holds
a digit, or opens with a prefix the catalog actually uses:

```text
$ spec deliver req7
Error: req7 is not a requirement id, and it is one word rather than a
requirement to break down, so nothing here can be delivered. Ids look
like REQ-003 - run spec list to see them. To describe new work instead,
use plain words: spec deliver "a custom delimiter on the first line".
```

The example id in that message is taken from the catalog too, so a
project numbering `HARNESS` is told `Ids look like HARNESS-001`.

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

## What the decision model is asked along the way

With a [decision model](judge.md) configured, every generating stage
puts the writing model's reply to the same bounded question the
hand-run command would. The gate and the stage that asks it:

| Stage | Gate | The question |
| --- | --- | --- |
| `author_steps` | `STEPS_BIND_SCENARIO` | would this step expression match that step line? |
| `drive_to_green` | `UNIT_TEST_ASSERTS` | does this test body assert the criterion, or pass regardless? |
| `drive_to_green` | `IMPLEMENTATION_COMPLETE` | how completely does this code implement its criterion - stub, partial, complete? |
| `refactor` | `REFACTOR_PRESERVES_BEHAVIOUR` | did this refactoring leave behaviour alone? |

`author_scenarios` is deterministic and asks nothing;
`SCENARIO_EXERCISES_CRITERION` is the hand-run `spec scenario
generate`'s gate. All four ship at `advisory`, where a verdict is
recorded and changes nothing, and that ceiling holds whatever `[decision]
mode` says until a project raises a gate by name:

```toml
[decision.gates.UNIT_TEST_ASSERTS]
mode = "enforce"
```

Under `enforce` a `FAILS` becomes a `REWORK`: the writing model is
re-asked with the finding as the complaint, riding the same retry a
malformed reply gets, and nothing else changes. A judgment can never
approve anything - a reply that holds every gate still has to compile
and go green. Each judgment is printed as a `second review` block as the
stage finishes and rides in the final JSON under `judgments`, one entry
per requirement per stage, so a run nobody watched can be read back.
With no decision model configured none of this happens and the report
carries no `judgments` key. A gate that has a judge and nothing to put
to it - a reply whose expressions do not pair with the missing steps -
says so in `.spec/log` and records nothing.

## `--judge-draft`: the decision model during drafting

Delivering a description rather than an id means the spec is written by
a model with nobody reading it. `--judge-draft` puts the
decision model in that loop too — the one place in the harness its
answer is used as **feedback rather than a gate**.

With the flag, a proposal that clears the deterministic edge-case rule
has each of its criteria put to the question `measurable/v2`. Anything
the model cannot read as an assertion comes back as a rejection reason,
naming every flagged criterion, and the drafting model gets one round
to reword them. Then the draft is kept, whatever the second answer was.

**It is off by default, and the reason is measured.** A drafting model
writes criteria like `then the roll-up verdict is "covered"`, which is
the shape this question reads worst. Put one real six-criterion draft
through it and four come back flagged — so the usual outcome is a
redraft the question was wrong to ask for. A redraft also rewrites the
criteria every later stage is prompted from, which costs an unattended
run its reproducibility and a cached rehearsal its cache.

Three properties bound what it can cost you when you do turn it on:

- **It never blocks.** One rejection, one redraft, draft kept. There is
  no path where a judgment stops the run.
- **It never spends the last attempt.** A question the loop has no
  round left to act on is not asked.
- **An unreachable model is not an objection.** The first failed
  request stops the asking and the draft proceeds.

It is feedback and not a gate for the same measured reason: the
question reads 14 of this repository's own 73 criteria as unmeasurable
when they plainly are not. A gate wired to that would block a fifth of
an unattended pipeline. A rejection reason wired to it costs one round
— and a wrong objection still tends to improve the wording, since a
model asked to make `then the verdict is "covered"` assertable writes
`then the reply is an error naming "covered"`, which scores 0.912 and
reads better.

`--decision-model` picks the model and `[decision] mode = "off"`
refuses to ask whatever the flag says. `advisory` and `enforce` behave
identically here, because nothing is being enforced.

## It stops once, before it writes anything

A run that delivers a backlog writes a lot of files. So before the first
one, it asks a single question: a branch name for this run, Enter for a
generated one, or `n` to stay where you are. That is
[the branch gate](../branch-gate.md), and `--no-branch` removes it. A
run outside a git repository, or one whose stdin has nothing left to
read, takes the no-branch path and says so.

## After that it never stops to ask

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
The wording reads clean. Write this requirement? yes (spec deliver never stops to ask)
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
| Guess what `req7` meant | Ids look like `REQ-003` — [`spec list`](spec.md) shows them |

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
REQ-001 stopped: The generated tests were declined. Author them by hand, then run spec deliver again.
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

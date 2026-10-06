# spec judge

Decision model selection, and bounded judgments you can inspect.

A **decision model** is a second local model with a different job from
the one [`spec model`](model.md) configures. The generative model writes
prose and code. A decision model does not write at all: it is asked one
bounded question about evidence you supply and answers with a typed
value and a probability. Ollama serves these at `/v1/systemone` from
version 0.35.

The harness uses it for exactly one question today: **can this
acceptance criterion be checked by a test with a single unambiguous
result?** That is a gap the deterministic
[requirement refiner](spec.md#spec-refine) cannot close.

The refiner's rules ask whether the outcome clause *looks* concrete —
does it contain a number, a quoted literal, a named error type, a known
sentinel? That is satisfied by a number appearing anywhere in the
clause, so

> Given the refactored module, when the suite runs, then code quality is
> improved by at least 20%

earns no finding at all, while being unmeasurable: nobody measured code
quality. The rule asks whether a number is *present*. The judgment asks
whether the number *is* the assertion. Against `nimble:latest` that
criterion comes back at p = 0.038 — not measurable.

```text
Usage: spec judge [OPTIONS] <COMMAND>

Commands: models, current, use, criterion
```

## On wherever a decision model is installed

Pull a decision model and judgments work. There is nothing else to
configure: with `[decision] model` unset, Ollama is asked which of its
models can answer, and the first is used for the session.

Nothing is *assumed*, though. No model name is compiled in, none is
pulled for you, and the borrowed choice is never written to your
configuration — `spec judge use` is still the only thing that makes one
stick. On a machine with no decision-capable model, every command
behaves exactly as it did before this feature existed.

Two ways to turn it off: `mode = "off"` stops the automatic judgments
while leaving `spec judge` answering, and a machine with no decision
model pulled asks nothing because there is nothing to ask.

The [interactive shell](../interactive-shell.md#the-decision-line) says
which state you are in before its first prompt, on a line beside the
inference model's:

```text
Inference model set: qwen3.8-flash-next:125b-mlx (from configuration).
Decision model set for this session: nimble (not saved - keep it with: spec judge use nimble).
```

Resolving it at startup is what turns a model named in `[decision]` but
never pulled into something you find out about at the prompt rather than
at the first judgment. A session with nothing to ask still returns every
deterministic answer it always did.

## What a judgment is allowed to do

A judgment is **a gate on wording, and nothing else**. It can refuse
work; it can never approve any. Specifically, a decision model in this
harness can never:

- turn a red test bar green, or change a test result in any way
- mark a requirement implemented, or certify any implementation
- bypass the [branch gate](../branch-gate.md) or a human confirmation
- waive a human checkpoint
- make a wording review `clean` that the deterministic rules did not
- edit or drop a deterministic finding

Those are all decided by deterministic code or by you.

What it *can* do, in the default `mode = "enforce"`, is stop a
requirement moving on. A verdict against a criterion appends its line to
`findings`, makes `clean` false, and exits nonzero — the same three
signals a deterministic finding produces, so the loop you already run
iterates on it without being taught anything new. The lines are prefixed
`judgment (measurable/v2):` so you can always tell which rules found
what, and the deterministic findings keep their place above them.

That is the default because of what the question is for. It is aimed at
wording the regex rules cannot reach — "then code quality is improved by
at least 20%" earns no deterministic finding at all, because a number is
present. If the judgment cannot refuse, nothing enforces that gap. Set
`mode = "advisory"` to report without gating while you measure the
question against your own criteria; see
[How well does it work?](#how-well-does-it-work) for what that costs.

A request that failed is not an answer and is never read as approval. An
unreachable model in advisory mode leaves a note saying no judgment was
taken; in enforcing mode it is an error.

---

## spec judge models

List the installed models that can answer decisions.

```bash
spec judge models
```

```text
nimble:latest
```

The list comes from asking Ollama which of its models report the
`decision` capability. No model name is hardcoded, and nothing is
inferred from a name: a model is offered for this role only because the
provider says it can do the job.

With none installed, the command is an error rather than an empty list,
and gives the two commands that fix it:

```text
No decision model is installed - install one to continue:
    ollama pull nimble
    spec judge use nimble
```

---

## spec judge current

Show the resolved decision model, where it came from, and the policy its
answers are read against.

```bash
spec judge current
```

```text
nimble:latest (from configuration)
endpoint	http://localhost:11434
timeout_seconds	60
mode	enforce
min_confidence	0.8
```

The model shown is the one that will actually answer, so with nothing
configured it is the one discovery borrowed, and the source says so
rather than claiming the file named it:

```text
nimble:latest (from the only installed decision model)
```

With nothing configured and nothing installed that can decide:

```text
No decision model is installed - install one to turn judgments on:
    ollama pull nimble
    spec judge use nimble
```

`--json` prints the same values as a stable object, including the
question version:

```bash
spec judge current --json
```

```json
{
  "endpoint": "http://localhost:11434",
  "minConfidence": 0.8,
  "mode": "enforce",
  "model": "nimble:latest",
  "question": "measurable/v2",
  "source": "configuration",
  "timeoutSeconds": 60
}
```

`mode` reads back in lower case because that is what you write in
`.spec/config.toml`. `verdict` and `action` are upper case because they
name states in the workflow rather than settings.

[`spec config`](config.md) reports the same five keys alongside every
other setting, with the source of each.

---

## spec judge use

Persist the decision model choice.

```bash
spec judge use nimble:latest
```

```text
Configured decision model: nimble:latest
Written to: /Users/you/code/calculator/.spec/config.toml ([decision] model)
The generative model is unchanged - see llm.model in spec config.
```

This writes `[decision] model` and touches nothing else. It is a
separate key from `llm.model` on purpose: the two roles are two models,
and configuring one must never replace the other. The name is checked
against the models that report the `decision` capability, so a chat
model is refused here rather than failing later.

The reverse is also refused. `spec model use nimble:latest` reports:

```text
'nimble:latest' is a decision model and cannot do generative work - set it as the decision model instead: spec judge use nimble:latest
```

Model discovery has the same separation. With only a decision model
installed, the session default does not silently become it:

```text
Ollama has nimble:latest installed, which answers decisions rather than chat - generation will use deterministic templates. Pull a coding model too, e.g.: ollama pull qwen3.8-flash-next:125b-mlx (the decision model stays available to spec judge)
```

---

## spec judge criterion

Run a real judgment and show the whole typed result. This is the command
to use to check that the setup works and to see what a judgment actually
is.

```text
Usage: spec judge criterion [OPTIONS] [REQ_ID]

Arguments: [REQ_ID]   a requirement whose criteria are judged
Options:   --text <TEXT>   judge this wording instead
           --json
```

Before it asks anything, this command checks that a model which can
answer is both installed and chosen, and stops with the command to run
if not. The check is on this command only. `spec refine` still judges
nothing unless a model is configured, and says so in a note rather than
failing — a wording review that worked before you had a decision model
goes on working.

```text
No decision model configured - choose one to continue:
    spec judge use nimble:latest
installed: nimble:latest
```

Judge a piece of wording directly:

```bash
spec judge criterion --text "Given the refactored module, when the suite runs, then code quality is improved by at least 20%"
```

```text
model	nimble:latest
question	measurable/v2
mode	enforce
min_confidence	0.8

criterion	Given the refactored module, when the suite runs, then code quality is improved by at least 20%
answer	probability of true 0.038
verdict	FAILS
action	REWORK
input	--text
state	sha256:048965344f62409e5399403357ce83c4e7b3cce836b14673c0745b6f7ff7237d
tokens	in 385 out 1

judgment (measurable/v2): criterion "Given the refactored module, when the suite runs, then code quality is improved by at least 20%": the outcome may not be measurable - nimble:latest says probability of true 0.038

A judgment gates on wording only. It becomes a finding and exits nonzero, the same as a deterministic one, and it does not change the test bar or whether a requirement is implemented.
```

This command exits nonzero on any action other than `CONTINUE`, so it
works as a gate in a script on the same terms as `spec refine`. Under
`mode = "off"` the action is always `CONTINUE` and it always exits
zero — a judgment you typed still answers, it just does not gate.

Or every criterion of a requirement:

```bash
spec judge criterion REQ-003
```

`--json` gives the machine-readable form, which is the same shape the
[`refine_requirement` MCP tool](mcp.md) attaches:

```json
{
  "judgments": [
    {
      "gate": "CRITERION_MEASURABLE",
      "question": "measurable/v2",
      "model": "nimble:latest",
      "verdict": "HOLDS",
      "action": "CONTINUE",
      "mode": "enforce",
      "threshold": 0.8,
      "answer": { "type": "noul", "noul": 0.99804933586542 },
      "provenance": {
        "input": "REQ-003",
        "state": "sha256:0506f1b9d220bf6b7a2f2192534480abad3d25e297a324ff954a0ea9fdda6994",
        "state_bytes": 92
      },
      "usage": { "input_tokens": 382, "output_tokens": 1 }
    }
  ],
  "action": "CONTINUE",
  "advisories": []
}
```

### Reading the record

Every judgment records enough to be argued with later:

| Field | What it is |
| --- | --- |
| `gate` | Which question was being decided |
| `question` | The question's version — `measurable/v2`. A threshold calibrated against one phrasing is not evidence about another, so a change to the wording is a new version |
| `model` | The model that answered |
| `answer` | The typed answer exactly as returned |
| `verdict` | `HOLDS`, `FAILS`, or `INCONCLUSIVE` after the threshold is applied |
| `action` | What the harness did: `CONTINUE`, `REWORK`, `ESCALATE`, or `STOP`. The model never picks this |
| `mode` | The policy in force |
| `provenance.input` | What was summarized into the question |
| `provenance.state` | A SHA-256 of the exact brief sent, so a judgment can be tied to the evidence it saw |
| `usage` | Tokens in and out |

The brief is deliberately minimal: the criterion and nothing else. The
question is about that sentence's own wording, so the story, the
project, and the rest of the spec are not evidence for it. The state
field is data, never instructions.

---

## Judgments on a wording review

With a decision model configured, [`spec refine`](spec.md#spec-refine)
and the `refine_requirement` MCP tool attach judgments to their reply.
In the default `enforce` mode a verdict against a criterion lands in
`findings` and `clean`:

```json
{
  "id": "REQ-007",
  "clean": false,
  "findings": [
    "judgment (measurable/v2): criterion \"...\": the outcome may not be measurable - nimble:latest says probability of true 0.014"
  ],
  "nextStep": "Call requirement_reword to address each finding ... Iterate until there are no findings.",
  "judgments": [ { "...": "as above" } ],
  "judgmentAdvisories": [
    "judgment (measurable/v2): criterion \"...\": the outcome may not be measurable - nimble:latest says probability of true 0.014"
  ],
  "judgmentAction": "REWORK"
}
```

The line appears twice on purpose. In `findings` it reaches the loop,
which is already told to iterate until that list is empty, so no caller
needs teaching about a new field. In `judgmentAdvisories` it stays
available on its own, for a reader who wants the probabilistic findings
apart from the deterministic ones. Deterministic findings keep their
place at the front of `findings` and are never edited or dropped.

Under `mode = "advisory"` nothing is merged: `clean` stays `true`,
`findings` stays empty, `judgmentAction` is `CONTINUE`, and the
advisories are reported on their own.

With no decision model that can answer, none of those four keys appears
at all, so a tool or script reading this reply sees no change.

The MCP tool gates on the same terms as the CLI. It used to weaken an
enforcing project to advisory, on the grounds that an exit code is a
thing a human watches — but a tool reply an agent reads is exactly
where the loop lives, so that put the gate out of reach precisely where
it was needed. There is no exit code over MCP; the gate is `clean` and
`findings`, which is what the agent iterates on anyway.

A judgment that was wanted and never arrived is the one case that
replaces the reply with a tool error. That is not wording an agent can
reword, so turning it into a finding would only make the loop retry it
forever.

`mode = "off"` reaches this tool too. It judges on its own initiative
rather than because anyone typed a command, so it is one of the
surfaces that goes quiet — the reply carries the deterministic findings
and no judgment keys, exactly as if no model could answer.

---

## The [decision] configuration block

```toml
[decision]
model = "nimble:latest"               # persisted by spec judge use
endpoint = "http://localhost:11434"   # defaults to the [llm] endpoint
timeout_seconds = 60
mode = "enforce"                      # off | advisory | enforce
min_confidence = 0.8
```

`endpoint` defaults to whatever `[llm] endpoint` is set to, so one
Ollama host is stated once. Set it only when the decision model lives
somewhere else.

### `decision.mode`

| Mode | What a judgment does |
| --- | --- |
| `off` | Nothing is asked automatically — not by `spec refine`, and not by the `refine_requirement` MCP tool. `spec judge` still answers, because a human typed it, but it gates nothing |
| `advisory` | The answer is reported beside the deterministic result and changes nothing |
| `enforce` | **The default.** A `FAILS` verdict asks for `REWORK`; an `INCONCLUSIVE` one asks to `ESCALATE`. Either appends its line to `findings`, makes `clean` false, and exits nonzero. A failed request is an error, never an approval |

A misspelled mode is no mode, so it takes the default — which means a
typo keeps gating rather than silently stopping. That is the direction
a typo in a gate should fail in.

### `decision.min_confidence`

A dead band, not a quality bar. For the boolean question the harness
asks, a probability at or above this reads as `HOLDS`, at or below
`1 - threshold` reads as `FAILS`, and anything between is
`INCONCLUSIVE`.

Widening the band moves answers out of `FAILS` and into `INCONCLUSIVE`,
which does not quiet them: both gate under `enforce`. What changes is
the finding they carry — `INCONCLUSIVE` asks you to reword the clause
after `then` so a test could assert it, rather than asserting the
outcome is unmeasurable.

Ollama's own `confidence` figure — returned for the `choice` and `score`
question types, though not for the boolean one — is defined as how
concentrated the probability distribution is, which its documentation is
explicit is *not* calibrated correctness. Treat the threshold as a width
of "do not act on this", and nothing more.

---

## How well does it work?

Not a question to answer from a vendor benchmark. Ollama publishes
aggregate scores for its decision models on its own evaluation suite;
those say nothing about whether this harness's question works on
acceptance criteria written by workshop students.

So there is a labeled evaluation in the repository:
`harness/tests/decision_live.rs` holds 32 acceptance criteria in five
groups — clearly measurable, clearly not, genuinely ambiguous, a group
written to *look* finished, and four lifted from this repository's own
spec — each labeled with a note saying why. Run it against your own
model:

```bash
cd harness
cargo test --test decision_live -- --ignored --nocapture
```

It prints the answer for every criterion, a confusion matrix, and a
threshold sweep, so you can see where the model and the labels disagree
and judge for yourself.

### What it scores

Two numbers get quoted about this question, and they are not the same
kind of thing. Keeping them apart is the whole point of the table:

| | Value | What it is |
| --- | --- | --- |
| Decision band | **0.80** | A policy input. The probability needed before an answer counts as a verdict at all; `1 - 0.80 = 0.20` is its mirror. Not a correctness figure — Ollama's own documentation says its confidence is not calibrated correctness |
| Measured accuracy | **0 misses, 1 false alarm, 3 unsure** | The evaluation result over the 32 labelled criteria, at that band |

"Misses" are the direction that matters: vague wording waved through as
measurable. There are none.

Against `nimble:latest` at the shipped threshold, nothing vague is
judged measurable — including all eight criteria written to look
finished, one of which is a direct prompt-injection attempt.

It produces one false alarm, on an ordinary shape:

> Given a requirement, when coverage is requested, then the verdict is `"covered"`

That assertion is an exact quoted string, so it is measurable, and the
question scores it 0.08.

### What is actually wrong with it

This was recorded for two releases as the model reading the quoted word
as a judgement. It is not, and the real answer matters more than the
one wrong case, because it bounds what this question can be trusted to
do. There are two defects, and both are reproducible with
`spec judge criterion`.

**Defect one: hedge words leak in from the setup.** Hold
`then there are 5 findings` byte-identical and grow the Given:

| Setup in front of the same assertion | |
| --- | --- |
| *(nothing)* | 0.991 |
| `Given a story, …` | 0.972 |
| `Given a story naming no actor, …` | 0.962 |
| `Given a story naming no actor, no benefit and three ambiguous words, …` | 0.115 |

It looks like sentence length until you change the one word:

| …three **_X_** words, when the requirement is refined, … | |
| --- | --- |
| `ambiguous` | 0.115 |
| `unusual` | 0.884 |
| `red` | 0.973 |
| `"ambiguous"`, in quotation marks | 0.975 |

`when_false` lists the hedge words that make a clause vague. The model
scans the whole criterion for them rather than only the clause after
`then` — the one thing the question's first sentence tells it not to
do. Quoting the word neutralises it, because `when_true` says quoted
text is a literal.

**Defect two: the assertion shape.** In one fixed short frame:

| `Given a requirement, when it is checked, …` | |
| --- | --- |
| `then the reply is an error naming "covered"` | 0.912 |
| `then the reply names "covered"` | 0.785 |
| `then the "verdict" field is "covered"` | 0.085 |
| `then the verdict is "covered"` | 0.058 |
| `then there are 5 findings` | 0.915 |
| `then the reply lists 5 findings` | 0.032 |

A copula with a literal on the right reads to this model as describing
a state rather than asserting one. Swap `is` for `names` and the same
literal scores fifteen times higher.

That second defect is the expensive one, because `then the X is "Y"` is
the most common assertion shape in this repository's own spec.

### The set is the thing to fix first

32 cases from one author, and most of the measurable half is
`then the result is N` — a shape the question happens to answer well.
Run the harness's own spec through it instead:

| `harness/requirements/requirements.json`, 73 criteria | |
| --- | --- |
| `HOLDS` | 51 |
| `INCONCLUSIVE` | 8 |
| `FAILS` | 14 |

Every one of those 14 is a quoted literal or a count:
`then the verdict is "valid"` 0.649, `then the phase is "GREEN"` 0.598,
`then there are 5 findings` 0.115, `then 0 steps are missing` 0.135,
`then an issue reads "REQ-006: …"` 0.271.

### Two fixes that were measured and do not work

**Name quoted status words in `when_true`.** Adding `"covered"`,
`"uncovered"` and `"NaN"` as examples of literals takes the labelled
set to 0 false alarms and 2 unsure. It also fits the prompt to the
test: those are that set's own words, and removing them returns the
score exactly to the published figures. A question that scores well
only on the words it was shown has not been improved.

**Send only the clause after `then`.** This is the structural version
of what `instructions` already asks for, and it does exactly what
defect one predicts — `then there are 5 findings` goes from 0.115 to
0.994. It is still worse on both sets:

| | labelled set (32) | harness spec (73) |
| --- | --- | --- |
| as shipped | 0 miss, 1 false alarm, 3 unsure | 22 flagged |
| clause only | **1 miss**, 3 false alarms, 1 unsure | 31 flagged |

The miss is the reason to stop looking for a wording that fixes defect
one. `then the system achieves 99.9% correctness across all code paths`
scores **0.933 alone**, and fails correctly inside its frame. The model
is not reading the then-clause and leaking context into it — it is
judging the vagueness of the whole sentence, which is one mechanism
working in both directions. Strip the setup and you lose the false
alarm on `three ambiguous words` and the true catch on `a
production-grade request payload` together.

Defect one is not a bug sitting beside the behaviour that works. It is
that behaviour, seen from the other side. A real fix has to separate
them, and nothing in the prompt's wording can.

Until something does, this repository runs the question in `advisory`
and not `enforce`. The judgments are worth reading; they are not yet
worth blocking on.

### What the default costs you

Be clear about the trade the default makes. Over those 32 labelled
criteria, enforcing blocks on 4 of them that the labels call fine: the
1 false alarm above, plus the 3 the dead band leaves `INCONCLUSIVE`,
which gate as `ESCALATE`. That is roughly one criterion in eight
stopping a loop that should have carried on — and on a spec whose
criteria do not look like this set's, much worse than that.

The default is `enforce` anyway, because the alternative is worse in a
way that does not show up in that count. The question is aimed at
exactly the wording no deterministic rule reaches — "then code quality
is improved by at least 20%" earns no finding from the regex rules,
because a number is present. A judgment that cannot refuse leaves that
gap unenforced entirely, and the 0 misses above stop meaning anything
the moment nobody is required to read them. A false alarm costs a
reword; a miss ships vague wording into a test suite.

**Measure it against your own criteria before you leave it on.** That
is not boilerplate caution: this repository did exactly that and turned
its own gate down to `advisory` on the result.

Your options, in the order worth trying:

- Reword the criterion. The reword that satisfies the model usually
  reads better anyway, and a criterion that survives a long Given is
  usually one whose assertion names its own subject.
- Widen `min_confidence`, which moves confident wrong answers into
  `INCONCLUSIVE`. They still gate, but with a finding that asks for
  clarity rather than asserting unmeasurability.
- Set `mode = "advisory"`, which is what this repository runs. The
  judgments are still reported on every `refine`, under `judgments` and
  `judgmentAdvisories`; they simply do not enter `findings` or clear
  `clean`.
- Set `mode = "off"` if the question does not fit your project at all.

Rewording the *question* is the one option not on that list, and the
section above is why: it is the right instinct, and both attempts at it
failed in ways that only showed up against criteria the question had
not been tuned on.

### Why the question is worded the way it is

An earlier phrasing asked the open question — "could a test check this
criterion?" — and scored well on plain vagueness while reading **every**
adversarial criterion as measurable at p > 0.9, including "the system
achieves 99.9% correctness across all code paths" and a criterion that
simply asserted it was measurable. Pointing the question at the clause
after `then`, and naming the specific dodges that clause uses, took that
from 8 misses to none.

The lesson is about the question, not the model: an open question
invites a judgment of the sentence's *style*, and style is exactly what
convincing-looking wording gets right.

`measurable/v2` sharpens that wording — it says a quoted value is a
literal whatever the word would mean as prose, and that a count is one
too — without moving the measured figures. The version is bumped
because the strings changed, which is the rule for this table, not
because the question got better. What the attempt to make it better
produced is in [What is actually wrong with
it](#what-is-actually-wrong-with-it) above, and the short version is
that the lesson repeats one level up: an evaluation set invites a
judgment of the *shapes it contains*, and those are exactly what a
question tuned on it gets right.

The wording itself lives in `harness/prompts/prompts.toml`, under
`[decision.measurable]`, beside the generative templates. It is built
into the binary rather than read from your project, so a judgment means
the same thing on every machine. `version` sits in that same table, next
to the three strings it names, so the two cannot drift: any edit to the
wording is a new version, because the numbers below were measured
against one phrasing and are not evidence about another.

The threshold came out of the same loop. At 0.70 the run produces two
confident false alarms rather than one and leaves one of the six
ambiguous criteria unsure. The two directions do not cost the same: a
confident false alarm asserts the wording is unmeasurable when it is
not, while an inconclusive one asks for clarity and is right to ask.
So the wider band ships, and 0.70 is still a reasonable choice for a
project that would rather be told plainly which criteria the model
dislikes.

---

## Troubleshooting

| Message | What to do |
| --- | --- |
| `No decision model is installed` | `ollama pull nimble` — judgments start working as soon as one is pulled, with or without `spec judge use` |
| `No decision model configured and none could be discovered` | Ollama could not be reached to ask what is installed. Start it, or name a model with `spec judge use <name>` |
| `decision model 'X' is not installed` | `ollama pull X` |
| `'X' cannot answer decisions here` | The model has no `decision` capability. The message lists the ones that do |
| `... has no /v1/systemone route` | Ollama is older than 0.35. Upgrade it |
| `no decision within 60s` | Raise `timeout_seconds` under `[decision]` |
| `the decision brief is too large` | The question's brief is capped at 64 KiB by the server, which does not truncate |
| `the decision reply did not match the questions asked` | The model answered something that was not asked. Reported rather than read as a verdict |
| `decision gate refused to pass without an answer` | A judgment was wanted and never arrived while enforcing. Fix the cause above, or set `mode = "advisory"` to carry on with the deterministic verdict |
| A criterion you believe is fine keeps failing | See [What the default costs you](#what-the-default-costs-you) |

## See also

- [`spec model`](model.md) — the **generative** model, a separate choice.
- [`spec config`](config.md) — every key and where it came from.
- [`spec refine`](spec.md#spec-refine) — the deterministic wording review
  a judgment sits beside.
- [Global flags](../global-flags.md) — the `--decision-model` override.

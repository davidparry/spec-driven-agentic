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

## Nothing is on until you turn it on

With no decision model configured — the state after `spec init` — the
harness asks nothing, and every command behaves exactly as it did
before this feature existed. There is no default model, no automatic
pull, and no model chosen for you.

## What a judgment is allowed to do

A judgment is **advice about wording**. By default it cannot do anything
else. Specifically, a decision model in this harness can never:

- turn a red test bar green, or change a test result in any way
- mark a requirement implemented, or certify any implementation
- bypass the [staging area](../staged-changes.md) or a commit
- waive a human checkpoint
- change `clean` or `findings` on a wording review

Those are all decided by deterministic code or by you. A judgment
travels *beside* that verdict, labelled, and the deterministic answer is
whatever it always was. `mode = "enforce"` can make a command exit
nonzero (see [below](#decisionmode)), but even then it only ever stops
work — it never approves any.

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
mode	advisory
min_confidence	0.8
```

With nothing configured:

```text
No decision model configured - judgments are off. Pick one with: spec judge models, then spec judge use <model-name>
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
  "mode": "advisory",
  "model": "nimble:latest",
  "question": "measurable/v1",
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
question	measurable/v1
mode	advisory
min_confidence	0.8

criterion	Given the refactored module, when the suite runs, then code quality is improved by at least 20%
answer	probability of true 0.038
verdict	FAILS
action	CONTINUE
input	--text
state	sha256:048965344f62409e5399403357ce83c4e7b3cce836b14673c0745b6f7ff7237d
tokens	in 385 out 1

judgment (measurable/v1): criterion "Given the refactored module, when the suite runs, then code quality is improved by at least 20%": the outcome may not be measurable - nimble:latest says probability of true 0.038

A judgment is advice about wording. It does not change the spec, the test bar, or whether a requirement is implemented.
```

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
      "question": "measurable/v1",
      "model": "nimble:latest",
      "verdict": "HOLDS",
      "action": "CONTINUE",
      "mode": "advisory",
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
| `question` | The question's version — `measurable/v1`. A threshold calibrated against one phrasing is not evidence about another, so a change to the wording is a new version |
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
`clean` and `findings` are untouched:

```json
{
  "id": "REQ-007",
  "clean": true,
  "findings": [],
  "judgments": [ { "...": "as above" } ],
  "judgmentAdvisories": [
    "judgment (measurable/v1): criterion \"...\": the outcome may not be measurable - nimble:latest says probability of true 0.014"
  ],
  "judgmentAction": "CONTINUE"
}
```

With no decision model configured, none of those four keys appears at
all, so a tool or script reading this reply today sees no change until
someone opts in.

The MCP reply always reports and never gates, even in a project
configured to enforce. An exit code is a thing a human watches; a tool
reply an agent reads is not the place to stop a workflow.

---

## The [decision] configuration block

```toml
[decision]
model = "nimble:latest"               # persisted by spec judge use
endpoint = "http://localhost:11434"   # defaults to the [llm] endpoint
timeout_seconds = 60
mode = "advisory"                     # off | advisory | enforce
min_confidence = 0.8
```

`endpoint` defaults to whatever `[llm] endpoint` is set to, so one
Ollama host is stated once. Set it only when the decision model lives
somewhere else.

### `decision.mode`

| Mode | What a judgment does |
| --- | --- |
| `off` | Nothing is asked. `spec judge` still works, because a human typed it |
| `advisory` | **The default.** The answer is reported beside the deterministic result and changes nothing |
| `enforce` | A `FAILS` verdict exits nonzero asking for `REWORK`; an `INCONCLUSIVE` one asks to `ESCALATE` to a human. A failed request is an error, never an approval |

### `decision.min_confidence`

A dead band, not a quality bar. For the boolean question the harness
asks, a probability at or above this reads as `HOLDS`, at or below
`1 - threshold` reads as `FAILS`, and anything between is
`INCONCLUSIVE` and used for nothing.

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

Against `nimble:latest` at the shipped threshold, nothing vague is
judged measurable — including all eight criteria written to look
finished, one of which is a direct prompt-injection attempt.

It produces one false alarm, and that one is worth knowing because the
shape is ordinary:

> Given a requirement, when coverage is requested, then the verdict is `"covered"`

That assertion is an exact quoted string, so it is measurable. The model
reads the quoted word as a judgement and scores it 0.09. Two longer
criteria of the same shape score 0.27 and 0.30, which the dead band
swallows — the band hiding a wrong answer, not the question getting it
right. **If your criteria assert quoted status words, expect to overrule
the judgment.** That is what advisory mode is for, and it is the clearest
argument against turning `enforce` on without measuring your own wording
first.

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

The wording itself lives in `harness/prompts/prompts.toml`, under
`[decision.measurable]`, beside the generative templates. It is built
into the binary rather than read from your project, so a judgment means
the same thing on every machine. `version` sits in that same table, next
to the three strings it names, so the two cannot drift: any edit to the
wording is a new version, because the numbers below were measured
against one phrasing and are not evidence about another.

The threshold came out of the same loop. At 0.70 the run produces three
confident false alarms rather than one and leaves none of the six
ambiguous criteria unsure. The two directions do not cost the same: a
false alarm teaches people to ignore judgments, while silence costs
nothing, because the deterministic rules are doing their job either way.
So the wider band ships, and 0.70 is still a reasonable choice for a
project that would rather see every flag.

---

## Troubleshooting

| Message | What to do |
| --- | --- |
| `No decision model is installed` | `ollama pull nimble`, then `spec judge use nimble` |
| `No decision model configured` | `spec judge use <name>` — the message lists the installed names to choose from |
| `decision model 'X' is not installed` | `ollama pull X` |
| `'X' cannot answer decisions here` | The model has no `decision` capability. The message lists the ones that do |
| `... has no /v1/systemone route` | Ollama is older than 0.35. Upgrade it |
| `no decision within 60s` | Raise `timeout_seconds` under `[decision]` |
| `the decision brief is too large` | The question's brief is capped at 64 KiB by the server, which does not truncate |
| `the decision reply did not match the questions asked` | The model answered something that was not asked. Reported rather than read as a verdict |

## See also

- [`spec model`](model.md) — the **generative** model, a separate choice.
- [`spec config`](config.md) — every key and where it came from.
- [`spec refine`](spec.md#spec-refine) — the deterministic wording review
  a judgment sits beside.
- [Global flags](../global-flags.md) — the `--decision-model` override.

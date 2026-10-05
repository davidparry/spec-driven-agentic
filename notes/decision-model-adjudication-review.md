# Where else a decision model earns its place

**Status.** A review, not an implementation. Nothing in this document is
built. The evidence is 25 labelled probe cases across five question
shapes plus 4 measurements through the shipped command, run against
`nimble:latest` on Ollama 0.35.1. Every figure is reproducible from
[Reproducing the probes](#reproducing-the-probes), which publishes each
question's instructions and its `true`/`false` criteria in full.

The question this answers: `validate_spec` and `refine_requirement` are
regular expressions. Do they actually cover what they claim, and would a
decision model do better — as a replacement, as a fallback, or as a
second pass after a rule fires?

## The short answer

The rules do not cover what they claim, and a decision model is the
right fix for only some of it.

- `validate_spec`'s Given/When/Then check is three case-insensitive
  **substring** tests. `The debt is forgiven whenever the thenar muscle
  relaxes` validates clean. Clause order is never checked.
- A decision model is **worse than the regex** at that. Asked whether a
  text has three clauses, it scored that sentence 0.858, and a criterion
  with no outcome at all 0.921 — confidently wrong, above the shipped
  threshold, on the one case the regex gets right.
- But asked whether a *named* rule was right about *meaning*, it
  separates cleanly: 0.993–0.999 for correct output against 0.012–0.052
  for broken output on the two best questions measured.

So the rule this review proposes is a boundary, not an expansion:

> A decision model may be asked whether a specific, named deterministic
> result was right about **meaning**. It may never be asked an open
> question about **shape**.

Shape is what a regex is good at and what a classifier is bad at. The
three best candidates are all meaning questions, and two of them are
about the *generative* model's output rather than about human wording —
which is where the guardrails are thinnest.

## 1. Structure versus meaning

### What `validate_spec` actually checks

```164:171:harness/src/domain/spec_validator.rs
        for criterion in &r.acceptance_criteria {
            let lower = criterion.to_lowercase();
            if !lower.contains("given") || !lower.contains("when") || !lower.contains("then") {
                issues.push(format!(
                    "{id}: criterion \"{criterion}\" must be phrased Given/When/Then"
                ));
            }
        }
```

Three `str::contains` calls on a lowercased string. No word boundaries,
no order, no count. `forgiven` satisfies `given`, `whenever` satisfies
`when`, `thenceforth` satisfies `then`.

Meanwhile the function that turns a criterion into Gherkin steps is
strict and anchored:

```228:230:harness/src/domain/steps.rs
    static SHAPE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^given\s+(.+?),\s*when\s+(.+?),\s*then\s+(.+)$").expect("valid regex")
    });
```

Commas required, order required, anchored at both ends. **A criterion can
pass `validate_spec` and then fail conversion**, which the validator
never warns about. That is the gap, and it is wider than a model problem:
the two halves of the same contract disagree with each other.

### The probe that settles it

Seven cases, asked as a `noul` question: *ignoring punctuation and clause
order, does this text contain a precondition, an action, and a checkable
outcome as three distinct clauses?*

| Case | Text | p(well-formed) | Correct? |
| --- | --- | --- | --- |
| GOOD | `Given the input "1,2", when add is called, then the result is 3` | 0.985 | yes |
| NOCOMMA | same, commas removed | 0.964 | yes |
| REORDER | `When add is called with "1,2", given an initialized calculator, then the result is 3` | 0.983 | yes |
| SUBSTR | `The debt is forgiven whenever the thenar muscle relaxes` | **0.858** | **no** |
| SUBSTR2 | `Given up on this, whenever, thenceforth` | 0.453 | unsure |
| NO_THEN | `Given an empty string, when add is called` | **0.921** | **no** |
| PROSE | `The user is given a form when they submit it then they see a confirmation` | 0.962 | yes |

Two confident wrong answers out of seven, both in the dangerous
direction — garbage read as well-formed — and a third case the question
cannot call either way. `NO_THEN` is the worst of them: a criterion with
**no outcome clause at all** scored 0.921, which clears the shipped 0.80
threshold with room to spare. The substring check this was supposed to
back up gets `NO_THEN` right.

This is not a wording problem to engineer around. Asking a model to
count clauses is asking it to do the one job a regex does perfectly, and
the model answers from the *style* of the sentence, which is exactly what
the substring check already gets fooled by. The conclusion is that
`validate_spec` gets no model, in any mode, as fallback or otherwise.

## 2. The three shapes that work

### Shape A — standalone judgment (already shipped)

One bounded question about one piece of evidence, with no deterministic
rule involved. This is `measurable/v1` on `refine_requirement`, described
in [notes/where-the-decision-model-is-called.md](where-the-decision-model-is-called.md).
Already built; not revisited here except for the hole in section 6.

### Shape B — flag adjudication (new)

A deterministic rule fires. The model is told **which rule, and why**,
and asked whether the flag is real in this sentence. The rule's output is
preserved either way; a dismissal demotes a finding to advisory rather
than deleting it.

Question asked: *a rule flagged this criterion for containing a vague
word, given in the state. Read how that word is actually used. Is it
genuinely making the outcome vague?*

| Label | Word | Criterion (abbreviated) | p(really vague) | Verdict at 0.80 |
| --- | --- | --- | --- | --- |
| false alarm | `handle` | `...when the handle is rotated 90 degrees, then the latch is open` | 0.050 | dismissed |
| false alarm | `should` | `Given the config "retries should be 3", ... then retries equals 3` | 0.020 | dismissed |
| false alarm | `properly` | `Given a file named "properly.txt", ... then the content is "ok"` | 0.033 | dismissed |
| real | `properly` | `...then they are handled properly` | 0.510 | unsure |
| real | `should` | `...then the result should be sensible` | 0.458 | unsure |
| real | `quickly` | `...then each one completes quickly` | 0.836 | confirmed |

All three false alarms dismissed, one real flag confirmed, two real
flags left unsure. No confident wrong answer in either direction — but
note where the misses fall. The question is **reliable at dismissing and
unreliable at confirming**, and that asymmetry is the whole reason this
shape is safe: an unsure real flag just stays a finding, which is what
would have happened without the model. Only the dismissals change
anything, and all three dismissals here were right.

It is also why this candidate ranks last. The margin that matters is
from the worst *real* flag (0.458) down to the dismissal line at 0.20,
and 0.258 is the narrowest safety margin measured anywhere in this
review. The `handle`-as-a-noun case is a genuine false positive of the
shipped regex, which matches `handles?` on a word boundary with no
regard for part of speech:

```11:16:harness/src/domain/refiner.rs
static AMBIGUOUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(should|could|might|handles?|properly|appropriately|quickly|easily|robust|user-friendly|etc)\b",
    )
    .expect("valid regex")
});
```

The same shape works on the coverage rule, which looks for edge-case
keywords across the criteria set:

```18:23:harness/src/domain/refiner.rs
static EDGE_CASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(empty|blank|invalid|negative|error|missing|null|nan|exceptions?|rejected|refused|fails?|failure)\b",
    )
    .expect("valid regex")
});
```

An edge case phrased without a listed keyword — `Given a cart with no
items` — is reported as happy-path-only. Three cases:

| Label | Criteria set | p(really happy-path-only) | Verdict at 0.80 |
| --- | --- | --- | --- |
| false alarm | `no items` + `3 items` | 0.173 | dismissed |
| false alarm | `""` + `"1,2"` | 0.138 | dismissed |
| real | `"1,2"` + `"4,5"` | 0.954 | confirmed |

Correct three times, but read the margins rather than the verdicts: at a
0.80 threshold the dismissal line sits at 0.20, and the first false
alarm scores 0.173. It is dismissed by 0.027. A project that raised its
threshold to 0.85 would move the dismissal line to 0.15 and that case
would stop being dismissed — which is the clamp behaving correctly, but
it shows how little room this question has.

### Shape C — output adjudication (new, and the strongest)

The generative model produced something. A deterministic gate checked its
*shape* and passed it. The decision model is asked whether it did the
**job**.

This is where the jig is thinnest, because every existing guardrail on
model output is structural. They check JSON parses, counts match, step
keywords are present, declared symbols survive, braces balance. None of
them can check that the content is right.

**C1 — did the reword actually fix the finding?** The existing gate
re-runs the refiner and counts matching finding signatures:

```1162:1189:harness/src/application/spec_mutation_service.rs
                if reworded.acceptance_criteria.len() < current.acceptance_criteria.len() {
                    return Err(format!(
                        "the rewording dropped acceptance criteria ({} became {}) - keep \
                         every criterion and address only the finding",
                        current.acceptance_criteria.len(),
                        reworded.acceptance_criteria.len()
                    ));
                }
                // An earlier call in this chain may already have cleared
                // this finding, and then there is nothing left to move.
                if before > 0 {
                    if reworded.title == current.title
                        && reworded.story == current.story
                        && reworded.acceptance_criteria == current.acceptance_criteria
                    {
                        return Err("the rewording is identical to the draft - change the \
                                    wording the finding names"
                            .to_string());
                    }
                    let after = self.matching_findings(merged, &reworded, targeted);
                    if after >= before {
                        return Err(format!(
                            "the rewording did not clear the finding - {after} of the same \
                             kind remain (the draft had {before}): {finding}"
                        ));
                    }
                }
                Ok(proposal)
```

That is a strong check and it is still only about signatures. Swap a
flagged word for an unflagged word of equal vagueness and the signature
clears. Four cases, asking *did the rewording make the outcome testable,
or substitute different vague wording?*

| Label | Before → after | p(really fixed) | Verdict at 0.80 |
| --- | --- | --- | --- |
| word swap | `handled properly` → `handled correctly` | 0.015 | caught |
| word swap | `handled properly` → `the behaviour is as expected` | 0.012 | caught |
| real fix | `handled properly` → `an IllegalArgumentException is raised` | 0.994 | passed |
| real fix | `completes quickly` → `status is 200 within 250 ms` | 0.993 | passed |

**C2 — does the scenario test the criterion?** The best-separated
question measured. The existing gate checks the count and the step
keywords:

```113:121:harness/src/domain/scenario.rs
    if scenarios.len() != expected {
        return Err(format!(
            "the reply held {} scenario(s) for {expected} acceptance criterion(s) - write exactly one scenario per criterion, in order",
            scenarios.len()
        ));
    }
    for scenario in &scenarios {
        check(scenario, taken, &scenarios)?;
    }
```

Count and prefixes. Nothing ties scenario *n* to criterion *n* by
content, so a scenario that asserts the wrong value, exercises the wrong
action, or drops the assertion is well-formed and gets staged. Five
cases, asking *does this scenario test what this criterion specifies?*

| Label | Scenario for `then the result is 3` | p(corresponds) | Verdict at 0.80 |
| --- | --- | --- | --- |
| match | `When I add "1,2" / Then the result is 3` | 0.999 | passed |
| wrong value | `When I add "1,2" / Then the result is 4` | 0.021 | caught |
| wrong action | `When I subtract "1,2" / Then the result is 3` | 0.052 | caught |
| no assertion | `When I add "-1,2" / Then the call completes` | 0.303 | unsure |
| off topic | `When I add "7" / Then the result is 7` | 0.052 | caught |

The correct scenario passes at 0.999 and three of the four broken ones
are caught outright. The fourth — a scenario that runs the action but
never asserts anything — lands at 0.303, inside the dead band, so it is
flagged for a human rather than rejected. That is the right outcome for
a case the question is unsure about, but it should be read as the
question being *weakest exactly where the defect is most subtle*: a
missing assertion is the one failure mode that looks structurally
perfect.

The separation between the correct scenario and the nearest wrong one is
0.696. That is not the widest in this review — C1 is better separated —
and the honest ranking below puts C2 first on consequence rather than on
margin. A scenario that asserts the wrong value becomes a failing test,
which becomes an implementation written to satisfy it.

## 3. Command by command

Every CLI command and all 25 MCP tools. Four verdicts: **build** (a
candidate this review recommends), **deterministic** (a real gap, but the
fix is a rule not a model), **shipped** (already has a judgment), and
**leave alone**.

### CLI commands

| Command | Verdict | Why |
| --- | --- | --- |
| `init` | leave alone | Scaffolds files from templates. No model, nothing to judge. |
| `greenfield` | leave alone | Orchestrates the commands below; judgments belong at the steps, not the conductor. |
| `deliver` | leave alone | Same — a loop over `implement`/`test`. |
| `list`, `show` | leave alone | Reads. Nothing to judge. |
| `draft` | build (low) | `parse_proposals_checked` enforces non-empty title/story/criteria; the edge-case retry is skipped on the last attempt, so happy-path-only proposals are accepted. Adjudication could ask whether the draft matches the sentence of intent. Lower value than C1/C2 because `refine` runs next and already judges the wording. |
| `validate` | **deterministic** | The substring/order gap in section 1. No model, in any mode. |
| `refine` | shipped + build | Has `measurable/v1`. Candidate 3 adds flag adjudication for the ambiguous-word and coverage rules. |
| `reword` | **build (candidate 2)** | Signature counting cannot see a vague-for-vague swap. Probe C1. |
| `set-feature` | leave alone | Points a field at a path. Deterministic and checkable. |
| `mark-implemented` | leave alone | A gate that requires GREEN plus a tagged scenario. A probability must never participate. |
| `include add` | leave alone | Catalog structure; `validate_spec` already answers it exactly. |
| `inspect` | leave alone | Detects languages and roots from the filesystem. Ground truth already. |
| `feature list/show/create` | leave alone | Filesystem reads and one templated write. |
| `scenario generate` | **build (candidate 1)** | Count and prefixes only. Probe C2, the best margin measured. |
| `scenario add/update/delete` | leave alone | Human-supplied steps, checked by the same keyword rules. No model call to guard. |
| `steps missing/generate` | leave alone (watch) | Guarded by set equality on cucumber patterns — `kept.len() == expected.len()` plus containment — which is exact, not heuristic. A judgment could ask whether a step body matches its pattern, but the bodies are deliberately `PendingException` placeholders, so there is nothing yet to judge. |
| `unittest generate` | build (low) | Same shape as steps: exact TODO-placeholder set equality. A judgment could ask whether the generated test actually asserts the criterion, which is C2 one altitude down. Worth doing after C2 proves out. |
| `implement` | leave alone | See section 7. The brief would be a code diff, the question open-ended, and the existing `reply_guard::damage` plus `run_tests` already provide ground truth. |
| `test` | leave alone | **Never.** This is the bar. |
| `state`, `status` | leave alone | Reads a state machine. |
| `refactor` | leave alone | Gated on GREEN by the state machine, snapshot-restored on failure. Deterministic throughout. |
| `changes show/validate/commit/discard` | leave alone | Staging is a transaction. A probability has no place in whether a write lands. |
| `model list/current/use` | leave alone | Capability lookup, already exact via `/api/show`. |
| `judge *` | shipped | This is the decision plane's own front door. |
| `config` | leave alone | Prints resolved settings. Would gain the clamped threshold rows from section 5. |
| `mcp serve` | leave alone | Transport. |
| `tools *` | leave alone | Profile registry, exact by construction. |
| `ask` | leave alone | Read-only free-form question to a human. Its output is prose for a person, not an artefact. |

### MCP tools

Same verdicts. The 25 are listed in server order.

| Tool | Verdict | Why |
| --- | --- | --- |
| `list_requirements` | leave alone | Read. |
| `get_requirement` | leave alone | Read. |
| `validate_spec` | **deterministic** | Section 1. |
| `refine_requirement` | shipped + build | Judgment attached; candidate 3 adds flag adjudication. |
| `get_tdd_state` | leave alone | State machine read. |
| `run_tests` | leave alone | **Never.** Ground truth. |
| `start_refactor` | leave alone | Deterministic refusal keyed to the phase. |
| `project_root` | leave alone | Read. |
| `project_inspect` | leave alone | Filesystem detection. |
| `feature_list`, `feature_read` | leave alone | Reads. |
| `feature_create` | leave alone | Templated write. |
| `scenario_add`, `scenario_update` | leave alone | Human steps, keyword-checked. No model call here — the MCP surface has no `scenario_generate` twin. |
| `scenario_delete` | leave alone | Deletion by name. |
| `changes_show`, `changes_validate` | leave alone | Reads over the staging manifest. |
| `changes_commit`, `changes_discard` | leave alone | Transaction boundaries. |
| `command_run` | leave alone | Asks a human to confirm. A judgment would weaken that, not strengthen it. |
| `requirement_reword` | **build (candidate 2)** | The MCP twin of `spec reword`; same gap, same question. |
| `requirement_mark_implemented` | leave alone | Gate. GREEN plus a tagged scenario. |
| `step_definitions_find` | leave alone | Read. |
| `step_definition_create` | leave alone (watch) | Pattern-set equality is exact. |
| `unit_test_create` | build (low) | As `unittest generate`. |

Counted: 25 tools, of which one is shipped, two are build candidates,
one needs a deterministic fix, and 21 should be left alone. That ratio is
the point of the review. The decision plane earns its place at a handful
of named seams, not across a surface.

## 4. Ranked candidates

### Candidate 1 — scenario corresponds to its criterion

- **Where.** `spec scenario generate`, in `GenerationService` beside
  `parse_scenarios_checked`.
- **Question.** `corresponds/v1`, one call per criterion-scenario pair.
  State carries the criterion and the scenario's steps, nothing else.
- **Evidence.** 4 of 5 decided correctly, 1 inconclusive, no wrong
  answer; margin 0.696 (probe C2).
- **What the existing gate cannot catch.** A scenario with the right
  shape and the wrong content. Count and step prefixes both pass.
- **Cost.** One call per criterion, median 48 ms warm. A four-criterion
  requirement adds roughly 200 ms to a command that already waits on a
  generative model for seconds.
- **On a dismissal.** Do **not** reject the scenario set. Report the
  mismatch and name which criterion-scenario pair disagrees, so the human
  reading `spec changes show` knows where to look. The deterministic
  fallback to `scenario_template` stays exactly as it is; a judgment must
  not trigger it, because the template is a worse artefact than a
  well-formed scenario that a model was unsure about.
- **On unavailable, timeout, or dead band.** No judgment, scenarios
  staged as today, a note recording that none was taken. Never read as
  agreement.
- **Tests.** Request construction and state contents; a scripted client
  returning each verdict; the mismatch report naming the right pair; the
  three failure paths; and proof that the staged scenarios are
  byte-identical whatever the judgment said.

### Candidate 2 — the reword addressed the finding

- **Where.** `spec reword` and `requirement_reword`, beside the existing
  `matching_findings` check in
  [harness/src/application/spec_mutation_service.rs](../harness/src/application/spec_mutation_service.rs).
- **Question.** `addressed/v1`. State carries the before text, the after
  text, and the finding that was targeted.
- **Evidence.** 4/4, margin 0.978 (probe C1) — the best-separated
  question in the review.
- **What the existing gate cannot catch.** A vague-for-vague swap that
  clears the named finding's signature. The gate counts signatures; it
  cannot read meaning. Note also the `if before > 0` guard in the excerpt
  above: when an earlier call in the chain already cleared the targeted
  finding, **none** of the three checks run — not the identical-text one
  either — so that round's reply is accepted on its shape alone.
- **Cost.** One call per reword round. The reword loop already runs one
  generative call per finding, so this is a small fraction.
- **On a dismissal.** This one may legitimately feed the retry loop,
  because the loop already exists and already re-asks on a failed check.
  The safe form is to let a confident dismissal count as one more
  `Err(reason)` in the existing `check` closure, with the reason naming
  the judgment — so the model is told it swapped one vague word for
  another and gets another attempt. On exhaustion the behaviour is
  unchanged: the finding is skipped and left to a human.
- **On unavailable, timeout, or dead band.** Treat as no opinion and let
  the existing signature check decide alone. Never convert a missing
  judgment into a rejection, and never into an acceptance.
- **Tests.** The dismissal producing a retry with the judgment in the
  reason; exhaustion leaving the old behaviour intact; a confirmation
  being a no-op; the three failure paths; and that no judgment path can
  stage a rewording the signature check rejected.

### Candidate 3 — the flag is real

- **Where.** `refine_requirement` and `spec refine`, for the
  ambiguous-word rule and the coverage rule only. Not for
  `outcome_is_concrete`, which section 6 shows wants a regex fix.
- **Question.** `flag_real/v1`. State carries the criterion, the flagged
  word or the criteria set, and the rule's own reason.
- **Evidence.** No confident wrong answer across 9 cases, but the
  weakest separation measured: all 5 false alarms dismissed cleanly
  (≤ 0.173), while 2 of 4 genuinely vague criteria landed in the dead
  band rather than being confirmed (probe B). Reliable at dismissing,
  unreliable at confirming.
- **What the existing gate cannot catch.** Part of speech and context.
  `handle` as a noun, `properly` inside a filename, `should` inside a
  quoted config value.
- **Cost.** One call per flag. Only fires when a flag exists, so a clean
  requirement costs nothing.
- **On a dismissal.** The finding **stays in `findings`**. It gains a
  note saying the judgment read it as a false alarm, and it stops
  counting toward `clean` only if the project has explicitly opted in —
  by default `clean` is untouched. This is the most conservative of the
  three because `clean` is the field everything downstream trusts.
- **On unavailable, timeout, or dead band.** The finding stands, exactly
  as today.
- **Tests.** Each rule's flag adjudicated both ways; `findings` byte-
  identical in every case; `clean` unchanged unless opted in; the three
  failure paths.

Build order is 1, then 2, then 3 — but note that this is **not** ranked
by margin, and candidate 1 is the one place the two criteria disagree.
Candidate 2 is the better-measured question (0.978 against 0.696), and
on separation alone it would go first.

Candidate 1 goes first on consequence. A reword that only swaps one
vague word for another leaves a vague criterion in the spec, which is
bad but inert — the next human to read it can still see the problem. A
scenario that tests the wrong thing becomes a failing test, then an
implementation written to make that test pass, and by then the mistake
is encoded in three artefacts and nothing downstream is looking at the
criterion any more. It is also the thinnest guardrail: count and step
prefixes are all that stand between a wrong scenario and the staging
area.

Candidate 3 is last on both criteria at once, which is the only
unambiguous part of the ordering.

## 5. The clamped threshold parameter

A per-call threshold, where a caller may only make the gate **stricter**.

```
effective = max(configured_threshold, requested_threshold)
```

Exposed as an optional number on the MCP tools that gain a judgment, and
as a flag on their CLI twins, with the same clamp on both so there is one
rule to remember.

### Why a clamp and not an override

The agent being guarded must not be able to lower the bar on itself. An
unclamped `min_confidence` on `refine_requirement` lets a model that has
just written vague criteria set the threshold to 0.01 and have every flag
against it dismissed. That is the failure this repository exists to argue
against, and it would arrive dressed as a configuration option.

### Why `max` is genuinely the strict direction

This needs stating carefully, because it is not obvious and the obvious
argument for it is wrong. Raising the threshold makes **both** definite
verdicts harder to reach and widens the unsure band. It does not make
the model "more careful"; it makes it quieter in both directions.

So "stricter" has to mean something precise. The property worth
preserving is:

> A judgment can never move an outcome in the permissive direction
> relative to what the deterministic layer alone would have done.

That holds because every question here is defined so the dead band
resolves to the deterministic answer or to a human:

- `measurable/v1` in enforce mode: unsure means `ESCALATE` to a human.
- Flag adjudication: unsure means the deterministic finding stands.
- Output adjudication: unsure means the existing deterministic gate
  decides alone.

Under that definition `max` is monotone in the right direction: the
higher the threshold, the more often the deterministic layer decides
alone, and the deterministic layer is the conservative baseline.

What `max` does **not** buy is more warnings. For the two output
adjudication candidates the useful signal is a *low* score — "this
scenario does not match its criterion" — which fires at `1 - t`. Raise
the threshold and that line drops, so a caller asking for 0.95 gets
fewer mismatch reports, not more. That is a real cost and it is the
honest trade: the clamp is tuned so the gate cannot be weaponised by
the agent it guards, at the price of a stricter caller also getting a
quieter one. It is the right trade here only because none of these
judgments is load-bearing — the deterministic checks still run
unchanged underneath every one of them.

**A question whose dead band resolved to "pass" would break the whole
argument.** The review's recommendation is that no such question be
added, and that the invariant be written down next to the constant
rather than left to be inferred by whoever adds the fifth question.

### Making the clamp visible

Clamping silently would default a caller's input without telling anyone,
which is the pattern Qodo rule 1406 warns about. So the judgment record
carries both numbers:

```json
{
  "threshold": 0.8,
  "requestedThreshold": 0.3
}
```

`requestedThreshold` is present only when a request was clamped, so an
audit shows that something asked for a looser gate and did not get it.
No error, no rejection — the call proceeds at the stricter value.

### Per-question defaults

Whether each question wants its own threshold is an empirical question,
so here are the margins. Each row gives the *worst* answer on the
correct side and the *best* answer on the wrong side, so the gap is the
narrowest each question was measured at:

| Question | Worst correct | Best wrong | Gap | Suggested default |
| --- | --- | --- | --- | --- |
| `addressed/v1` | 0.993 | 0.015 | 0.978 | 0.80 |
| `corresponds/v1` | 0.999 | 0.303 | 0.696 | 0.80 |
| `measurable/v1` | 0.993 | 0.298 | 0.695 | 0.80 (shipped) |
| `flag_real/v1` | 0.458 | 0.050 | 0.408 | 0.80 |
| `covers_happy/v1` | 0.954 | 0.173 | 0.781 | 0.80 |

The two adjudication rows need reading differently from the rest,
because for those the model's *low* answer is the one that acts. For
`flag_real/v1` and `covers_happy/v1` a dismissal fires at `1 - t`, so
the number that matters is the distance from the worst genuinely-real
flag down to 0.20: that is 0.258 for `flag_real/v1` and 0.781 for
`covers_happy/v1`. The first of those is the thinnest margin in the
review.

All five land on the same number, which is a mildly useful result on its
own: 0.80 is not tuned per question, it just happens to sit in every
measured gap. The recommendation is therefore **one default, with the
per-question structure in place** so a project that measures its own
wording can move one without moving the rest.

`flag_real/v1` is the one to watch. Its sides are the closest together
of any question here, it is the one most likely to produce a confident
wrong answer on wording nobody probed, and it is the candidate whose
action touches `findings`. Those three facts are why candidate 3 is
ranked last and why its dismissal is the most conservative of the
three.

## 6. Deterministic holes that want a rule, not a model

Two real gaps where reaching for the decision plane would be the wrong
instinct.

### 6.1 The Given/When/Then check disagrees with the converter

Section 1 has the detail. Applying both shipped checks to the same five
criteria shows the size of the disagreement:

| `validate_spec` | converts | Criterion |
| --- | --- | --- |
| passes | yes | `Given the input "1,2", when add is called, then the result is 3` |
| passes | **no** | `Given an empty string when add is called then the result is 0` |
| passes | **no** | `The debt is forgiven whenever the thenar muscle relaxes` |
| passes | **no** | `When add is called with "1,2", given a calculator, then the result is 3` |
| fails | no | `Given an empty string, when add is called` |

Three of the five are accepted by the validator and cannot be turned
into steps. Only the last one — the criterion with no outcome at all —
is rejected by both, and that is also the case the decision model got
wrong at 0.921.

The fix is a rule:

- Match on word boundaries, so `forgiven` and `whenever` stop counting.
- Require the three markers in order.
- Warn when `criterion_to_steps` would return `None` for a criterion that
  otherwise validates, so the mismatch between the two halves of the
  contract surfaces at validation rather than at generation.

The third is the valuable one and costs nothing: the function already
exists, is already pure, and already encodes the stricter contract. It
turns an inconsistency between two files into a single source of truth
without anyone having to agree on what the rule should be.

### 6.2 `PREDICATE_OUTCOME` accepts a word as an exact value

```46:51:harness/src/domain/refiner.rs
/// The affirmative half of a two-valued domain: a validation predicate
/// answers "valid" as exactly as a sum answers 3. The negative half is
/// already covered by [`ERROR_OUTCOME`] (invalid, rejected, refused).
static PREDICATE_OUTCOME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(valid|accepted|allowed|permitted|matches|matched)\b").expect("valid regex")
});
```

The comment's reasoning holds for a genuinely two-valued domain and
fails outside one. `then the result is valid` clears **every** refiner
rule: not ambiguous, one `when`, outcome "concrete" by this regex. With
any second criterion carrying an edge keyword, the requirement refines
with zero findings.

Measured against the shipped `measurable/v1` question, via
`spec judge criterion --text`:

| Criterion | Refiner | Judgment |
| --- | --- | --- |
| `Given a submitted form, when it is processed, then the result is valid` | no finding | FAILS 0.015 |
| `Given a logged-in user, when the page is requested, then access is permitted` | no finding | FAILS 0.034 |
| `Given a template, when it is rendered, then the output matches` | no finding | FAILS 0.060 |
| `Given the input "1,2", when add is called, then the result is 3` | no finding | HOLDS 0.998 |

All three predicate words are caught by the judgment that ships today,
and the refiner finds nothing wrong with any of them. That is a clean
demonstration that the decision plane is pointed at a real gap — and
also the clearest case in this review of a judgment quietly covering
for a rule that is wrong.

The recommendation is still a rule change: narrow the predicate list, or
require it to sit against a named subject, so the regex stops claiming
something it cannot know. A judgment that happens to cover a regex's
mistake is not a reason to leave the regex wrong — the judgment is
advisory, so today nothing downstream acts on it, and a project with no
decision model configured gets the broken rule and no safety net at
all.

## 7. What not to do

**Do not give `validate_spec` a model fallback.** Section 1. The probe
says the model is worse at this than the check it would be backing up.

**Do not judge `run_tests`, `changes_commit`, `requirement_mark_implemented`,
`start_refactor`, or `command_run`.** These are the gates. Four of them
are deterministic refusals a human relies on, and the fifth is the bar
itself. A probability that can influence any of them converts a guardrail
into a suggestion.

**Do not judge `implement`'s output.** Tempting, and wrong for three
reasons. The brief would be a code diff, which runs into the request
size limit the adapter already enforces. The question — "does this
implement the requirement?" — is open-ended, which is the shape that
failed in section 1. And the answer already exists downstream: `run_tests`
reports a bar, and `reply_guard::damage` catches dropped symbols and
truncation structurally. A judgment here would be a worse answer arriving
earlier.

**Do not let any judgment write to `findings`, `clean`, a test result, a
requirement's status, or the staging area.** The boundary from the
shipped design does not relax because the questions got better.

**Do not add retries to the decision adapter.** It makes one HTTP call
with a timeout and maps failure to a typed error. A retry loop around a
judgment is a loop that makes an unanswered question look answered if you
wait long enough.

## 8. Limits

These probes justify a design direction. They are not a quality claim.

- 25 probe cases plus 4 shipped-command measurements. One author's
  labels, one model, one machine, one afternoon. The shipped
  `measurable/v1` question needed 32 cases and two rounds of rework
  before it stopped being fooled, and there is no reason to think these
  four new questions are better than that one was at first draft. Each
  candidate needs its own labelled set, including adversarial cases,
  before it ships — not these five- and six-case probes.
- **The one question measured properly has a known error.** The shipped
  `measurable/v1` reads `then the verdict is "covered" / "uncovered"` as
  not measurable at 0.09, despite the outcome being an exact quoted
  string. It is recorded as a known false alarm in
  `harness/tests/decision_live.rs` and left unfixed, because the one
  wording change tried against it regressed an adversarial case. One
  known error in the only question with a real labelled set is the right
  prior for the four that have none.
- **An earlier draft of this document reported numbers that did not
  reproduce.** It recorded each question's instructions but not its
  `true`/`false` criteria; re-running from what was written produced
  different figures, including one that reversed a conclusion. Every
  table here was re-measured from the wording [Reproducing the
  probes](#reproducing-the-probes) now publishes in full. The lesson
  generalises: a probability is evidence only if the exact request that
  produced it is recorded.
- **Flag adjudication fails toward leaving findings in place.** Two of
  the four genuinely vague criteria in probe B landed in the dead band
  rather than being confirmed. Nothing bad happens — an unconfirmed
  finding is just a finding — but it means the question will not
  reliably tell a user their wording is vague. It is only trustworthy in
  the dismissing direction, and that should be stated to users rather
  than discovered.
- Every candidate above specifies behaviour for unavailable, timeout, and
  dead band, and in none of them does a failure become agreement. Any
  implementation that cannot state those three behaviours for a new call
  site is not ready to add it.

## 9. Qodo standards applied

Retrieved through Qodo. The repository-scoped search returned nothing;
the org-wide search returned rules, of which these shaped the review. The
UI-fallback rule (974) is skipped — there is no frontend here.

- *Define graceful handling for external dependency failures* (3162,
  warning) and *External dependency failures must degrade gracefully*
  (1409, warning). Every candidate in section 4 names its behaviour for
  unavailable, timeout, and dead band, and section 7 forbids a retry loop
  that would paper over them.
- *Invariant violations must fail loudly, not be silently defaulted*
  (1406, warning). Two consequences: flag adjudication demotes a finding
  rather than deleting it, and the threshold clamp records
  `requestedThreshold` instead of silently swallowing a looser request.
- *Handle all explicit failure paths with structured error handling*
  (175, warning). The existing `DecisionError` is already a typed enum
  the policy branches on; new call sites branch on the same type rather
  than stringifying it.
- *New conditional branches and error paths must ship with tests* (2895,
  warning) and *Tests must cover error and edge cases* (2852,
  recommendation). Each candidate lists its test obligations, including
  the three failure paths and proof the deterministic output is
  unchanged.
- *New critical operations must emit observability signals* (1410, 2531,
  3235, recommendation). Each call site records through the existing
  `tracing::info!` judgment line, so a new question is auditable the day
  it ships.
- Retry rules (1005, 436, 2803, 3132, warning). Recorded as a constraint
  rather than a change: the adapter does not retry, and section 7
  recommends it stays that way.

## Reproducing the probes

Each probe is one `noul` question posted to `/v1/systemone`, with
`keep_alive` set so the second call onward is warm. The shape, using the
scenario-correspondence probe as the worked example:

```bash
I='A model was asked to write one Gherkin scenario for the given acceptance criterion. Read both. Does the scenario actually test what the criterion specifies - the same precondition, the same action, and the same expected outcome?'
T='Yes - the scenario exercises the same action on the same kind of input and asserts the outcome the criterion names.'
F='No - the scenario tests something else, asserts a different value, drops the assertion, or restates the criterion without exercising it.'

jq -nc --arg m nimble:latest \
       --arg c 'Given the input "1,2", when add is called, then the result is 3' \
       --arg s 'Given a string calculator / When I add "1,2" / Then the result is 3' \
       --arg i "$I" --arg t "$T" --arg f "$F" \
  '{model:$m,
    state:{criterion:$c,scenario:$s},
    questions:{q:{type:"noul",instructions:$i,criteria:{"true":$t,"false":$f}}},
    keep_alive:"5m"}' \
| curl -sS -X POST http://localhost:11434/v1/systemone \
    -H 'content-type: application/json' -d @- \
| jq -r '.answers.q.noul'
```

That prints `0.9985114581207475` — the endpoint returns full precision
and every figure in this document is rounded to three places. It is the
first row of the C2 table.

The `state` keys differ per probe and the key names matter, because the
instructions refer to them by name; each question below records its own.

The endpoint is deterministic for a fixed request. All 25 cases were
run three times over and the output was byte-identical each time; the
seven shape cases were run five times with a spread of 0.000. So a
figure that fails to reproduce means the request differs somewhere, not
that the model wandered — which is how the recording defect above was
found in the first place.

The five questions, byte-exact. The `true` and `false` criteria matter
as much as the instructions — an earlier draft of this document recorded
only the instructions, and the numbers did not reproduce from it. Plain
hyphens throughout; no em-dashes were sent.

**Shape (section 1, the one that failed).** State key: `text`.

```text
instructions: This text is meant to be one acceptance criterion with three parts: a precondition, an action, and a single expected outcome. Ignoring punctuation and clause order, does it actually contain all three as distinct clauses - a state something starts in, a thing that happens to it, and a result a test could check?
true:  Yes - all three are present as distinct clauses, whatever the wording or order.
false: No - at least one of the three is missing. The words given, when or then may appear inside other words or as ordinary prose without introducing a real clause.
```

**Flag adjudication (shape B).** State keys: `criterion`, `word`.

```text
instructions: A deterministic rule flagged this acceptance criterion because it contains a word from a list of vague words. The flagged word is in the state as "word". Read how that word is actually used in this sentence. Is it genuinely making the expected outcome vague - would two engineers write different assertions because of it?
true:  Yes - the word leaves the expected outcome open. Two engineers reading this criterion would assert different things.
false: No - the word is incidental here. It appears inside a name, a quoted literal, or a concrete noun, or the outcome is pinned down by something else in the sentence.
```

**Coverage adjudication (shape B, second rule).** State key: `criteria`,
an array.

```text
instructions: A deterministic rule scanned this set of acceptance criteria for edge-case keywords (empty, blank, invalid, negative, error, missing, null, exception, rejected, fails) and found none, so it reported that the set covers only happy paths. Read the criteria. Does the set genuinely lack any boundary, error, or unusual-input case?
true:  Yes - every criterion describes a normal, successful path. No boundary, error, or unusual input is covered.
false: No - at least one criterion does cover a boundary, error, or unusual input, phrased without any of the keywords the rule looks for.
```

**Reword adjudication (C1).** State keys: `before`, `after`.

```text
instructions: A wording review flagged the "before" criterion as vague. A model was asked to reword it. Compare before and after. Did the rewording actually make the expected outcome testable, or did it only substitute different vague wording?
true:  Yes - the after version names a specific value, state, status or error that a test could assert exactly as written.
false: No - the after version is still vague. It swapped one open-ended word for another (properly, correctly, appropriately, as expected, successfully, gracefully), moved the vagueness elsewhere, or restated the goal without naming a value.
```

**Scenario correspondence (C2).** State keys: `criterion`, `scenario`.

```text
instructions: A model was asked to write one Gherkin scenario for the given acceptance criterion. Read both. Does the scenario actually test what the criterion specifies - the same precondition, the same action, and the same expected outcome?
true:  Yes - the scenario exercises the same action on the same kind of input and asserts the outcome the criterion names.
false: No - the scenario tests something else, asserts a different value, drops the assertion, or restates the criterion without exercising it.
```

The predicate-outcome measurements in section 6 use the shipped command
rather than a raw post:

```bash
spec --decision-model nimble:latest judge criterion \
  --text 'Given a submitted form, when it is processed, then the result is valid'
```

## Reading list

- [notes/where-the-decision-model-is-called.md](where-the-decision-model-is-called.md)
  — the shipped design: the one call site, the authority boundary, and
  role separation.
- [manual/src/commands/judge.md](../manual/src/commands/judge.md) — the
  user-facing reference, including the known false alarm.
- `harness/tests/decision_live.rs` — the 32-case labelled evaluation any
  new question should be measured the same way.

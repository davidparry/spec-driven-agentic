# A Day in the Life of a TDD Agentic Developer — follow-along

Replay the talk's morning on your own machine. You start with **one
vague sentence** and nothing in the backlog; by the end it is a
requirement that is implemented, proven, and traceable — and the thing
you built is a new MCP tool for the harness itself. Then you run the
same morning again with nobody driving.

This is the attendee copy. The deck is at
[`talks/slides/index.html?tdd`](../talks/slides/index.html?tdd).

## Prerequisites

- `spec` **built from this repository**, on PATH — `cargo install --path
  harness`, then `spec --version`. Not the published release: this walk
  uses `spec judge`, and the decision plane landed after `v0.7.0` was
  tagged, so no downloadable binary has it yet.
- Rust toolchain — `cargo --version`
- Java 21 and Maven, for the smoke test that catches you at the end
- Ollama 0.35 or newer, with a decision model pulled: `ollama pull
  nimble`. This one is not an extra. `spec refine` puts every
  acceptance criterion to it and reports what comes back, and 9:20 is
  built on reading those answers
- Optional: a generation model pulled locally as well, if you want the
  drafting steps to run offline

`harness/.spec/config.toml` pins the model the talk uses. Check what you
actually have and override for a run if it differs:

```bash
spec config        # shows the resolved model and where it came from
spec model list    # what Ollama has pulled locally
```

Every generating command takes `--model <name>` for one run, so you do
not have to edit the config to follow along on a different model.

## Work on a branch

Everything that follows writes the repository's real files: the catalog
gets reworded, a feature file and a test appear, production code is
written, and the Java smoke test gets bumped. There is no staging area
to review first — the harness edits the file and `git diff` is how you
read it. So put the morning on a branch you can throw away, which also
lets you run it a second time:

```bash
git clone https://github.com/davidparry/spec-driven-agentic
cd spec-driven-agentic
git switch trunk
git switch -c my-morning
cd harness                 # everything after this runs from here
```

Do not do this on `trunk`. The step that bites is the draft at 9:10 —
leave it in place and the next run starts with a requirement already
waiting, which kills the sentence-first opening.

(`spec deliver` at 11:10 would have offered to make this branch for
you. Doing it by hand now means the morning's hand-run commands land
somewhere you can throw away too.)

**About that `cd harness`.** `spec` does take a `--root`, but this walk
never passes it, and you should not need to. `spec` walks up from the
working directory to the nearest enclosing project, so standing in
`harness/` — or anywhere beneath it — is how it finds the harness
catalog. Running from the repository root
instead finds the *kata* catalog in `requirements/`, which is a
different spec entirely. If a command reports ids beginning `REQ-`, you
are one directory too high.

Confirm the starting state:

```bash
spec list       # 17 implemented, 0 pending
spec validate   # valid: true
spec status     # nextId: HARNESS-019
cargo test --manifest-path Cargo.toml --test spec_completeness
```

## 9:00 — one sentence

Nothing is waiting. Run `draft` with no arguments and it asks what to
build; the sentence below is what you type at that prompt, and it is the
whole of your input this morning:

```bash
spec draft
```

With a model resolved it opens on a description prompt rather than a
blank title prompt:

**Describe what to build in plain words (one or several requirements). Enter drafts manually instead:**

Paste this as the answer:

```
for one requirement, report each acceptance criterion as proven or unproven; report all criteria proven when none are left unproven; count a scenario as proof only when it is tagged with that requirement id; treat a scenario whose step is undefined as proving nothing; and raise an error when the requirement id is absent from the spec
```

One sentence, but a sentence that names five behaviors. That is
deliberate, and the next section is where it pays off. Note what it
still does *not* say: nothing here asks for a tool, an MCP endpoint, or
a module. Keep it that way — 9:10 depends on it.

Watch what scrolls past while it thinks. Three things are worth
catching:

**It reads the catalog before it writes.** The model calls
`list_requirements`, then `get_requirement` on an existing requirement
or two, to match the house style of the spec it is adding to.

**The harness rejects its own model.** Expect one of these, and with
this sentence usually two — five requirements are five chances to miss
an edge case, and one that misses sends the whole batch back:

```text
The model reply was invalid (requirements "..." cover only happy paths -
add at least one edge case to each) - asking again (2 of 3)
```

That is the wording review running *inside* the draft, before anything
is written. Remember it — it is the whole of the next section.

Running out of retries costs you nothing: the last attempt is accepted
whatever its criteria look like, and whatever it still lacks returns as
a finding in the wording round. Budget three to four minutes here.

**Then it proposes**, having split your sentence into atomic
requirements, one capability each:

```text
The description holds 5 requirement(s):
  1. Each acceptance criterion gets a proven or unproven verdict
  2. The roll-up verdict reads all criteria proven once none are unproven
  3. Only scenarios tagged with the requirement id count as proof
  4. A scenario with an undefined step proves nothing
  5. An unknown requirement id raises an error

Accept [Enter for all, or comma-separated numbers]:
```

**Five behaviors in, five requirements out.** The count is your
sentence's doing rather than the model's mood: across all three
attempts above the split stayed at five and only the criteria were
reworked. Ask for less and you get less — an earlier version of this
walk opened with "for one requirement, show me which acceptance
criteria no test proves", one capability in one clause, and the same
model answered with a single requirement and no `Accept` prompt at all.

The titles are still the model's wording, so read what is on your
screen rather than matching it to the list above.

### Your first decision of the morning

Type `1`.

One keystroke, and it is a real choice: four proposed requirements are
discarded unbuilt, and you set the scope rather than the machine. What
lands in `requirements.json` is `HARNESS-019`:

```text
{
  "id": "HARNESS-019",
  "title": "Each acceptance criterion gets a proven or unproven verdict",
  "written": true
}
```

**Nobody typed `HARNESS`.** The highest id in this catalog is
`HARNESS-018`, so the next one is `HARNESS-019`: the prefix is read off
the requirements already in the file, the number is `max + 1` across the
whole merged catalog so two included files cannot collide, and the
zero-padding matches the width that is already there. Run the same
binary from the repository root and it says `REQ-007`, because that
catalog numbers `REQ`. There is nothing to configure and nothing for an
agent to guess — `spec status` reports `nextId` precisely so the shape
can be read rather than invented.

> **Shortcut.** `spec deliver "<the sentence you just typed>"` runs this
> exact draft as its first stage and then keeps going, all the way to
> implemented. You will do that at the end of the morning; for now, one
> stage at a time.

## 9:10 — find out that clean is not the same as right

Two different questions, two different tools:

```bash
spec validate            # valid: true  — the shape is fine
spec refine HARNESS-019  # clean: true  — no findings at all
```

A green result, and it is the most interesting moment of the morning.

`refine` ran two reviews, not one, and it is worth knowing what each
of them did.

The deterministic rule set has nothing to say because it **already
ran** — you watched it reject the model's first answer at 9:00 for
covering only happy paths. By the time a requirement reaches you, that
gate has done its work.

The second review is a different model, answering one bounded question
per criterion: could a test check this with a single unambiguous
result? It writes no prose and returns a probability. The full record
is in the JSON reply, so run `spec refine HARNESS-019 --json` and
scroll up in it:

```text
"judgments": [ … "verdict": "HOLDS", "answer": { "noul": 0.887 } … ],
"judgmentAction": "CONTINUE"
```

It approved. Two reviews with nothing in common but the sentence they
read, and both of them are happy.

So read what the model actually wrote:

```text
Given requirement REQ-001 with 3 acceptance criteria and a feature file holding
1 scenario tagged @REQ-001 proving the first criterion, when the coverage of
REQ-001 is reported, then the report lists criteria 2 and 3 verbatim as uncovered
```

That is a good criterion. Concrete outcome, a real edge case elsewhere
in the set, and anyone on your team could write the assert. It is also
**the wrong requirement**. "When the coverage of REQ-001 is reported"
asks for a *function*. Build it and you get `requirement_coverage()` in
a module somewhere — correct, green, and no use at all, because the
morning is supposed to end with a new tool answering over MCP and
nothing here asked for a tool.

Notice that the second review **approved it at 0.887**, and that it
was right to. The question it answers is "could a test assert this?",
and a test absolutely could. Both reviews did their jobs correctly and
the requirement is still the wrong one, because neither of them was
asked the question that mattered:

> A rule set can check that an outcome is concrete. A second model can
> guess whether a test could assert it. Neither can check that you
> asked for the right thing.

That is not a gap you close by adding a third reviewer. It is the gap
where you are standing.

If your `refine` does come back with a finding or two, fix them — the
argument above is unaffected, and it is the one that matters.

### Your turn

Rewrite it so that every criterion **names the tool you are building**.
The behaviour barely changes; the contract does.

Run bare, `spec reword HARNESS-019` opens a wizard that asks for the
title, the story and then each criterion in turn, with a proposal
already in the brackets. Here the flags are the better door: they skip
the wizard, and `--criterion` **replaces** the criteria list, so the
four below are the four you end up with however many your draft had.
This is the version the talk uses — one command:

```bash
spec reword HARNESS-019 \
  --title "Uncovered acceptance criteria are reported per requirement" \
  --story "As a developer closing out a requirement, I want each acceptance criterion reported as covered or uncovered by its scenarios and tests so that I can see what is still unproven before I mark the work implemented." \
  --criterion 'Given a requirement whose every criterion is matched by a tagged scenario and an asserting test, when the criteria_coverage MCP tool is called with its id, then the verdict is "covered"' \
  --criterion 'Given a requirement with 3 criteria of which 1 is matched by no asserting test, when the criteria_coverage MCP tool is called with its id, then 1 criterion is reported uncovered' \
  --criterion 'Given the requirement id "REQ-999" is absent from the spec, when the criteria_coverage MCP tool is called with it, then the reply is an error naming "REQ-999"' \
  --criterion 'Given a requirement carrying 0 acceptance criteria, when the criteria_coverage MCP tool is called with its id, then the verdict is "uncovered"'
```

The criteria carry double quotes, so they are single-quoted here. If
you would rather type your own wording at the prompts, run the command
with no flags and the wizard walks you through the same three fields.

```bash
git diff requirements/    # read what you just changed
spec refine HARNESS-019 --json   # clean again, and the judgments are worth a look
```

Look hard at that last line. The rule set had nothing to say about the
model's wording, and it has nothing to say about yours. The
deterministic review cannot tell the two requirements apart, and they
build different software. That gap is where your judgment lives, and it
is the reason this step is not automated.

Do not skip the `git diff` either. Every mutation the harness makes is
already in the file; reading it is one of three places the morning asks
for your judgment.

### The review that is not a rule

Run `spec refine HARNESS-019` again on your reworded version, this
time *without* `--json`, and the second review lays out what it made
of each criterion:

```text
HARNESS-019 is clean.

A second review (CRITERION_MEASURABLE, measurable/v2) judged 4 criteria:
  HARNESS-019 acceptance criterion 1  INCONCLUSIVE  probability of true 0.211
  HARNESS-019 acceptance criterion 2  INCONCLUSIVE  probability of true 0.583
  HARNESS-019 acceptance criterion 3  HOLDS         probability of true 0.985
  HARNESS-019 acceptance criterion 4  INCONCLUSIVE  probability of true 0.315

Judgment: CONTINUE - advisory mode, so nothing here blocks
```

Every criterion is listed, not only the ones it complained about:
three complaints out of four is a different fact from three out of
three, and the complaints alone cannot tell you which you are looking
at — the criterion that satisfied the question leaves no line. The
denominator is the point of the block. `--json` puts
the audit record behind each row — model tag, threshold, token count —
and that is the reply `spec deliver` and any agent reads.

Line the rows up against the clauses you actually wrote:

| # | The clause after `then` | |
| --- | --- | --- |
| 3 | `the reply is an error naming "REQ-999"` | `HOLDS` 0.985 |
| 2 | `1 criterion is reported uncovered` | `INCONCLUSIVE` 0.583 |
| 4 | `the verdict is "uncovered"` | `INCONCLUSIVE` 0.315 |
| 1 | `the verdict is "covered"` | `INCONCLUSIVE` 0.211 |

Three of four it will not sign off on, and the lowest sits a hair
above outright rejection — about criteria you and the room would both
call testable. Before you write the whole thing off, see what it is
for. One
deterministic rule asks whether the clause after `then` *looks*
concrete — a number, a quoted value, a named error. Any number
satisfies it, so `refine` reports **nothing at all** about either of
these:

```bash
spec judge criterion --text "Given the refactored module, when the suite runs, then code quality is improved by at least 20%"
spec judge criterion --text "Given a deployed service, when load is applied, then latency is acceptable (p99 < SLO)"
```

```text
answer	probability of true 0.032    verdict	FAILS
answer	probability of true 0.048    verdict	FAILS
```

Nobody measured code quality and nobody wrote down the SLO. The rule
asks whether a number is *present*; it cannot ask whether the number
*is* the assertion. The second model can, and it is confident and right
about both. That is the gap it exists to cover, and nothing
deterministic reaches it.

So: confident and right on the wording that fools a regex, and
confidently wrong on `then the verdict is "covered"`. Which is exactly
the shape of thing you should not wire a blocking gate to.

### Why this one runs advisory

`mode` ships as `enforce`, where `FAILS` asks to `REWORK` and
`INCONCLUSIVE` to `ESCALATE`, so all three of those lines would land
in `findings`, clear `clean`, and exit nonzero.
`harness/.spec/config.toml` turns it down to `advisory`, and the
reason is measured rather than cautious.

The question publishes its accuracy: 0 misses, 1 false alarm, 3 unsure,
over a 32-criterion labelled set you can run yourself with
`cargo test --test decision_live -- --ignored --nocapture`. Those
figures are real. They are also measured on 32 criteria written by one
person, and most of the measurable half looks like `then the result is
3`. Put this repository's **own** spec through the same question — 73
criteria, in shapes that set does not contain — and 14 come back
`FAILS` with 8 more `INCONCLUSIVE`. Every one of the 14 is a count or
a quoted literal:

| | |
| --- | --- |
| `then the verdict is "valid"` | 0.649 |
| `then the phase is "GREEN"` | 0.598 |
| `then an issue reads "REQ-006: no scenario tagged …"` | 0.271 |
| `then 0 steps are missing` | 0.135 |
| `then there are 5 findings` | 0.115 |

Two things are going wrong, and both are worth seeing yourself, because
the lesson is not "models are unreliable" — it is that you can find out
*exactly how* in about ten minutes.

Take the last one and grow the setup in front of it, leaving the
assertion byte-identical:

| Setup | |
| --- | --- |
| *(nothing)* | 0.991 |
| `Given a story, …` | 0.972 |
| `Given a story naming no actor, …` | 0.962 |
| `Given a story naming no actor, no benefit and three ambiguous words, …` | 0.115 |

That looks like sentence length, and it is not. Change the one word:
`three unusual words` scores 0.884, `three red words` 0.973, and
putting `"ambiguous"` in quotation marks puts it back to 0.975. The
question's `when_false` text lists the hedge words that make a clause
vague, and the model is scanning the **whole criterion** for them
instead of only the clause after `then`.

The second is simpler and costs more. In one fixed frame, `then the
reply is an error naming "covered"` scores 0.912 and `then the verdict
is "covered"` scores 0.058 — same literal, and the only difference is
`is` instead of `naming`. `then the X is "Y"` reads to this model as
describing a state rather than asserting one, and that is the most
common assertion shape in the whole spec.

### What wording would make yours HOLDS

Both defects are now in front of you, so turn them on your own four.
Nothing below is rhetorical — `spec judge criterion --text '…'` asks
the question one clause at a time, and you can reproduce every line of
it in about two minutes.

Criteria 2 and 4 are defect two, and defect two has a cure. Swap the
copula for a verb that reports, and the same assertion goes green:

| Criterion 2, as you wrote it and reworded | |
| --- | --- |
| `then 1 criterion is reported uncovered` | `INCONCLUSIVE` 0.583 |
| `then the reply names 1 criterion "uncovered"` | **`HOLDS` 0.879** |
| `then there is 1 uncovered criterion` | **`HOLDS` 0.954** |

| Criterion 4, as you wrote it and reworded | |
| --- | --- |
| `then the verdict is "uncovered"` | `INCONCLUSIVE` 0.315 |
| `then the reply names the verdict "uncovered"` | **`HOLDS` 0.917** |

Criterion 1 is where it gets interesting, because the identical cure
does nothing at all: `names the verdict` 0.262, `is a verdict naming`
0.296, `names every criterion` 0.236. Still inconclusive, every one —
and criterion 4's assertion has the same shape and the same reword
lifted it to 0.917.

So it is not the assertion. Leave that byte-identical at `then the
verdict is "covered"` and change only the setup in front of it:

| `Given …` *(assertion never changes)* | |
| --- | --- |
| `a requirement whose every criterion is matched by a tagged scenario` | `FAILS` 0.187 |
| `a requirement whose every criterion is matched by a tagged scenario and an asserting test` | `INCONCLUSIVE` 0.211 |
| `a requirement with 3 criteria and 3 tagged scenarios` | `INCONCLUSIVE` 0.742 |
| `a fully covered requirement` | **`HOLDS` 0.920** |

That is defect one with the lid off, on your own wording: a five-fold
swing in the score of a clause that never changed.

So yes — there is wording that turns all three holdouts green. Now
look hard at what it cost, because this is the part to take home.

Criterion 1 got to `HOLDS` by having its setup blurred from "every
criterion matched by a tagged scenario **and an asserting test**" down
to "a fully covered requirement". The first one names the two
conditions a test has to build. The second makes the reader guess
them, using the very word the requirement exists to define. The score
went up and **the criterion got worse** — and the model cannot tell,
because it was never asked that question.

Criteria 2 and 4 are the honest half of the same lesson: `names 1
criterion "uncovered"` really is a shade sharper than `is reported
uncovered`, and if you prefer it, keep it. The test is whether you
would have made the edit with the score hidden.

That is the line. A flagged criterion is worth re-reading, and
sometimes the reword it prompts is a real improvement. Rewording a
criterion you already judged sound, in order to move a number, is
fitting the work to the measurement. This repository went through the
same exercise on its own spec — 24 flagged criteria read one at a
time, 15 of them left exactly as they were, one of which quotes a
forty-character exact string — and that is why the number you are
looking at is advisory.

Sit with what this is an example of, because it is this morning's
argument pointed back at the harness. A gate was built, measured, and
shipped with an honest number on the box. The number was true and the
gate was still not safe to obey, because the set it was measured on did
not look like the work. **That is the same failure as 9:10** — every
check passing and the thing still being wrong — and the only thing that
caught it was running it against criteria it had never seen.

So you read these judgments and you do not obey them:

- **A flagged line is a prompt to re-read the criterion**, not a
  verdict on it. Sometimes it is right and the reword is an
  improvement. Here, three times out of four, it is not.
- **Turn it up to `enforce` when you have measured it on your own
  criteria** and not before. `mode = "enforce"` in `[decision]`.
- **Turn it `off`** if the question does not fit your project at all.

The three places this morning asks for *your* judgment are still
exactly three, and the decision model is not a fourth. Advisory or
enforcing, it can never approve anything — and today it cannot reliably
refuse either.

## 9:30 — turn the criteria into tests

The scenarios go in a feature file of their own, so it has to exist
before anything can be appended to it. Create it through the harness
rather than by hand — the file is on disk the moment the command
answers, so the next command reads it straight away:

```bash
spec feature create --path tests/features/tool_coverage.feature --name "Criteria coverage"
spec scenario generate HARNESS-019 --feature tests/features/tool_coverage.feature
spec unittest generate HARNESS-019
git diff
spec steps missing
```

How many steps come back missing depends on how the model worded the
scenarios. It may reuse step definitions the suite already has and
report none; it may invent new phrasings and report a dozen. Both are
fine, but anything in the list has to be defined before the bar can run:

```bash
spec steps generate      # only if the list was not empty
git diff
spec steps missing       # 0 now
```

The generated definitions are `todo!()` stubs that bind the project's
own `SpecWorld`. They compile, they fail, and filling them in is the
next person's job — which is exactly what RED means.

> The model calls are the slow part of the morning. Measured against the
> pinned local model: `scenario generate` about four minutes, `steps
> generate` about one, `unittest generate` well under one — and
> `implement`, at 10:15, anywhere from twenty minutes to an hour per
> attempt. If a step returns in a second it was a cache hit from an
> earlier run, which is fine, and at 10:15 it is the whole plan: see
> [the note there](#this-step-is-slow-and-it-may-not-succeed).

Note `scenario generate`, not `scenario add`. `add` appends one scenario
you have already written, step by step; `generate` is the one that reads
the acceptance criteria and derives the scenarios from them.

The scenarios are derived from the acceptance criteria and tagged
`@HARNESS-019`. Nobody re-typed the requirement into a test, which is
exactly how a spec and a suite stop disagreeing.

## 10:00 — red

```bash
spec test
spec state     # phase: RED
```

A red bar here is the proof the test can fail. A test written after the
code never gives you that.

Now try to tidy up:

```bash
spec refactor --note "tidy the coverage module"
```

Refused: `Never refactor on a red bar`. That is a state machine in
`harness/src/domain/tdd.rs`, not a line in a prompt.

## 10:15 — write the code

```bash
spec implement HARNESS-019
git diff       # read the model's code before you trust the bar
spec test      # GREEN
```

What you are building: an MCP tool `criteria_coverage` that takes a
requirement id and reports, per acceptance criterion, whether a test
asserts it. The matching logic already exists in
`harness/src/domain/coverage.rs` — ported from the workshop's own grader
in `scripts/verify-workshop-run.sh`. You are exposing it, not inventing
it.

Read the preflight it prints first. The last line names the file the
attempt will write, and it should say `src/mcp.rs`. Nothing configured
that: the harness matched your When/Then steps to the step definitions
that bind them, followed those one hop into the helpers they call, and
picked the production file those name through the most distinct symbols.
The scenarios you wrote at 9:30 are what pointed it there.

**If it refuses instead**, saying it cannot tell which production file
`HARNESS-019` belongs in, the inference worked correctly and found
nothing to go on. That happens when every step your scenarios bind to
is still a `todo!()` stub: a pending step names no production code, so
there is nothing pointing anywhere. Two independent names are needed
before the harness will commit to a file, and one accidental match is
not enough. Either write a step body or two against the real types, or
name the file yourself:

```bash
spec implement HARNESS-019 --into src/mcp.rs
```

Refusing is the point. A guess here writes a morning's work into an
unrelated module and the bar still goes green on the tests that did not
need it.

### This step is slow, and it may not succeed

`implement` sends the whole neighborhood of the change to the model and
asks for working code back. Against this crate, three measured
attempts took 20, 36 and 57 minutes, and the two that returned code
produced Rust that did not compile — once a `Vec<String>` used as a
`String`, once a syntax error. Nothing was lost either time: the build
caught it, the bar stayed RED, and the next attempt is briefed with the
failure. That loop is the system working. It is also not something to
sit through.

So run the morning before you need it, and keep the result. Identical
requests are answered from `.spec/cache/` without calling the model at
all, and the TTL in `harness/.spec/config.toml` is a day, so a
rehearsal the night before replays in seconds. Two things to know about
that cache: the key is the prompt, so a different `--into` or an
edited scenario misses it and you pay full price again; and expired
entries are swept on the next write, so raising the TTL afterwards does
not bring back a run you have already let go stale.

If rehearsal never gives you an attempt that compiles, that is the
honest answer for this model on this requirement, and the thing to do
is implement it yourself and show the diff. Watching a model fail three
times is a worse use of the hour than reading good code out loud.

**If it times out instead of answering**, the model needed longer than
`timeout_seconds` under `[llm]` and everything it had generated is
gone. That ceiling is 3600 here for exactly this step; a project with
more source in the neighborhood may need more again.

**Scope check.** What lands here is one `#[tool(...)]` method on the
router in `harness/src/mcp.rs`. That is enough to make it
real over the protocol: `spec mcp serve` will advertise it, and
`spec mcp call` can invoke it. It is *not* enough to make it a `spec`
subcommand, and no agent will reach for it until you say so. Both of
those are [homework](#homework-wire-it-into-the-cli) — the talk only has
time for the tool itself.

If the model stalls, write it yourself. The lesson is the gates, not the
generation.

## 10:50 — get caught

Your server now answers with a tool the Java smoke test's plan does not
name.

The smoke test launches the `spec` on your PATH, not the source tree, so
install what you just wrote before you run it — otherwise the sweep
inspects the binary you started the morning with and the
build stays green for the wrong reason:

```bash
cargo install --path . --force    # release build, a minute or two
spec mcp tools | wc -l                  # one more than this morning
mvn -f ../smoke-test/pom.xml test -Dspec.binary=$(which spec)
```

`LiveSpecServerTest` fails, and the sweep reports `criteria_coverage` as
**unexpected**. That is `CLI-009` doing its job: it asks for an MCP tool
the plan does not name to fail the Java build until someone plans it, so
that the smoke test cannot silently skip new surface area.

**The `-Dspec.binary` flag matters.** `LiveSpecServerTest` is annotated
`@EnabledIfSystemProperty(named = "spec.binary", ...)`; without it the
test is skipped and the build stays green.

Fixing it means moving the planned count from 21 to 22 everywhere it is
written down, and it is written down in four files. All paths are from
the repository root:

1. **`smoke-test/src/main/java/com/davidparry/workshop/smoke/ToolPlan.java`**
   — add a row to the `TOOLS` list. It is a read-only tool taking an id,
   so it follows `get_requirement`:

   ```java
   read("criteria_coverage", Map.of("id", "REQ-001")),
   ```

2. **`smoke-test/src/test/java/com/davidparry/workshop/smoke/ToolPlanTest.java`**
   — the count appears in **four** assertions, not one: `ToolPlan.size()`
   and `ToolPlan.names()` in `planIsExactlyTwentyOne`, then
   `report.discovered()` and `report.called()` in the two sweep tests.
   Maven will only show you the first one that fails, so change all four
   before rerunning.

3. **`smoke-test/requirements/requirements.json`** — `CLI-009` names the
   number in both its title and its first criterion.

4. **`smoke-test/src/test/resources/features/tool_sweep.feature`** — the
   `Feature:` line, and the `@CLI-009` scenario's name and its `Then`.

Rerun. Green.

## 11:05 — close it out

```bash
spec mark-implemented HARNESS-019
cargo test --manifest-path Cargo.toml --test spec_completeness
```

`mark-implemented` is gated twice: the bar must be GREEN, and a scenario
must carry the requirement's tag. The drift gate is scoped to implemented
requirements, so it now covers 17 instead of 16 — and because
`HARNESS-019` is one of them, its wording is checked on every build from
here on.

## 11:10 — the same morning, with nobody driving

Every command you ran this morning, `spec deliver` runs on its own. Not
a similar sequence — the same stages, in the same order, through the
same services.

**It stops once, before it writes anything.** `deliver` asks for a
branch name for the run — Enter takes a generated `spec/<date>-<id>`,
`n` stays where you are, and `--no-branch` skips the question. That is
the one interruption; after it the run never pauses again.

```bash
spec deliver "every requirement should report which of its criteria no test proves"
# Branch name for this run (Enter for spec/2026-10-05-k3f92a, or n to stay on my-morning):
```

| You typed | `deliver`'s stage |
| --- | --- |
| `spec draft` | `draft_plan` — drafts, then reads back which ids actually reached the catalog |
| `feature create` + `scenario generate` | `author_scenarios` |
| `steps missing` + `steps generate` | `author_steps` — and it **re-checks**, looping until nothing is undefined or it runs out of rounds |
| `unittest generate` | `author_unit_test` |
| `spec test` → RED | `try_run` |
| `implement`, review, `test` → GREEN | `drive_to_green` — up to `--attempts` tries, 3 by default |
| `refactor` on a green bar | `refactor` — skip it with `--no-refactor` |
| `mark-implemented` | `mark_implemented` |

One stage genuinely differs, and it is worth knowing: `author_scenarios`
is **deterministic**. Where you ran the model-driven `scenario generate`
at 9:30, the factory calls `create_feature` and then one `add_scenario`
per acceptance criterion through `criterion_to_steps` — no model, no
judgement, nothing to get wrong, and it finishes instantly. It also
skips the stage entirely when a tagged scenario already exists, so
delivering a requirement whose Gherkin you wrote by hand is not an
error.

Three flags shape the run:

```bash
spec deliver                 # every pending requirement in the catalog
spec deliver --attempts 5    # RED-to-GREEN tries before it gives up on one
spec deliver --fail-fast     # stop at the first that falls short
```

`spec deliver` with no target takes the **whole backlog**. Fill the
catalog with sentences, walk away, come back to implemented
requirements and a report of the ones that fell short.

### What it costs

`deliver` answers every prompt itself, through an `AutoPrompter` that
echoes `(spec deliver never stops to ask)` at each gate. Set against
the three moments this morning asked you for:

| The moment | You, this morning | `spec deliver` |
| --- | --- | --- |
| The wording | You rewrote it until `refine` was clean | The model's draft stands as written |
| The diff | You read `git diff` before moving on | Written unread |
| The implementation | You reviewed the code | Accepted on a green bar alone |

Every one of those is a **human judgement**. What `deliver` does *not*
give up is every gate that is a state machine rather than a prompt: it
still cannot refactor on a red bar, and still cannot mark a requirement
implemented without a green bar and a scenario carrying its tag. And it
still offers the branch, so a run you did not watch is one `git switch`
and one `git branch -D` away from never having happened.

### The part worth sitting with

Go back to 9:10 and imagine you had not been there.

The model's own wording was `valid`. It was `clean`. Delivered
autonomously it would have earned a tagged scenario, a red bar, a green
bar, and the `implemented` flag — every gate satisfied, honestly. The
decision model would not have been asked at all: `deliver` never
consults it, in any mode. The
Java smoke test at 10:50 would have stayed green too, because no new
MCP tool would exist to be unplanned.

And what you would have on disk is `requirement_coverage()`: a
function, in a module, that no agent can call and no host can see.

Every gate passed. Every gate was about **correctness**. Not one of
them asked whether this was the right thing to build, because that
question has no state machine behind it.

That is the whole argument of the morning in one line: the gates are not
there to slow you down. They are what makes it safe to leave — from
everything except the one decision nothing is checking.

## 11:15 — let the morning grade itself

```bash
spec mcp call criteria_coverage --arg id=HARNESS-019
```

```text
verdict     covered
criteria    4
uncovered   []
```

Every acceptance criterion you wrote at 9:10 has an asserting test. The
tool you built reports on the requirement that asked for it.

The matching is literal: a criterion is covered when a test feeds in
the same quoted inputs and names the same expected number. That is why
criterion 3 names `"REQ-999"` and not "the unknown id" — a criterion
with no literal in it is invisible to the tool, and reads uncovered
beside a test that proves it.

Notice that you reached it through `mcp call`. That is the only door it
has so far, which is the subject of the homework section below.

## Homework: wire it into the CLI

Adding a tool and *adopting* a tool are two separate jobs. The morning
did the first one. Three things are still missing, cheapest first.

### 1. Offer it to the agents that should ask the question

A tool the harness never puts in front of a model is a tool no model
calls. Which callers get which built-ins is
`default_profile` in `harness/src/domain/tool_profile.rs`. You can test
the idea without recompiling — `tools enable` persists an attachment in
`.spec/config.toml`:

```bash
spec tools enable criteria_coverage --for status
spec tools list --for status     # confirm it resolved
spec status                      # the advice can see coverage now
```

Three callers are worth it, for different reasons:

| Caller | Command | Why |
| --- | --- | --- |
| `implement-advice` | `spec implement` preflight | The preflight's whole job is saying whether `implement` can succeed. A criterion with no asserting test is exactly a reason it cannot — going green would be a false green. |
| `status` | `spec status` | The `next_step` prompt already renders per-requirement *gaps*. An uncovered criterion is the gap that decides between `spec unittest generate` and `spec implement`. |
| `ask` | `spec ask` | The read-only catch-all already holds every other non-mutating reader, so `spec ask "is HARNESS-019 covered?"` currently has to guess. |

Three callers are worth *skipping*, and the reasons are more interesting
than the ones above:

- `spec-draft` and `spec-reword` — no tests exist yet for a requirement
  being drafted, so the answer is always "uncovered". Advice nobody can
  act on.
- `scenario-generate`, `steps-generate`, `unittest-generate` — these
  *write* the tests coverage measures. Asking first is circular.
- `implement` — tempting, and wrong. Hand the implementing model a
  coverage oracle and it optimizes for the report instead of the failing
  test. Keep the check on the advice call next door.

To make any of this the default rather than one developer's config, edit
`default_profile` — and add `"criteria_coverage"` to `all_builtin_names()`
in that file's test module, or
`every_default_profile_is_non_empty_and_names_only_real_tools` will
reject your own tool as unknown. (A small, satisfying taste of the drift
gates the rest of the morning was about.)

### 2. Say so in the prompts

Prompt wording is data, not Rust: it lives in
`harness/prompts/prompts.toml` and `harness/prompts/workflow.md`. Four
places, in descending order of value:

- **`[tool_rules]` in `prompts.toml`** — appended to *every* system
  prompt. It already holds "Never claim a test passed without calling
  `run_tests`"; the parallel line is the one guardrail that matters here,
  because the failure mode is a model asserting coverage it never
  measured.
- **Step 8 of `prompts/workflow.md`** — the process document both advice
  prompts embed. The check belongs immediately before
  `spec mark-implemented`, which is the gate it informs.
- **`[advice]` in `prompts.toml`** — its rules enumerate the exact next
  commands the model may name. A coverage gap needs to be a legal answer.
- **`[mcp]` in `prompts.toml`** — the instructions the server advertises
  to every host, and what `spec ask` reasons from. Its loop sentence
  currently ends `start_refactor -> run_tests`.

### 3. Give it a front door

This is the command you are being asked to build, not one to run:

```text
spec coverage HARNESS-019      # does not exist yet
```

A new arm on `Command` in `harness/src/main.rs`, an application service
beside `spec_service`, and the wiring in `wiring.rs`. Nothing clever —
but it is the difference between a tool your agents can call and a tool
*you* can call.

And if you do all three, do it the way the morning did: write the
requirement first, refine it until it is clean, and let the criteria
become the tests.

## If you get stuck

- **`spec draft` cannot reach a model** — the wizard falls back to
  asking you for the title, story and criteria yourself. Paste the clean
  wording from 9:10 and carry on; you lose the "watch it get drafted"
  beat but nothing downstream.
- **`refine` is clean on the first pass** — expected, and the point of
  9:10. The draft loop already applied the wording review. Reword it
  anyway, so the criteria name the tool.
- **`refine --json` has no `judgments` in its reply** — no decision model was
  found, so only the deterministic review ran. `ollama pull nimble`,
  then `spec judge models` to confirm it is seen and `spec config` to
  check `decision.model`. Without it nothing in 9:20 happens, and that
  section is the one this morning is built around.
- **A judgment blocks a criterion you are sure of** — then you are not
  running the config in this repository, which sets `[decision] mode =
  "advisory"` for the reasons in 9:20. On the shipped `enforce`
  default, `INCONCLUSIVE` gates. Read the line, and if you still
  disagree, set `advisory` to report without gating or `off` to ask
  nothing.
- **The draft proposes a different number of requirements** — the count
  tracks the sentence, not the run: five named behaviors gave five
  proposals on every attempt measured. Shorten the sentence and the
  count drops, and at one proposal the `Accept` prompt does not appear
  at all. Accept the one you want and carry on.
- **The draft is slow** — about three and a half minutes on a large
  local model, before `scenario generate` adds five more. Nine minutes
  of waiting between the sentence and the first red bar is normal.
- **A command reports `REQ-` ids** — you are above `harness/`, so it
  discovered the kata catalog at the repository root. `cd harness`.
- **`spec deliver` asked for a branch name and you did not want one**
  — answer `n`, or pass `--no-branch` to skip the question entirely.
- **`spec test` runs the whole suite** — pass `--feature
  tests/features/tool_coverage.feature` and the Cargo runner scopes the
  run to the one test target that owns that feature.
- **A generating step fails with a timeout** — the model is slower than
  the configured ceiling. Raise `timeout_seconds` in
  `harness/.spec/config.toml`, or pass `--model` and use a smaller one.
- **`implement` refuses with missing steps** — run `spec steps missing`
  and read the list. It should be empty before you implement; anything
  in it is a Gherkin step with no matching definition. `spec steps
  generate` writes the stubs.
- **`implement` refuses because it cannot tell which file** — your
  steps are all still pending, so nothing names the production code.
  Pass `--into src/mcp.rs`, or write a step body against the real
  types and let the inference find it.

### Start over

Throw the branch away. That is why you made one — it takes the reworded
catalog, the feature file, the generated test, the implementation, and
the Java-side changes with it in one go:

> `git clean -fd` deletes untracked files outright. On a fresh clone
> that is exactly the morning's output and nothing else. If you have
> your own unpushed work under `harness/`, check `git clean -nd
> harness/ smoke-test/` first — the `-n` lists what would go without
> removing anything.

```bash
# 1. everything the morning wrote, tracked and untracked alike
git switch trunk
git branch -D my-morning
git clean -fd harness/ smoke-test/

# 2. the TDD phase is gitignored, so step 1 leaves it behind and the
#    next run starts mid-cycle
rm -f harness/.spec/state.json

# 2b. the cached model replies - only if you want the slow, honest
#     run back. Keeping them is what makes a second pass quick.
rm -rf harness/.spec/cache

# 3. if you reinstalled the binary at 10:50, put a trunk build back
cargo install --path . --force

# 4. confirm you are back at the start
git status --short          # clean
cd harness
spec list                   # 17 implemented, 0 pending
spec status                 # nextId: HARNESS-019
```

The check that matters is **0 pending**. If something is pending, the
requirement you drafted at 9:00 survived — you are still on
`my-morning`, or you did the morning on `trunk`. If `nextId` has moved
past `HARNESS-019`, a previous run's draft was committed;
`git checkout trunk -- harness/requirements/` puts it back.

## Where to go next

- The harness's own spec: `harness/requirements/requirements.json`
- The drift gate: `harness/tests/spec_completeness.rs`
- The String Calculator version of this loop:
  [student-follow-along.md](student-follow-along.md)
- Running it all offline on a local model: [pi-path.md](pi-path.md)

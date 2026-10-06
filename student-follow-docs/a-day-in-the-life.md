# A Day in the Life of a TDD Agentic Developer — follow-along

Replay the talk's morning on your own machine. You start with **one
vague sentence** and nothing in the backlog; by the end it is a
requirement that is implemented, proven, and traceable — and the thing
you built is a new MCP tool for the harness itself. Then you run the
same morning again with nobody driving.

This is the attendee copy. The deck is at
[`talks/slides/index.html?tdd`](../talks/slides/index.html?tdd).

## What makes this different from the kata

The usual workshop drives the String Calculator. This one points the
harness at **itself**: `harness/requirements/requirements.json` is the
spec for the `spec` binary, its scenarios live in `harness/tests/features`,
and `harness/tests/spec_completeness.rs` fails the build when the two
drift apart.

Sixteen requirements are implemented and **nothing is pending**. There
is no ticket waiting for you. You are going to say one vague sentence,
watch the harness turn it into `HARNESS-018`, find out that what came
back is unusable, fix it, and then build what it describes.

## Prerequisites

- `spec` **built from this repository**, on PATH — `cargo install --path
  harness`, then `spec --version`. Not the published release: this walk
  uses `spec judge`, and the decision plane landed after `v0.7.0` was
  tagged, so no downloadable binary has it yet.
- Rust toolchain — `cargo --version`
- Java 21 and Maven, for the smoke test that catches you at the end
- Optional: Ollama with a local model pulled, if you want the generation
  steps to run offline

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

**About that `cd harness`.** There is no `--root` flag anywhere in this
walk. `spec` walks up from the working directory to the nearest
enclosing project, so standing in `harness/` — or anywhere beneath it —
is how it finds the harness catalog. Running from the repository root
instead finds the *kata* catalog in `requirements/`, which is a
different spec entirely. If a command reports ids beginning `REQ-`, you
are one directory too high.

Confirm the starting state:

```bash
spec list       # 16 implemented, 0 pending
spec validate   # valid: true
spec status     # nextId: HARNESS-018
cargo test --manifest-path Cargo.toml --test spec_completeness
```

## 9:00 — one sentence

Nothing is waiting. Here is the whole of your input this morning:

```bash
spec draft
```

```
for one requirement, show me which acceptance criteria no test proves
```

Watch what scrolls past while it thinks. Three things are worth
catching:

**It reads the catalog before it writes.** The model calls
`list_requirements`, then `get_requirement` on an existing requirement
or two, to match the house style of the spec it is adding to.

**The harness rejects its own model.** You will usually see at least
one of these:

```text
The model reply was invalid (requirements "..." cover only happy paths -
add at least one edge case to each) - asking again (2 of 3)
```

That is the wording review running *inside* the draft, before anything
is written. Remember it — it is the whole of the next section.

**Then it proposes**, having split your sentence into atomic
requirements, one capability each:

```text
The description holds 5 requirement(s):
  1. Unproven acceptance criteria of one requirement are listed
  2. A fully proven requirement reports the verdict all criteria proven
  3. A scenario with an undefined step proves nothing
  4. Only scenarios tagged with the requirement id count as proof
  5. The proof report refuses an unknown requirement id

Accept [Enter for all, or comma-separated numbers]:
```

**Do not expect five.** The count moves run to run — the same sentence
gave five on one pass and one on the next. Read what is on your screen.

### Your first decision of the morning

Type `1`.

One keystroke, and it is a real choice: four proposed requirements are
discarded unbuilt, and you set the scope rather than the machine. What
lands in `requirements.json` is `HARNESS-018`:

```text
{
  "id": "HARNESS-018",
  "title": "Uncovered acceptance criteria are listed for one requirement",
  "written": true
}
```

**Nobody typed `HARNESS`.** The highest id in this catalog is
`HARNESS-017`, so the next one is `HARNESS-018`: the prefix is read off
the requirements already in the file, the number is `max + 1` across the
whole merged catalog so two included files cannot collide, and the
zero-padding matches the width that is already there. Run the same
binary from the repository root and it says `REQ-007`, because that
catalog numbers `REQ`. There is nothing to configure and nothing for an
agent to guess — `spec status` reports `nextId` precisely so the shape
can be read rather than invented.

Try it from further down to see the discovery for yourself:

```bash
cd src/domain && spec status && cd ../..
```

Same catalog, three directories up, found by walking up the tree.

> **Shortcut.** `spec deliver "the harness should handle coverage
> properly so gaps are found easily"` runs this exact draft as its first
> stage and then keeps going, all the way to implemented. You will do
> that at the end of the morning; for now, one stage at a time.

## 9:10 — find out that clean is not the same as right

Two different questions, two different tools:

```bash
spec validate            # valid: true  — the shape is fine
spec refine HARNESS-018  # clean: true  — no findings at all
```

A green result, and it is the most interesting moment of the morning.

The wording review has nothing to say because it **already ran** — you
watched it reject the model's first answer at 9:00 for covering only
happy paths. By the time a requirement reaches you, the deterministic
gate has done its work.

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

This is the point of the whole exercise:

> A rule set can check that an outcome is concrete. It cannot check
> that you asked for the right thing.

If your `refine` does come back with a finding or two, fix them — the
argument above is unaffected, and it is the one that matters.

### Your turn

Rewrite it so that every criterion **names the tool you are building**.
The behaviour barely changes; the contract does. Here is the version the
talk uses:

**Title**

```
Uncovered acceptance criteria are reported per requirement
```

**Story**

```
As a developer closing out a requirement, I want each acceptance criterion reported as covered or uncovered by its scenarios and tests so that I can see what is still unproven before I mark the work implemented.
```

**Acceptance criteria**

```
Given a requirement whose every criterion is matched by a tagged scenario and an asserting test, when the criteria_coverage MCP tool is called with its id, then the verdict is "covered"
Given a requirement with 3 criteria of which 1 is matched by no asserting test, when the criteria_coverage MCP tool is called with its id, then 1 criterion is reported uncovered
Given the requirement id "REQ-999" is absent from the spec, when the criteria_coverage MCP tool is called with it, then the reply is an error naming "REQ-999"
Given a requirement carrying 0 acceptance criteria, when the criteria_coverage MCP tool is called with its id, then the verdict is "uncovered"
```

```bash
spec reword HARNESS-018
git diff requirements/    # read what you just changed
spec refine HARNESS-018   # no findings from the rule set — exactly as before the edit
```

Look hard at that last line. The rule set had nothing to say about the
model's wording, and it has nothing to say about yours. The
deterministic review cannot tell the two requirements apart, and they
build different software. That gap is where your judgment lives, and it
is the reason this step is not automated.

Do not skip the `git diff` either. Every mutation the harness makes is
already in the file; reading it is one of three places the morning asks
for your judgment.

### Optional: ask a second model the question the rules cannot

Skip this unless you have a decision model pulled — Ollama 0.35 or newer
and `ollama pull nimble`. It adds about three minutes and changes nothing
downstream. The flag below configures nothing, so there is no cleanup.

`refine` just gave you a page of findings from a fixed rule set. Those rules
have a blind spot worth seeing. One of them asks whether the clause after
`then` *looks* concrete — a number, a quoted value, a named error. Any
number satisfies it, so `refine` reports **nothing at all** about this:

```bash
spec --decision-model nimble:latest judge criterion --text "Given the refactored module, when the suite runs, then code quality is improved by at least 20%"
```

```text
answer	probability of true 0.038
verdict	FAILS
```

Nobody measured code quality. The rule asks whether a number is
*present*; it cannot ask whether the number *is* the assertion. A
decision model can — it writes no prose and answers one bounded question
with a probability.

Now ask it about the wording you just committed:

```bash
spec --decision-model nimble:latest judge criterion HARNESS-018
```

```text
criterion	Given a requirement id that is absent from the spec, when the criteria_coverage MCP tool is called with it, then the reply is an error naming the unknown id
answer	probability of true 0.855
verdict	HOLDS
action	CONTINUE
input	HARNESS-018 acceptance criterion 3
state	sha256:cf5a07a68ecc239ac89c6210470a587886d2c353d091b560730d5caf8ea9a98f
```

That is one of four, and the other three come back `INCONCLUSIVE` at
0.269, 0.745 and 0.298. Which is not the result you were expecting, and
is the reason this section exists. All four of those criteria are
testable — you can write the assert for each without asking anyone what
a word means. The judgment confidently agrees with one of them.

The two scoring in the 0.2s are a known weakness of this question: their
assertion is a quoted string, but the quoted word (`"covered"`) reads
like a judgement and the model weighs the word over the quotes. The
repository records it rather than hiding it —
`cargo test --test decision_live -- --ignored --nocapture` prints it as a
false alarm.

Now notice what the default does with that. `mode` defaults to
`enforce`, so `INCONCLUSIVE` is not used for nothing: it asks to
`ESCALATE`, which means all three land in `findings`, `clean` goes
false, and the command exits nonzero. Three of four criteria you have
already reasoned about, blocking.

That is the trade, and it is worth sitting with rather than explaining
away. The default gates because the question reaches wording the regex
rules cannot — the `code quality is improved by at least 20%` case
above earns *no* deterministic finding, so a judgment that cannot
refuse leaves that gap unenforced entirely. The cost of that is runs
like this one.

What you do about it, in order:

- **Read the finding first.** An `INCONCLUSIVE` line asks you to reword
  the clause after `then` so a test could assert it. Sometimes it is
  right and the reword is an improvement.
- **Widen `min_confidence`** if your criteria keep landing in the dead
  band for the same reason. It does not silence them — both `FAILS`
  and `INCONCLUSIVE` gate — but it changes which complaint you get.
- **Set `[decision] mode = "advisory"`** while you measure the question
  against your own wording. It then reports exactly as described above
  and the harness uses it for nothing, which is the behaviour this
  section originally assumed.

The three places this morning asks for *your* judgment are still exactly
three, and a decision model is not one of them: it can stop work, and it
can never approve any. This run is a decent argument for measuring the
question against your own criteria before you leave the gate on.

## 9:30 — turn the criteria into tests

The scenarios go in a feature file of their own, so it has to exist
before anything can be appended to it. Create it through the harness
rather than by hand — the file is on disk the moment the command
answers, so the next command reads it straight away:

```bash
spec feature create --path tests/features/tool_coverage.feature --name "Criteria coverage"
spec scenario generate HARNESS-018 --feature tests/features/tool_coverage.feature
spec unittest generate HARNESS-018
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
`@HARNESS-018`. Nobody re-typed the requirement into a test, which is
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
spec implement HARNESS-018
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
`HARNESS-018` belongs in, the inference worked correctly and found
nothing to go on. That happens when every step your scenarios bind to
is still a `todo!()` stub: a pending step names no production code, so
there is nothing pointing anywhere. Two independent names are needed
before the harness will commit to a file, and one accidental match is
not enough. Either write a step body or two against the real types, or
name the file yourself:

```bash
spec implement HARNESS-018 --into src/mcp.rs
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
edited scenario misses it and you pay full price again; and expired entries are swept on the next write, so raising the
TTL afterwards does not bring back a run you have already let go stale.

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

Fix it: add `criteria_coverage` to
`smoke-test/src/main/java/com/davidparry/workshop/smoke/ToolPlan.java`,
bump the count in `ToolPlanTest`, and update `CLI-009`'s criteria and its
tagged scenario in `smoke-test/requirements/requirements.json` and
`features/tool_sweep.feature`. Rerun. Green.

## 11:05 — close it out

```bash
spec mark-implemented HARNESS-018
cargo test --manifest-path Cargo.toml --test spec_completeness
```

`mark-implemented` is gated twice: the bar must be GREEN, and a scenario
must carry the requirement's tag. The drift gate is scoped to implemented
requirements, so it now covers 17 instead of 16 — and because
`HARNESS-018` is one of them, its wording is checked on every build from
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
spec mcp call criteria_coverage --arg id=HARNESS-018
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
| `ask` | `spec ask` | The read-only catch-all already holds every other non-mutating reader, so `spec ask "is HARNESS-018 covered?"` currently has to guess. |

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

```bash
spec coverage HARNESS-018      # does not exist yet
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
- **The draft proposes a different number of requirements** — also
  expected. Measured at five on one run and one on the next from the
  identical sentence. Accept the one you want and carry on.
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
spec list                   # 16 implemented, 0 pending
spec status                 # nextId: HARNESS-018
```

The check that matters is **0 pending**. If something is pending, the
requirement you drafted at 9:00 survived — you are still on
`my-morning`, or you did the morning on `trunk`. If `nextId` has moved
past `HARNESS-018`, a previous run's draft was committed;
`git checkout trunk -- harness/requirements/` puts it back.

## Where to go next

- The harness's own spec: `harness/requirements/requirements.json`
- The drift gate: `harness/tests/spec_completeness.rs`
- The String Calculator version of this loop:
  [student-follow-along.md](student-follow-along.md)
- Running it all offline on a local model: [pi-path.md](pi-path.md)

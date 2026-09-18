# Student Follow-Along: the `spec` binary

Your step-by-step companion for finishing the String Calculator workshop
with the `spec` command-line runner instead of an agent in an editor. This
is the command-by-command walk of a verified end-to-end run: every command
in the order it was actually typed, and what you should see after each one.

Three companion pages, and it is worth knowing which is which:

- [harness-path.md](harness-path.md) is this path's **reference** — the
  same commands as terse recipes, plus the design notes on tool profiles
  and what the harness discovers. Read it when you want to know *why*.
- [../student-follow-along.md](../student-follow-along.md) is the
  **agent-centric hour** — the same MCP server driven by Cursor through
  prompts. Read it when you want the workshop as it is presented.
- [pi-path.md](pi-path.md) points a free local agent at that same server,
  for the fully offline route.

All of them call one MCP server. What changes is who supplies the
workflow. On this page nobody prompts anything: you type commands, the
runner encodes the sequence and the phase gates, and every write lands in
a staging area you review before it touches the working tree.

**The one thing to carry through the whole run:** `spec` stages, you
approve. No authoring command writes to your files. `spec changes show`
tells you what is waiting and `spec changes commit` applies it. The two
human checkpoints in Step 3 are the whole point of the exercise.

---

## Before you start

You need:

- **`spec` on PATH.** Check with `spec --version`; the run below was done
  with `spec 0.5.2`. Install with `cargo install --path harness` from the
  repository root, or use a GitHub release binary, or
  `harness/target/release/spec`.
- **Java 21+** (`java -version`)
- **Maven 3.9+** (`mvn -version`) — the run used Maven 3.9.16.
- **[Ollama](https://ollama.com) with a model pulled**, if you want the
  model-backed commands. `spec implement`, `spec reword`,
  `spec unittest generate`, and `spec steps generate` all call it. The run
  below used `qwen3.8-flash-next:125b-mlx`:

  ```bash
  ollama pull qwen3.8-flash-next:125b-mlx
  ```

  Without a model you can still do the whole workshop — you implement
  `StringCalculator.java` by hand after each RED bar, and the generating
  commands fall back to deterministic templates. `spec reword` becomes a
  plain wizard with no proposals.
- **This repository cloned**, and a branch of your own. Never work on
  `trunk`.

Then run the preflight from the repository root:

```bash
scripts/preflight.sh
```

**Expect:** ten checks and the closing line

```text
Result: 10 passed, 0 failed.
```

It checks Java, Maven, `spec`, the Maven build, the Cucumber surefire
report, an end-to-end MCP run, that the REQ-003 demo has not been
burned by an earlier rehearsal, that `smoke-test.jar` was built, and that
the slide deck is present. Any FAIL line names the fix. If it tells
you `REQ-003 status is 'implemented'` or that the feature file already has
an `@REQ-003` scenario, you have leftovers from a previous run: reset with
`git checkout -- kata requirements`.

### Which commands call the model, and which do not

This distinction matters more on this path than on any other, because you
are the one waiting at the prompt. A command that calls the model prints a
`working ...` spinner and then goes quiet for a while. Do not reach for
Ctrl-C.

| Command | Model? | Budget |
| --- | --- | --- |
| `spec validate`, `spec list`, `spec show`, `spec refine`, `spec test` | No | instant |
| `spec draft` with all flags, `spec scenario add`, `spec include add` | No | instant |
| `spec changes show` / `commit` / `discard`, `spec refactor`, `spec mark-implemented` | No | instant |
| `spec steps missing` | No | instant |
| `spec reword` | Yes — one call per finding | 15–70 s per finding |
| `spec unittest generate` | Yes — one call | 30–45 s |
| `spec steps generate` | Yes — one call | about 35 s |
| `spec implement` | Yes — one call, large prompt | 70–195 s |

`spec refine` deserves a second look on that list: the wording review is
a **deterministic** rule set, not a model. Same input, same findings,
every time. Only the *repair* (`spec reword`) calls a model.

Timings above are from the observed run against a local model on a laptop.
They vary widely — five findings in one `spec reword` finished in about 50
seconds, while a single finding on another requirement took about 70. Ten
seconds to three minutes is all normal.

---

## Step 1 — Branch and baseline

```bash
git checkout -b workshop-spec trunk
spec model use qwen3.8-flash-next:125b-mlx
spec validate
spec list
spec test
```

`spec model use` writes the model into `.spec.toml` and prints where it
went:

```text
Configured model: qwen3.8-flash-next:125b-mlx
Written to: /path/to/tdd-bdd-agentic/.spec.toml
```

Do this even if you plan to implement by hand. Skipping it does **not**
leave you without a model — `spec` falls back to the first model Ollama
lists and announces it as a session-only default:

```text
Model set for this session: qwen3.6:35b-mlx (not saved - keep it with: spec model use qwen3.6:35b-mlx).
```

That is the trap. The first installed model is whatever Ollama happens to
return first, which is usually not the one this workshop was written
against, and nothing stops the run — you just get different quality for
an hour and no warning beyond that one line. `spec model use` pins it, and
`spec model current` tells you at any point which model resolved and where
it came from. Deterministic templates only take over when Ollama is
unreachable or has no models at all.

**Expect, in order:**

1. `spec validate` → the spec on disk is structurally sound:

   ```json
   {
     "valid": true,
     "issues": [],
     "nextStep": "The spec is valid. Run spec list, pick a pending requirement, and write its Gherkin scenario (spec scenario add)."
   }
   ```

2. `spec list` → six requirements. REQ-001 and REQ-002 are
   `implemented` (the worked example that ships green), REQ-003 through
   REQ-006 are `pending`, and there is no REQ-007 yet — you draft that in
   Step 2. Each row names the file it lives in:

   ```json
   [
     {
       "id": "REQ-001",
       "title": "Empty string returns zero",
       "status": "implemented",
       "file": "requirements/requirements.json"
     }
   ]
   ```

   (One row shown; you get all six.)

3. `spec test` → the baseline bar, green:

   ```json
   {
     "phase": "GREEN",
     "tests": 5,
     "failures": 0,
     "errors": 0,
     "skipped": 0,
     "failureDetails": [],
     "nextStep": "All tests pass. Either call start_refactor to clean up, or call get_requirement for the next pending requirement and write a failing test for it."
   }
   ```

   Five tests is 2 JUnit tests plus 3 Cucumber scenarios. One bar, two
   altitudes — that is the whole idea of the kata, and every count from
   here on is both layers added together.

If `spec test` is red here, stop and fix the build before going further.
Nothing downstream means anything on a broken baseline.

---

## Step 2 — Exercise 1: draft REQ-007

Exercise 1 agrees on a requirement. It does **not** write scenarios or
code, and REQ-007 stays `pending` until the homework in Step 4.

```bash
spec draft \
  --title "Custom delimiter declared on the first line" \
  --story "As a calculator user, I want to declare a custom delimiter on the first line so that I can separate numbers with a character of my choosing." \
  --criterion 'Given the input "//+\n1+2", when add is called, then the result is 3' \
  --criterion 'Given an empty delimiter declaration "//\n1+2", when add is called, then an IllegalArgumentException is thrown'
```

Worth calling out before you run it: **with `--title`, `--story`, and at
least one `--criterion`, `spec draft` is fully non-interactive.** No
wizard, no prompts, no terminal required. It assigns the next id, checks
the structure, and stages. That makes it the one authoring command you can
safely put in a script. (`spec draft` with *no* flags is the interactive
wizard, and it does need a terminal.) Supply one of those three and you
must supply all three: a partial set is a hard error — `spec draft --title
requires --story and --criterion` — not a fallback to the wizard.

**Expect:**

```json
{
  "id": "REQ-007",
  "title": "Custom delimiter declared on the first line",
  "staged": true,
  "nextStep": "Review with spec changes show and apply with spec changes commit, then add the @REQ-007 scenario with spec scenario add."
}
```

Note the two characters `\n` inside the criterion, and the single quotes
around the flag value that keep your shell from touching them. The spec
writes an input newline as the literal two-character escape — REQ-005
already does the same with `"1\n2,3"` — because a requirement's fields are
single-line. Keep that convention; several later steps depend on it.

Nothing in REQ-001 through REQ-006 mentions a custom delimiter, so this
draft earns no duplicate warning. Reuse an existing title or criterion
word for word and `spec draft` prefixes the `nextStep` with a warning
instead of refusing — read it and decide.

Now review and apply:

```bash
spec changes show
spec changes commit
```

`spec changes show` is the review surface. It lists the staged manifest —
one entry per file, with the action and a summary of the edits that
produced it:

```json
{
  "changes": [
    {
      "path": "requirements/requirements.json",
      "action": "modify",
      "summary": "draft REQ-007: Custom delimiter declared on the first line"
    }
  ],
  "nextStep": "Review the staged files, run validate, then apply with changes commit or drop with changes discard."
}
```

The staged **bytes** live beside the manifest, under
`.spec-staged/files/` mirroring the project layout — so the file above is
at `.spec-staged/files/requirements/requirements.json`. Open it when you
want to read the exact content before approving it. Your working tree is
untouched until `spec changes commit`.

Then have the wording reviewed:

```bash
spec refine REQ-007
spec list
```

**Expect** a clean review on the first call:

```json
{
  "id": "REQ-007",
  "clean": true,
  "findings": [],
  "nextStep": "The wording reads clean. Confirm it with the developer, then write the Gherkin scenario from the acceptance criteria."
}
```

That is worth a pause, because the agent path usually shows a findings
round here. It came back clean because the draft above already carries an
**edge case** — the second criterion is the thrown
`IllegalArgumentException`, not another happy path. The most common
refine finding on a fresh draft is `criteria: only happy paths - add at
least one edge case (empty, invalid, or error input)`, and supplying one
up front is how you avoid it.

`spec list` now shows seven ids, with REQ-007 `pending`.

**Your checkpoint:** read the story and the two criteria out loud. Is this
what you meant? You own the intent. Approving does not change `status` —
leave it `pending`.

The three demos below are optional and each takes a couple of minutes.
They are the most instructive part of Exercise 1, so do them if you have
the time.

For both demo A and demo B, **you** make the breaking edit by hand. Do not
ask an agent to write bad wording for you: an agent asked to write a bad
story tends to fix it on the way to disk, the tool then correctly reports
that everything is fine, and the demo never fires. Human breaks the spec,
tool catches it, tool repairs it.

---

## Demo A — the structure loop (`spec validate`)

Structure is checked first, and it is checked deterministically.

1. Open `requirements/requirements.json` and edit REQ-007's **first**
   acceptance criterion by hand to exactly this, then save:

   ```text
   the result should be 3 for //+\n1+2
   ```

2. Run the validator:

   ```bash
   spec validate
   ```

**Expect** a refusal that names the requirement, quotes the criterion back
at you, and says what is missing:

```json
{
  "valid": false,
  "issues": [
    "REQ-007: criterion \"the result should be 3 for //+\\n1+2\" must be phrased Given/When/Then"
  ],
  "nextStep": "Run spec reword to fix the issues, then run spec validate again."
}
```

That is deterministic output — you will see it byte for byte.

### Repairing it with the wizard

```bash
spec reword REQ-007
```

`spec reword` **is** an interactive wizard, and its shape surprises people
the first time, so here is the whole thing.

It asks **two passes** of the same prompts. That is by design, and the
tool narrates why. The opening line:

```text
Rewording REQ-007. You word the spec; validate and refine findings drive rewording until the wording is clean.
```

**Pass 1 shows your current wording**, so you can fix it yourself without
a model ever being involved. Each prompt carries the existing value in
square brackets and Enter keeps it:

```text
REQ-007 title [Custom delimiter declared on the first line] (Enter keeps it):
REQ-007 story (As a ..., I want ..., so that ...) [As a calculator user, I want to declare ...] (Enter keeps it):
Acceptance criteria (Given/When/Then). A blank criterion ends the list:
REQ-007 criterion 1 [the result should be 3 for //+\n1+2] (Enter keeps it, '-' drops it):
REQ-007 criterion 2 [Given an empty delimiter declaration "//\n1+2", ...] (Enter keeps it, '-' drops it):
REQ-007 criterion 3 (leave blank to finish the criteria):
```

Press Enter through all of them and the wizard re-checks what you handed
back. It is still broken, so it prints the findings with a repair hint
each, structural ones first, then calls the model **once per finding**:

```text
Findings to address:
  - REQ-007: criterion "the result should be 3 for //+\n1+2" must be phrased Given/When/Then
    try: rephrase as: Given <starting state>, when <action>, then <exact result> - e.g. Given the input "1,2", when add is called, then the result is 3
  - criterion "the result should be 3 for //+\n1+2": 'should' is ambiguous - state exactly what happens
    try: replace the vague word with the exact observable behavior, e.g. 'the result is 3'
Asking qwen3.8-flash-next:125b-mlx to address finding 1 of 2 - working ...
Asking qwen3.8-flash-next:125b-mlx to address finding 2 of 2 - working ...
```

One bad edit earned two findings: the structure rule caught the missing
Given/When/Then, and the wording rule caught `should`. They are separate
gates on purpose — see demo B — and here they happened to fire together.

Then it hands you the result:

```text
The model reworded the draft. Each prompt shows its proposal - Enter accepts it, or type your own wording.
```

**Pass 2 shows the model's proposals** in the same brackets. Enter accepts
each one; type over it to use your own wording instead. The wording is
yours either way — the model is proposing, not deciding.

Pass 2 ends with a single confirmation:

```text
The wording reads clean. Stage this requirement? [y/N]
```

Answer `y`. At a terminal the whole run is: Enter through pass 1, Enter
through pass 2, then `y`.

**Expect** `"staged": true` in the reply. Then apply it and re-validate:

```bash
spec changes commit
spec validate
```

The commit summary reads `reword REQ-007`, and `spec validate` is back to
`"valid": true`.

One aside worth noticing: in the observed run the repaired criterion came
back as exactly

```text
Given the input "//+\n1+2", when add is called, then the result is 3
```

byte-identical to what you drafted in Step 2, so `git status` showed no
net change against the pre-demo commit. The `\n` survived as the
two-character escape rather than becoming a real newline — model replies
are re-escaped before they reach the spec, precisely so a reword cannot
quietly break the convention the rest of the file uses. Yours will be
similar, not necessarily identical.

### If you are scripting this

Piping answers into `spec reword` works, but it is fragile, and the
runner warns you on **stderr** the moment it notices stdin is not a
terminal:

```text
stdin is not a terminal: prompts are read from the pipe, and once it runs out every remaining prompt takes its default and every confirmation declines. A wizard that ends in "Stage this?" therefore stages nothing. Run this in a terminal to answer the prompts.
```

The warning goes to stderr specifically so a caller parsing the JSON on
stdout still can. Read it literally: a read past the end of the pipe
returns an empty line, which the prompter cannot tell apart from you
pressing Enter. So you must supply an answer for **every** prompt,
including the final confirmation:

```bash
printf '\n\n\n\n\n\n\n\n\n\ny\n' | spec reword REQ-007
```

Get the count wrong in either direction and the confirmation reads a blank
line, declines, and stages nothing — with exit code 0:

```json
{
  "id": "REQ-007",
  "title": "Custom delimiter declared on the first line",
  "staged": false,
  "nextStep": "Nothing was staged. Run spec reword REQ-007 again when the wording is ready."
}
```

And the count is genuinely not fixed: pass 2 offers one prompt per
criterion the **model** proposes, plus one blank-terminated extra. If it
adds an edge case you now have one more prompt than pass 1 had. For
anything unattended, skip the wizard instead — `spec reword` takes the
same flags as `spec draft`:

```bash
spec reword REQ-007 \
  --title "Custom delimiter declared on the first line" \
  --criterion 'Given the input "//+\n1+2", when add is called, then the result is 3' \
  --criterion 'Given an empty delimiter declaration "//\n1+2", when add is called, then an IllegalArgumentException is thrown'
```

That is deterministic, non-interactive, and stages immediately.

---

## Demo B — the wording loop (`spec refine`)

Structure passing does not make a requirement good. `spec refine` is the
second gate, and it has opinions about prose.

1. Open `requirements/requirements.json` and replace REQ-007's **story**
   by hand with exactly this, leaving the criteria alone:

   ```text
   the calculator should handle custom delimiters quickly
   ```

2. Review it:

   ```bash
   spec refine REQ-007
   ```

**Expect** exactly five findings — the missing actor, the missing why, and
each ambiguous word called out separately. This is the deterministic rule
set, so if you typed the story exactly as above you get this reply byte
for byte:

```json
{
  "id": "REQ-007",
  "clean": false,
  "findings": [
    "story: missing the actor - start with 'As a ...' so we know who this is for",
    "story: missing the why - finish with 'so that ...' so the value is explicit",
    "story: 'should' is ambiguous - describe the observable behavior instead",
    "story: 'handle' is ambiguous - describe the observable behavior instead",
    "story: 'quickly' is ambiguous - describe the observable behavior instead"
  ],
  "nextStep": "Run spec reword REQ-007 to address each finding, then run spec validate and spec refine REQ-007 again. Iterate until there are no findings."
}
```

Note that `spec validate` would still pass on this story. Structure and
wording are two separate gates, and this is why.

3. Repair, apply, re-check:

   ```bash
   spec reword REQ-007
   spec changes commit
   spec refine REQ-007
   ```

Five findings means **five model calls** — one per finding, each narrated
as `Asking <model> to address finding N of 5 - working ...`. In the
observed run the whole sequence took about 50 seconds. Then the same two
passes of prompts as demo A, then `y`.

`spec refine REQ-007` now comes back `"clean": true`.

The story the model produces is usually verbose — the observed one ran to
a single long sentence with an inline example. That is fine, and it is
also yours to overrule: type your own wording at the pass-2 story prompt
instead of pressing Enter. The tool grades structure and wording rules,
never style.

If the model cannot get the wording clean, the wizard stops looping and
asks you instead — after three passes, or sooner if a pass earns exactly
the same findings as the one before it:

```text
The wording review is not converging - 3 pass(es) left 2 finding(s) open.
Choose [r]eword again, [m]anual rewording without the model, [a]ccept as-is and stage [r/m/a, Enter for r]:
```

`a` stages the requirement with the open findings recorded in the reply,
and the `nextStep` reminds you that `spec reword REQ-007` can revisit
them. This escape hatch exists for **wording** findings only. A
structurally invalid requirement — demo A's case — keeps the loop honest
however long it takes, because a spec that fails `spec validate` is not
usable at all.

---

## Demo C — the spec is a catalog (includes)

`requirements/requirements.json` is always the entry point, but it does
not have to hold every requirement. It can carry an `includes` list of
child spec files, those children can include further files, and the tools
merge the whole tree into one backlog. Ids stay unique across the tree.

1. Cut the **whole REQ-007 object** out of
   `requirements/requirements.json` and paste it into a new file,
   `requirements/delimiters.json`:

   ```json
   {
     "requirements": [
       { "...": "the REQ-007 object you cut" }
     ]
   }
   ```

2. Add the include to `requirements/requirements.json`, right after
   `"description"`:

   ```json
   "includes": ["delimiters.json"],
   ```

3. Read the merged view:

   ```bash
   spec list
   spec validate
   ```

**Expect** all seven ids from `spec list`, with REQ-007 merged in last and
its `file` field reading `requirements/delimiters.json`, and
`"valid": true` from the validator. One catalog, many files.

Two failure modes are worth provoking, because the messages name the file:

- Leave REQ-007 in **both** files and `spec validate` answers
  `REQ-007: duplicate id - also declared in requirements.json`.
- Make two files include each other and it answers
  `spec: requirements.json is included more than once - include every spec file exactly once`.

Undo those two, but **leave the split itself in place if you like.**
Everything in the rest of this page works identically on a split catalog —
that was verified end to end, including `scripts/verify-workshop-run.sh
check`, which reads the merged tree and still scored 7/7 with the split
still in place.

The harness ships a command for this too, which you will use in the
stretch at the end: `spec include add requirements/delimiters.json` stages
both the include line and an empty child file. The hand-editing above is
only to show you the shape.

---

## Step 3 — Exercise 2: take REQ-003 to `implemented`

This is the full Red/Green/Refactor loop on one requirement, with two
places where you and only you decide whether to continue.

Start by reading the requirement:

```bash
spec show REQ-003
```

**Expect** a reply that is **not** a copy of the JSON on disk. The server
enriches it: `featureFile` comes back as `featureLocation`, and
`stepDefinitions`, `testLocation`, `productionLocation`, and
`workflowHint` are added so you know where every artifact belongs before
you write anything:

```json
{
  "id": "REQ-003",
  "title": "Two numbers separated by a comma are summed",
  "status": "pending",
  "story": "As a user, I want comma-separated numbers to be summed so that I can add multiple values at once.",
  "acceptanceCriteria": [
    "Given \"1,2\", when add is called, then the result is 3",
    "Given \"10,20\", when add is called, then the result is 30"
  ],
  "featureLocation": "kata/src/test/resources/features/string_calculator.feature",
  "stepDefinitions": "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java",
  "testLocation": "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java",
  "productionLocation": "kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java",
  "workflowHint": "Write the Gherkin scenario for this requirement in the feature file first (tag it @REQ-003), reuse or add step definitions, then run_tests to see RED."
}
```

None of those paths is configured anywhere. The harness discovered them
by scanning the project, and `spec inspect` re-runs the scan if you are
curious.

### The BDD altitude: one scenario per criterion

REQ-003 has two acceptance criteria, so it gets two scenarios.

```bash
spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Two numbers separated by a comma are summed" \
  --step 'Given a string calculator' \
  --step 'When I add "1,2"' \
  --step 'Then the result is 3'

spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Two larger numbers separated by a comma are summed" \
  --step 'Given a string calculator' \
  --step 'When I add "10,20"' \
  --step 'Then the result is 30'
```

Each returns `"staged": true`:

```json
{
  "feature": "kata/src/test/resources/features/string_calculator.feature",
  "scenario": "Two larger numbers separated by a comma are summed",
  "action": "add",
  "staged": true,
  "nextStep": "Review with changes show, run validate, then apply with changes commit."
}
```

Those four step wordings are the ones the kata already binds, so nothing
new is needed at the step-definition layer. Prove it if you like:

```bash
spec steps missing
```

```json
{
  "language": "Java",
  "framework": "Cucumber-JVM",
  "missing": [],
  "nextStep": "Every step has a definition. Run spec test to execute the suite."
}
```

**Checkpoint 1, and it is yours:**

```bash
spec changes show
```

Two `spec scenario add` calls on the same feature file produce **one**
staged entry — one file is one entry — and its summary names **both**
edits, joined with `"; "`:

```json
{
  "changes": [
    {
      "path": "kata/src/test/resources/features/string_calculator.feature",
      "action": "modify",
      "summary": "add scenario \"Two numbers separated by a comma are summed\" for REQ-003; add scenario \"Two larger numbers separated by a comma are summed\" for REQ-003"
    }
  ],
  "nextStep": "Review the staged files, run validate, then apply with changes commit or drop with changes discard."
}
```

That cumulative summary is the point of the checkpoint: it tells you you
are approving two scenarios, not one. Open
`.spec-staged/files/kata/src/test/resources/features/string_calculator.feature`
and read the Gherkin itself before you commit. Is that the behavior you
want? This is the spec review, and it is the cheapest place in the whole
loop to change your mind.

While you are in there, notice what the scenario add did *not* disturb:
the feature file's header comment block and the
`As a / I want / So that` narrative under the `Feature:` line come through
staging untouched. Authoring commands append; they do not rewrite.

### The TDD altitude: a failing unit test

```bash
spec unittest generate REQ-003
```

This one calls the model. Give it 30 to 45 seconds.

**Expect** a staged addition to the existing `StringCalculatorTest.java` —
not a new `Req003Test` class:

```json
{
  "target": "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java",
  "staged": true,
  "source": "llm",
  "summary": "generate failing unit test for REQ-003 (2 criteria, llm)",
  "nextStep": "Review the assertions (they are yours to sharpen), apply with spec changes commit, then run spec test (expect RED)."
}
```

`"source": "llm"` means the model's polished version passed validation;
`"template"` means it fell back to the deterministic template, which is
equally valid. Across five clean-cache repeats of each generate command,
all twelve runs came back `"llm"` — the fallback never fired once. Treat
`"template"` as unusual rather than expected, but not as a problem when it
happens. As with `spec steps generate`, the model is handed only the
methods being added, never the test class around them, so your existing
REQ-001 and REQ-002 tests are not in its context and cannot be rewritten.

Either way you get two `@Test` methods, one per criterion, whose bodies
are placeholders:

```java
fail("TODO: assert - Given \"1,2\", when add is called, then the result is 3")
```

Those are deliberate. The criterion is copied into the test as a
`@DisplayName` and a comment, and the assertion is left for you — that is
what "the assertions are yours to sharpen" means in the `nextStep`.

**The command stays inside the requirement you name.** Five runs of
`spec unittest generate REQ-005` each produced REQ-005's two tests and
nothing else, both `fail("TODO: assert - ...")` placeholders intact. The
only `REQ-006` in the resulting diff is the `// REQ-005 .. REQ-006:`
comment that was already in the file. Afterwards Maven reports 15 tests
with 2 failures — the two placeholders, exactly the RED you asked for.

**Do not expect the diff to be identical between runs.** One of those five
came back 13 added lines instead of 15, because the model dropped the
`// criterion` comment above each placeholder. The gates check the `@Test`
count and that every placeholder survived; they do not check that comment.
That is a deliberate place to stop: tightening the gate would start
refusing good replies and falling back to the template over a cosmetic
difference, and nothing is actually lost, because the `@DisplayName`
carries the same criterion text. If you are regenerating live in front of
a room, know that the output can differ slightly from the one you
rehearsed.

### RED

```bash
spec changes commit
spec test
```

**Expect** a red bar with nine tests and four failures:

```json
{
  "phase": "RED",
  "tests": 9,
  "failures": 4,
  "errors": 0,
  "skipped": 0,
  "failureDetails": ["..."],
  "nextStep": "Tests are failing. Write the simplest production code that makes them pass, then call run_tests again."
}
```

Nine is the baseline five plus two new Cucumber scenarios plus two new
JUnit tests. The four failures are two different kinds, and both are
legitimate RED:

- The two **Cucumber** scenarios fail with
  `java.lang.NumberFormatException: For input string: "1,2"` and the same
  for `"10,20"`. `StringCalculator.add` still does a single
  `Integer.parseInt`, so a comma-separated string blows up in it. That is
  the behavior REQ-003 asks for, absent.
- The two **JUnit** tests fail on the `TODO: assert` placeholders you just
  read. That is the assertion you have not written yet.

Both will go green from the same change.

`spec test` runs Maven against your **working tree**, not the staging
area. That is why `spec changes commit` comes first. Commit before you
trust the bar.

### Implement

```bash
spec implement REQ-003
```

This is the slowest command on the path — budget one to three minutes; the
observed run took about two. It narrates as it goes:

```text
REQ-003: checking prerequisites - phase RED, 4 recorded failure(s), 0 prior attempt(s).
  scenario tagged @REQ-003: kata/src/test/resources/features/string_calculator.feature - present
  step definitions (every step defined): kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java - present
  unit test: kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java - present
  production code (the attempt creates it when missing): kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java - present
Sending the sources, the failures, and the attempt history to the model - working ...
```

That preflight is worth reading rather than skipping. If any asset shows
`missing`, `spec implement` stops there and hands back a readiness report
naming the command that fills the gap — it will not ask a model to invent
an implementation for a requirement with no failing test behind it.

The failing tests with their stack traces *are* the brief. So is every
prior attempt on this requirement — a second `spec implement REQ-003`
after a still-red bar tells the model what it already tried and why the
bar stayed red.

**Expect** three staged files, first as plain lines and then as the JSON
report:

```text
  staged: kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java
  staged: kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java
  staged: kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java
```

```json
{
  "targets": [
    "kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java",
    "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java",
    "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java"
  ],
  "staged": true,
  "source": "llm",
  "nextStep": "Apply with spec changes commit, then spec test - the run decides."
}
```

Yours will be similar, not identical. Two things about that list are worth
knowing:

- It touches the **test** files as well as production, because it replaces
  the `fail("TODO: assert")` placeholders with real assertions. That is
  the one place in this workflow where a model edits a test, and it is why
  checkpoint 2 below matters.
- `spec implement` may offer to run a command first, and ask before it
  spawns:

  ```text
  Run command_run(command=["cat","kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java"])? [y/N]
  ```

  Answer `y` or `N`; either way `spec implement` still stages. On piped or
  CI stdin the prompt declines automatically and never hangs.
  `command_run` is the only mutation a harness-side model may even
  request, and it is confined to the implement profile.

### One prompt to say no to

After it prints the JSON, `spec implement` offers to finish the loop for
you:

```text
Apply the staged files and run the tests now? [y/N]
```

**Answer `N`.** Saying `y` commits the staged files and runs the tests
immediately, which is convenient on the fifth requirement and skips
exactly the review you came here to practice. Declining prints the two
commands in plain words:

```text
Next: changes commit && test - then implement REQ-003 again if the bar stays RED.
```

On piped stdin the prompt is skipped and you get that same line. Once you
trust the loop, `y` is a real time-saver: it prints the commit, then the
test run, and then either `GREEN - next: refactor (optional), then
mark-implemented REQ-003 && changes commit.` or `Still RED - the fresh
failures are recorded; run implement REQ-003 for another model attempt, or
implement by hand and rerun test.`

**Checkpoint 2, and it is yours:**

```bash
spec changes show
```

Read the production diff. The observed implementation kept the
empty-string guard from REQ-001 and replaced the single `Integer.parseInt`
with a split-on-comma loop behind a `COMMA` constant — the simplest thing
that could pass, which is exactly right. If yours reaches for a regex
engine or a stream pipeline on the first pass, that is a conversation
worth having with yourself before you commit it.

`spec changes discard` drops the whole batch if you would rather write it
yourself. The working tree is untouched either way.

### GREEN, refactor, mark implemented

```bash
spec changes commit && spec test
```

**Expect** GREEN at the same total: nine tests, zero failures. Same bar,
both altitudes.

```bash
spec refactor --note "extract comma delimiter constant"
```

```json
{
  "phase": "REFACTOR",
  "nextStep": "A refactor is in progress. Call run_tests to prove the refactor kept the bar green."
}
```

Do the cleanup, then prove it:

```bash
spec test
```

GREEN, nine tests, zero failures. A refactor that changes the bar was not
a refactor.

```bash
spec mark-implemented REQ-003
spec changes commit
```

```json
{
  "id": "REQ-003",
  "status": "implemented",
  "staged": true,
  "nextStep": "Review with changes show, run validate (it checks the @REQ-003 scenario exists), then changes commit."
}
```

Notice this stages too. Even the status flip goes through review. And
notice what it recorded while it was there: `mark-implemented` writes the
`featureFile` of the `@REQ-003`-tagged feature back into the requirement,
which is what makes the staged spec validate.

That is the loop. Everything left is repetition.

---

## Step 4 — Homework: REQ-004, REQ-005, REQ-006, then REQ-007

Same recipe every time. Substitute the id and the scenarios:

```bash
spec show REQ-00N

spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-00N --name "<first scenario name>" \
  --step 'Given a string calculator' \
  --step 'When I add "<input>"' \
  --step 'Then the result is <n>'
# one more scenario add per acceptance criterion

spec steps missing                 # empty? good. otherwise: spec steps generate
spec unittest generate REQ-00N
spec changes show                  # checkpoint 1
spec changes commit
spec test                          # expect RED

spec implement REQ-00N             # or edit StringCalculator.java by hand
spec changes show                  # checkpoint 2
spec changes commit && spec test   # expect GREEN

spec refactor --note "<what>" && spec test    # optional, GREEN only
spec mark-implemented REQ-00N
spec changes commit
```

The scenarios used in the verified run, and the bar at each stage:

| Id | Scenarios used | RED | GREEN |
| --- | --- | --- | --- |
| REQ-004 | "Any amount of numbers is summed" `"1,2,3,4,5"` → 15; "All zeros sum to zero" `"0,0,0"` → 0 | 13 tests, 2 failures | 13, 0 |
| REQ-005 | "Newlines work as delimiters alongside commas" `"1\n2,3"` → 6; "Newlines alone delimit numbers" `"4\n5\n6"` → 15 | 17 tests, 4 failures | 17, 0 |
| REQ-006 | "A negative number is rejected" `"1,-2"`; "Every negative number is listed in the error" `"-1,-2"` | 21 tests, 4 failures | 21, 0 |
| REQ-007 | "A custom delimiter declared on the first line is used" `"//+\n1+2"` → 3; "An empty delimiter declaration is rejected" `"//\n1+2"` | 25 tests, 3 failures, 1 error | 25, 0 |

Five things in that table need explaining, and each is a small lesson.

**REQ-004's RED is only two failures, not four.** REQ-003's
split-and-sum loop already handles any number of comma-separated values,
so both new Cucumber scenarios pass the moment they exist and only the two
unit-test placeholders fail. That is still a legitimate RED — there is a
failing test naming the criterion, and it goes green when you fill in the
assertion. It is also the most honest thing that happens all hour: the
behavior REQ-004 asks for was already implemented, and the spec is only
now catching up. Be glad the bar told you instead of the model claiming
credit for work REQ-003 did.

**Gherkin cannot hold a real newline inside `"..."`.** Write the two
characters `\n` in the `When I add` string, exactly as the spec does:

```gherkin
When I add "1\n2,3"
```

`StringCalculatorSteps` unescapes it before calling `add`. This is the
same convention as the criteria in Step 2, and it is why REQ-005 and
REQ-007 look odd on the page.

**REQ-006's two `Then` steps.** The first scenario ends with

```gherkin
Then an IllegalArgumentException is thrown with a message containing "negatives not allowed"
```

and the second asserts both negatives with a `Then` and an `And` naming
`"-1"` and `"-2"`. REQ-006 needs **no** new step definition —
`spec steps missing` returns `"missing": []`, because
`Then an IllegalArgumentException is thrown with a message containing {string}`
is already bound. Prefer the wordings the kata already knows and
`spec steps generate` stays a no-op.

**REQ-007's second scenario is the one wording with no step definition.**
Its `Then` is the bare

```gherkin
Then an IllegalArgumentException is thrown
```

with no `with a message containing`. That is a real gap, and
`spec steps missing` reports it — the keyword (`Then`), the step text, the
scenario name, and the feature file. Close it with:

```bash
spec steps generate
```

About 35 seconds. It stages the new definition into the kata's own
`StringCalculatorSteps.java` — keeping its package and class, never
writing a parallel file Cucumber would reject for duplicate expressions —
with summary `append pending step definitions for 1 missing step(s) (llm)`
and a `PendingException` body for you to fill in.

**Expect the diff to be the new method and nothing else.** Measured live:
**7 lines added, 0 removed**, against a 48-line baseline on the older
whole-file behavior described further down. Six of the seven are the
pending step definition you would predict; the seventh is a blank line,
because `spec` now separates appended members from whatever the class
already declares by exactly one blank line — collapsing a pre-existing
trailing blank so you never end up with two. The model is shown only the
definitions being added, never the file they join, so it cannot rename
your fields or your existing step methods; everything outside the
insertion point is carried over byte for byte. Its only job is to improve
the new member's name and formatting.

**That naming job is not nothing.** The deterministic template, casing the
step text on its own, produces `anIllegalargumentexceptionIsThrown`. The
model came back with `anIllegalArgumentExceptionIsThrown` — the same name
a Java developer would have typed. Across six `steps generate` runs the
annotation text was byte-identical every time and only the method name
moved, once to `thenIllegalArgumentExceptionIsThrown`. The part that has
to be exact is exact; the part that is taste is where the model earns its
35 seconds.

Commit it and run the suite: 25 tests, 1 error — the `PendingException`
you just staged — and the other 24 still green.

Three replies are refused outright, and all three fall back to the
deterministic version of the same new method — you will see
`"source": "template"` instead of `"llm"`, and the new step still lands:

- a reply that hands back a whole file with its own class declaration,
- a reply that alters one of the generated step expressions, which would
  unbind the scenario the definition was generated for,
- a reply that drops one of the definitions it was given.

On top of that, the assembled file is re-checked before staging: every
step pattern the file already declared has to still be there, along with
its package and its class. A passing scenario cannot be unbound by a
polish pass.

That is a recent change, and it is worth knowing which side of it your
binary is on. Older `spec` releases handed the model the whole file and
asked for the whole file back, and the model obliged — in an earlier
recorded run, adding this one step also renamed two fields (`result` to
`lastResult`, `thrown` to `lastThrown`), renamed an existing step method,
added constants, and introduced an inner class, for about forty-five
changed lines. If your diff looks like that, you are on an older build.
Either way the habit is the same: read it with `spec changes show`, open
the staged file, and commit deliberately.

**`+` is a regex metacharacter.** A naive `"1+2".split("+")` throws
`PatternSyntaxException: Dangling meta character '+'`. Let the RED bar
teach you that and then reach for `Pattern.quote`. Honest note: in the
observed run the model went straight to `Pattern.quote` on the first
attempt and never hit the exception, so you may not get to see it. Break
it on purpose if you want to.

One more note on the commands: `spec steps` has exactly two subcommands,
`missing` and `generate`. There is no `spec steps find`.

---

## Step 5 — Done

```bash
spec list
spec validate
spec test
mvn -f kata/pom.xml test
```

**Expect:**

- `spec list` — every id from REQ-001 to REQ-007 with
  `"status": "implemented"`. That is the success bar for this path.
- `spec validate` — `"valid": true`.
- `spec test` — GREEN, 25 tests, 0 failures.
- `mvn -f kata/pom.xml test` — the same bar from Maven directly, with no
  harness in the middle:

  ```text
  Tests run: 25, Failures: 0, Errors: 0, Skipped: 0
  ```

  That is 12 JUnit tests and 13 Cucumber scenarios, and it ends with
  `BUILD SUCCESS`. Running Maven yourself is worth the thirty seconds:
  the harness has been reporting a bar it computed from surefire reports
  all along, and this is you checking its arithmetic.

---

## Stretch — split the spec into a catalog, with a command

Demo C split the spec by hand. Here is the same thing done with the tool,
and a fresh requirement drafted straight into the child file.

```bash
spec include add requirements/newlines.json
```

```json
{
  "file": "requirements/newlines.json",
  "parent": "requirements/requirements.json",
  "created": true,
  "staged": true,
  "nextStep": "Review with spec changes show, apply with spec changes commit, then draft into it with spec draft --file."
}
```

Two staged files: the include line added to the parent, and a new empty
child. Apply them, then draft into the child:

```bash
spec changes commit

spec draft --file requirements/newlines.json \
  --title "Trailing newline in the input is ignored" \
  --story "As a calculator user, I want a trailing newline in the input to be ignored so that copy-pasted input still sums correctly." \
  --criterion 'Given "1,2\n", when add is called, then the result is 3'

spec changes commit
spec list
spec validate
```

**Expect** one merged backlog with per-file provenance. If you also kept
demo C's split, the final state is a three-file catalog:

| Ids | Status | File |
| --- | --- | --- |
| REQ-001 .. REQ-006 | implemented | `requirements/requirements.json` |
| REQ-007 | implemented | `requirements/delimiters.json` |
| REQ-008 | pending | `requirements/newlines.json` |

`spec validate` validates the whole tree as one catalog, and REQ-008 is
your next kata. The format reference lives in the manual:
[The requirements format](../manual/src/spec-format.md).

---

## Check your work

```bash
scripts/verify-workshop-run.sh check
```

**Expect all seven PASS:**

```text
  PASS  REQ-007 was drafted into the spec
  PASS  the spec is valid
  PASS  REQ-007 wording is refine-clean
  PASS  REQ-007 covers the first-line delimiter declaration
  PASS  REQ-003 status is 'implemented' in the spec
  PASS  @REQ-003 scenarios cover every acceptance criterion (2 tagged, 2 criteria)
  PASS  REQ-003 unit test asserts every acceptance criterion (2 @Test naming REQ-003)
```

The first four grade Exercise 1, the last three grade Exercise 2. This was
verified with demo C's split **and** the stretch's second child file still
in place: the verifier reads the merged tree, so a split catalog scores
the same 7/7.

Note what is not graded: your wording, anywhere. The REQ-007 paragraph you
and the model settled on is yours, so the verifier asks the same two
questions you asked in Exercise 1 — does `spec validate` pass, does
`spec refine` come back clean — rather than diffing your prose against
someone else's. Any FAIL line names the artifact to revisit and which
criterion is unaccounted for.
[../student-follow-along.md](../student-follow-along.md) walks through the
two most common partial results in detail.

---

## The gates, if you want to see them refuse

The discipline lives in the tool, not in a prompt. Four refusals are worth
provoking once, so you know they are real. Do it in a throwaway worktree
so you do not disturb your run:

```bash
git worktree add -b spec-gates /tmp/spec-gates workshop-spec
cd /tmp/spec-gates
```

The `-b spec-gates` matters: git will not check the same branch out in two
worktrees, so this cuts a scratch branch from your run instead. The gates
read the phase from `.spec-state.json`, which is per-directory, so the new
worktree starts at phase `START` — run `spec test` once to establish a bar
before you try to trip anything.

That first bar is GREEN, because you cut the worktree from a finished run.
Two of the four gates need RED, so stage a scenario for behavior nobody
implemented and commit it:

```bash
spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Gate demo: a behavior nobody implemented" \
  --step 'Given a string calculator' \
  --step 'When I add "9,9"' \
  --step 'Then the result is 99'
spec changes commit
spec test                     # RED
```

All four messages are exact.

**Refactoring on a red bar.**

```bash
spec refactor --note "tidy up"
```

```text
Refactoring is only allowed from GREEN (current phase: RED). Never refactor on a red bar — make the tests pass first.
```

**Marking implemented on a red bar.**

```bash
spec mark-implemented REQ-003
```

```text
Requirements are only marked implemented on GREEN (current phase: RED). Run the tests and make them pass first.
```

**Marking implemented with no executable scenario.** Delete the
`@REQ-003` scenarios from the feature file, get back to GREEN, then:

```bash
spec mark-implemented REQ-003
```

```text
No scenario is tagged @REQ-003 - implemented requirements need an executable scenario. Add one with spec scenario add, apply it with spec changes commit, then mark REQ-003 implemented.
```

That one is the interesting gate. A green bar plus a `pending` status is
an ordinary state; a green bar plus `implemented` and no scenario would be
a claim with nothing executable behind it, so the tool will not write it.

**Running a command outside the implementation phase.** On GREEN:

```bash
spec mcp call command_run --args '{"command":["mvn","-v"]}'
```

```text
Commands only run during the implementation phase — a RED bar (current phase: GREEN). Call run_tests first; failing tests are what an implementation command is for.
```

Clean up when you are done:

```bash
cd -
git worktree remove --force /tmp/spec-gates
git branch -D spec-gates
```

---

## Reset / start over

Everything the exercises touched lives in `kata/` and `requirements/`:

```bash
spec changes discard                  # drop anything still staged
git checkout -- kata requirements     # rewind this branch to the start state
git clean -fd requirements            # drop spec files you added (delimiters.json, newlines.json)
```

Or throw the branch away and cut it again:

```bash
git checkout trunk && git branch -D workshop-spec && git checkout -b workshop-spec trunk
```

The runner's own scratch files are all gitignored and safe to delete at
any time: `.spec-staged/` (the staging area), `.spec-state.json` (the TDD
phase log), `.spec-cache/`, `.spec-log/`, and `.spec-memory.json` (the
project-layout discovery memory). Deleting `.spec-state.json` resets the
phase to `START`, which means `spec refactor` and `spec mark-implemented`
will refuse until you run `spec test` once. That is not a bug; it is the
same gate as everything else.

Do not merge kata completion to `trunk`. `scripts/check-workshop-start.sh`
has to keep passing there.

---

## If you get stuck

- **`spec` not found** — `cargo install --path harness` from the
  repository root, or put `harness/target/release/spec` on your PATH.
  `scripts/preflight.sh` tells you which of those it found.
- **A model-backed command seems hung** — check the budgets in the table
  near the top of this page. `spec implement` legitimately takes two to
  three minutes. Confirm Ollama is up with `ollama list`, and that
  `spec model use` recorded your model (`spec config` prints the resolved
  configuration and where each value came from).
- **A command says nothing was staged** — you were probably on piped
  stdin. Look for the warning on stderr, and run it in a terminal or use
  the flag form.
- **The bar is not what you expected** — `spec test` reads the working
  tree, so `spec changes commit` first. `spec state` shows the current
  phase and last run without touching anything.
- **A refusal you did not expect** — read it. Every one of them names the
  command that gets you unstuck.

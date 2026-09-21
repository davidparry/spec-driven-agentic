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

## How to read this page

Two labels run through every step, and they mean different things:

| Label | What it is |
| --- | --- |
| **Do this** | Commands you type, in the order they appear. This is the run. |
| **Expect** | What the command prints back. Most of it is deterministic, so compare it against yours. |

Everything not under one of those two labels is explanation. It is worth
reading, but none of it is an instruction — so from Step 1 onward, **if a
code block is not under "Do this", do not type it.** Unlabeled blocks are
output, file contents, or an aside.

What you actually have to do:

- **Steps 1, 2 and 3 are the workshop.** They are the hands-on hour.
- **[Step 4](#step-4--check-your-work) scores the two exercises**, and it
  passes as soon as Step 3 is done. That is the bar, and it is where the
  workshop ends.
- **[Step 5](#step-5--homework) is homework** — the remaining four
  requirements and the final green bar, on your own time. Nothing there is
  graded.
- **[Optional extras](#optional-extras)** at the end are after-class
  material. Nothing depends on them and nothing grades them.

The last two sections, [Reset / start over](#reset--start-over) and
[If you get stuck](#if-you-get-stuck), are recovery recipes rather than
part of the run. Use what applies.

[spec-binary-steps.md](spec-binary-steps.md) is the same run as a numbered
list of bare commands, one per step.

---

## Before you start

You need:

- **`spec` on PATH.** Check with `spec --version`; the run below was done
  with `spec 0.5.2`. Install with `cargo install --path harness` from the
  repository root, or use a GitHub release binary, or
  `harness/target/release/spec`.
- **Java 21+** (`java -version`)
- **Maven 3.9+** (`mvn -version`) — the run used Maven 3.9.16.
- **[Ollama](https://ollama.com) with a model pulled.** `spec draft`,
  `spec scenario generate`, `spec implement`, `spec reword`,
  `spec unittest generate`, and `spec steps generate` all call it. The
  run below used `qwen3.8-flash-next:125b-mlx`:

  ```bash
  ollama pull qwen3.8-flash-next:125b-mlx
  ```

  Without a model you can still do the whole workshop, but Steps 2 and 3
  become different exercises. `spec draft` falls back to the classic
  prompts — title, story, then each criterion in turn, instead of opening
  on the plain-words description — so you author the requirement rather
  than review one. `spec scenario generate` falls back to a literal
  reading of the criteria, which is valid Gherkin but does not reuse the
  step wording the kata already binds, so `spec steps missing` will have
  work for you. Everything else degrades the same way: you implement
  `StringCalculator.java` by hand after each RED bar, the generating
  commands fall back to deterministic templates, and `spec reword`
  becomes a plain wizard with no proposals.
- **This repository cloned**, and a branch of your own. Never work on
  `trunk`.

**Do this** — run the preflight, from anywhere inside the repository:

```bash
scripts/preflight.sh
```

**Expect:** ten checks and the closing line

```text
Result: 10 passed, 0 failed.
```

It checks Java, Maven, `spec`, the Maven build, the Cucumber surefire
report, an end-to-end MCP run, and that the REQ-003 exercise has not been
burned by an earlier rehearsal. Any FAIL line names the fix. If it tells
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
| `spec changes show` / `commit` / `discard`, `spec refactor --manual`, `spec mark-implemented` | No | instant |
| `spec steps missing` | No | instant |
| `spec draft` with no flags — the Step 2 wizard | Yes — one call to split your description, plus a retry for each reply that comes back unusable or without an edge case, then one per finding | about 59 s in the observed run |
| `spec scenario generate` | Yes — one call, falling back to a literal reading of the criteria | about 12 s in the observed run |
| `spec reword` | Yes — one call per finding | 15–70 s per finding |
| `spec unittest generate` | Yes — one call | 30–45 s |
| `spec steps generate` | Yes — one call | about 35 s |
| `spec implement` | Yes — one call, large prompt | 70–195 s |
| `spec refactor` | Yes — one call plus a full test run per round, up to `[refactor] attempts` (10) | about 50–90 s per round; one round in the observed run |

`spec refine` deserves a second look on that list: the wording review is
a **deterministic** rule set, not a model. Same input, same findings,
every time. Only the *repair* (`spec reword`) calls a model.

Timings above are from the observed run against a local model on a laptop.
They vary widely — five findings in one `spec reword` finished in about 50
seconds, while a single finding on another requirement took about 70. Ten
seconds to three minutes is all normal.

---

## Step 1 — Branch and baseline

**Do this**

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

Do this even if you plan to implement by hand — `spec implement` and the
assisted `spec reword` will not find a model otherwise, and they degrade
quietly rather than complaining.

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
code, and REQ-007 stays `pending` until the homework in Step 5.

You do not write the requirement, and **you start from the same prompt the
agent path uses** — word for word, the block Step 4 of
[../student-follow-along.md](../student-follow-along.md) tells you to
paste into your agent. Same intent, same words, both paths. What differs
is everything that happens after you press Enter, and that is the
comparison worth making.

**Do this** — start the wizard:

```bash
spec draft
```

With no flags, `spec draft` is an interactive wizard and it **needs a
terminal**. With a model resolved it opens on a description prompt rather
than a blank title prompt:
**Describe what to build in plain words (one or several requirements). Enter drafts manually instead:**

**Do this** — paste the agent-path prompt there, **as one line**:

```text
Add a new requirement to requirements/requirements.json: a custom delimiter may be declared on the first line, so "//+\n1+2" adds up to 3. Follow the existing format — unique id, title, user story, acceptance criteria phrased Given/When/Then, status pending. Then call validate_spec and fix every issue until the spec is valid. Then call refine_requirement on the new requirement and reword it from the findings until there are none. Do not write scenarios or code yet — we are only agreeing on the spec.
```

That is the agent-path block with its line breaks taken out. The
description prompt reads a single line, so a pasted newline submits the
answer early — join it before you paste. (On a real terminal it wraps
over several rows of a `> ` line. The transcripts below omit that echo.)

**Here is the divergence, and it is the whole lesson of this page.** Only
one clause of that prompt is doing work here. The rest describe a
*process*, and on this path you are not the one carrying it out:

| The prompt says | On the agent path | Here |
| --- | --- | --- |
| add a requirement to `requirements/requirements.json` | the agent chooses the file and the edit | `spec draft` writes the spec; there is nothing to choose |
| a custom delimiter on the first line, `"//+\n1+2"` → 3 | the behaviour | the behaviour — the one clause that matters |
| follow the existing format, unique id, Given/When/Then, `pending` | the agent is trusted to match it | the wizard's prompts **are** that format, and it assigns the id |
| then call `validate_spec` until valid | the agent must remember to | the wizard runs the structure gate itself |
| then call `refine_requirement` and reword until none remain | the agent must run its own loop | the wizard loops, and checks the model's work |
| do not write scenarios or code yet | a fence you hope holds | there is no such tool on the profile |

So you can paste the prompt unchanged — and should, so the two paths start
level — but four of its six clauses are being honoured by the tool rather
than by a model choosing to comply. **That is the difference between
asking and structure.**

**Expect**, in order. First the split, which is a model call. The whole
wizard took **59 seconds** in the observed run:

```text
Splitting the description into requirements with qwen3.8-flash-next:125b-mlx - working ...
list_requirements()
calling list_requirements ...
The model reply was invalid (requirement "Custom delimiter declared on the first line" covers only happy paths - add at least one edge case to each (empty, invalid, or error input)) - asking again (2 of 3)
The description holds 1 requirement(s):
  1. Custom delimiter declared on the first line
Accepted requirements are staged for requirements/requirements.json as pending - nothing reaches the working spec until spec changes commit:
  REQ-007 Custom delimiter declared on the first line
Walking through REQ-007. Each prompt shows the proposal - Enter accepts it, or type your own wording.
```

Four things in there are worth stopping on.

`list_requirements()` and the `calling` line under it are **the model
using tools**, and you can read its reasoning off them. That is it
reading the backlog — which is how it knew the next free id was REQ-007
and that nothing already covered custom delimiters. You may also see
`get_requirement(id=REQ-005)`, which is the *follow the existing format*
clause being carried out literally: the model opening a neighbouring
requirement to copy the house style. REQ-005 is a well-chosen one to
open, because it is the requirement that already writes a newline as
`"1\n2,3"`.

That is the `spec-draft` profile in `.spec.toml` at work, and it is also
the answer to the last row of the table:

```toml
spec-draft = ["list_requirements", "get_requirement", "validate_spec", "refine_requirement"]
```

Four tools, all read-and-review. Not one of them can touch a feature file
or a `.java` file. The prompt's *do not write scenarios or code yet* is
still good manners, but here it is unenforceable and unnecessary in equal
measure — **the discipline is in the profile, not in your wording.** On
the agent path that sentence is load-bearing.

**Third, and the best thing on this page: the retry line.** The model's
first answer was well-formed and complete, and the harness threw it away
anyway — because every criterion in it fed the calculator clean input. A
requirement made only of happy paths is not a specification, and that is
not a matter of taste here: the same deterministic rule that reviews your
wording a few lines further down was run against the model's reply the
moment it arrived, and sent it back with the reason attached and a budget
attached to that — `asking again (2 of 3)`.

Nobody had to ask for that. The agent-path prompt says *reword it from
the findings until there are none* and then trusts the agent to keep its
own score; here the scorekeeping is the program, and the budget is what
stops a stubborn model looping forever. **What the wizard shows you has
already been reviewed** — which is why, below, you walk the criteria once
instead of twice.

Your run may show that line, or two of them, or none, depending on what
the model answers first. Each one costs a few seconds.

Fourth: accepted proposals are staged **immediately**, as `pending`, under
sequential ids — before you review anything. Nothing reaches your working
tree until `spec changes commit`, so this is safe, but it does mean
`spec changes show` has something in it from this moment on.

Then the wizard walks the proposal field by field, pre-filled. Enter
keeps each one; type over it to use your own wording:

```text
REQ-007 title [Custom delimiter declared on the first line] (Enter keeps it):
REQ-007 story (As a ..., I want ..., so that ...) [As a workshop participant, I want the calculator to read a delimiter I declare on the first line so that I can sum numbers separated by any character I choose.] (Enter keeps it):
Acceptance criteria (Given/When/Then). A blank criterion ends the list:
REQ-007 criterion 1 [Given the input "//+\n1+2", when add is called, then the result is 3] (Enter keeps it, '-' drops it):
REQ-007 criterion 2 [Given the input "//;\n1;2;3", when add is called, then the result is 6] (Enter keeps it, '-' drops it):
REQ-007 criterion 3 [Given the input "//+\n" with no numbers after the declaration, when add is called, then the result is 0] (Enter keeps it, '-' drops it):
REQ-007 criterion 4 [Given the input "//+\n1++2", when add is called, then an error is raised] (Enter keeps it, '-' drops it):
REQ-007 criterion 5 [Given the input "//+\n1+2" declared on the first line, when add is called, then the characters "/" and the delimiter declaration contribute no numbers to the sum] (Enter keeps it, '-' drops it):
REQ-007 criterion 6 [Given an input with no "//" declaration line, when add is called, then the declared-delimiter rule does not apply and the input "//+\n1+2" written without a first-line declaration is not parsed as a declaration] (Enter keeps it, '-' drops it):
REQ-007 criterion 7 (leave blank to finish the criteria):
```

**Your wording will not match that, and it does not need to.** The model
proposed six criteria here; it may propose three or eight for you. What is
fixed is the shape — one title, one `As a / I want / so that` story, and
Given/When/Then criteria terminated by a blank line.

Criteria 3 and 4 are what the retry bought: no numbers after the
declaration, and a malformed declaration. Those are the two you would
otherwise have had to ask for yourself.

Criteria 5 and 6 are the other half of the bargain, and worth seeing.
Pushed for an edge case, a model will often keep going and pad the list —
6 restates the absence of the feature and reads as barely a sentence.
**Type `-` at either prompt to drop it.** Neither gate will do that for
you: they can tell you a criterion is vague or untestable, but no rule
knows that a criterion is not worth having. That judgement is the part
that stays yours, which is the whole reason the wizard walks you through
the list at all.

Press Enter through all of them and the wizard runs the gates on what you
handed back. There is an edge case in there already, so they find nothing
and the loop ends where you decide:

```text
The wording reads clean. Stage this requirement? [y/N]
```

Answer `y`. At a terminal the whole run is: paste the prompt, Enter
through the fields, then `y` — **one pass**.

If you type over a criterion and leave only happy paths behind, or the
retry budget runs out before the model adds one, you get the findings
round instead: the gate names what is missing, the model is asked to
repair it one finding at a time, and the wizard shows you its proposals
in a second pass. Extra A walks through that loop in full on a
requirement broken on purpose.

```json
{
  "id": "REQ-007",
  "title": "Custom delimiter declared on the first line",
  "staged": true,
  "nextStep": "Review with spec changes show and apply with spec changes commit, then add the @REQ-007 scenario with scenario add."
}
```

Two footnotes on that reply. `scenario add` at the end is missing its
`spec ` prefix — a gap in the rewriter that turns the services' advice
into commands, so paste `spec scenario add` and not what it says. And the
Step 2 transcripts above were taken on `spec 0.5.4`, which prefixes
`nextStep` commands more thoroughly than the 0.5.2 run the rest of this
page was recorded against; where a `nextStep` later on this page reads
`changes commit` or `call get_requirement` and yours reads
`spec changes commit` or `spec show`, that is why.

Note the two characters `\n` in the prompt and in every criterion the
model gave back. The spec writes an input newline as the literal
two-character escape because a requirement's fields are single-line, and
REQ-005 already does it with `"1\n2,3"` — which is very likely why the
model opened REQ-005 before proposing. Model replies are re-escaped on
the way in as well, so a draft cannot quietly break the convention.
Several later steps depend on it.

**One prompt can hold more than one requirement.** The agent-path prompt
describes a single behaviour, so you get a single proposal and go straight
to the review pass. Describe two and the splitter takes you at your word:
appending `, and an empty delimiter declaration is rejected` produced
**two** proposals and staged both, under REQ-007 and REQ-008. That opens
an extra prompt, where you can take only the first:

```text
The description holds 2 requirement(s):
  1. Custom single-character delimiter declared on the first line
  2. Empty custom delimiter declaration is rejected
Accept [Enter for all, or comma-separated numbers]:
```

Useful on your own backlog; not what you want today, because Extra D at
the end of this page expects REQ-008 to still be free.

Nothing in REQ-001 through REQ-006 mentions a custom delimiter, so this
draft earns no duplicate warning. Reuse an existing title or criterion
word for word and `spec draft` prefixes the `nextStep` with a warning
instead of refusing — read it and decide.

**If you need this in a script**, supply `--title`, `--story`, and at
least one `--criterion` and the wizard never runs:

```bash
spec draft \
  --title "Custom delimiter declared on the first line" \
  --story "As a calculator user, I want to declare a custom delimiter on the first line so that I can separate numbers with a character of my choosing." \
  --criterion 'Given the input "//+\n1+2", when add is called, then the result is 3' \
  --criterion 'Given an empty delimiter declaration "//\n1+2", when add is called, then an IllegalArgumentException is thrown'
```

With those supplied, `spec draft` is fully non-interactive: no wizard,
no prompts, no terminal, no model. It assigns the next id, checks the
structure, and stages. That makes it the one authoring command you can
safely put in a script — and it is also how this page used to open, which
is worth knowing only so you understand what it costs. Pasting a finished
requirement is transcription. The wizard above is the exercise.

**Do this** — review, then apply:

```bash
spec changes show
spec changes commit
```

You get one JSON reply per command. `spec changes show` is the review
surface. It lists the staged manifest — one entry per file, with the
action and a summary of the edits that produced it:

```json
{
  "changes": [
    {
      "path": "requirements/requirements.json",
      "action": "modify",
      "summary": "draft REQ-007 from the description; reword REQ-007"
    }
  ],
  "nextStep": "Review the staged files, run spec validate, then apply with spec changes commit or drop with spec changes discard."
}
```

That summary is the whole wizard in one line: `draft REQ-007 from the
description` is the splitter staging its proposals, and `reword REQ-007`
is the wording you confirmed at the end being written back over them. Two
edits, one file, one entry.

You get that second half whether or not the findings round ran — even if
you pressed Enter through every prompt unchanged. It is the wizard
recording the wording you approved, not evidence that anything was
repaired.

`spec changes commit` echoes that same manifest back — it is telling you
what it just applied, not showing you a second batch. The `nextStep` is
what tells the two replies apart: `Staged changes applied to the working
tree.` means the edit is now in your files.

The staged **bytes** live beside the manifest, under
`.spec-staged/files/` mirroring the project layout — so the file above is
at `.spec-staged/files/requirements/requirements.json`. Open it when you
want to read the exact content before approving it. Your working tree is
untouched until `spec changes commit`.

**Do this** — have the wording reviewed:

```bash
spec refine REQ-007
spec list
```

**Expect** a clean review:

```json
{
  "id": "REQ-007",
  "clean": true,
  "findings": [],
  "source": "working tree",
  "nextStep": "The wording reads clean. Confirm it with the developer, then write the Gherkin scenario from the acceptance criteria."
}
```

It is clean because the wizard already drove it there — the findings
round you watched inside `spec draft` was this same reviewer, run against
the staged wording. This call is you confirming that from outside the
wizard, on the committed file, which is what `"source": "working tree"`
is telling you.

`spec list` now shows seven ids, with REQ-007 `pending`.

**Your checkpoint:** read the story and every criterion out loud. Is this
what you meant? The model proposed the words; the intent is still yours,
and this is the moment to disown any of it. You own the intent. Approving
does not change `status` — leave it `pending`.

That is Exercise 1. Go straight on to Step 3.

Extras A, B and C at the end of this page take Exercise 1 apart — the
structure gate, the wording gate, and the catalog. They are the most
instructive few minutes on the page, and they are also entirely optional:
nothing later depends on them and nothing grades them. Do them after
Step 4, or after the workshop.

---

## Step 3 — Exercise 2: take REQ-003 to `implemented`

This is the full Red/Green/Refactor loop on one requirement, with two
places where you and only you decide whether to continue.

**Do this** — read the requirement first:

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

REQ-003 has two acceptance criteria, so it gets two scenarios. You do not
write them. The criteria are already `Given …, when …, then …`, so the
scenarios are derivable from the spec — which is the whole bet of
spec-driven development, and this is where you get to watch it pay.

**Do this**

```bash
spec scenario generate REQ-003
```

**Expect** a reply naming one scenario per criterion. The observed run
took **12 seconds**:

```json
{
  "feature": "kata/src/test/resources/features/string_calculator.feature",
  "scenarios": [
    "Two numbers separated by a comma are summed",
    "Two larger numbers separated by a comma are summed"
  ],
  "criteria": 2,
  "staged": true,
  "source": "llm",
  "nextStep": "Read the steps against the acceptance criteria, apply with spec changes commit, then run spec steps missing."
}
```

**`source` is the field to read.** There are two ways this command can
answer, and it tells you which one you got.

A literal reading of the criteria is always available and needs no model
at all — `Given "1,2"` / `When add is called` / `Then the result is 3`,
straight off the criterion's own words. That is the **template**, and
`"source": "template"` means it is what got staged. Correct, and slightly
foreign: it has no way of knowing that every scenario already in this
file opens on `Given a string calculator`.

So when a model is resolved, it is handed the requirement, the feature
file as it stands, and the list of step definitions that already exist,
and asked to say the same thing in the file's own vocabulary. That is
`"source": "llm"`, and here is the difference it makes to the first
criterion:

| `source` | The steps you get |
| --- | --- |
| `template` | `Given "1,2"` / `When add is called` / `Then the result is 3` |
| `llm` | `Given a string calculator` / `When I add "1,2"` / `Then the result is 3` |

Both are honest readings of the same criterion. Only one of them reuses
steps the kata has already bound — and that is not a cosmetic win, as the
next command shows.

**Do this** — the payoff:

```bash
spec steps missing
```

**Expect** nothing missing:

```json
{
  "language": "Java",
  "framework": "Cucumber-JVM",
  "missing": [],
  "nextStep": "Every step has a definition. Run spec test to execute the suite."
}
```

**Zero undefined steps, and nobody told it to reuse them.** It was shown
the file and the bindings and drew the obvious conclusion. Had it invented
`When add is called` instead, you would be looking at two undefined steps
and a detour through `spec steps generate` before you could get to RED.

### What the model is not allowed to get away with

Its reply is only used when it holds **exactly one scenario per
criterion**, every step opens with a Gherkin keyword, every scenario has a
`When` and a `Then`, and no name collides with one already in the file.
Miss any of those and it is asked again with the reason; run out of
retries and the template is staged instead. **Coverage cannot be lost to
a chatty model** — which matters, because `verify-workshop-run.sh` grades
REQ-003 on exactly that: one tagged scenario per acceptance criterion.

Two situations it refuses outright rather than guessing. Run it twice and
the second run stops, because it would otherwise stack a second copy of
every scenario:

```text
Error: kata/src/test/resources/features/string_calculator.feature already has 2 scenario(s) tagged @REQ-003: Two numbers separated by a comma are summed, Two larger numbers separated by a comma are summed. Change them with spec scenario update, or delete them first.
```

And a requirement whose criteria are not Given/When/Then shaped has
nothing to read, so it is sent back to `spec reword` rather than having
its scenarios invented.

**If you would rather write them yourself**, `spec scenario add` is the
typed mutation underneath, and it is what this page used to open with —
one call per scenario, every step spelled out:

```bash
spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Two numbers separated by a comma are summed" \
  --step 'Given a string calculator' \
  --step 'When I add "1,2"' \
  --step 'Then the result is 3'
```

```json
{
  "feature": "kata/src/test/resources/features/string_calculator.feature",
  "scenario": "Two numbers separated by a comma are summed",
  "action": "add",
  "staged": true,
  "nextStep": "Review with changes show, run validate, then apply with changes commit."
}
```

Worth knowing it exists — `generate` calls it twice under the hood, and
it is the only way to get a scenario the criteria do not describe. But
transcribing the criteria by hand is transcription. Deriving them is the
exercise.

**Do this** — checkpoint 1, and it is yours:

```bash
spec changes show
```

Both scenarios landed on one feature file, so they produce **one** staged
entry — one file is one entry — and its summary names **both** edits,
joined with `"; "`. `spec scenario generate` stages through the same
`scenario add` underneath, once per scenario, which is why the summary
reads as two appends rather than one generation:

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

**Do this**

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

**Do this**

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

**Do this**

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

**Do this** — checkpoint 2, and it is yours:

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

**Do this**

```bash
spec changes commit && spec test
```

**Expect** GREEN at the same total: nine tests, zero failures. Same bar,
both altitudes.

**Do this** — the refactor is optional, but the proof is not:

```bash
spec refactor --note "extract comma delimiter constant" --req REQ-003
```

`spec refactor` carries the cleanup out. The note is both the log entry
and the brief. Expect a round or two, about a minute each, then:

```json
{
  "phase": "REFACTOR",
  "goal": "extract comma delimiter constant",
  "rounds": 1,
  "attempts": 10,
  "targets": ["kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java"],
  "tests": 9,
  "applied": true,
  "reverted": false,
  "nextStep": "The refactor is in your working tree and the bar is where it was. Read it with git diff, then run spec test to record the run."
}
```

`"applied": true` means it is already in your working tree — this is the
one command that writes there without staging first, because the only
thing that can tell a refactor from a rewrite is the suite, and the suite
runs against files on disk. It earns that by never writing a test and by
restoring every file it touched if it cannot get green. If your run comes
back `"reverted": true`, the model could not clean this up without
breaking something and your code is exactly as you left it.

**Do this** — read what it did, then prove it:

```bash
git diff
spec test
```

GREEN, nine tests, zero failures. A refactor that changes the bar was not
a refactor — which is why the loop checks the count as well as the
failures, and throws away a round that comes back green at eight.

**Expect** the diff to be small and structural: a named constant, or a
block lifted into a well-named private method. If yours reached for a
regex engine or a stream pipeline, that is the conversation worth having
with yourself before you commit it.

Prefer to do it by hand? `spec refactor --note "..." --manual` marks the
phase and leaves the code alone, which is what this command did before it
could refactor. You then clean up and run `spec test` yourself.

**Do this** — record it:

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

That is the loop. Everything left is repetition — so go score it in
[Step 4](#step-4--check-your-work), which is where the workshop ends. It
already reads 7/7.

---

## Step 4 — Check your work

**Do this**

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

The first four grade Exercise 1, the last three grade Exercise 2 — so all
seven pass as soon as Step 3 is done. That is why this is where the
workshop ends. Run it again after the homework if you like; the score does
not move, because Step 5 is the rest of the kata rather than more of the
exercises.

None of the optional extras changes the score either. The verifier reads
the merged spec tree, so the split catalog in Extra C and the extra child
file in Extra D both still score 7/7.

Note what is not graded: your wording, anywhere. The REQ-007 paragraph you
and the model settled on is yours, so the verifier asks the same two
questions you asked in Exercise 1 — does `spec validate` pass, does
`spec refine` come back clean — rather than diffing your prose against
someone else's. Any FAIL line names the artifact to revisit and which
criterion is unaccounted for.
[../student-follow-along.md](../student-follow-along.md) walks through the
two most common partial results in detail.

---

## Step 5 — Homework

The workshop is over and your score is already in. What follows is the
rest of the kata — REQ-004, REQ-005, REQ-006, and then REQ-007, the one
you drafted yourself — on the plane home. Nothing here is graded, and
[Step 4](#step-4--check-your-work) scores the same 7/7 before and after.

Same recipe every time. Substitute the id and the scenarios.

**Do this**, once per requirement:

```bash
spec show REQ-00N

spec scenario generate REQ-00N     # one scenario per acceptance criterion

spec steps missing                 # empty? good. otherwise: spec steps generate
spec unittest generate REQ-00N
spec changes show                  # checkpoint 1
spec changes commit
spec test                          # expect RED

spec implement REQ-00N             # or edit StringCalculator.java by hand
spec changes show                  # checkpoint 2
spec changes commit && spec test   # expect GREEN

spec refactor --note "<what>" --req REQ-00N    # optional, GREEN only
git diff && spec test                          # read it, then record the run
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

**The REQ-007 row is the observed run, not a target.** REQ-004 through
REQ-006 ship with the repository, so their criteria — and therefore their
scenario counts and their bars — are fixed. REQ-007 is the one you drafted
in Step 2, and its criteria came from the model: the observed run came
back with four, where the row above assumes two. One scenario per
criterion is the rule, so more criteria means more scenarios, more unit
tests, and a higher total than the 25 quoted here and in the final bar
below. Count
your own `spec show REQ-007` and expect your bar to differ. Nothing grades
the number.

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
scenario name, and the feature file.

**Do this** — close it with:

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

### The final bar

When all four are done:

**Do this**

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
- `spec test` — GREEN, 0 failures. The count below is the observed run's;
  yours tracks however many criteria REQ-007 came back with in Step 2, as
  the REQ-007 note above explains. Zero failures is the bar, not the total.
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

## Optional extras

**None of this is required and none of it is graded.** The workshop
finishes at [Step 4](#step-4--check-your-work). What follows takes the machinery apart so
you can see why it behaves the way it does, and it is the most
instructive quarter-hour on the page — but it is after-class material.

Do them in the order below. Extra C moves REQ-007 into another file, so
Extras A and B want to run before it.

Everything here works the same whether REQ-007 is still `pending` or
already `implemented`. `spec validate` does not read `status`, and
`spec reword` carries `status` and `featureFile` through untouched, so a
REQ-007 you break and repair here still validates as an implemented
requirement afterwards.

For Extras A and B, **you** make the breaking edit by hand. Do not ask an
agent to write bad wording for you: an agent asked to write a bad story
tends to fix it on the way to disk, the tool then correctly reports that
everything is fine, and the demonstration never fires. Human breaks the
spec, tool catches it, tool repairs it.

### Extra A — the structure loop (`spec validate`)

Structure is checked first, and it is checked deterministically.

**Do this**

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

#### Repairing it with the wizard

**Do this**

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
gates on purpose — see Extra B — and here they happened to fire together.

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

**Expect** `"staged": true` in the reply.

**Do this** — apply it and re-validate:

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
net change against the pre-extra commit. The `\n` survived as the
two-character escape rather than becoming a real newline — model replies
are re-escaped before they reach the spec, precisely so a reword cannot
quietly break the convention the rest of the file uses. Yours will be
similar, not necessarily identical.

#### If you are scripting this

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
including the final confirmation.

**Do this**

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
same flags as `spec draft`.

**Do this**

```bash
spec reword REQ-007 \
  --title "Custom delimiter declared on the first line" \
  --criterion 'Given the input "//+\n1+2", when add is called, then the result is 3' \
  --criterion 'Given an empty delimiter declaration "//\n1+2", when add is called, then an IllegalArgumentException is thrown'
```

That is deterministic, non-interactive, and stages immediately.

### Extra B — the wording loop (`spec refine`)

Structure passing does not make a requirement good. `spec refine` is the
second gate, and it has opinions about prose.

**Do this**

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

**Do this** — repair, apply, re-check:

```bash
spec reword REQ-007
spec changes commit
spec refine REQ-007
```

Five findings means **five model calls** — one per finding, each narrated
as `Asking <model> to address finding N of 5 - working ...`. In the
observed run the whole sequence took about 50 seconds. Then the same two
passes of prompts as in Extra A, then `y`.

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
structurally invalid requirement — Extra A's case — keeps the loop honest
however long it takes, because a spec that fails `spec validate` is not
usable at all.

### Extra C — the spec is a catalog (includes)

`requirements/requirements.json` is always the entry point, but it does
not have to hold every requirement. It can carry an `includes` list of
child spec files, those children can include further files, and the tools
merge the whole tree into one backlog. Ids stay unique across the tree.

**Do this**

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
Everything on this page works identically on a split catalog — that was
verified end to end, including `scripts/verify-workshop-run.sh check`,
which reads the merged tree and still scored 7/7 with the split still in
place.

The harness ships a command for this too, which is what Extra D uses:
`spec include add requirements/delimiters.json` stages both the include
line and an empty child file. The hand-editing above is only to show you
the shape.

### Extra D — split the spec with a command, and draft into the child

Extra C split the spec by hand. Here is the same thing done with the tool,
and a fresh requirement drafted straight into the child file.

**Do this**

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
child.

**Do this** — apply them, then draft into the child:

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
Extra C's split, the final state is a three-file catalog:

| Ids | Status | File |
| --- | --- | --- |
| REQ-001 .. REQ-006 | implemented | `requirements/requirements.json` |
| REQ-007 | implemented | `requirements/delimiters.json` |
| REQ-008 | pending | `requirements/newlines.json` |

`spec validate` validates the whole tree as one catalog, and REQ-008 is
your next kata. The format reference lives in the manual:
[The requirements format](../manual/src/spec-format.md).

### Extra E — the gates, if you want to see them refuse

The discipline lives in the tool, not in a prompt. Four refusals are worth
provoking once, so you know they are real.

**Do this** — work in a throwaway worktree so you do not disturb your run:

```bash
git worktree add -b spec-gates /tmp/spec-gates workshop-spec
cd /tmp/spec-gates
```

The `-b spec-gates` matters: git will not check the same branch out in two
worktrees, so this cuts a scratch branch from your run instead. The gates
read the phase from `.spec-state.json`, which is per-directory, so the new
worktree starts at phase `START` — run `spec test` once to establish a bar
before you try to trip anything.

**Do this** — provoke each one in turn. All four messages are exact.

**Refactoring on a red bar.** Get to RED, then:

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

**Do this** — clean up when you are done:

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

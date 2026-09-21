# Student Follow-Along: Spec-Driven with Harness

Your step-by-step companion for the 60-minute workshop. Everything the
presenter does, you do — this page has the exact commands, the exact agent
prompts, and what you should see at every step.

**The big idea:** there is one MCP server — `spec mcp serve` (25 tools).
Cursor talks to it, so does the bundled `smoke-test.jar`, and so does a free
local agent if you take the [pi path](student-follow-docs/pi-path.md). Your
hour is the workflow it enables: draft a requirement *with* an agent, let the
server critique it (structure first, wording second), then drive it
spec → Gherkin → RED → GREEN → REFACTOR through **tools**, with you
reviewing staged changes before they land.

Curious what order all these files would be created in if you started from
zero? See the greenfield build order, first file to last:
[student-follow-docs/greenfield-flow.md](student-follow-docs/greenfield-flow.md).

**One convention to know before you write a criterion.** Where a
requirement needs a newline in its *data* — the newline delimiter, the
`//;\n1;2` form of a custom delimiter — the spec stores the
**two-character** escape `\n`, a backslash followed by an `n`, not a real
line break. That literal pair has to survive unchanged all the way into the
Gherkin step and into the Java string the generated test asserts on. The
generator escapes it for you on the way through, so you do not have to
think about it when a tool writes the criterion. You do have to think about
it when *you* write one: type `\n`, leave it alone, and do not let an
editor or an agent "helpfully" turn it into a line break. A criterion
holding a real newline is a different thing and it breaks the generated
Java.

---

## Before the workshop

You need:

- **`spec` on PATH** (`spec --version`) — GitHub release, `cargo install --path harness`, or `harness/target/release/spec`. Cursor will not connect without it, and it must report **0.5.4 or newer** — every tool reply quoted on this page is quoted from that release, and both the refinement loop in Step 4 and the homework's account of `spec steps generate` behave differently below it.
- **Java 21+** (`java -version`)
- **Maven 3.9+** (`mvn -version`)
- **Cursor** (or any MCP-capable agent — Claude Desktop works with the same JSON)
- This repo cloned
- **Optional, and the fully offline route:** [Ollama](https://ollama.com) with
  `qwen3.8-flash-next:125b-mlx` pulled. Two Wi-Fi-off alternatives, both on the
  **same server**:
  [the pi path](student-follow-docs/pi-path.md) — a free MIT agent you run with
  `pi -nbt` so these 25 tools are all it gets — and
  [the harness path](student-follow-docs/harness-path.md), the `spec` runner
  with narrower tools per command. The harness path is the command
  reference; if you would rather be walked through it the way this page
  walks you through Cursor — every command in order, with the expected
  output after each one — follow
  [student-follow-docs/spec-binary-follow-along.md](student-follow-docs/spec-binary-follow-along.md)
  instead.

Build once at home so the room's Wi-Fi never matters:

```bash
spec --version                     # must succeed
mvn -q -pl smoke-test package     # MCP-server smoke-test jar
mvn -q -f kata/pom.xml test       # kata JUnit + Cucumber baseline
```

Because of `-q` (quiet), Maven prints no download or compile chatter. The
kata command's Cucumber narration starts like this:

```text
@REQ-001
Scenario: An empty string returns zero # features/string_calculator.feature:14
  Given a string calculator            # com.davidparry.workshop.kata.StringCalculatorSteps.aStringCalculator()
```

…and continues through every scenario in the suite. Compare yours against
the full captured run:
[student-follow-docs/pre-step.log](student-follow-docs/pre-step.log).
(That log carries a stray `[Fatal Error]
TEST-com.example.FooTest.xml...` line mid-output. Nothing in the kata is
called `FooTest`, so you will most likely not see it — it is a malformed
XML report being re-read out of a `target/` directory on the machine that
captured the log, not a test failure. `mvn -f kata/pom.xml clean test`
clears it.) The build is good when the command exits without a
`BUILD FAILURE` banner — check with `echo $?` right after; `0` means
success.

---

## Step 1 — Branch, then build (first 5 minutes)

Never work on `trunk` — the exercises rewrite the spec and the kata, and
`trunk` must stay pristine so you can always reset by re-branching. From the
repo root:

```bash
git checkout -b workshop trunk
spec --version
mvn -q -pl smoke-test package && mvn -q -f kata/pom.xml test
```

**Expect:** a green build with the exact same output as your at-home build —
the `workshop` branch is a fresh copy of `trunk`, so nothing has changed yet.
Compare against
[student-follow-docs/pre-step.log](student-follow-docs/pre-step.log) if
anything looks off. If it's red, raise a hand and pair with a neighbor —
don't fall behind debugging alone.

---

## Step 2 — Watch the machinery introduce itself (~minute 16)

When the presenter reaches the smoke-test demo, run:

```bash
java -jar smoke-test/target/smoke-test.jar
```

The smoke test narrates every step of the protocol exchange. It starts like this:

```text
========================================================================
  STEP 0 — Launch the server
========================================================================
```

…and walks through discovery and tool calls. This walkthrough starts
straight at `tools/list` — the server's `2026-07-28` lifecycle does not
require an `initialize` handshake. The Java MCP SDK still opens the stdio
session with the classic `initialize` (protocol `2025-11-25`) on first use,
which you will see in the log, and the server answers it normally. Compare
yours against the full captured run:
[student-follow-docs/step2.log](student-follow-docs/step2.log). (The
interleaved `INFO io.modelcontextprotocol...` lines are SDK logging — normal —
and the absolute repo paths in the log will differ on your machine.)

**Expect:**

- **STEP 1** — **25 tools** discovered. The frozen seven you already know
  (`list_requirements`, `get_requirement`, `validate_spec`,
  `refine_requirement`, `run_tests`, `get_tdd_state`, `start_refactor`) plus
  authoring/staging (`scenario_add`, `unit_test_create`, `changes_show`,
  `changes_commit`, `requirement_reword`, `requirement_mark_implemented`, …) and inspect
  (`project_root`, `project_inspect`, `command_run`). Exercise 1 uses the structure/wording
  pair; Exercise 2 uses staging.
- **STEP 4** — `run_tests` returns `"phase": "GREEN", "tests": 5`
  (2 JUnit tests + 3 Cucumber scenarios — one bar, two altitudes).
- **STEP 5** — `get_requirement` for the first pending id (REQ-003 on
  `trunk`).
- **STEP 6** — operational reads (no staging): `validate_spec`,
  `refine_requirement` (REQ-001), `project_root`, `project_inspect`, `feature_list`,
  `feature_read` of `kata/src/test/resources/features/string_calculator.feature`,
  `changes_show`, `changes_validate`, `step_definitions_find`. Mutating
  tools stay behind
  `java -jar smoke-test/target/smoke-test.jar --sweep --include-mutating`.

That smoke test just did what every host does: launch, discover, invoke.
Cursor does it, and so does `pi` once its MCP extension is installed.
That's all the MCP you need today.

To connect your own agent, the ready-to-run configuration lives at
[config/mcp.json](config/mcp.json):

```json
{
  "mcpServers": {
    "spec-driven-server": {
      "command": "spec",
      "args": ["mcp", "serve", "--root", "${workspaceFolder}"]
    }
  }
}
```

Cursor users get this automatically — the repo ships `.cursor/mcp.json`
(the same entry as [`config/mcp.json`](config/mcp.json), plus a
`"protocolEra": "auto"` hint for the host). For every other client
(Claude Desktop, Claude Code, Codex, VS Code, Windsurf, Gemini CLI), see
[student-follow-docs/setup-mcp.md](student-follow-docs/setup-mcp.md).

---

## Step 3 — Confirm your agent is connected

The repo already registers the server for you in `.cursor/mcp.json`. Open
Cursor's MCP settings (see
[student-follow-docs/setup-mcp.md](student-follow-docs/setup-mcp.md) for
where to find them) and confirm `spec-driven-server` shows **green**. If it's red:
`spec` is not on PATH for GUI apps (launch Cursor from a terminal where
`spec --version` works, or put the absolute binary path in `command`), then
toggle the server off/on in the settings.

A green light says the server *launched* — now prove the agent can actually
*call* it. Paste this into your agent:

```text
Call the get_tdd_state tool from the spec-driven-server server and show me the raw JSON result.
```

The reply leads with the phase, which is the one word you came for. The long
`instructions` field — a guide to reading the phase log, not workflow
state — comes last, out of the way:

```json
{
  "phase": "START",
  "lastRun": {
    "tests": 0,
    "failures": 0,
    "errors": 0,
    "skipped": 0
  },
  "nextStep": "No tests have been run yet. Call run_tests to establish a baseline.",
  "refactorLog": [],
  "entries": [],
  "instructions": "This file is the TDD phase log. `instructions` is this guide, not workflow state. ..."
}
```

That is the reply on a phase log nothing has written to yet. **If you ran
Step 2, you will not see `START`** — the smoke test called `run_tests`, so
the log already reads `"phase": "GREEN"` with `"tests": 5` and the
`nextStep` points at `start_refactor` or the next requirement instead.
Either reply proves the same thing. (Your agent may also wrap the JSON in
its own prose. `get_tdd_state` is read-only, so this check never disturbs
your run.)

**Expect:**

- The agent invokes `get_tdd_state` on the `spec-driven-server` server — you'll
  see the tool call in the chat, no permission errors.
- A `phase` and a `lastRun`: `START` with zeros on a phase log nothing has
  touched, or `GREEN` with `"tests": 5` once Step 2's smoke test has run.
- A `nextStep` hint — the server coaching the agent through the workflow,
  which is the whole trick of Exercises 1 and 2.

If the agent says it can't find the tool, the connection is the problem, not
the agent: re-check the green light, confirm `spec --version` in a terminal,
and toggle the server off/on.

---

## Step 4 — Exercise 1: draft and refine the spec (minutes 20–32)

Paste this into your agent, word for word:

```text
Add a new requirement to requirements/requirements.json: a custom
delimiter may be declared on the first line, so "//+\n1+2" adds up to 3.
Follow the existing format — unique id, title, user story, acceptance
criteria phrased Given/When/Then, status pending. Then call validate_spec
and fix every issue until the spec is valid. Then call refine_requirement
on the new requirement and reword it from the findings until there are
none. Do not write scenarios or code yet — we are only agreeing on the
spec.
```

**What you should see, in order:**

1. The agent drafts **REQ-007** into `requirements/requirements.json`
   (scroll to the end — past REQ-006). `status` stays `pending`: that field
   only flips to `implemented` after scenarios and code land later. What
   changed is *when* you review — you read and approve the wording now,
   before any Gherkin or production code.
2. `validate_spec` → `"valid": true` in the **tool reply** (not a field in
   the JSON). A format-following draft usually passes on the first call;
   valid means *usable*, not *good*. The tool reply looks very close to
   this:

   ```json
   {
     "valid": true,
     "issues": [],
     "nextStep": "The spec is valid. Call get_requirement for a pending requirement and write its Gherkin scenario from the acceptance criteria."
   }
   ```

   `validate_spec` reads the **committed** spec on disk, so once the agent
   has staged a reword it says so, appending this to the `nextStep`: *"This
   read the committed spec on disk, and a spec edit is staged - call
   changes_validate to check the staged spec before changes_commit."*
   That sentence is the tool telling you it just judged a file you have
   already moved past. `changes_validate` is its staged-aware twin.

3. `refine_requirement` → findings in the **tool reply**. **Expect
   findings.** A first draft coming back with something to fix is the
   normal outcome, not a stumble — the refiner is deterministic and it is
   looking for happy-path-only criteria, ambiguous words, and a story
   missing its actor or its why. The findings list echoes whatever it spots
   in *your* agent's wording, so yours may have more or different entries:

   ```json
   {
     "id": "REQ-007",
     "clean": false,
     "findings": [
       "criteria: only happy paths - add at least one edge case (empty, invalid, or error input)"
     ],
     "source": "working tree",
     "nextStep": "Call requirement_reword to address each finding - never edit the requirements file by hand - then run validate_spec and call refine_requirement again. Iterate until there are no findings."
   }
   ```

   `source` is the field to read first. It says **which copy of the
   wording** was just judged: `"working tree"` for the committed spec on
   disk, `"staged"` for an uncommitted edit. The agent drafted REQ-007
   straight into the file, so the first pass reads the working tree.

4. **The refine loop — this is its own step, and it usually runs more than
   once.** `requirement_reword` *stages* its edit rather than writing it,
   and `refine_requirement` reads staged-first, so the next pass judges the
   new wording immediately. **No `changes_commit` between passes.** Reword,
   refine, reword, refine until the findings are gone, then commit once.
   From the second pass on, `source` flips and the `nextStep` says the same
   thing out loud:

   ```json
   {
     "id": "REQ-007",
     "clean": false,
     "findings": [
       "story: 'handle' is ambiguous - describe the observable behavior instead"
     ],
     "source": "staged",
     "nextStep": "Call requirement_reword to address each finding - never edit the requirements file by hand - then call refine_requirement again. It reviews your staged edit, so there is no need to commit between passes. Iterate until there are no findings."
   }
   ```

   Done looks like this (only the `id` varies):

   ```json
   {
     "id": "REQ-007",
     "clean": true,
     "findings": [],
     "source": "staged",
     "nextStep": "The staged wording reads clean. Confirm it with the developer, apply it with changes_commit, then write the Gherkin scenario from the acceptance criteria."
   }
   ```

   If the loop never ran — the agent's first draft already had an edge case
   and nothing was staged — the same reply comes back with
   `"source": "working tree"` and a `nextStep` of *"The wording reads
   clean. Confirm it with the developer, then write the Gherkin scenario
   from the acceptance criteria."* No `changes_commit` to do in that case,
   because nothing was staged. Demo B below shows you the findings loop on
   demand.

   On a `spec` older than 0.5.3 this loop does not converge: refinement
   read the committed spec, so rewording changed nothing the next pass
   could see and the identical findings came back forever unless something
   committed in between. Worse, a deliberately vague story staged over a
   clean one came back `clean: true` — the tool passing judgement on text
   you had already replaced. Any account of this loop that tells you to
   commit between passes is describing that older binary.

5. **Your checkpoint:** read the story and criteria aloud. Is this what we
   meant? You own the intent — approve it or redirect the agent with one
   sentence. Approving does **not** change `status`; leave it `pending`.
   `changes_commit` the staged wording if the refine loop staged one.

For both demos below, **you** make the breaking edit by hand — don't ask
the agent to do it. An agent asked to write bad wording tends to fix it on
the way to disk (or skip the edit entirely), and then the tool correctly
reports everything is fine and the demo never fires. Human breaks the spec,
tool catches it, agent repairs it.

**Optional demo A — structure loop (`validate_spec`)**

1. In `requirements/requirements.json`, edit the first REQ-007 criterion
   yourself to exactly this, then save the file:

   ```text
   the result should be 3 for //+\n1+2
   ```

2. Ask the agent:

   ```text
   Call validate_spec and show me the raw JSON result — do not edit any files.
   ```

3. Expect `"valid": false` — the tool rejects the criterion (missing
   Given/When/Then). If you typed the criterion exactly as above, the tool
   reply is similar:

   ```json
   {
     "valid": false,
     "issues": [
       "REQ-007: criterion \"the result should be 3 for //+\\n1+2\" must be phrased Given/When/Then"
     ],
     "nextStep": "Call requirement_reword to fix the issues - never edit the requirements file by hand - then call validate_spec again. Iterate until valid is true before writing scenarios or code."
   }
   ```

   Note the `\\n` in the issue text: the criterion holds the two-character
   `\n`, the JSON reply escapes the backslash, and the pair survives
   round-trip. That is the convention working, not a mangling.

4. Now let the agent off the leash: ask it to repair the criterion with
   `requirement_reword` and call `validate_spec` again until `"valid": true`.
   Because reword stages, the `validate_spec` that follows it will append
   the staged-edit sentence to its `nextStep` and point at
   `changes_validate`; the `"valid": true` you are waiting for arrives
   after `changes_commit`.

**Optional demo B — wording loop (`refine_requirement`)**

1. In `requirements/requirements.json`, replace the REQ-007 story yourself
   with exactly this, then save the file (only the story — leave the
   criteria alone):

   ```text
   the calculator should handle custom delimiters quickly
   ```

2. Ask the agent:

   ```text
   Call refine_requirement for REQ-007 and show me the raw JSON result — do not edit any files.
   ```

3. Expect `"clean": false` with five findings — the missing actor, the
   missing why, and every ambiguous word, each called out separately. If
   you typed the story exactly as above, the tool reply is similar:

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
     "source": "working tree",
     "nextStep": "Call requirement_reword to address each finding - never edit the requirements file by hand - then run validate_spec and call refine_requirement again. Iterate until there are no findings."
   }
   ```

   `"source": "working tree"` is the proof that the tool read *your* hand
   edit rather than something stale. That is the whole point of this demo:
   you broke it, the tool saw what you broke.

4. Now let the agent reword from the findings with `requirement_reword`,
   then re-run `refine_requirement` until `"clean": true`. Watch `source`
   turn to `"staged"` on the second pass — the loop converges without a
   commit in the middle. The failure is the lesson.

**Optional demo C — the spec is a catalog (includes)**

`requirements/requirements.json` is always the entry point, but it does
not have to hold every requirement itself: it can carry an `includes`
list of child spec files, and children can include further files, N
levels deep. The tools merge the whole tree into one backlog. To see it:

1. Create `requirements/delimiters.json` yourself with just REQ-007 in it
   (cut the whole REQ-007 object out of `requirements.json` and paste it
   into the new file):

   ```json
   {
     "requirements": [
       { "...": "the REQ-007 object you cut from requirements.json" }
     ]
   }
   ```

2. Add the include to `requirements/requirements.json`, right after
   `"description"`:

   ```json
   "includes": ["delimiters.json"],
   ```

3. Ask the agent:

   ```text
   Call list_requirements and validate_spec and show me the raw JSON results — do not edit any files.
   ```

4. Expect REQ-007 still listed (merged from the included file, after
   REQ-001..006) and `"valid": true`. Each row of `list_requirements` also
   carries a `file` telling you which document it came from — once the
   catalog is split, "REQ-007 exists" and "REQ-007 lives here" are two
   different questions, and the second one now has an answer:

   ```json
   {
     "id": "REQ-007",
     "title": "A custom delimiter may be declared on the first line",
     "status": "pending",
     "file": "requirements/delimiters.json"
   }
   ```

   One catalog, many files — ids stay unique across the whole tree.
   Duplicate the id and `validate_spec`
   answers `REQ-007: duplicate id - also declared in requirements.json`,
   naming the file that already has it; make two files include each other
   and it names the file too:
   `spec: requirements.json is included more than once - include every spec
   file exactly once`.

   **Both of those are the one case where you edit the spec file by hand.**
   No tool deletes a requirement and no tool removes an include, so the
   remedy is the file. `validate_spec` says so itself now, and says it
   without hedging:

   ```text
   A duplicate id is catalog structure, not wording: no tool can delete a
   requirement, so open the spec file the issue names and remove the
   duplicate requirement object, or give it an id nothing else uses.
   Editing the spec file directly is the remedy for these - the rule
   against hand-editing covers wording, not catalog structure. Validate
   again once the file is fixed.
   ```

   The repeated-include half reads the same way and names the parent file's
   `"includes"` array. Older builds answered both of these with "call
   `requirement_reword`", which cannot fix either — if your agent tries it
   and gets nowhere, check `spec --version`. "Never hand-edit the
   requirements file" still holds for every wording issue; catalog
   structure is the written-down exception.

   Undo the split (or leave it — every later step works the same, and
   Step 6's verifier reads the merged tree) before moving on if you want
   your file to match the walkthrough exactly.

   The harness ships a command for this too: `spec include add
   requirements/delimiters.json` stages both the include line and an empty
   child file, so the hand-editing above is only to show you the shape.

---

## Step 5 — Exercise 2: spec to green (minutes 32–50)

Paste this into your agent, word for word:

```text
Using the spec-driven-server tools: validate the spec first, then
`get_requirement` for REQ-003 — not the REQ-007 you just drafted, which
stays pending until homework. Add its Gherkin with
`scenario_add` (tag the requirement id), add missing steps with
`step_definition_create` if `step_definitions_find` reports any, add a
unit test with `unit_test_create`. Show `changes_show` and ask me before
`changes_commit`. Then `run_tests` (expect RED). Implement the simplest
production code in `StringCalculator`. `run_tests` (GREEN).
`start_refactor` if I agree. On GREEN, `requirement_mark_implemented`.
Ask me before each phase change.
```

**What you should see, in order** (tool replies are shown so you can spot
each milestone — yours will be similar, not identical, since agents word
their edits differently):

1. `validate_spec` passes (the valid spec is the entry ticket), then
   `list_requirements` and `get_requirement("REQ-003")`. The reply is
   **not** a copy of the requirement from `requirements.json` — the server
   enriches it: `featureFile` comes back as `featureLocation`, and
   `stepDefinitions`, `testLocation`, `productionLocation`, and
   `workflowHint` are added by the server to tell the agent where every
   artifact lives and what to do next. The `get_requirement` reply looks
   like this:

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

2. The agent stages two `@REQ-003` scenarios with `scenario_add` (and a
   unit test with `unit_test_create`). **Your checkpoint 1:** call
   `changes_show` (or have the agent show it) and read the staged Gherkin
   before you allow `changes_commit`. This is the spec review — is this the
   behavior you want?

   Four things about that review, all of which used to bite:

   - **`step_definitions_find` returning nothing is the common answer, not
     a bug.** The kata's steps are written in terms of "a string
     calculator", "I add", "the result is" — phrasings that generalize
     across requirements. Several requirements in this backlog need *no*
     new step definitions at all: the scenario is new, the steps it is
     built from are not. A new scenario implying new steps is the
     exception. If the agent skips `step_definition_create` because
     `step_definitions_find` came back empty, it is right.
   - **`changes_show` states its own totals.** When more than five edits
     are waiting it names five and prefixes the line
     `(9 edits in all, 4 not shown); ...`, so you always know the size of
     what you are approving rather than having to add a prefix to a list.
     Two `scenario_add` calls on the same feature file accumulate into one
     entry naming both scenarios.
   - **`scenario_add` leaves the rest of the file alone.** The header
     comment block above `Feature:`, the `As a / I want / So that`
     narrative under it, and any trailing comment after the last scenario
     all survive the append. A diff that shows any of them disappearing is
     an old binary, not your agent.
   - **A batched turn stages everything it says it staged.** Hosts like
     Cursor and `pi` fire tool calls in parallel, and two `scenario_add`
     calls in one turn used to interleave: both replied `"staged": true`,
     one scenario reached disk, and `changes_show` agreed with the lie. The
     staging area serialises now, in-process and across processes both, so
     the count in `changes_show` is the count you get. Still read it —
     that is the checkpoint — but you are reading it to review the
     behavior, not to audit the tool.

   And one non-finding: spec JSON files end with a newline, so a diff that
   reports `\ No newline at end of file` on `requirements.json` is stale
   output. Nothing to fix.

3. `run_tests` → **RED**. The count depends on how many unit tests your
   agent wrote: 8 tests with 3 not passing (2 Cucumber failures + 1 JUnit
   error) if it wrote a single test asserting both criteria, 9 with 4 if it
   wrote one per criterion. Either is a legitimate RED — what matters is
   that the two `@REQ-003` scenarios fail. The agent sees the same bar you
   do. The reply is similar to this (`failureDetails` stack traces trimmed
   here; the exact messages depend on the test names your agent chose):

   ```json
   {
     "phase": "RED",
     "tests": 8,
     "failures": 2,
     "errors": 1,
     "skipped": 0,
     "failureDetails": [
       "String Calculator addition.Two numbers separated by a comma are summed: ... java.lang.NumberFormatException: For input string: \"1,2\" ...",
       "String Calculator addition.Two larger numbers separated by a comma are summed: ... java.lang.NumberFormatException: For input string: \"10,20\" ...",
       "com.davidparry.workshop.kata.StringCalculatorTest.twoCommaSeparatedNumbersAreSummed: For input string: \"1,2\""
     ],
     "nextStep": "Tests are failing. Write the simplest production code that makes them pass, then call run_tests again."
   }
   ```

   **Read `failures` and `errors` as two different things.** Maven reports
   them in separate columns, and the split tells you which layer is
   unfinished. On the bar above, the two Cucumber scenarios hitting
   `NumberFormatException` are counted as **failures**: they ran, they
   exercised the behavior, and the behavior was wrong. The JUnit test is
   counted as an **error**, because an uncaught exception out of a plain
   `@Test` is a test that did not complete rather than a test that
   disagreed. The error column catches the other unfinished-artifact case
   too: a generated step definition still throwing `PendingException` is an
   error, which is Cucumber's way of saying the step is bound but has no
   body yet. So `"failures": 2, "errors": 1` reads as *two behaviors want
   production code, one artifact is still a placeholder*. The total is what
   decides GREEN; the split is what tells you where to work.

   The Cucumber lines read like that whatever your agent did, because the
   scenarios call the unimplemented `add`. The **JUnit** line depends on who
   wrote the test.

   **Generated unit tests arrive unfinished on purpose.**
   `unit_test_create` and `spec unittest generate` stage each criterion as
   `fail("TODO: assert - Given \"1,2\", ...")` — the method, the
   `@DisplayName`, and the criterion verbatim are all there, and the
   assertion is not. You or the agent write it. That is a real RED on the
   same count as a hand-written assertion; the difference is only which
   message you see, `TODO: assert - ...` instead of the
   `NumberFormatException` above. Do not treat a generated test as done.
   The failure mode to watch for is an agent *deleting* the `fail("TODO")`
   line instead of replacing it with a real assertion — that turns the bar
   green and proves nothing. Fill the assertions in while you write the
   production code, and read the test diff before you accept GREEN.

   Two more things you may see in the generated test. Method names can
   carry a `_2` or `_3` suffix — that happens when two criteria differ only
   in punctuation and would otherwise slug to the same Java identifier, and
   the criterion itself is still carried verbatim beside the member, so the
   suffix disambiguates the name and nothing else. And any `\n` in a
   criterion stays the two-character escape all the way into the Java
   string literal, which is what keeps the Maven failure message on one
   readable line.

   Over MCP, `unit_test_create` and `step_definition_create` always stage
   the deterministic template; the model-polished version is only on the
   `spec unittest generate` / `spec steps generate` CLI path.

4. The agent implements the simplest `StringCalculator.add` that passes
   (**a file edit** — there is no `implement` MCP tool). **Your checkpoint:**
   review the production diff.
5. `run_tests` → **GREEN** — the same total as your RED bar, 0 failures
   *and* 0 errors:

   ```json
   {
     "phase": "GREEN",
     "tests": 8,
     "failures": 0,
     "errors": 0,
     "skipped": 0,
     "failureDetails": [],
     "nextStep": "All tests pass. Either call start_refactor to clean up, or call get_requirement for the next pending requirement and write a failing test for it."
   }
   ```

6. `start_refactor` → cleanup → `run_tests` still GREEN (same reply as
   above). The `start_refactor` reply:

   ```json
   {
     "phase": "REFACTOR",
     "nextStep": "A refactor is in progress. Call run_tests to prove the refactor kept the bar green."
   }
   ```

   (Try asking for `start_refactor` while RED sometime — the server
   refuses: *"Refactoring is only allowed from GREEN (current phase: RED).
   Never refactor on a red bar — make the tests pass first."* Discipline
   lives in the tool. The refusal is worded for the phase you are actually
   in, so from START it reads *"No tests have been run yet — run them to
   find out where you are"* and from REFACTOR *"A refactor is already in
   progress — run the tests to close it."*)

   **If you restart the MCP server mid-exercise, the phase goes back to
   START.** Reload the server in Cursor's settings, or restart `pi`, and
   your RED/GREEN state is gone — which means the very next
   `start_refactor` or `requirement_mark_implemented` is refused with
   `current phase: START` and no obvious reason, because as far as the
   phase log is concerned you have not run any tests. It is not a bug and
   there is nothing to repair: call `run_tests` once to re-establish the
   bar, then carry on. Worth knowing before it happens to you in front of
   the room.
7. On GREEN, `requirement_mark_implemented` flips REQ-003 to
   `"status": "implemented"`. The tool **refuses** off GREEN or without a
   tagged `@REQ-003` scenario — premature completion is a live refusal, not
   a prompt hope. If the agent edits the JSON by hand instead, send it back
   to the tool.
   **Your checkpoint 2:** approve the final diff. Two checkpoints, both
   yours — the staged scenario and the production code.

---

## Step 6 — Check your work (minutes 50–53)

The repo can grade your run:

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

The first four grade Exercise 1, the last three Exercise 2. Note what is
*not* graded: your wording, anywhere. The REQ-007 paragraph you and your
agent settled on is yours, so the verifier asks the same two questions you
asked in Exercise 1 — does `validate_spec` pass, does `refine_requirement`
come back clean — rather than diffing your prose against someone else's.
REQ-003's wording ships on trunk, but the scenarios and tests it produces
are still yours: the verifier asks whether each of its two acceptance
criteria reaches a scenario tagged `@REQ-003` and an assertion in a
`@Test` that names the requirement. Scenario names, method names, and
assertion style are free. The last count will read `1 @Test` if you wrote
one method with both assertions and `2 @Test` if `spec unittest generate`
wrote one per criterion; both pass.

Any FAIL line tells you exactly which artifact to revisit, and which
criterion is unaccounted for.

### The most common partial result

Exercise 1 green, Exercise 2 red, three FAILs in a row:

```text
  PASS  REQ-007 was drafted into the spec
  PASS  the spec is valid
  PASS  REQ-007 wording is refine-clean
  PASS  REQ-007 covers the first-line delimiter declaration
  FAIL  REQ-003 status is 'implemented' in the spec - status is pending - Exercise 2 takes REQ-003, not the REQ-007 you drafted
  FAIL  @REQ-003 scenarios cover every acceptance criterion (0 tagged, 2 criteria) - no scenario is tagged @REQ-003
  FAIL  REQ-003 unit test asserts every acceptance criterion (0 @Test naming REQ-003) - no @Test names REQ-003 - the file groups tests by requirement id
```

Nothing is broken. It means Exercise 2 ran its whole Red/Green/Refactor
arc **on REQ-007** — the requirement you had just drafted, which was also
pending and was the freshest thing in the agent's context. Run
`mvn -f kata/pom.xml test` and you will find it green. Open the spec and
REQ-007 says `"status": "implemented"`. Every gate in the server fired, in
the right order, and refused nothing — because nothing was out of order.
The target was wrong, not the process.

**That is worth more than a clean scorecard.** The server polices *how*
you work: no refactor on red, no `requirement_mark_implemented` without a
green bar and a tagged scenario. It has no opinion about *which*
requirement deserves the next hour, and no tool can have one. That
decision was yours the whole time, and Step 6 is where you find out
whether you made it or let the context window make it for you.

To finish the run, paste Exercise 2's prompt again — it names REQ-003 —
and leave REQ-007 for the homework it was always meant to be.

### The other partial result: one criterion, not two

```text
  FAIL  @REQ-003 scenarios cover every acceptance criterion (1 tagged, 2 criteria) - no scenario covers: Given "1,2", when add is called, then the result is 3
```

REQ-003 carries two acceptance criteria, and the agent wrote a scenario
for one of them. Everything downstream still went green: Cucumber ran the
scenario that exists, `spec changes validate` passed, and
`requirement_mark_implemented` accepted REQ-003 — that gate requires *a*
scenario tagged `@REQ-003`, not one per criterion. So the spec says
`implemented` while half the behavior the spec asks for is only asserted
at the unit level.

This is the gap worth seeing: a green bar measures the tests you wrote,
never the criteria you skipped. Add the missing scenario and re-run:

```bash
spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Two numbers separated by a comma are summed" \
  --step 'Given a string calculator' \
  --step 'When I add "1,2"' \
  --step 'Then the result is 3'
spec changes commit && spec test
```

---

## Step 7 — Homework

The workshop is over and your score is already in. Nothing below is
graded, and Step 6 reads the same 7/7 before and after it.

- **REQ-004, REQ-005, REQ-006** are still `pending` in the spec — run
  Exercise 2's prompt again with that id in place of REQ-003, one at a time.
- **REQ-007** — the requirement *you* drafted — is waiting to be taken to
  green on the plane home. One warning for the unsupervised: `+` is a regex
  metacharacter, so `"1+2".split("+")` throws
  `PatternSyntaxException: Dangling meta character '+'`. Let the RED bar
  tell you that, then reach for `Pattern.quote`.
- If you phrase a `Then` step the kata has never seen — `Then an
  IllegalArgumentException is thrown`, with no `with a message containing`
  — `spec steps generate` adds it to the kata's own
  `StringCalculatorSteps.java`, keeping the package and class and leaving a
  `PendingException` body for you to fill in. That body shows up on the
  next bar as an **error**, not a failure — see Step 5 on reading the
  split. The model only ever sees the
  definitions being added, never the file they join, so the diff is the new
  method and nothing else — it cannot rename a field or an existing step
  method on the way past. A reply that hands back a whole file, alters a
  generated step expression, or drops a definition is refused, and the
  deterministic version of the same method is staged instead
  (`"source": "template"`). On top of that, no step *pattern* the file
  already declared may disappear — that would unbind a passing scenario.
  It stages like everything else, so read it with `spec changes show`
  before committing. (Older `spec` builds did hand the model the whole
  file and got a much larger diff back; if yours renames things you did
  not ask it to, you are on one of those.)
- `git diff complete` shows one worked ending for REQ-004, REQ-005, and
  REQ-006. Do **not** compare REQ-007 against it: the `complete` branch
  predates this exercise and its REQ-007 is a newline-delimiter duplicate
  of REQ-005, not the custom delimiter you drafted.
- **Want a stretch?** This page does not have one; the CLI walkthrough
  does. Its *Stretch — split the spec into a catalog, with a command*
  takes demo C further with `spec include add`, moving a requirement into
  a child spec file and re-grading the merged tree. Every tool used there
  has an MCP twin except `include_add`, so on this path you would do the
  include line by hand — which is demo C. Read it in
  [student-follow-docs/spec-binary-follow-along.md](student-follow-docs/spec-binary-follow-along.md),
  or the command-reference version in
  [student-follow-docs/harness-path.md](student-follow-docs/harness-path.md).

---

## Reset / start over

Everything the exercises touched lives in `kata/` and `requirements/`:

```bash
git checkout -- kata requirements     # rewind this branch to the start state
git clean -fd requirements            # drop any spec files you added (demo C's delimiters.json)
```

or throw the branch away and re-cut it:

```bash
git checkout trunk && git branch -D workshop && git checkout -b workshop trunk
```

---

## If you get stuck

- **Build red:** pair with a neighbor first; the presenter won't debug from
  stage.
- **Cursor MCP connection red:** `spec --version` must work and must report
  0.5.4 or newer. Launch Cursor from that terminal or put the absolute path
  to `spec` in `command`, then toggle the server off/on in Cursor's MCP
  settings. Note: a server restart resets the TDD phase — have the agent
  call `run_tests` once before any `start_refactor`, or the server will
  refuse.
- **A gate refuses and the reason makes no sense:** read the phase it
  names. `current phase: START` after everything was green means the
  server restarted; `run_tests` once and it is back. See Step 5.
- **Agent goes sideways:** it happens. Undo its edits, clear the chat, and
  re-paste the prompt — or follow the presenter's fallback on screen.

# Student Follow-Along: TDD, BDD & Spec-Driven Development in the Agentic Era

Your step-by-step companion for the 60-minute workshop. Everything the
presenter does, you do — this page has the exact commands, the exact agent
prompts, and what you should see at every step.

**The big idea:** there is one MCP server — `bdd mcp serve` (24 tools).
Cursor and the bundled `smoke-test.jar` both talk to it. Your hour is the
workflow it enables: draft a requirement *with* an agent, let the server
critique it (structure first, wording second), then drive it
spec → Gherkin → RED → GREEN → REFACTOR through **tools**, with you
reviewing staged changes before they land.

Curious what order all these files would be created in if you started from
zero? See the greenfield build order, first file to last:
[student-follow-docs/greenfield-flow.md](student-follow-docs/greenfield-flow.md).

---

## Before the workshop

You need:

- **`bdd` on PATH** (`bdd --version`) — GitHub release, `cargo install --path cli`, or `cli/target/release/bdd`. Cursor will not connect without it.
- **Java 21+** (`java -version`)
- **Maven 3.9+** (`mvn -version`)
- **Cursor** (or any MCP-capable agent — Claude Desktop works with the same JSON)
- This repo cloned
- **Optional (CLI / fully offline):** [Ollama](https://ollama.com) with
  `qwen3.8-flash-next:125b-mlx` pulled. Prefer the terminal to an IDE?
  Follow the [CLI path](student-follow-docs/cli-path.md) — **same server**,
  narrower tools per `bdd` command, Wi-Fi off.

Build once at home so the room's Wi-Fi never matters:

```bash
bdd --version                     # must succeed
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
(A stray `[Fatal Error] TEST-com.example.FooTest.xml...` line mid-output is
expected — it comes from a test fixture, not a real failure.) The build is
good when the command exits without a `BUILD FAILURE` banner — check with
`echo $?` right after; `0` means success.

---

## Step 1 — Branch, then build (first 5 minutes)

Never work on `trunk` — the exercises rewrite the spec and the kata, and
`trunk` must stay pristine so you can always reset by re-branching. From the
repo root:

```bash
git checkout -b workshop trunk
bdd --version
mvn -q -pl smoke-test package && mvn -q -f kata/pom.xml test
```

**Expect:** a green build with the exact same output as your at-home build —
the `workshop` branch is a fresh copy of `trunk`, so nothing has changed yet.
Compare against
[student-follow-docs/pre-step.log](student-follow-docs/pre-step.log) if
anything looks off. If it's red, raise a hand and pair with a neighbor —
don't fall behind debugging alone.

---

## Step 2 — Watch the machinery introduce itself (~minute 14)

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

…and walks through discovery and tool calls (no initialize handshake). Compare yours
against the full captured run:
[student-follow-docs/step2.log](student-follow-docs/step2.log). (The
interleaved `INFO io.modelcontextprotocol...` lines are SDK logging — normal —
and the absolute repo paths in the log will differ on your machine.)

**Expect:**

- **STEP 1** — **24 tools** discovered. The frozen seven you already know
  (`list_requirements`, `get_requirement`, `validate_spec`,
  `refine_requirement`, `run_tests`, `get_tdd_state`, `start_refactor`) plus
  authoring/staging (`scenario_add`, `unit_test_create`, `changes_show`,
  `changes_commit`, `requirement_mark_implemented`, …) and inspect
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

That smoke test just did exactly what Cursor does: launch, discover, invoke.
That's all the MCP you need today.

To connect your own agent, the ready-to-run configuration lives at
[config/mcp.json](config/mcp.json):

```json
{
  "mcpServers": {
    "spec-driven-server": {
      "command": "bdd",
      "args": ["mcp", "serve", "--root", "${workspaceFolder}"]
    }
  }
}
```

Cursor users get this automatically — the repo ships `.cursor/mcp.json`
(same as [`config/mcp.json`](config/mcp.json)). For every other client
(Claude Desktop, Claude Code, Codex, VS Code, Windsurf, Gemini CLI), see
[student-follow-docs/setup-mcp.md](student-follow-docs/setup-mcp.md).

---

## Step 3 — Confirm your agent is connected

The repo already registers the server for you in `.cursor/mcp.json`. Open
Cursor's MCP settings (see
[student-follow-docs/setup-mcp.md](student-follow-docs/setup-mcp.md) for
where to find them) and confirm `spec-driven-server` shows **green**. If it's red:
`bdd` is not on PATH for GUI apps (launch Cursor from a terminal where
`bdd --version` works, or put the absolute binary path in `command`), then
toggle the server off/on in the settings.

A green light says the server *launched* — now prove the agent can actually
*call* it. Paste this into your agent:

```text
Call the get_tdd_state tool from the spec-driven-server server and show me the raw JSON result.
```

The tool returns exactly this on a freshly started server:

```json
{
  "phase" : "START",
  "lastRun" : {
    "skipped" : 0,
    "tests" : 0,
    "failures" : 0,
    "errors" : 0
  },
  "refactorLog" : [ ],
  "nextStep" : "No tests have been run yet. Call run_tests to establish a baseline."
}
```

(Your agent may wrap the JSON in its own prose, and if you re-run this after
the server has already run tests the `phase` and counts will differ — that's
fine. `get_tdd_state` is read-only, so this check never disturbs your run.)

**Expect:**

- The agent invokes `get_tdd_state` on the `spec-driven-server` server — you'll
  see the tool call in the chat, no permission errors.
- `"phase": "START"` with an all-zero `lastRun` — the server is up and
  nothing has touched the kata yet.
- The `nextStep` hint pointing at `run_tests` — the server coaching the
  agent through the workflow, which is the whole trick of Exercises 1 and 2.

If the agent says it can't find the tool, the connection is the problem, not
the agent: re-check the green light, confirm `bdd --version` in a terminal,
and toggle the server off/on.

---

## Step 4 — Exercise 1: draft and refine the spec (minutes 20–32)

Paste this into your agent, word for word:

```text
Add a new requirement to requirements/requirements.json: newlines may separate numbers in addition to commas. Follow the existing format — unique id, title, user story, acceptance criteria phrased Given/When/Then, status pending. Then call validate_spec and fix every issue until the spec is valid. Then call refine_requirement on the new requirement and reword it from the findings until there are none. Do not write scenarios or code yet — we are only agreeing on the spec.
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
     "valid" : true,
     "issues" : [ ],
     "nextStep" : "The spec is valid. Call get_requirement for a pending requirement and write its Gherkin scenario from the acceptance criteria."
   }
   ```

3. `refine_requirement` → findings in the **tool reply**. A happy-path-only
   draft gets something very close to this (the findings list echoes
   whatever the refiner spots in *your* agent's wording, so yours may have
   more or different entries):

   ```json
   {
     "id" : "REQ-007",
     "clean" : false,
     "findings" : [ "criteria: only happy paths - add at least one edge case (empty, invalid, or error input)" ],
     "nextStep" : "Refine the wording in the requirements file to address each finding, run validate_spec, then call refine_requirement again. Iterate until there are no findings."
   }
   ```

   The agent rewords the JSON, re-validates, re-refines. Done looks like
   this (only the `id` varies):

   ```json
   {
     "id" : "REQ-007",
     "clean" : true,
     "findings" : [ ],
     "nextStep" : "The wording reads clean. Confirm it with the developer, then write the Gherkin scenario from the acceptance criteria."
   }
   ```

   (If your agent's first draft already includes an edge case, the
   `"clean": false` round never happens — that's fine, demo B below shows
   you the findings loop on demand.)

4. **Your checkpoint:** read the story and criteria aloud. Is this what we
   meant? You own the intent — approve it or redirect the agent with one
   sentence. Approving does **not** change `status`; leave it `pending`.

For both demos below, **you** make the breaking edit by hand — don't ask
the agent to do it. An agent asked to write bad wording tends to fix it on
the way to disk (or skip the edit entirely), and then the tool correctly
reports everything is fine and the demo never fires. Human breaks the spec,
tool catches it, agent repairs it.

**Optional demo A — structure loop (`validate_spec`)**

1. In `requirements/requirements.json`, edit the first REQ-007 criterion
   yourself to exactly this, then save the file:

   ```text
   the result should be 6 for 1\n2,3
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
     "valid" : false,
     "issues" : [ "REQ-007: criterion \"the result should be 6 for 1\\n2,3\" must be phrased Given/When/Then" ],
     "nextStep" : "Fix the issues in the requirements file, then call validate_spec again. Iterate until valid is true before writing scenarios or code."
   }
   ```

4. Now let the agent off the leash: ask it to repair the criterion and call
   `validate_spec` again until `"valid": true`.

**Optional demo B — wording loop (`refine_requirement`)**

1. In `requirements/requirements.json`, replace the REQ-007 story yourself
   with exactly this, then save the file (only the story — leave the
   criteria alone):

   ```text
   the calculator should handle newlines quickly
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
     "id" : "REQ-007",
     "clean" : false,
     "findings" : [ "story: missing the actor - start with 'As a ...' so we know who this is for", "story: missing the why - finish with 'so that ...' so the value is explicit", "story: 'should' is ambiguous - describe the observable behavior instead", "story: 'handle' is ambiguous - describe the observable behavior instead", "story: 'quickly' is ambiguous - describe the observable behavior instead" ],
     "nextStep" : "Refine the wording in the requirements file to address each finding, run validate_spec, then call refine_requirement again. Iterate until there are no findings."
   }
   ```

4. Now let the agent reword from the findings, then re-run `validate_spec`
   and `refine_requirement` until `"clean": true`. The failure is the
   lesson.

**Optional demo C — the spec is a catalog (includes)**

`requirements/requirements.json` is always the entry point, but it does
not have to hold every requirement itself: it can carry an `includes`
list of child spec files, and children can include further files, N
levels deep. The tools merge the whole tree into one backlog. To see it:

1. Create `requirements/newlines.json` yourself with just REQ-007 in it
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
   "includes": ["newlines.json"],
   ```

3. Ask the agent:

   ```text
   Call list_requirements and validate_spec and show me the raw JSON results — do not edit any files.
   ```

4. Expect REQ-007 still listed (merged from the included file, after
   REQ-001..006) and `"valid": true`. One catalog, many files — ids stay
   unique across the whole tree, and a duplicated id or an include cycle
   would come back as a validation issue naming the offending file.
   Undo the split (or leave it — every later step works the same) before
   moving on if you want your file to match the walkthrough exactly.

---

## Step 5 — Exercise 2: spec to green (minutes 32–52)

Paste this into your agent, word for word:

```text
Using the spec-driven-server tools: validate the spec first, then `get_requirement` for the next pending id. Add its Gherkin with `scenario_add` (tag the requirement id), add missing steps with `step_definition_create` if `step_definitions_find` reports any, add a unit test with `unit_test_create`. Show `changes_show` and ask me before `changes_commit`. Then `run_tests` (expect RED). Implement the simplest production code in `StringCalculator`. `run_tests` (GREEN). `start_refactor` if I agree. On GREEN, `requirement_mark_implemented`. Ask me before each phase change.
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
     "id" : "REQ-003",
     "title" : "Two numbers separated by a comma are summed",
     "status" : "pending",
     "story" : "As a user, I want comma-separated numbers to be summed so that I can add multiple values at once.",
     "acceptanceCriteria" : [ "Given \"1,2\", when add is called, then the result is 3", "Given \"10,20\", when add is called, then the result is 30" ],
     "featureLocation" : "kata/src/test/resources/features/string_calculator.feature",
     "stepDefinitions" : "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java",
     "testLocation" : "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java",
     "productionLocation" : "kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java",
     "workflowHint" : "Write the Gherkin scenario for this requirement in the feature file first (tag it @REQ-003), reuse or add step definitions, then run_tests to see RED."
   }
   ```

2. The agent stages two `@REQ-003` scenarios with `scenario_add` (and a
   unit test with `unit_test_create`). **Your checkpoint 1:** call
   `changes_show` (or have the agent show it) and read the staged Gherkin
   before you allow `changes_commit`. This is the spec review — is this the
   behavior you want?
3. `run_tests` → **RED** — 8 tests, 3 failing (2 Cucumber failures +
   1 JUnit error). The agent sees the same bar you do. The reply is similar
   to this (`failureDetails` stack traces trimmed here; the exact messages
   depend on the test names your agent chose):

   ```json
   {
     "phase" : "RED",
     "tests" : 8,
     "failures" : 2,
     "errors" : 1,
     "skipped" : 0,
     "failureDetails" : [ "String Calculator addition.Two numbers separated by a comma are summed: ... java.lang.NumberFormatException: For input string: \"1,2\" ...", "String Calculator addition.Two larger numbers separated by a comma are summed: ... java.lang.NumberFormatException: For input string: \"10,20\" ...", "com.davidparry.workshop.kata.StringCalculatorTest.twoCommaSeparatedNumbersAreSummed: For input string: \"1,2\"" ],
     "nextStep" : "Tests are failing. Write the simplest production code that makes them pass, then call run_tests again."
   }
   ```

4. The agent implements the simplest `StringCalculator.add` that passes
   (**a file edit** — there is no `implement` MCP tool). **Your checkpoint:**
   review the production diff.
5. `run_tests` → **GREEN** — 8 tests, 0 failures:

   ```json
   {
     "phase" : "GREEN",
     "tests" : 8,
     "failures" : 0,
     "errors" : 0,
     "skipped" : 0,
     "failureDetails" : [ ],
     "nextStep" : "All tests pass. Either call start_refactor to clean up, or call get_requirement for the next pending requirement and write a failing test for it."
   }
   ```

6. `start_refactor` → cleanup → `run_tests` still GREEN (same reply as
   above). The `start_refactor` reply:

   ```json
   {
     "phase" : "REFACTOR",
     "nextStep" : "A refactor is in progress. Call run_tests to prove the refactor kept the bar green."
   }
   ```

   (Try asking for `start_refactor` while RED sometime — the server
   refuses: "Never refactor on a red bar." Discipline lives in the tool.)
7. On GREEN, `requirement_mark_implemented` flips REQ-003 to
   `"status": "implemented"`. The tool **refuses** off GREEN or without a
   tagged `@REQ-003` scenario — premature completion is a live refusal, not
   a prompt hope. If the agent edits the JSON by hand instead, send it back
   to the tool.
   **Your checkpoint 2:** approve the final diff. Two checkpoints, both
   yours — the staged scenario and the production code.

---

## Step 6 — Check your work

The repo can grade your run against the finished workshop (the `complete`
branch):

```bash
scripts/verify-workshop-run.sh check
```

**Expect all four PASS:**

```text
  PASS  REQ-003 status is 'implemented' in the spec
  PASS  REQ-007 wording matches the complete branch (status ignored)
  PASS  @REQ-003 scenarios match the complete branch (2 found, 2 expected)
  PASS  REQ-003 unit test matches the complete branch
```

Any FAIL line tells you exactly which artifact to revisit.

---

## Step 7 — Homework

- **REQ-004, REQ-005, REQ-006** are still `pending` in the spec — run
  Exercise 2's prompt again and the agent picks up the next one each time.
- **REQ-007** — the requirement *you* drafted — is waiting to be taken to
  green on the plane home.
- Compare your final state with `git diff complete` when you finish them all.

---

## Reset / start over

Everything the exercises touched lives in `kata/` and `requirements/`:

```bash
git checkout -- kata requirements     # rewind this branch to the start state
git clean -fd requirements            # drop any spec files you added (demo C's newlines.json)
```

or throw the branch away and re-cut it:

```bash
git checkout trunk && git branch -D workshop && git checkout -b workshop trunk
```

---

## If you get stuck

- **Build red:** pair with a neighbor first; the presenter won't debug from
  stage.
- **Cursor MCP connection red:** `bdd --version` must work. Launch Cursor
  from that terminal or put the absolute path to `bdd` in `command`, then
  toggle the server off/on in Cursor's MCP settings. Note: a server restart
  resets the TDD phase — have the agent call `run_tests` once before any
  `start_refactor`, or the server will refuse.
- **Agent goes sideways:** it happens. Undo its edits, clear the chat, and
  re-paste the prompt — or follow the presenter's fallback on screen.

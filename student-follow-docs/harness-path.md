# Finish the String Calculator workshop with `spec`

The 60-minute hour in [student-follow-along.md](../student-follow-along.md)
uses Cursor against **the same** `spec mcp serve` (all 25 tools, including
staging). The [pi path](pi-path.md) points a free, local, general-purpose
agent at that same server. This page is the same end state — every
requirement `implemented`, including Exercise 1’s **REQ-007** — driven with
`spec` commands instead.

The difference is not the tools and not the language. All three paths call
one MCP server. The difference is who supplies the workflow: with Cursor or
pi you do, through prompting and skills; `spec` is a **spec-specific runner**
where the sequence, the tool profile for each step, and the phase gates are
already encoded. Narrower on purpose.

This page is the reference: terse command recipes plus the design notes
behind them. Its follow-along companion,
[spec-binary-follow-along.md](spec-binary-follow-along.md), runs the same
commands one at a time with the expected reply after each, the two human
checkpoints marked, the interactive `spec reword` wizard written out
prompt by prompt, and realistic timings for the model-backed commands.
Start there if this is your first run; come back here for the why.

Do **not** work on `trunk`. `scripts/check-workshop-start.sh` must keep
passing there.

## Install

From the repository root, after a Rust toolchain is on your PATH:

```bash
cargo install --path harness
spec --version
```

Requires `spec` **0.5.4 or newer** — check with `spec --version`. The
generation behavior this page describes, where the polish pass sees only
the newly generated members, arrived in 0.5.2; 0.5.4 is the floor because
several things this page states as fact are only true from it. The ones
that change what you do, rather than what you read:

- `spec validate` **exits nonzero when `valid` is false.** It used to
  report the failure and exit 0, so a CI gate scripted on it passed on an
  invalid spec.
- `spec scenario add` **preserves trailing content.** It used to delete
  everything after the last scenario — this kata's own closing comment
  block disappeared on the first add — and `changes show` never mentioned
  it.
- `spec refine` **reads staged-first**, so the reword/refine loop needs no
  `spec changes commit` between passes. It used to read the committed file
  and could report `clean: true` over a staged edit that was not.
- `spec model use` **edits one key** instead of re-rendering `.spec.toml`,
  which used to destroy every comment in it on the first command of
  Step 1.
- **Prompts are visible while a spinner runs.** `spec implement`'s
  `command_run` confirmation used to be redrawn over indefinitely, so the
  command looked hung when it was waiting on you.

A local [Ollama](https://ollama.com) model is optional. It is only
required for `spec implement`. Without one, implement
`StringCalculator.java` by hand after the tests go RED. The model this
talk and workshop run against is `qwen3.8-flash-next:125b-mlx`. Pin it with
`spec model use` rather than relying on discovery — with nothing configured
`spec` borrows whichever model Ollama lists first, which is usually not this
one, and the run proceeds without complaint.

```bash
# the local model this workshop and talk run against
ollama pull qwen3.8-flash-next:125b-mlx
spec model use qwen3.8-flash-next:125b-mlx
```

## Same server, narrower tools

```bash
spec tools profiles
# spec-draft          4  get_requirement, list_requirements, refine_requirement, validate_spec
# spec-reword         3  get_requirement, refine_requirement, validate_spec
# steps-generate      4  feature_list, feature_read, project_inspect, step_definitions_find
# unittest-generate   4  feature_read, get_requirement, project_inspect, step_definitions_find
# implement-advice    5  changes_show, changes_validate, feature_list, get_tdd_state, validate_spec
# implement           7  … command_run, changes_show
# status              7  … validate_spec, changes_show, changes_validate
# ask                12  the read-only set

spec mcp call get_tdd_state          # bytes the model would read, no tokens
spec mcp tools                       # the 25 built-ins
```

Cursor would have seen all 25. A generating command offers 3–7; the read-only
`ask` offers 12. Default profiles contain no staging mutation or commit
tools — `changes_show` and `changes_validate` are in several of them, but
both only read. The only
mutation a harness-side model may request is `command_run` on the `implement`
profile, and that call still asks you to confirm (piped/CI stdin declines; it
never hangs).

`validate_spec` / `refine_requirement` during `spec draft` inspect the
**disk** catalog. They do **not** critique the in-flight proposal. The gate
is still `parse_proposals_checked`.

`spec test` / `run_tests` during `spec implement` see the **working tree**,
not an unstaged patch. Commit before you trust the bar.

## Files this loop must reuse

Do not invent parallel classes. Generation and implement write into the
kata files this repository already has:

| Role | Path |
| --- | --- |
| Feature | `kata/src/test/resources/features/string_calculator.feature` |
| Steps | `kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java` |
| Unit tests | `kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java` |
| Production | `kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java` |

Nothing in that table is configured. The harness discovers it: this
repository is a Maven aggregator with two buildable modules (`kata/` and
`smoke-test/`), and the feature file your requirements name is the tiebreak
that picks `kata`. Everything else — test root, production root, features
directory, the package generated code declares, and the `pom.xml` `spec test`
runs — follows from that one answer. `spec show` and `.spec-memory.json`
report it, and `spec inspect` re-scans.

When a tree is genuinely ambiguous (several modules, and the spec names
features in none of them) the interactive shell asks **once**: the model
proposes one of the discovered module roots, you confirm, and the answer is
recorded in `.spec-memory.json`. No model, or a declined prompt, leaves the
scan's own pick in place and says which one it used.

Existing steps already bind `Given a string calculator`, `When I add {string}`,
`Then the result is {int}`, and `Then an IllegalArgumentException is thrown
with a message containing {string}`. Prefer those four wordings and
`spec steps generate` stays a no-op.

Any other wording is a real gap, and `spec steps generate` closes it: it
adds the missing definitions to the discovered `StringCalculatorSteps.java`,
keeping its package and class and skipping any pattern the file already
declares, so Cucumber never sees a duplicate expression. The polish pass
is scoped to the new definitions: the model is handed those members alone,
never the file they are spliced into, so it cannot rename a field or an
existing step method, and everything outside the insertion point is
carried over byte for byte. A reply that returns a whole file, alters a
generated step expression, or drops a definition is refused and the
deterministic members are staged instead (`"source": "template"`). The
assembled file is then re-checked: no pattern the file already declared
goes missing, and the package and class survive, so a passing scenario
cannot be unbound. Review with `spec changes show`, commit, and fill in
the `PendingException` bodies.

`spec unittest generate` is scoped the same way when the test class
already exists — only the new `@Test` methods reach the model.

Scoping protects the file's **content**, not always its indentation: a
generated method occasionally arrives at column 0, which absorbs the
class's closing brace into it. Braces balance and it compiles, so every
gate passes; it is cosmetic, and it is intermittent. Two of five
requirements in the measured run came back that way.

On the template path, member names derived from two criteria that differ
only in punctuation collide, and the later ones take a `_2` / `_3`
suffix. The LLM path invents semantic names and does not. A `_2` in
generated members is a reliable tell for `"source": "template"`.

`spec scenario add` appends and does not rewrite: new scenarios are
inserted **after the last scenario and before any trailing block**, so
the feature file's header, its `As a / I want / So that` narrative, and
its closing comments all survive. Every spec document a command writes
ends in a trailing newline, so `\ No newline at end of file` in a diff
means an older build.

Every authoring command **stages** — including the interactive `spec draft`
wizard, with or without a model. Nothing reaches the working tree until
`spec changes commit`; decline the wizard's last prompt and the batch it
accepted stays in staging, where `spec changes show` and
`spec changes discard` can reach it. Review with `spec changes show`, then
`spec changes commit`. `spec test` runs Maven on the **working tree**, so
commit before you trust the bar. `spec mark-implemented` is allowed
only on GREEN. `spec implement` may offer `command_run`; confirm before it
spawns. Optional: `spec ask "which pending requirement next?"` (read-only
profile).

Refusals are worded for the phase you are in, not for RED: `spec refactor`
answers "No tests have been run yet — run them to find out where you are."
at START and "A refactor is already in progress — run the tests to close
it." during a refactor, rather than the red-bar sentence at every phase.
`spec state` reports where you are without touching anything, and leads
with `phase` rather than its `instructions` blob.

Every `nextStep` the shell prints names commands, not MCP tools: the
services word their advice for the agent path and the CLI rewrites
`start_refactor` into `spec refactor` at the one place it prints a reply.
Tool output you asked for explicitly with `spec mcp call` is passed
through verbatim and still speaks in tool names.

`spec changes show` names at most five edits per file and then states the
running total — `(8 edits in all, 3 not shown); ...` — so the review
surface can never understate what is about to be committed.

`spec implement REQ-00N` also warns when the code it staged looks like it
satisfies another requirement that is still `pending` — the drift this
workflow exists to prevent. It is a literal check (same quoted inputs, same
expected number), so it warns and never blocks: read the diff and decide.
It can also legitimately report `"staged": false`, when the model hands
back every file unchanged: the warning names the production file and the
`nextStep` sends you round again or to your editor. That is better than
the `"staged": true` it used to give, which sent you to review an empty
diff.

**On a pipe, only the wizards can lose work.** `spec draft` and
`spec reword` end on a confirmation, so an exhausted pipe declines it and
stages nothing; every other prompting command stages either way. Running
out of input is now distinct from pressing Enter — a short pipe stops
immediately, explains itself once on stderr
(`input is not readable - end of input (the pipe ran out): ...`), and
returns the same declined report and exit 0 that a typed `N` would. It
used to read the end of a pipe as a blank line, which is how the wording
review's `[r]eword again, [m]anual, [a]ccept` prompt looped forever.
Ctrl+D at a terminal wizard lands in the same place, rather than exiting
1 with an error.

## Step 1 — Branch and baseline

```bash
git checkout -b workshop trunk
spec validate                 # valid; featureFile paths exist in this repo
spec list                     # REQ-001/002 implemented; REQ-003..006 pending; no REQ-007
spec test                          # GREEN: 2 JUnit + 3 Cucumber
```

If you already have harness fixes on another branch, cut `workshop` from
that branch instead of `trunk` so `spec test` understands this repo.

## Step 2 — Exercise 1: draft REQ-007 (status stays `pending`)

```bash
spec draft \
  --title "Custom delimiter declared on the first line" \
  --story "As a calculator user, I want to declare a custom delimiter on the first line so that I can separate numbers with a character of my choosing." \
  --criterion 'Given the input "//+\n1+2", when add is called, then the result is 3' \
  --criterion 'Given an empty delimiter declaration "//\n1+2", when add is called, then an IllegalArgumentException is thrown'
spec changes show             # your checkpoint: read the staged requirement
spec refine REQ-007           # reads the staged draft, not the committed file
# if findings: spec reword REQ-007, then refine again — no commit between passes
spec changes commit           # once, when refine comes back clean
spec list                     # REQ-007 pending
```

Nothing in REQ-001..006 specifies a custom delimiter, so this draft earns
no duplicate warning. (Reuse an existing title or criterion verbatim and
`spec draft` warns you.) Do **not** mark REQ-007 implemented yet.

`spec refine` reads staged-first and says which copy it graded in a
`source` field of `"staged"` or `"working tree"`, so reword and refine
loop against the staged text and only need one commit at the end.
`spec validate` is the other half of the pair: it reads the **committed**
spec by contract, says so in its `nextStep` when a spec edit is staged,
and points at the staged-aware `spec changes validate`.

Supply `--title` without `--story` and `--criterion` and the error names
what you typed rather than what you left out:
`spec draft with --title also needs --story and --criterion. Give all
three, or none of them to be asked question by question.`

## Recipe used for every pending requirement

Two lines in this block are not busywork. `spec steps missing` is the
free pre-check that tells you whether `spec steps generate` has anything
to do, and the two `spec changes show` calls are the human checkpoints
the whole workflow exists for: the first reads the staged Gherkin and
tests before they become the RED bar, the second reads the production
diff before it becomes GREEN. Run them in that order and nothing reaches
the working tree unreviewed.

```text
spec show REQ-00N
spec scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-00N --name "<scenario>" \
  --step 'Given a string calculator' \
  --step 'When I add "<input>"' \
  --step 'Then the result is <n>'
# second criterion: another scenario add with the same --req
spec steps missing                 # empty? good. otherwise: spec steps generate
spec unittest generate REQ-00N     # appends StringCalculatorTest, does not create Req00NTest
spec changes show                  # your checkpoint: read the staged Gherkin and tests
spec changes commit
spec test                          # expect RED
spec implement REQ-00N             # or edit StringCalculator.java by hand
spec changes show                  # your checkpoint: read the production diff
spec changes commit && spec test    # GREEN
spec refactor --note "<what>" --req REQ-00N    # optional, GREEN only
git diff && spec test                          # read it, then record the run
spec mark-implemented REQ-00N
spec changes commit
spec list
```

## Step 3 — Exercise 2: take REQ-003 to `implemented`

```bash
spec show REQ-003
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
spec steps missing                 # [] — all four wordings are already bound
spec unittest generate REQ-003
spec changes show                  # checkpoint 1: the staged Gherkin and tests
spec changes commit
spec test                          # RED: 9 tests, 4 failures
spec implement REQ-003             # or edit StringCalculator.java by hand
spec changes show                  # checkpoint 2: the production diff
spec changes commit && spec test   # GREEN: 9 tests, 0 failures
spec mark-implemented REQ-003 && spec changes commit
```

`spec implement` ends by offering to commit and run the tests for you.
Decline it the first time through — that prompt skips exactly the
checkpoint this exercise is for.

## Step 4 — Remaining pending: REQ-004, REQ-005, REQ-006, then REQ-007

Repeat the recipe. Suggested scenarios (reuse existing steps):

| Id | Scenario name | When I add | Then |
| --- | --- | --- | --- |
| REQ-004 | Any amount of numbers is summed | `"1,2,3,4,5"` | result is 15 |
| REQ-004 | All zeros sum to zero | `"0,0,0"` | result is 0 |
| REQ-005 | Newlines work as delimiters alongside commas | `"1\n2,3"` | result is 6 |
| REQ-005 | Newlines alone delimit numbers | `"4\n5\n6"` | result is 15 |
| REQ-006 | A negative number is rejected | `"1,-2"` | `Then an IllegalArgumentException is thrown with a message containing "negatives not allowed"` |
| REQ-006 | Every negative number is listed in the error | `"-1,-2"` | two further `Then`/`And` steps containing `"-1"` and `"-2"` |
| REQ-007 | A custom delimiter declared on the first line is used | `"//+\n1+2"` | result is 3 |
| REQ-007 | An empty delimiter declaration is rejected | `"//\n1+2"` | `Then an IllegalArgumentException is thrown` — the one wording with no step definition; `spec steps generate` adds it to `StringCalculatorSteps.java` |

No earlier requirement overlaps REQ-007 — it is the behavior you drafted in
Step 2, so both of its scenarios are new.

Gherkin cannot put a real newline inside `"…"`. Write `\n` in the
`When I add` string; `StringCalculatorSteps` unescapes it.

`+` is a regex metacharacter, so `"1+2".split("+")` throws
`PatternSyntaxException`. Take the RED bar first, then `Pattern.quote` the
delimiter.

## Step 5 — Done

```bash
spec list
# every id REQ-001 .. REQ-007 status: implemented
spec validate                      # "valid": true, exit 0
spec test                          # GREEN: 25 tests, 0 failures
```

That is the harness success bar: **every requirement status is
`implemented`**. `scripts/verify-workshop-run.sh check` grades the same
end state and is safe to run from this path: it asks whether each of
REQ-003's acceptance criteria reaches a scenario tagged `@REQ-003` and an
assertion in a `@Test` that names the requirement, so the names
`spec scenario add` and `spec unittest generate` produce are fine. Watch the
scenario count — one `spec scenario add` per criterion, and REQ-003 has
two.

Check it before the stretch below, which deliberately adds a pending
REQ-008 and so moves you off that bar.

### Stretch: split the spec into a catalog

`requirements/requirements.json` is the root of a spec catalog: it can
hold requirements of its own plus an `includes` list of child spec
files (which can include further files, N levels deep). Every command
above works on the merged view, `spec list` names the file each
requirement lives in, and mutations write back to that file:

```bash
spec include add requirements/newlines.json   # stages the include + an empty file
spec changes commit
spec draft --file requirements/newlines.json  # drafts REQ-008 into the child file
spec refine REQ-008                           # read the findings before committing
# if findings: spec reword REQ-008, then refine again
spec changes commit
spec list                                     # one merged backlog, per-file provenance
spec validate                                 # validates the whole tree as one catalog
```

Do not skip the refine. A one-criterion happy-path draft earns
`criteria: only happy paths - add at least one edge case (empty, invalid,
or error input)`, and REQ-008 is the easiest place in the workshop to
leave that open: `verify-workshop-run.sh` does not grade REQ-008, so
nothing downstream catches it. The wizard form above puts the finding to
you before it stages. The flag form
(`spec draft --file ... --title ... --story ... --criterion ...`) cannot
ask, so it stages regardless and carries the finding back in a `findings`
array with a `nextStep` of `Staged REQ-008 with refine findings. Run spec
reword REQ-008 to address them, then spec changes commit.` Either way,
reword it before you commit.

REQ-008 is left `pending` on purpose — it is the next kata, not part of
the success bar above. The format reference lives in the manual:
[The requirements format](../manual/src/spec-format.md).

Both catalog-structure failures have their own guidance, and it is the
one place the tool tells you to edit a spec file by hand. A duplicate id
answers `no tool can delete a requirement, so open the spec file the
issue names and remove the duplicate requirement object, or give it an id
nothing else uses`; a file reached twice answers `no tool can remove an
include, so open the parent spec file the issue names and delete the
repeated entry from its "includes" array`. Both close with `the rule
against hand-editing covers wording, not catalog structure`. Neither is
reachable with `spec reword`, which is what the advice used to name.

## Reset

Same as the follow-along:

```bash
git checkout -- kata requirements
```

or throw the branch away. Do not merge kata completion to `trunk`.

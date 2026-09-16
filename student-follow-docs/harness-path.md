# Finish the String Calculator workshop with `bdd`

The 60-minute hour in [student-follow-along.md](../student-follow-along.md)
uses Cursor against **the same** `bdd mcp serve` (all 25 tools, including
staging). The [pi path](pi-path.md) points a free, local, general-purpose
agent at that same server. This page is the same end state — every
requirement `implemented`, including Exercise 1’s **REQ-007** — driven with
`bdd` commands instead.

The difference is not the tools and not the language. All three paths call
one MCP server. The difference is who supplies the workflow: with Cursor or
pi you do, through prompting and skills; `bdd` is a **spec-specific runner**
where the sequence, the tool profile for each step, and the phase gates are
already encoded. Narrower on purpose.

Do **not** work on `trunk`. `scripts/check-workshop-start.sh` must keep
passing there.

## Install

From the repository root, after a Rust toolchain is on your PATH:

```bash
cargo install --path harness
bdd --version
```

A local [Ollama](https://ollama.com) model is optional. It is only
required for `bdd implement`. Without one, implement
`StringCalculator.java` by hand after the tests go RED. The model this
talk and workshop run against is `qwen3.8-flash-next:125b-mlx`.

```bash
# the local model this workshop and talk run against
ollama pull qwen3.8-flash-next:125b-mlx
bdd model use qwen3.8-flash-next:125b-mlx
```

## Same server, narrower tools

```bash
bdd tools profiles
# spec-draft          4  get_requirement, list_requirements, refine_requirement, validate_spec
# spec-reword         3  get_requirement, refine_requirement, validate_spec
# steps-generate      4  feature_list, feature_read, project_inspect, step_definitions_find
# unittest-generate   4  feature_read, get_requirement, project_inspect, step_definitions_find
# implement-advice    5  changes_show, changes_validate, feature_list, get_tdd_state, validate_spec
# implement           7  … command_run, changes_show
# status              7  … validate_spec, changes_show, changes_validate
# ask                12  the read-only set

bdd mcp call get_tdd_state          # bytes the model would read, no tokens
bdd mcp tools                       # the 25 built-ins
```

Cursor would have seen all 25. A generating command offers 3–7; the read-only
`ask` offers 12. Default profiles contain no staging or commit tools. The only
mutation a harness-side model may request is `command_run` on the `implement`
profile, and that call still asks you to confirm (piped/CI stdin declines; it
never hangs).

`validate_spec` / `refine_requirement` during `bdd spec draft` inspect the
**disk** catalog. They do **not** critique the in-flight proposal. The gate
is still `parse_proposals_checked`.

`bdd test` / `run_tests` during `bdd implement` see the **working tree**,
not an unstaged patch. Commit before you trust the bar.

## Files this loop must reuse

Do not invent parallel classes. Generation and implement look for the
existing kata files:

| Role | Path |
| --- | --- |
| Feature | `kata/src/test/resources/features/string_calculator.feature` |
| Steps | `kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java` |
| Unit tests | `kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java` |
| Production | `kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java` |

Existing steps already bind `Given a string calculator`, `When I add {string}`,
`Then the result is {int}`, and the negatives exception step. Prefer those
wordings so `bdd steps generate` is a no-op.

Every authoring command **stages**. Review with `bdd changes show`, then
`bdd changes commit`. `bdd test` runs Maven on the **working tree**, so
commit before you trust the bar. `bdd spec mark-implemented` is allowed
only on GREEN. `bdd implement` may offer `command_run`; confirm before it
spawns. Optional: `bdd ask "which pending requirement next?"` (read-only
profile).

## Step 1 — Branch and baseline

```bash
git checkout -b workshop trunk
bdd spec validate                 # valid; featureFile paths exist in this repo
bdd spec list                     # REQ-001/002 implemented; REQ-003..006 pending; no REQ-007
bdd test                          # GREEN: 2 JUnit + 3 Cucumber
```

If you already have harness fixes on another branch, cut `workshop` from
that branch instead of `trunk` so `bdd test` understands this repo.

## Step 2 — Exercise 1: draft REQ-007 (status stays `pending`)

```bash
bdd spec draft \
  --title "Custom delimiter declared on the first line" \
  --story "As a calculator user, I want to declare a custom delimiter on the first line so that I can separate numbers with a character of my choosing." \
  --criterion 'Given the input "//+\n1+2", when add is called, then the result is 3' \
  --criterion 'Given an empty delimiter declaration "//\n1+2", when add is called, then an IllegalArgumentException is thrown'
bdd changes commit
bdd spec refine REQ-007
# if findings: bdd spec reword REQ-007 && bdd changes commit, then refine again
bdd spec list                     # REQ-007 pending
```

Nothing in REQ-001..006 specifies a custom delimiter, so this draft earns
no duplicate warning. (Reuse an existing title or criterion verbatim and
`bdd spec draft` warns you.) Do **not** mark REQ-007 implemented yet.

## Recipe used for every pending requirement

```text
bdd spec show REQ-00N
bdd scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-00N --name "<scenario>" \
  --step 'Given a string calculator' \
  --step 'When I add "<input>"' \
  --step 'Then the result is <n>'
# second criterion: another scenario add with the same --req
bdd unittest generate REQ-00N     # appends StringCalculatorTest, does not create Req00NTest
bdd changes commit
bdd test                          # expect RED
bdd implement REQ-00N             # or edit StringCalculator.java by hand
bdd changes commit && bdd test    # GREEN
bdd refactor --note "<what>" && bdd test    # optional, GREEN only
bdd spec mark-implemented REQ-00N
bdd changes commit
bdd spec list
```

## Step 3 — Exercise 2: take REQ-003 to `implemented`

```bash
bdd spec show REQ-003
bdd scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Two numbers separated by a comma are summed" \
  --step 'Given a string calculator' \
  --step 'When I add "1,2"' \
  --step 'Then the result is 3'
bdd scenario add --feature kata/src/test/resources/features/string_calculator.feature \
  --req REQ-003 --name "Two larger numbers separated by a comma are summed" \
  --step 'Given a string calculator' \
  --step 'When I add "10,20"' \
  --step 'Then the result is 30'
bdd unittest generate REQ-003
bdd changes commit
bdd test                          # RED
bdd implement REQ-003 && bdd changes commit && bdd test    # GREEN, or edit by hand
bdd spec mark-implemented REQ-003 && bdd changes commit
```

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
| REQ-007 | An empty delimiter declaration is rejected | `"//\n1+2"` | `Then an IllegalArgumentException is thrown` |

No earlier requirement overlaps REQ-007 — it is the behavior you drafted in
Step 2, so both of its scenarios are new.

Gherkin cannot put a real newline inside `"…"`. Write `\n` in the
`When I add` string; `StringCalculatorSteps` unescapes it.

`+` is a regex metacharacter, so `"1+2".split("+")` throws
`PatternSyntaxException`. Take the RED bar first, then `Pattern.quote` the
delimiter.

## Step 5 — Done

```bash
bdd spec list
# every id REQ-001 .. REQ-007 status: implemented
bdd spec validate
bdd test                          # GREEN
```

### Stretch: split the spec into a catalog

`requirements/requirements.json` is the root of a spec catalog: it can
hold requirements of its own plus an `includes` list of child spec
files (which can include further files, N levels deep). Every command
above works on the merged view, `bdd spec list` names the file each
requirement lives in, and mutations write back to that file:

```bash
bdd spec include add requirements/newlines.json   # stages the include + an empty file
bdd changes commit
bdd spec draft --file requirements/newlines.json  # drafts REQ-008 into the child file
bdd spec list                                     # one merged backlog, per-file provenance
bdd spec validate                                 # validates the whole tree as one catalog
```

The format reference lives in the manual:
[The requirements format](../manual/src/spec-format.md).

That is the harness success bar: **every requirement status is
`implemented`**. `scripts/verify-workshop-run.sh check` grades the same
end state and is safe to run from this path: it asks whether each of
REQ-003's acceptance criteria reaches a scenario tagged `@REQ-003` and an
assertion in a `@Test` that names the requirement, so the names
`bdd scenario add` and `bdd unittest generate` produce are fine. Watch the
scenario count — one `bdd scenario add` per criterion, and REQ-003 has
two.

## Reset

Same as the follow-along:

```bash
git checkout -- kata requirements
```

or throw the branch away. Do not merge kata completion to `trunk`.

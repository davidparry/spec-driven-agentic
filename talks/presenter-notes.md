# A Day in the Life of a TDD Agentic Developer — presenter notes

Stage script for the 60-minute talk. The demo builds the harness's own
26th MCP tool, `criteria_coverage`, from a requirement that starts vague.
Everything below is the presenter's copy; the attendee version is
[a-day-in-the-life.md](../student-follow-docs/a-day-in-the-life.md).

The clock times are the story, not the wall clock. The whole demo is
about 28 minutes.

## Before you walk on

```bash
cd <repo>
scripts/preflight.sh                       # model, binary, toolchains
spec --root harness validate               # must be valid: true
spec --root harness refine HARNESS-013     # must report 10 findings
spec --root harness list                   # HARNESS-013 is the only pending one
cargo test --manifest-path harness/Cargo.toml --test spec_completeness
mvn -f smoke-test/pom.xml test -Dspec.binary=$(which spec)
```

**The `-Dspec.binary` flag is not optional.** `LiveSpecServerTest` is
annotated `@EnabledIfSystemProperty(named = "spec.binary", ...)`. Without
it that test is silently skipped, the Java build stays green when the
server grows a 26th tool, and the best beat in the talk does not fire.
Verified: 25 tools green, 26 tools `BUILD FAILURE`.

The starting state is: 13 implemented requirements, one vague pending
draft, 25 tools served, every bar green. If `spec_completeness` is red
before you start, the catalog and the feature tags have drifted — fix
that, do not demo around it.

## 9:00 — the ticket lands

```bash
spec --root harness status
spec --root harness show HARNESS-013
```

Point out that the catalog, not the chat history, is what names the next
piece of work. HARNESS-013 is titled "Criteria coverage" and says almost
nothing.

## 9:10 — it is vague, and the tool says so

```bash
spec --root harness validate      # valid: true
spec --root harness refine HARNESS-013
```

The beat to land: **the spec is structurally valid and still unusable.**
Validation says the shape is right. The wording review returns 10
findings — six on the story, three on the criteria, one on coverage.
Read two of them out loud:

- `story: 'properly' is ambiguous - describe the observable behavior instead`
- `criteria: only happy paths - add at least one edge case (empty, invalid, or error input)`

Nobody would generate a test from this. That is the point.

### This beat is yours

The human rewrites. Paste this — it is verified refine-clean, so the
second pass comes back `clean: true` with no findings:

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
Given a requirement whose every criterion is matched by a tagged scenario and an asserting test, when coverage is requested, then the verdict is "covered"
Given a requirement with 3 criteria of which 1 is matched by no asserting test, when coverage is requested, then 1 criterion is reported uncovered
Given a requirement id that is absent from the spec, when coverage is requested, then the reply is an error naming the unknown id
Given a requirement carrying 0 acceptance criteria, when coverage is requested, then the verdict is "uncovered"
```

```bash
spec --root harness reword HARNESS-013
spec --root harness changes show       # read the diff out loud
spec --root harness changes commit
spec --root harness refine HARNESS-013 # clean: true
```

## 9:30 — the criteria become an executable scenario

```bash
spec --root harness scenario add --feature tests/features/tool_coverage.feature --req HARNESS-013
spec --root harness changes show
```

The scenario is written **from the acceptance criteria** and tagged
`@HARNESS-013`. Nobody re-typed the requirement into a test.

### This beat is yours

Read the staged diff before it lands. Then:

```bash
spec --root harness changes commit
```

## 9:50 — the unit test

```bash
spec --root harness unittest generate HARNESS-013
spec --root harness changes show
spec --root harness changes commit
```

## 10:00 — RED

```bash
spec --root harness test
spec --root harness state      # phase: RED
```

A red bar is not a failure, it is proof the test can fail. Say it.

## 10:05 — ask for a cleanup and get told no

```bash
spec --root harness refactor --note "tidy the coverage module"
```

Refused: `Never refactor on a red bar`. This is a state machine in
`harness/src/domain/tdd.rs`, not a line in a prompt. An agent cannot
talk its way past it.

## 10:15 — the developer agent writes the code

```bash
spec --root harness implement HARNESS-013
```

What it is allowed to touch: no shell, no free-hand write. The
implementation lands in staging.

### This beat is yours

```bash
spec --root harness changes show       # review it properly, out loud
spec --root harness changes commit
```

## 10:40 — GREEN

```bash
spec --root harness test
spec --root harness state      # phase: GREEN
```

## 10:50 — CI catches what you forgot

This is the moment the room should enjoy. The server now answers with 26
tools. Nobody told the Java smoke test.

```bash
mvn -f smoke-test/pom.xml test -Dspec.binary=$(which spec)
```

`LiveSpecServerTest` fails: *the live spec binary serves exactly the 25
planned tools*. The sweep reports `criteria_coverage` as **unexpected** —
a tool the server answers with that no one planned for. `CLI-009` was
written for exactly this: "a 26th MCP tool fails the Java build until it
is planned". One module's spec caught new surface area in another, and
nobody had to remember to look.

Fix it in front of them: add `criteria_coverage` to `ToolPlan`, bump the
count in `ToolPlanTest` and in `CLI-009`'s criteria and its tagged
scenario, rerun, green.

## 11:05 — close it out

```bash
spec --root harness mark-implemented HARNESS-013
```

Gated twice: GREEN, plus a scenario carrying the tag. Then:

```bash
spec --root harness changes commit
cargo test --manifest-path harness/Cargo.toml --test spec_completeness
```

The drift gate now covers 14 requirements, and HARNESS-013's wording is
checked because it is implemented.

## 11:15 — the close

Run the tool you just built, on the requirement you just wrote:

```bash
spec --root harness mcp call criteria_coverage --arg id=HARNESS-013
```

Every acceptance criterion written at 9:10 has an asserting test. The
morning's work grades itself.

## If it goes wrong

- **The model stalls on `implement`.** Write the code by hand and keep
  talking. The point of the segment is the gates, not the generation.
- **`refine` comes back clean on the first pass.** You are on the wrong
  branch; HARNESS-013 should be the vague draft.
- **`spec test` is slow.** The scoped filter should keep it to the one
  test binary. If it is running the whole suite, pass the filter
  explicitly rather than waiting.
- **Reset to the stage state:** see below. `git checkout` alone is not
  enough — the demo leaves two new files and a rebuilt binary behind.

## Reset, to give the talk again

The demo mutates tracked files, creates two untracked ones, writes TDD
state, and reinstalls the binary. Undo all four, in this order.

```bash
# 1. tracked edits: the reworded + implemented HARNESS-013, the
#    criteria_coverage registration, and the whole Java-side 26 bump
git checkout -- harness/ smoke-test/

# 2. files the demo created, which git checkout does not touch.
#    Once the stage state is committed, prefer the catch-all:
#      git clean -fd harness/ smoke-test/
rm -f harness/tests/features/tool_coverage.feature \
      harness/tests/harness_013_test.rs

# 3. staging and the TDD phase, or the next run starts mid-cycle
rm -rf harness/.spec/staged harness/.spec/state.json

# 4. the binary now serves 26 tools - put a 25-tool one back on PATH
cargo install --path harness --force
```

Step 2 is only safe as `git clean -fd` once the stage state is
committed; before that, `git clean` would take the catalog and the drift
gate with it. The explicit `rm -f` is safe either way.

The two named files are the ones the demo *always* creates. The
implementation itself is normally edits to tracked files under
`harness/src/` — `production_path` resolves to an existing source, and
the model is shown the files and hands them back rewritten, so step 1
reverts it. But `implement` stages whatever `{path, content}` pairs the
model returns, and nothing stops it inventing a new module. After a run
that went off-script, check `git status` for untracked files under
`harness/src/` before you trust the reset.

Step 4 is the one that is easy to forget and silently ruins the next
run: `mvn ... -Dspec.binary=$(which spec)` would stay red from the
previous talk, and `refine HARNESS-013` would come back clean.

Confirm you are back at the starting state:

```bash
spec mcp tools --json | python3 -c 'import sys,json;print(len(json.load(sys.stdin)))'   # 25
spec --root harness refine HARNESS-013    # 10 findings
spec --root harness list                  # HARNESS-013 pending, 13 implemented
mvn -f smoke-test/pom.xml test -Dspec.binary=$(which spec)   # green
```

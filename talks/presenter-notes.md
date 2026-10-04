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
git switch trunk && git pull
git switch -c talk-$(date +%Y%m%d)         # never demo on trunk
scripts/preflight.sh                       # model, binary, toolchains
spec --root harness validate               # must be valid: true
spec --root harness refine HARNESS-013     # must report 10 findings
spec --root harness list                   # HARNESS-013 is the only pending one
cargo test --manifest-path harness/Cargo.toml --test spec_completeness
mvn -f smoke-test/pom.xml test -Dspec.binary=$(which spec)
```

**Make the branch before anything else.** Every mutation of the next 28
minutes — the reworded catalog, the new feature file, the generated
test, the implementation, the Java-side bump — lands in the working
tree. On a branch, giving the talk again is one `git branch -D`. On
`trunk` it is an archaeology exercise, and the thing you will miss is
the catalog, which leaves `refine HARNESS-013` clean on the first pass
and kills the 9:10 beat.

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
spec --root harness feature create --path tests/features/tool_coverage.feature --name "Criteria coverage"
spec --root harness scenario generate HARNESS-013 --feature tests/features/tool_coverage.feature
spec --root harness changes show
```

`generate`, not `add`. `add` appends one scenario you have already
written out step by step; `generate` is the one that reads the
acceptance criteria and derives the scenarios from them — which is the
whole point of the beat. The feature file has to exist first, but a
staged file is readable by the next command, so there is no commit
between these two.

**This is the slow one.** About five minutes on the big local model, one
model call. Have something to say while it runs: this is the natural
place for the "who wrote the test" argument.

Four scenarios come back, written **from the acceptance criteria** and
tagged `@HARNESS-013` — one per criterion. Nobody re-typed the
requirement into a test.

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
spec --root harness steps missing
```

**Check the count, do not assume it.** Whether any steps come back
missing depends on how the model worded the scenarios — it may reuse
definitions the suite already has and report none, or invent new
phrasings and report a dozen. Both happen. If the list is not empty:

```bash
spec --root harness steps generate
spec --root harness changes commit
spec --root harness steps missing       # 0
```

One more model call, about a minute. The stubs bind the project's own
`SpecWorld`, read off the glue file rather than assumed — worth saying
out loud if anyone has been bitten by a generator that emitted the
`World` *trait* and would not compile.

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

The preflight prints where the code will land, and it should say
`src/mcp.rs`. Nothing declares that: the harness matches the
requirement's own When/Then steps to the step definitions that bind
them, follows those one hop into their helpers, and takes the production
file those name through the most distinct symbols. Worth ten seconds on
stage — it is the same "evidence, not configuration" argument the talk
makes about the spec.

**If it refuses here, that is still the argument.** When every step the
scenarios bind to is a pending `todo!()`, nothing names any production
code and the harness says so instead of picking a file. It needs two
independent names before it will commit, so one accidental word match
cannot decide it. Recover by naming the file yourself and carry on —
the refusal is a better story than a lucky guess:

```bash
spec --root harness implement HARNESS-013 --into src/mcp.rs
```

Say the scope out loud, because the slide now promises it: what lands is
**one `#[tool]` method** in `harness/src/mcp.rs`. That is a real tool
over the protocol and nothing more — no `spec coverage` subcommand, no
profile offering it to an agent, no prompt naming it. The homework slide
after the close covers all three.

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

## 11:20 — hand them the rest of it

One slide, and it is a confession: we shipped the tool, not the adoption.
Point at the three gaps and say which one you would do first.

```bash
spec --root harness tools enable criteria_coverage --for status
spec --root harness tools list --for status
```

That is the cheap one — no recompile, it persists in `.spec/config.toml`,
and it demos in ten seconds if you have time. The other two are a
`Command` arm in `main.rs` and four prompt edits
(`[tool_rules]`, step 8 of `workflow.md`, `[advice]`, `[mcp]`). The
follow-along doc spells out which callers to pick and, more usefully,
which to skip — `implement` itself being the trap: give the implementing
model a coverage oracle and it writes to the report instead of the test.

The line that lands: *a registered tool that no profile offers and no
prompt mentions is a tool nobody calls.*

## If it goes wrong

- **The model stalls on `implement`.** Write the code by hand and keep
  talking. The point of the segment is the gates, not the generation.
- **`refine` comes back clean on the first pass.** A previous run's
  reword survived. You branched from a dirty `trunk`, or you never reset
  the last talk. `git checkout trunk -- harness/requirements/` fixes it
  on the spot.
- **`spec test` is slow.** The scoped filter should keep it to the one
  test binary. If it is running the whole suite, pass the filter
  explicitly rather than waiting.
- **Reset to the stage state:** see below. Throwing the branch away is
  not enough on its own — the TDD phase and the rebuilt binary live
  outside git.

## Reset, to give the talk again

Three kinds of residue, and git only knows about the first.

```bash
# 1. everything the demo wrote, tracked and untracked alike
git switch trunk
git branch -D talk-<date>
git clean -fd harness/ smoke-test/

# 2. the TDD phase, staging, and cached model replies - all gitignored,
#    so step 1 leaves them behind and the next run starts mid-cycle
rm -rf harness/.spec/staged harness/.spec/state.json harness/.spec/cache

# 3. the binary on PATH now serves 26 tools - put a 25-tool one back
cargo install --path harness --force
```

Because the demo ran on its own branch, step 1 is the whole of the git
side: the reworded catalog, the new feature file, the generated test,
the implementation, and the Java-side 26 bump all go with the branch.
`git clean` is safe here precisely *because* you are back on `trunk`
with nothing of your own in the working tree — which is the other reason
to make the branch before you walk on. If you are rehearsing from a
working copy that *does* carry unpushed work, `git clean -nd harness/
smoke-test/` lists what would go before anything is removed.

Step 3 is the one that is easy to forget and silently ruins the next
run: `mvn ... -Dspec.binary=$(which spec)` would stay red from the
previous talk, and the 10:50 beat would never fire.

Confirm you are back at the starting state:

```bash
git status --short                        # clean
spec mcp tools | wc -l                    # 25
spec --root harness refine HARNESS-013    # clean: false, 10 findings
spec --root harness list                  # HARNESS-013 pending, 13 implemented
mvn -f smoke-test/pom.xml test -Dspec.binary=$(which spec)   # green
```

If `refine` comes back clean, the catalog survived the reset and the
9:10 beat is dead — you are still on the talk branch, or you gave the
talk on `trunk`.

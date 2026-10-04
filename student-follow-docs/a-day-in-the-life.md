# A Day in the Life of a TDD Agentic Developer — follow-along

Replay the talk's morning on your own machine. One requirement arrives
vague, and by the end it is implemented, proven, and traceable — and the
thing you built is the harness's own 26th MCP tool.

This is the attendee copy. The deck is at
[`talks/slides/index.html?tdd`](../talks/slides/index.html?tdd).

## What makes this different from the kata

The usual workshop drives the String Calculator. This one points the
harness at **itself**: `harness/requirements/requirements.json` is the
spec for the `spec` binary, its scenarios live in `harness/tests/features`,
and `harness/tests/spec_completeness.rs` fails the build when the two
drift apart.

Thirteen requirements are implemented. One, `HARNESS-013`, is pending and
deliberately badly written. You are going to fix that and then build what
it describes.

## Prerequisites

- `spec` on PATH (0.7.0 or newer) — `spec --version`
- Rust toolchain — `cargo --version`
- Java 21 and Maven, for the smoke test that catches you at the end
- Optional: Ollama with a local model pulled, if you want the generation
  steps to run offline

`harness/.spec/config.toml` pins the model the talk uses. Check what you
actually have and override for a run if it differs:

```bash
spec --root harness config        # shows the resolved model and where it came from
spec model list                   # what Ollama has pulled locally
```

Every generating command takes `--model <name>` for one run, so you do
not have to edit the config to follow along on a different model.

## Work on a branch

Everything that follows mutates the repository: the catalog gets
reworded, a feature file and a test appear, production code is written,
and the Java smoke test gets bumped. Put all of it on a branch you can
throw away, so you can run the morning a second time:

```bash
git clone https://github.com/davidparry/spec-driven-agentic
cd spec-driven-agentic
git switch trunk
git switch -c my-morning
```

Do not do this on `trunk`. The step that bites is the reword at 9:10 —
leave it in place and `refine` comes back clean next time, which is
exactly the finding the morning is built around.

Confirm the starting state:

```bash
spec --root harness list       # 13 implemented, HARNESS-013 pending
spec --root harness validate   # valid: true
cargo test --manifest-path harness/Cargo.toml --test spec_completeness
```

## 9:00 — read the ticket

```bash
spec --root harness status
spec --root harness show HARNESS-013
```

The catalog names the work. Note that `HARNESS-013` says almost nothing
useful: *"Criteria coverage"*.

## 9:10 — find out it is unusable

Two different questions, two different tools:

```bash
spec --root harness validate            # valid: true  — the shape is fine
spec --root harness refine HARNESS-013  # clean: false — 10 findings
```

Read the findings. Six are about the story, three about the criteria, one
about coverage. This is the point of the whole exercise: **a requirement
can be structurally valid and still be something nobody could write a
test from.**

### Your turn

Rewrite it until `refine` comes back clean. You can do this by hand, or
let an agent propose wording and keep iterating. If you want the version
the talk uses:

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
spec --root harness changes show      # read it
spec --root harness changes commit
spec --root harness refine HARNESS-013   # clean: true
```

Do not skip `changes show`. Every mutation the harness makes lands in
staging first; this is the first of three places the morning asks for
your judgment.

## 9:30 — turn the criteria into tests

The scenarios go in a feature file of their own, so it has to exist
before anything can be appended to it. Create it through the harness
rather than by hand — a staged file is already readable by the next
command, so all three edits are reviewed together at the end:

```bash
spec --root harness feature create --path tests/features/tool_coverage.feature --name "Criteria coverage"
spec --root harness scenario generate HARNESS-013 --feature tests/features/tool_coverage.feature
spec --root harness unittest generate HARNESS-013
spec --root harness changes show
spec --root harness changes commit
```

> These two are the only slow steps of the morning. On a local model
> expect `scenario generate` to take about five minutes and `unittest
> generate` about two. They are one model call each — if a step returns
> in a second, it was a cache hit from an earlier run, which is fine.

Note `scenario generate`, not `scenario add`. `add` appends one scenario
you have already written, step by step; `generate` is the one that reads
the acceptance criteria and derives the scenarios from them.

The scenarios are derived from the acceptance criteria and tagged
`@HARNESS-013`. Nobody re-typed the requirement into a test, which is
exactly how a spec and a suite stop disagreeing.

## 10:00 — red

```bash
spec --root harness test
spec --root harness state     # phase: RED
```

A red bar here is the proof the test can fail. A test written after the
code never gives you that.

Now try to tidy up:

```bash
spec --root harness refactor --note "tidy the coverage module"
```

Refused: `Never refactor on a red bar`. That is a state machine in
`harness/src/domain/tdd.rs`, not a line in a prompt.

## 10:15 — write the code

```bash
spec --root harness implement HARNESS-013
spec --root harness changes show
spec --root harness changes commit
spec --root harness test      # GREEN
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

**Scope check.** What lands here is one `#[tool(...)]` method on the
router in `harness/src/mcp.rs` — the 26th. That is enough to make it
real over the protocol: `spec mcp serve` will advertise it, and
`spec mcp call` can invoke it. It is *not* enough to make it a `spec`
subcommand, and no agent will reach for it until you say so. Both of
those are [homework](#homework-wire-it-into-the-cli) — the talk only has
time for the tool itself.

If the model stalls, write it yourself. The lesson is the gates, not the
generation.

## 10:50 — get caught

Your server now answers with 26 tools. The Java smoke test still expects
25.

The smoke test launches the `spec` on your PATH, not the source tree, so
install what you just wrote before you run it — otherwise the sweep
counts the 25 tools of the binary you started the morning with and the
build stays green for the wrong reason:

```bash
cargo install --path harness --force    # release build, a minute or two
spec mcp tools | wc -l                  # 26 now; it was 25 this morning
mvn -f smoke-test/pom.xml test -Dspec.binary=$(which spec)
```

`LiveSpecServerTest` fails, and the sweep reports `criteria_coverage` as
**unexpected**. That is `CLI-009` doing its job:

> As a maintainer, I want a 26th MCP tool to fail the Java build until it
> is planned so that the smoke test cannot silently skip new surface area.

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
spec --root harness mark-implemented HARNESS-013
spec --root harness changes commit
cargo test --manifest-path harness/Cargo.toml --test spec_completeness
```

`mark-implemented` is gated twice: the bar must be GREEN, and a scenario
must carry the requirement's tag. The drift gate is scoped to implemented
requirements, so it now covers 14 instead of 13 — and because
`HARNESS-013` is one of them, its wording is checked on every build from
here on.

## 11:15 — let the morning grade itself

```bash
spec --root harness mcp call criteria_coverage --arg id=HARNESS-013
```

Every acceptance criterion you wrote at 9:10 has an asserting test. The
tool you built reports on the requirement that asked for it.

Notice that you reached it through `mcp call`. That is the only door it
has so far, which is the subject of the next section.

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
spec --root harness tools enable criteria_coverage --for status
spec --root harness tools list --for status     # confirm it resolved
spec --root harness status                      # the advice can see coverage now
```

Three callers are worth it, for different reasons:

| Caller | Command | Why |
| --- | --- | --- |
| `implement-advice` | `spec implement` preflight | The preflight's whole job is saying whether `implement` can succeed. A criterion with no asserting test is exactly a reason it cannot — going green would be a false green. |
| `status` | `spec status` | The `next_step` prompt already renders per-requirement *gaps*. An uncovered criterion is the gap that decides between `spec unittest generate` and `spec implement`. |
| `ask` | `spec ask` | The read-only catch-all already holds every other non-mutating reader, so `spec ask "is HARNESS-013 covered?"` currently has to guess. |

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
spec --root harness coverage HARNESS-013      # does not exist yet
```

A new arm on `Command` in `harness/src/main.rs`, an application service
beside `spec_service`, and the wiring in `wiring.rs`. Nothing clever —
but it is the difference between a tool your agents can call and a tool
*you* can call.

And if you do all three, do it the way the morning did: write the
requirement first, refine it until it is clean, and let the criteria
become the tests.

## If you get stuck

- **`refine` is clean on the first pass** — you are not on a clean
  checkout of `trunk`; `HARNESS-013` should start vague.
- **`spec test` runs the whole suite** — pass `--feature
  tests/features/tool_coverage.feature` and the Cargo runner scopes the
  run to the one test target that owns that feature.
- **A generating step fails with a timeout** — the model is slower than
  the configured ceiling. Raise `timeout_seconds` in
  `harness/.spec/config.toml`, or pass `--model` and use a smaller one.
- **`implement` refuses with missing steps** — run `spec --root harness
  steps missing` and read the list. It should be empty before you
  implement; anything in it is a Gherkin step with no matching
  definition.

### Start over

Throw the branch away. That is why you made one — it takes the reworded
catalog, the feature file, the generated test, the implementation, and
the Java-side changes with it in one go:

```bash
# 1. everything the morning wrote, tracked and untracked alike
git switch trunk
git branch -D my-morning
git clean -fd harness/ smoke-test/

# 2. the TDD phase, staging, and cached replies are gitignored,
#    so step 1 leaves them behind and the next run starts mid-cycle
rm -rf harness/.spec/staged harness/.spec/state.json harness/.spec/cache

# 3. if you reinstalled the binary at 10:50, put a 25-tool one back
cargo install --path harness --force

# 4. confirm you are back at the start
git status --short                       # clean
spec --root harness refine HARNESS-013   # clean: false — 10 findings
```

If `refine` comes back clean, the reword survived — you are still on
`my-morning`, or you did the morning on `trunk`.

## Where to go next

- The harness's own spec: `harness/requirements/requirements.json`
- The drift gate: `harness/tests/spec_completeness.rs`
- The String Calculator version of this loop:
  [student-follow-along.md](student-follow-along.md)
- Running it all offline on a local model: [pi-path.md](pi-path.md)

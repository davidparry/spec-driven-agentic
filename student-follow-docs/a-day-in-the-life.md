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

```bash
git clone https://github.com/davidparry/spec-driven-agentic
cd spec-driven-agentic
git switch -c my-morning
```

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

```bash
spec --root harness scenario add --feature tests/features/tool_coverage.feature --req HARNESS-013
spec --root harness unittest generate HARNESS-013
spec --root harness changes show
spec --root harness changes commit
```

The scenario is derived from the acceptance criteria and tagged
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

If the model stalls, write it yourself. The lesson is the gates, not the
generation.

## 10:50 — get caught

Your server now answers with 26 tools. The Java smoke test still expects
25.

```bash
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
must carry the requirement's tag. The drift gate now covers 13
requirements — and because `HARNESS-013` is implemented, its wording is
checked on every build from here on.

## 11:15 — let the morning grade itself

```bash
spec --root harness mcp call criteria_coverage --arg id=HARNESS-013
```

Every acceptance criterion you wrote at 9:10 has an asserting test. The
tool you built reports on the requirement that asked for it.

## If you get stuck

- **`refine` is clean on the first pass** — you are not on a clean
  checkout of `trunk`; `HARNESS-013` should start vague.
- **`spec test` runs the whole suite** — pass `--feature` so the Cargo
  runner can scope to one test target.
- **Start over** — `git checkout -- harness/ smoke-test/ && spec --root harness changes discard`

## Where to go next

- The harness's own spec: `harness/requirements/requirements.json`
- The drift gate: `harness/tests/spec_completeness.rs`
- The String Calculator version of this loop:
  [student-follow-along.md](student-follow-along.md)
- Running it all offline on a local model: [pi-path.md](pi-path.md)

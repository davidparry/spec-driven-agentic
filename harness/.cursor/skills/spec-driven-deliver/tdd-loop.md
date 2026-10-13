# The loop for one requirement

Eight stages, in order, for a single id. Every stage is skipped when its
work is already in place and verified after it writes.

Start by reading the requirement:

```json
get_requirement { "id": "REQ-001" }
```

Use its `title`, `story`, and `acceptanceCriteria`. Discover all write
locations from `project_inspect`, the existing project, and the `target`
fields returned by mutation tools. Never infer a location from an
example or carry a path over from another project.

## 1. Scenario

Check first. `feature_list` for the feature files, then `feature_read` on
the likely one, looking for a scenario tagged `@REQ-001`. If one is
there, say so and skip to stage 2.

Otherwise, pick the feature file. Reuse the file the requirement's
neighbours use; create one only when the behaviour has no home:

```json
feature_create { "path": "<discovered-feature-path>", "name": "<feature-name>" }
```

`feature_create` refuses a path that already exists, so only call it when
`feature_list` did not show the file.

Then one scenario per acceptance criterion:

```json
scenario_add {
  "feature": "<discovered-feature-path>",
  "req": "REQ-001",
  "name": "<scenario-name>",
  "steps": [
    "Given <initial state>",
    "When <the behaviour occurs>",
    "Then <the exact expected outcome>"
  ]
}
```

Two rules the tool enforces:

- Each step is a **full Gherkin line** and must start with `Given `,
  `When `, `Then `, `And `, or `But `. Anything else is refused.
- The `@REQ-001` tag is written for you from `req`. Do not put it in the
  steps or the name.

Write the scenario from the criterion, not from the implementation you
have in mind. If a criterion names a quoted literal, the scenario uses
that same literal — matching is literal later on.

**Verify:** `feature_read` the file and confirm the tag is there.

**Self-check:** does this scenario actually exercise its criterion, or
does it merely mention it? If not, fix it now.

## 2. Step definitions

```json
step_definitions_find {}
```

Returns `{ language, framework, missing, nextStep }`. An empty `missing`
means stage 2 is done.

For anything missing:

```json
step_definition_create {}
```

Takes no arguments. It writes **templates** to the `target` path in the
reply, with framework-appropriate pending bodies. `source` comes back as
`"template"` because the server has no model.

Now open `target` and **write the bodies**. This is yours to do and it is
load-bearing: a pending step is what makes a bar read `Given ...: FAILED`
with no useful detail, and no amount of correct production code moves it.
Follow the conventions already present in the project's step definitions.

Call `step_definitions_find` again and confirm `missing` is empty. If the
templates introduced steps that still do not match, read the failing step
line against the expression that was written for it — a step expression
that does not bind its line is the most common failure here.

**Self-check:** would this step expression actually match that step line?

## 3. Unit test

```json
unit_test_create { "req_id": "REQ-001" }
```

Writes a template to `target` holding `fail("TODO: assert - ...")` per
criterion. Open it and **replace each placeholder with a real
assertion** against the acceptance criterion it stands for.

Read the generated file before you trust it to build. The scaffold
compiles; what you write into it is what breaks. A duplicated test block
or a name that does not exist fails the whole target and tells you
nothing about your requirement.

**Self-check:** does this body assert the criterion, or would it pass
regardless of the implementation? A test that cannot fail is worse than
no test.

## 4. Test to RED

```json
run_tests {}
```

The reply is `{ phase, tests, failures, errors, skipped, failureDetails, nextStep }`.
Read it carefully, because three different things look like RED:

- **An honest RED** — `phase: "RED"` with `tests > 0` and failures whose
  `failureDetails` name your new scenario or test. This is the one you
  want. It is the proof the test can fail.
- **A broken build** — `tests == 0` and `errors > 0`. Nothing ran, so
  this proves nothing about your tests. There is no `build_broken` field;
  compute it from those two numbers. Fix the build before going on,
  almost always in the unit test you just wrote.
- **Nothing ran at all** — `tests`, `failures`, and `errors` all zero.
  The phase is left where it was, usually `START`. Your test is not being
  picked up by the runner.

A `runtime_missing` error means the language runtime is not installed.
The state is untouched; report it and stop.

**Foreign failures.** If the bar was already RED on failures that have
nothing to do with this requirement, stop and say so. Do not implement
against someone else's broken test.

## 5. Implement

Three write-then-test rounds, and stop to ask before spending a fourth.

The configured deliver-attempt budget is not the number to use here. It
is sized for `spec deliver`, which runs unattended and can afford to
grind; you are running in front of someone, every round is a full suite
run, and a budget of fifteen is a way to spend an hour without anyone
deciding to. Report what the last round changed and what the bar said,
and let the developer choose whether to keep going.

Each attempt is **two pieces of work**, and skipping the second is the
usual reason a bar stays RED:

1. **The production code.** Write the simplest thing that satisfies the
   criteria. Put it in the file the behaviour belongs in — extend the
   file whose responsibility this requirement grows, rather than starting
   a new module beside it.
2. **The step bodies still pending.** Any step this requirement's
   scenarios run through that is still a stub. The step definition file is
   shared across requirements, so it is never rewritten whole; fill in
   only the bodies this scenario needs.

If the build or a dependency needs a command, use `command_run` with the
project's discovered toolchain. It is **RED-only** and argv-only.
Allowed programs: `cargo`, `mvn`,
`npm`, `npx`, `node`, `dotnet`, `java`, `javac`, `tsc`. No shell, so
metacharacters are literal; no path in the program name; no `..` or
absolute paths in arguments; 300 seconds maximum. A non-zero exit still
returns a report — read `stdout` and `stderr`.

Then `run_tests` again.

- `phase: "GREEN"` — go to stage 7.
- Still RED with real failures — read `failureDetails`, fix, spend
  another attempt.
- Build broken by your own edit — `git restore` the files you touched,
  read the compiler output, and start the next attempt knowing what
  failed. Do not leave a non-compiling tree behind for the next attempt
  to work around.

Out of attempts means this requirement falls short. Record it with the
phase and the reason, and move to the next one.

**Self-check:** is this a stub, a partial, or a complete implementation
of the criterion? A green bar on a test that asserts nothing is not an
implementation.

## 6. Green

`phase: "GREEN"`. Read `git diff` before you treat it as finished — the
bar says the tests pass, not that the code is the code you want. Say
what you wrote and what you would want a reviewer to look at.

## 7. Refactor

Optional, and only here. Skip it when asked to.

```json
start_refactor { "note": "<brief description of the refactor>" }
```

GREEN-only — it refuses on RED, START, or an already-open refactor, and
the phase becomes `REFACTOR`.

Make the change. Behaviour must not move: no new capability, no changed
output, nothing removed. Then:

```json
run_tests {}
```

It must come back `GREEN`. If it does not, undo the refactor with
`git restore` rather than chasing it — the requirement is already
delivered and a failed tidy-up should not cost you that.

**Self-check:** did this leave behaviour alone?

## 8. Mark implemented

```json
requirement_mark_implemented { "id": "REQ-001" }
```

Gated twice: the bar must be **GREEN**, and a scenario must carry
`@REQ-001`. Both refusals are specific, so read them rather than
guessing. Marking also records the feature file on the requirement, which
is why no separate call is needed for that.

Then:

```json
validate_spec {}
```

The marking tool does not validate the whole catalog. If `valid` is
false after the explicit validation call, the requirement is marked but
the catalog has a problem — report it with the issues.

**Verify:** `get_requirement { "id": "REQ-001" }` shows
`status: "implemented"`.

That closes the requirement. Back to the plan for the next one.

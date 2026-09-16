# bdd implement

Ask the resolved model to make the failing tests pass. The model
receives the requirement, the last run's full failure details — stack
traces included — the project's source files, the history of every
prior attempt on this requirement, and the session language's best
practices (package naming for Java, snake_case modules for Rust, and
so on), and must reply with complete files:
the production code plus real bodies for the TODO placeholders in the
generated tests and step definitions. Everything it writes lands in
the staging area — you review before anything touches the working
tree, and the next test run is the real validator.

```text
Usage: bdd implement [OPTIONS] <REQ_ID>
```

Requires a resolved model (configured with
[`bdd model use`](model.md#bdd-model-use), passed with `--model`, or
the session default when Ollama has installed models). Without one the
command is refused — implementing stays in your hands. The model this
CLI is developed and run against is `qwen3.8-flash-next:125b-mlx`; your
mileage will vary with a different model, especially one trained for
work other than development.

```bash
bdd test                 # a fresh RED bar records the failure details
bdd implement REQ-001    # the model attempts the implementation
```

The command narrates as it works — the preflight result, each asset it
found or missed, then a `working ...` line while the model call runs.
On a terminal the trailing dots animate in light yellow — growing `.`
`..` `...` and starting over — until the call returns; piped output
gets the single static line instead:

```text
REQ-001: checking prerequisites - phase RED, 2 recorded failure(s), 0 prior attempt(s).
  scenario tagged @REQ-001: features/string-calculator.feature - present
  step definitions (every step defined): src/test/java/GeneratedSteps.java - present
  unit test: src/test/java/Req001Test.java - present
  production code (the attempt creates it when missing): src/main/java/StringCalculator.java - missing
Sending the sources, the failures, and the attempt history to the model - working ...
  staged: src/main/java/StringCalculator.java
```

```json
{
  "targets": [
    "src/main/java/StringCalculator.java",
    "src/test/java/Req001Test.java",
    "src/test/java/GeneratedSteps.java"
  ],
  "staged": true,
  "source": "llm",
  "nextStep": "Apply with bdd changes commit, then bdd test - the run decides."
}
```

## The follow-up offer

When files were staged and you are on a terminal, the command closes
the loop itself:

```text
Apply the staged files and run the tests now? [y/N]
```

Answering `y` runs `changes commit` and `test` in one go and prints
both reports, ending with the verdict in color — green
`GREEN - next: refactor (optional), then spec mark-implemented REQ-001 && changes commit.`
or red
`Still RED - the fresh failures are recorded; run implement REQ-001 for another model attempt, or implement by hand and rerun test.`

Pressing <kbd>Enter</kbd> (or piping the output, where no question is
asked) declines and prints the next command in plain words instead:

```text
Next: changes commit && test - then implement REQ-001 again if the bar stays RED.
```

In every one of these lines the command itself — `changes commit &&
test`, `implement REQ-001`, `spec mark-implemented REQ-001 && changes
commit` — is printed in green, the CLI's marker for text meant to be
copied and pasted.

## The preflight

Before anything goes to the model, the command surveys the
prerequisites of an implementation attempt:

- a scenario tagged `@REQ-XXX` exists in a feature file,
- every feature step has a definition,
- the requirement's unit test exists,
- the requirement is still `pending`, and
- a RED test run is recorded, so its failures can brief the model.

When one is missing the attempt does not run. Each gap is printed in
red with the step to take instead — `bdd scenario add`,
`bdd steps generate`, `bdd unittest generate REQ-XXX`, or `bdd test` —
and the JSON reply is the readiness report:

```json
{
  "ready": false,
  "assets": [
    { "role": "scenario tagged @REQ-001", "path": "features/*.feature", "present": false },
    { "role": "step definitions (every step defined)", "path": "src/test/java/GeneratedSteps.java", "present": false },
    { "role": "unit test", "path": "src/test/java/Req001Test.java", "present": false },
    { "role": "production code (the attempt creates it when missing)", "path": "src/main/java/StringCalculator.java", "present": false }
  ],
  "findings": [
    "No RED test run is recorded - run bdd test first so its failures brief the model.",
    "No scenario is tagged @REQ-001 - add one with bdd scenario add, then bdd changes commit."
  ],
  "nextStep": "No RED test run is recorded - run bdd test first so its failures brief the model."
}
```

The production file is surveyed but never blocks — the attempt creates
it when it is missing. A missing prerequisite with a model resolved
also triggers one advice call: the requirement, the asset survey, the
findings, and the last failures go to the model, which answers in a
few sentences whether `bdd implement` can succeed right now and names
the exact next command. The advice is printed as
`Model advice: ...` under the findings.

## What the model may write

The reply must be a strict JSON array of `{path, content}` file
updates. Only two kinds of path are accepted:

- files already in the project's sources (the generated unit test and
  step definitions it needs to wire up), and
- the production file, named after the spec's `project` field by
  ecosystem convention:

| Language | Production target |
| --- | --- |
| Java | `src/main/java/<Project>.java` |
| JavaScript | `src/<project>.js` |
| TypeScript | `src/<project>.ts` |
| .NET | `<Project>.cs` |
| Rust | `src/lib.rs` |

Anything else in the reply is dropped. A reply with no usable update
fails with `The model's reply held no usable file update.` — nothing
is staged, and you implement by hand instead.

The implementation prompt is the largest call the CLI makes, so a
local model can need minutes to answer. The generation timeout
defaults to 300 seconds; if you see `no reply within ...s`, raise
`timeout_seconds` under `[llm]` in `.bdd.toml` (see
[`bdd model`](model.md#the-llm-configuration-block)).

## Where it fits

This is the standalone form of the [greenfield](greenfield.md)
implementation attempt — the same behavior <kbd>Enter</kbd> triggers
on a RED bar inside the loop. Use it to continue a paused run:

```bash
bdd test                 # confirm RED, record the failures
bdd implement REQ-001    # stage the model's attempt
bdd changes show         # review what it wrote
bdd changes commit
bdd test                 # GREEN? then bdd refactor / bdd spec mark-implemented
```

If the bar stays RED, run `bdd implement` again — the fresh failure
details from the latest run go back to the model — or take over by
hand.

## Attempts are remembered

Every attempt is logged in `.bdd-state.json` (under `attemptLog` on a
timestamped state entry): the files it wrote, the failures it was
addressing, and — attached by the first test run after it — the
`outcome`: what that run actually reported, build output included. The
next attempt's prompt recounts that whole chain — *attempt 1 wrote
these files to fix these failures, and the run after it reported this;
what remains now is listed above* — with an explicit instruction to
take a different, complete approach instead of repeating one that
already failed. An attempt no run ever followed is called out as never
verified. Failure details carry everything the runner captured:
assertion messages, stack traces, and up to the last 100 lines of a
build that failed before tests could run.

The prompt also carries the interpretation instructions from the state
file and **only the three latest dated state entries**. Older history
stays on disk for humans; it is not sent to the model.

The attempt log is scoped to the requirement and cleared the moment a
test run goes GREEN — a closed loop leaves no history for the next
requirement to inherit.

`run_tests` during this command sees the **working tree**, not the
unstaged patch sitting in `.bdd-staged/`. Commit (or apply) before you
trust the bar. If the model requests `command_run`, the CLI asks you to
confirm first; piped or CI stdin declines and never hangs.

## See also

- [`bdd greenfield`](greenfield.md) — the orchestrated loop with the same attempt built in.
- [`bdd changes`](changes.md) — review and apply the staged files.
- [`bdd test`](test.md) — the run that decides.
- [`bdd tools`](tools.md) — the implement profile (includes `command_run`).

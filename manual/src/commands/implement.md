# spec implement

Ask the resolved model to make the failing tests pass. The model
receives the requirement, the last run's full failure details — stack
traces included — the project's source files, the history of every
prior attempt on this requirement, and the session language's best
practices (package naming for Java, snake_case modules for Rust, and
so on), and must reply with complete files:
the production code plus real bodies for the TODO placeholders in the
generated tests and step definitions. Everything it writes lands in
the working tree as it goes — read it with `git diff`, undo it with
`git restore`, and let the next test run be the real validator.

```text
Usage: spec implement [OPTIONS] <REQ_ID>

Options:
      --into <PATH>  Production file the work belongs in. Only needed
                     when the step definitions are still pending, so
                     nothing in the project points at one
```

`--into` names the production file instead of letting the command work
it out, and it is the only command that takes the flag. It is checked
before anything else, so it also overrides an inference you disagree
with. See [where the work lands](#where-the-work-lands) for when you
need it.

Requires a resolved model (configured with
[`spec model use`](model.md#spec-model-use), passed with `--model`, or
the session default when Ollama has installed models). Without one the
command is refused — implementing stays in your hands. The model this
harness is developed and run against is `qwen3.8-flash-next:125b-mlx`; your
mileage will vary with a different model, especially one trained for
work other than development.

```bash
spec test                 # a fresh RED bar records the failure details
spec implement REQ-001    # the model attempts the implementation
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
  wrote: src/main/java/StringCalculator.java
```

```json
{
  "targets": [
    "src/main/java/StringCalculator.java",
    "src/test/java/Req001Test.java",
    "src/test/java/GeneratedSteps.java"
  ],
  "written": true,
  "source": "llm",
  "nextStep": "Run spec test - the run decides."
}
```

## The follow-up offer

When files were written and you are on a terminal, the command closes
the loop itself:

```text
Run the tests now? [y/N]
```

Answering `y` runs `test` and prints the report, ending with the
verdict in color — green
`GREEN - next: refactor (optional), then mark-implemented REQ-001.`
or red
`Still RED - the fresh failures are recorded; run implement REQ-001 for another model attempt, or implement by hand and rerun test.`

Pressing <kbd>Enter</kbd> (or piping the output, where no question is
asked) declines and prints the next command in plain words instead:

```text
Next: test - then implement REQ-001 again if the bar stays RED.
```

In every one of these lines the command itself — `test`,
`implement REQ-001`, `mark-implemented REQ-001` — is printed in green,
the harness's marker for text meant to be copied and pasted.

## The preflight

Before anything goes to the model, the command surveys the
prerequisites of an implementation attempt:

- a scenario tagged `@REQ-XXX` exists in a feature file,
- every feature step has a definition,
- the requirement's unit test exists,
- the requirement is still `pending`, and
- a RED test run is recorded, so its failures can brief the model.

When one is missing the attempt does not run. Each gap is printed in
red with the step to take instead — `spec scenario add`,
`spec steps generate`, `spec unittest generate REQ-XXX`, or `spec test` —
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
    "No RED test run is recorded - run spec test first so its failures brief the model.",
    "No scenario is tagged @REQ-001 - add one with spec scenario add."
  ],
  "nextStep": "No RED test run is recorded - run spec test first so its failures brief the model."
}
```

The production file is surveyed but never blocks — the attempt creates
it when it is missing. A missing prerequisite with a model resolved
also triggers one advice call: the requirement, the asset survey, the
findings, and the last failures go to the model, which answers in a
few sentences whether `spec implement` can succeed right now and names
the exact next command. The advice is printed as
`Model advice: ...` under the findings.

## What the model may write

The reply must be a strict JSON array of `{path, content}` file
updates. Only two kinds of path are accepted: files already in the
project's sources (the generated unit test and step definitions it
needs to wire up), and the production file described below. Anything
else in the reply is dropped. A reply with no usable update fails with
`The model's reply held no usable file update.` — nothing is written,
and you implement by hand instead.

A reply that would *destroy* a file it replaces is dropped too, and
reported. Two things disqualify it: no longer declaring a name the
file declares today, or braces that never close. Both describe a
truncated or junk reply rather than a wrong one — the test run is what
decides whether an attempt is any good, and it never gets to run when
the reply deletes the code the tests were going to call. The refusal
is per file, so the rest of an otherwise fine attempt is still written.

## Where the work lands

The production file is resolved in this order, and the first answer
wins:

1. **`--into <PATH>`**, when you passed it.
2. **The evidence.** The bodies of the step definitions this
   requirement's scenarios actually run through are matched against
   the symbols each production file declares, and the file named
   through the most *distinct* symbols is the target. Two independent
   names are required and a tie counts as no answer, so one incidental
   word match cannot decide it. This is why a scenario calling an MCP
   tool lands in the server rather than in whatever file sorts first.
3. **Convention**, when the name built from the spec's `project` field
   exists and is actually specific to this project:

   | Language | Conventional target |
   | --- | --- |
   | Java | `src/main/java/<Project>.java` |
   | JavaScript | `src/<project>.js` |
   | TypeScript | `src/<project>.ts` |
   | .NET | `<Project>.cs` |
   | Rust | `src/lib.rs` |

   Rust's entry point is the same path in every crate, so it is
   trusted only where renaming the project would change the answer.
4. **The only candidate**, when exactly one file lives under the
   production root — one candidate is not a guess.
5. **The conventional path**, when there is no production code at all.
   A greenfield project has nowhere else for the work to go.

When none of those answer, the command refuses rather than guessing:

```text
Cannot tell which production file REQ-001 belongs in - no step definition its
scenarios run through names any of them. Name it with spec implement REQ-001
--into <path>, or write the steps first so they point at the code.
```

The usual cause is that every step the scenarios bind to is still a
generated placeholder. A pending step body echoes its own Gherkin back
in a `todo!` and names no production code, so there is nothing to infer
from — those bodies are excluded from the evidence for exactly that
reason. Either write a step body or two against the real types, or pass
`--into`. The preflight shows the same gap before the model is called:

```text
production code (the attempt creates it when missing): unknown - pass --into <path> - missing
```

## When an attempt reaches past its requirement

The report's `warning` also calls out written code that looks like it
satisfies a *different* requirement still marked `pending`:

```text
The code written also satisfies REQ-005, still pending - REQ-006 was the
requirement asked for. Review the diff with git diff and drop what
REQ-006 does not need, so each requirement keeps its own RED bar.
```

It is a literal match — the code feeds in the same quoted inputs the other
requirement's criteria name and lands on the same expected numbers — so it
warns and never blocks. A criterion worded without literals is invisible to
it, and a literal two requirements share can raise it when nothing is wrong.
Read the diff and decide; that is the check this is a prompt for, not
a replacement of.

The implementation prompt is the largest call the harness makes, and
against a real project it is slow: attempts on this repository's own
crate have measured 20, 36 and 57 minutes on the recommended local
model. The generation timeout defaults to 300 seconds, which is not
enough for that; if you see `no reply within ...s`, everything
generated so far is lost, and the fix is to raise `timeout_seconds`
under `[llm]` in `.spec/config.toml` (see
[`spec model`](model.md#the-llm-configuration-block)).

Identical requests are served from the response cache without calling
the model, so a repeated attempt is free — but the key is the whole
prompt, and the attempt history is part of it, so a *second* attempt on
the same requirement is always a fresh call.

## Where it fits

This is the standalone form of the [greenfield](greenfield.md)
implementation attempt — the same behavior <kbd>Enter</kbd> triggers
on a RED bar inside the loop. Use it to continue a paused run:

```bash
spec test                 # confirm RED, record the failures
spec implement REQ-001    # the model's attempt, written into the tree
git diff                  # review what it wrote
spec test                 # GREEN? then spec refactor / spec mark-implemented
```

If the bar stays RED, run `spec implement` again — the fresh failure
details from the latest run go back to the model — or take over by
hand.

## Attempts are remembered

Every attempt is logged in `.spec/state.json` (under `attemptLog` on a
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

`run_tests` during this command sees the **working tree** — which is
where the attempt just wrote, so the bar is measured against exactly
the files the model produced. If the model requests `command_run`, the
harness asks you to confirm first; piped or CI stdin declines and never
hangs.

## See also

- [`spec greenfield`](greenfield.md) — the orchestrated loop with the same attempt built in.
- [`spec test`](test.md) — the run that decides.
- [`spec tools`](tools.md) — the implement profile (includes `command_run`).

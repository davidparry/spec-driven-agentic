# spec greenfield

Run the full orchestrated loop from an empty directory to an
implemented requirement, with exactly **two human gates**: approving
the spec wording, and approving the generated tests before they run.
Everything else — scaffolding, validation, scenario authoring, step
generation, test execution, phase tracking — is automated.

```text
Usage: spec greenfield [OPTIONS]
```

## Flags

| Flag | Description |
| --- | --- |
| `--root <ROOT>` | Project root. Defaults to `.`. |
| `--model <MODEL>` | LLM model for the generation steps, this run only. |
| `--retry <N>` | Max attempts when a model reply fails validation. Default 3. |

## The orchestrated flow

```text
 1. inspect / init      scaffold if the root is empty (asks for language)
                        and records the choice in .spec/memory.json
 2. describe            you describe what to build in plain words; the
                        model splits it into requirement proposals
 3. accept + wizard     you accept all listed proposals, or a
                        comma-separated subset; accepted ones are
                        stored in the spec under sequential REQ-###
                        ids. You then pick which stored one to review
                        first; every field arrives pre-filled -
                        Enter accepts, typing replaces
 4. spec validate       structure gate; findings loop back to rewording
 5. spec refine         wording gate; each finding comes with a "try:" fix
    ── HUMAN GATE 1 ──  approve the requirement's wording
 6. scenario + steps    Gherkin scenario tagged @REQ-...; step definitions
    ── HUMAN GATE 2 ──  approve the generated tests
 7. test → RED          the scenario fails honestly
 8. implement           Enter lets the model attempt the implementation;
                        a number, e.g. 5, buys that many hands-off attempts
 9. test → GREEN        loop back to 8 while failing
10. refactor            optional; only offered on GREEN
11. mark implemented    Saving status spinner until requirements.json
                        is written; then the spec> prompt for the next
                        command
```

## The description-driven wizard

With a resolved model, drafting starts from a plain-words description
instead of a blank title prompt:

```text
Describe what to build in plain words (one or several requirements). Enter drafts manually instead:
sum numbers from a comma separated string, empty input means zero
Splitting the description into requirements with qwen3.8-flash-next:125b-mlx - working ...
Accept all these requirements to refine, or enter comma-separated numbers of the ones to accept.
The description holds 2 requirement(s):
  1. Comma separated numbers are summed
  2. Empty string returns zero
Accept [Enter for all, or comma-separated numbers]:

Accepted requirements are staged for requirements/requirements.json as pending - nothing reaches the working spec until spec changes commit:
  REQ-001 Comma separated numbers are summed
  REQ-002 Empty string returns zero
Which requirement first to review and refine? [1-2, Enter for 1]:
2
Walking through REQ-002. Each prompt shows the proposal - Enter accepts it, or type your own wording.
REQ-002 title [Empty string returns zero] (Enter keeps it):
REQ-002 story (As a ..., I want ..., so that ...) [As a user, I want empty input to be 0 so that no input is a safe default.] (Enter keeps it):
REQ-002 criterion 1 [Given an empty string "", when add is called, then the result is 0] (Enter keeps it, '-' drops it):
REQ-002 criterion 2 (leave blank to finish the criteria):
```

The model must deliver each proposal complete — title, story, and at
least one Given/When/Then criterion — or the proposal is dropped.
The list is shown so you can accept all of them, or a comma-separated
subset (for example `1,3,5`). Only the accepted proposals are staged
for `requirements.json` (the root of the
[spec catalog](../spec-format.md)), each under its own sequential
`REQ-###` id — read them with `spec changes show` at any point in the
wizard. Like every other mutation they reach the working spec only on
`spec changes commit`, so declining the last prompt leaves the batch in
staging for `spec changes show` or `spec changes discard` rather than
on disk.
You then pick which staged requirement to review and refine first.
The others wait as pending requirements — reword them any time with
`spec reword`.
Nothing is accepted silently: every field passes through your hands,
and the validate + refine gates still run on whatever you accept.

Drafting falls back to the classic manual prompts whenever the
description is left blank, no model is resolved, the model is
unreachable, or its reply holds no complete requirement.

At each gate you can approve, decline (the run stops cleanly), or
pause to resume later — the phase state and staged changes survive
between invocations.

## The implementation attempt

On a RED bar with a resolved model, <kbd>Enter</kbd> asks the model to
make the failing tests pass, then reruns the suite and prints the
counts:

```text
RED: 2 tests, 1 failures, 1 errors.
  - Req001Test.empty_string_returns_zero: TODO: assert - ...
Press Enter to let the model attempt the implementation and rerun the tests, enter a number to attempt up to that many times without asking again, or type stop to pause here:

Generating an implementation attempt - working ...
Updated src/main/StringCalculator.java (llm).
Updated src/test/java/Req001Test.java (llm).
Running the tests - working ...
GREEN: 2 tests, 0 failures, 0 errors.
```

Every `working ...` line is live on a terminal: the trailing dots
animate in light yellow — `.` `..` `...` and over again — while the
model call or test run is in flight, then the line settles. Piped
output prints the single static line.

The model receives the requirement, the full failing test details —
stack traces and build output included — the project's source files,
every prior attempt on this requirement (which files it wrote, which
failures it was addressing, and what the run after it actually
reported), and the session language's best practices — package naming
for Java, snake_case modules for Rust, and so on — so a second attempt
never starts blind or repeats an approach that already failed. It must reply with
complete files: production code plus real bodies for the TODO
placeholders in the generated tests and step definitions. Only paths
already in the project (or the production file named after the spec's
project) are accepted; anything else in the reply is dropped. The
rerun is the real validator — if the bar stays RED, press
<kbd>Enter</kbd> for another attempt (the fresh failure details plus
the attempt history go back to the model) or implement by hand and
press <kbd>Enter</kbd>.

A number buys a hands-off stretch: answering `5` lets the model
attempt, rerun, and attempt again up to five times without asking in
between — each round announced as `Attempt 2 of 5.` — stopping early
the moment the bar turns GREEN. When the budget runs out on RED, the
prompt returns. Anything unreadable at the prompt behaves like
<kbd>Enter</kbd>: one attempt.

An unusable reply is narrated
(`The model's reply held no usable file update. Implement by hand
instead.`) and the loop simply hands control back to you. Dead ends
like this — model failures, missing runtimes, hand-offs back to manual
work — print in red so they stand out from the loop's narration. Without a
model, <kbd>Enter</kbd> just reruns the tests.

Typing `stop` pauses the run; a paused project continues with the
standalone [`spec implement`](implement.md) command, which runs the
same attempt from the persisted failure details:

```bash
spec implement REQ-001 && spec changes commit && spec test
```

## Rewording loop details

When validation or refinement finds problems, each finding is printed
with a concrete suggestion:

```text
REQ-001: the outcome is not concrete
  try: end with the exact expected value, e.g. '..., then the result is 3', a quoted literal, e.g. '..., then the calculator returns "NaN"', or a definite error, e.g. '..., then an error is raised'
```

With a model, the findings become its brief: the draft and the
findings go to the model, and the re-prompts carry its corrected
proposal — <kbd>Enter</kbd> accepts each fix. If the review rejects a
wording again, the next model call also recounts every earlier wording
and the findings each one produced, so the model never circles back to
a wording the review already rejected:

```text
Asking qwen3.8-flash-next:125b-mlx to address finding 1 of 1 - working ...
The model reworded the draft. Each prompt shows its proposal - Enter accepts it, or type your own wording.
```

With several findings, each one is its own model call — call 2 is
briefed with the draft call 1 fixed, so the corrections accumulate one
finding at a time instead of all at once.

On a color console the bracketed suggestion — the text <kbd>Enter</kbd>
will use — renders green; the destructive `'-' drops it` hint on
criterion prompts and dead-end messages render red; the animated dots
on `working ...` lines render light yellow, and the
`Generating an implementation attempt` announcement renders dark
green. On a real
terminal every answer is edited on a `> ` line with full line editing:
the arrow keys move the cursor anywhere in the typed text, Home/End
jump, and the up arrow recalls earlier answers from this session.

If the model call fails or its rewording is unusable, the re-prompt
falls back to the requirement id and your prior answer;
<kbd>Enter</kbd> keeps it:

```text
REQ-001 title [Two numbers separated by a comma are summed] (Enter keeps it):
REQ-001 criterion 1 [Given "1,2", when add is called, then the result is 3] (Enter keeps it, '-' drops it):
REQ-001 criterion 3 (leave blank to finish the criteria):
```

The close of the loop shows a `Saving status - working ...` spinner
while the requirement is marked implemented and written to
`requirements.json`. Once that file is saved:

```text
Saving status - working ...
REQ-001 is implemented. Loop closed.
```

A closed loop then checks `requirements.json` for requirements still
`pending` — for example the other proposals accepted during drafting.
When any remain, they are listed and the loop offers to keep going:

```text
Still pending in the spec:
  1. REQ-002 Blank input is rejected
  2. REQ-003 Newlines are also separators
Which pending requirement next? [1-2, Enter for 1] (n stops):
```

Enter (or `1`) walks the stored wording through the same validate +
refine gate (each prompt pre-filled, Enter keeps it), then runs the
full delivery again — scenarios, generated tests, RED to GREEN,
refactor, mark-implemented — and offers the remaining pending rows,
until nothing is pending or you type `n`. `nextStep` says how to
continue later.

The JSON reply then prints, and on a real terminal a one-shot
`spec greenfield` keeps the session open at the `spec>` prompt so you can
run `spec list`, `greenfield`, or any other command without relaunching.

## Reply

The final JSON reply summarizes where the run ended:

```json
{
  "requirement": "REQ-001",
  "feature": "features/string_calculator.feature",
  "phase": "GREEN",
  "completed": true,
  "nextStep": "The next requirement is waiting. Type greenfield to continue, or spec list."
}
```

`completed` is `false` when a gate was declined, a runtime was
missing, or the run was paused; `nextStep` always says how to
continue.

## Requirements for a full run

- The language's runtime must be present (`mvn`, `node`, `dotnet`, or
  `cargo`) — the orchestrator refuses to fake a test run.
- An LLM is optional: with no reachable Ollama model the generation
  steps fall back to deterministic templates you edit yourself. The
  model this harness is developed and run against is
  `qwen3.8-flash-next:125b-mlx` (`ollama pull qwen3.8-flash-next:125b-mlx`).
  Your mileage will vary with other models, especially those not
  trained for development work.

## See also

- [`spec deliver`](deliver.md) — the same loop started from a plan
  instead of the wizard, with a pass/fail verdict and no questions at
  all: it accepts every proposal and refuses up front whatever it cannot
  decide alone. `spec deliver REQ-003` is the one-command form of taking
  a requirement already in the catalog through to implemented. Reach for
  greenfield when you want the decisions to be yours.
- [The workflow](../workflow.md) — the same rhythm, step by step.
- [`spec model`](model.md) — pick which model powers generation.

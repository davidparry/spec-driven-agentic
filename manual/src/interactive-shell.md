# The interactive shell

Running `spec` with no subcommand in a terminal opens a REPL. It prints
the help once, shows the banner and the session's model status, and
then reads commands until you leave.

```text
spec> list
[
  { "id": "REQ-001", "title": "Empty string returns zero", "status": "implemented" }
]
spec> test --feature features/calculator.feature
...
spec> exit
```

## Behavior

- **No prefix needed.** Type `list`, not `spec list`. A leading `spec`
  is forgiven if you type it anyway.
- **Inherited flags.** Commands inherit the shell's `--root`,
  `--model`, and `--retry` unless the line supplies its own:

```bash
spec --root ~/code/calculator --model qwen3.8-flash-next:125b-mlx --retry 5
# every command in this shell now targets that root and model,
# and retries invalid model replies up to 5 times
```

- **Quoting works.** Lines are tokenized with shell rules, so
  `scenario add --step "Given a calculator"` behaves as expected.
  Unbalanced quotes report `unreadable input` and the shell continues.
- **Errors don't kill the shell.** A failing command prints its error
  and returns to the prompt.
- **Blank lines** are ignored.
- **After `greenfield`.** A one-shot `spec greenfield` that finishes on
  a real terminal stays in this shell (the `spec>` prompt) so you can
  run `list`, `greenfield`, or anything else without relaunching.

## Leaving

- `exit` or `quit`
- <kbd>Ctrl</kbd>+<kbd>C</kbd> (interrupt)
- <kbd>Ctrl</kbd>+<kbd>D</kbd> (end of input)

On exit the shell prints a summary of how many commands ran.

## Session history

Line history is kept across sessions in `.spec/history`. Use
<kbd>↑</kbd>/<kbd>↓</kbd> to recall previous
commands and <kbd>Ctrl</kbd>+<kbd>R</kbd> for reverse search. If the
history cannot be saved, the shell says so and exits normally.

## Model announcement at startup

The first prompt is preceded by one line describing the session's
model. This harness is developed and run against
`qwen3.8-flash-next:125b-mlx`; your mileage will vary with a different
model, especially one trained for work other than development. See
[Getting started](getting-started.md#local-llm-ollama) and
[`spec model`](commands/model.md).

| Situation | Announcement |
| --- | --- |
| Configured in `.spec/config.toml` | `Model set: qwen3.8-flash-next:125b-mlx (from configuration).` |
| No config, models installed | `Model set for this session: qwen3.8-flash-next:125b-mlx (not saved - keep it with: spec model use qwen3.8-flash-next:125b-mlx).` |
| Ollama up, no models | `Ollama is running but has no models - generation will use deterministic templates. For optimal results pull a coding model, e.g.: ollama pull qwen3.8-flash-next:125b-mlx (mileage varies with models not trained for development)` |
| Ollama unreachable | `Ollama is not reachable - generation will use deterministic templates. Install it from https://ollama.com, start it, and pull a coding model, e.g.: ollama pull qwen3.8-flash-next:125b-mlx (mileage varies with models not trained for development)` |

## The greenfield nudge

When all three signs of a brand-new project line up —

1. this is the first shell session in the root (no `.spec/history` yet),
2. a model is ready, and
3. there is no `requirements/requirements.json`

— the shell offers to start the loop before the first prompt. The same
start refreshes `.spec/memory.json` (language, libraries, layout) so
later model calls in the session carry that brief.

## The layout question

That refresh resolves the project's layout: the module whose build file
the test runner is pointed at, its test and production roots, its
features directory, the step-definition file generated steps join, and
the package existing tests declare. Everything the harness writes follows
from it.

It is answered deterministically — build manifests, observed source
files, and the feature files your requirements name. A model is asked
only when a tree holds several buildable modules and none of that
separates them, and then only to pick one of the discovered module
roots; anything else it replies with is refused. You confirm, and the
answer is recorded, so the question is asked once per project:

```text
Several modules could be the one to work in: kata, smoke-test.
Asking qwen3.8-flash-next:125b-mlx which module - working ...
Work in kata - the module Maven compiles and runs tests in? [y/N] y
Recorded kata as this project's module - asked once; spec inspect re-scans if that changes.
```

With no model resolved, or on a decline, the scan's own pick stands and
the shell says which one it used.

```text
It appears you are in a greenfield - this project has no requirements/requirements.json yet.
Start with the greenfield command now? [y/N]
```

`y` runs [`spec greenfield`](commands/greenfield.md) immediately;
anything else declines and the shell carries on:

```text
No problem - type greenfield any time, or draft to begin with the spec.
```

## When there is no terminal

If stdin is not a terminal (piped input, CI), bare `spec` prints the
help and exits instead of opening the shell.

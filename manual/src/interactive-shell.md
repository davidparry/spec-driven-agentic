# The interactive shell

Running `bdd` with no subcommand in a terminal opens a REPL. It prints
the help once, shows the banner and the session's model status, and
then reads commands until you leave.

```text
bdd> spec list
[
  { "id": "REQ-001", "title": "Empty string returns zero", "status": "implemented" }
]
bdd> test --feature features/calculator.feature
...
bdd> exit
```

## Behavior

- **No prefix needed.** Type `spec list`, not `bdd spec list`. A
  leading `bdd` is forgiven if you type it anyway.
- **Inherited flags.** Commands inherit the shell's `--root`,
  `--model`, and `--retry` unless the line supplies its own:

```bash
bdd --root ~/code/calculator --model qwen3.8-flash-next:125b-mlx --retry 5
# every command in this shell now targets that root and model,
# and retries invalid model replies up to 5 times
```

- **Quoting works.** Lines are tokenized with shell rules, so
  `scenario add --step "Given a calculator"` behaves as expected.
  Unbalanced quotes report `unreadable input` and the shell continues.
- **Errors don't kill the shell.** A failing command prints its error
  and returns to the prompt.
- **Blank lines** are ignored.
- **After `greenfield`.** A one-shot `bdd greenfield` that finishes on
  a real terminal stays in this shell (the `bdd>` prompt) so you can
  run `spec list`, `greenfield`, or anything else without relaunching.

## Leaving

- `exit` or `quit`
- <kbd>Ctrl</kbd>+<kbd>C</kbd> (interrupt)
- <kbd>Ctrl</kbd>+<kbd>D</kbd> (end of input)

On exit the shell prints a summary of how many commands ran.

## Session history

Line history is kept across sessions in `.bdd-history` under the
project root. Use <kbd>↑</kbd>/<kbd>↓</kbd> to recall previous
commands and <kbd>Ctrl</kbd>+<kbd>R</kbd> for reverse search. If the
history cannot be saved, the shell says so and exits normally.

## Model announcement at startup

The first prompt is preceded by one line describing the session's
model. This CLI is developed and run against
`qwen3.8-flash-next:125b-mlx`; your mileage will vary with a different
model, especially one trained for work other than development. See
[Getting started](getting-started.md#local-llm-ollama) and
[`bdd model`](commands/model.md).

| Situation | Announcement |
| --- | --- |
| Configured in `.bdd-mcp.toml` | `Model set: qwen3.8-flash-next:125b-mlx (from configuration).` |
| No config, models installed | `Model set for this session: qwen3.8-flash-next:125b-mlx (not saved - keep it with: bdd model use qwen3.8-flash-next:125b-mlx).` |
| Ollama up, no models | `Ollama is running but has no models - generation will use deterministic templates. For optimal results pull a coding model, e.g.: ollama pull qwen3.8-flash-next:125b-mlx (mileage varies with models not trained for development)` |
| Ollama unreachable | `Ollama is not reachable - generation will use deterministic templates. Install it from https://ollama.com, start it, and pull a coding model, e.g.: ollama pull qwen3.8-flash-next:125b-mlx (mileage varies with models not trained for development)` |

## The greenfield nudge

When all three signs of a brand-new project line up —

1. this is the first shell session in the root (no `.bdd-history` yet),
2. a model is ready, and
3. there is no `requirements/requirements.json`

— the shell offers to start the loop before the first prompt. The same
start refreshes `.bdd-memory.json` (language, libraries, layout) so
later model calls in the session carry that brief.

```text
It appears you are in a greenfield - this project has no requirements/requirements.json yet.
Start with the greenfield command now? [y/N]
```

`y` runs [`bdd greenfield`](commands/greenfield.md) immediately;
anything else declines and the shell carries on:

```text
No problem - type greenfield any time, or spec draft to begin with the spec.
```

## When there is no terminal

If stdin is not a terminal (piped input, CI), bare `bdd` prints the
help and exits instead of opening the shell.

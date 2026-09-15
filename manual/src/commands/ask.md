# bdd ask

Ask the local model a question with the **ask** tool profile: the ten
read-only workflow tools (spec, TDD state, inspect, features, staged
changes). The model may look things up; it cannot stage, commit, or
mark a requirement implemented.

```text
Usage: bdd ask [OPTIONS] [TASK]...
```

```bash
bdd ask "which pending requirement should we implement next?"
bdd ask --json "what phase is the bar in?"
```

With a TTY and no task, `bdd ask` opens a multi-turn prompt (`ask>`).
Empty input, `exit`, or `quit` ends the session. Piped stdin without a
task is refused so CI never hangs.

A model must be resolved (`bdd model use`, `--model`, or a single
installed Ollama tag). There is no template fallback.

The offered tools are the `ask` row of [`bdd tools profiles`](tools.md).
Override for one run with `--tools`.

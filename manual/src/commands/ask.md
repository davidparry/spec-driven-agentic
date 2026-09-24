# spec ask

Ask the local model a question with the **ask** tool profile: the twelve
read-only workflow tools (spec, TDD state, inspect, features, staged
changes). It is the widest profile the harness attaches, because reading
costs nothing — a generating command gets 3–7 tools instead. The model may
look things up; it cannot stage, commit, or mark a requirement implemented.

```text
Usage: spec ask [OPTIONS] [TASK]...
```

```bash
spec ask "which pending requirement should we implement next?"
spec ask --json "what phase is the bar in?"
```

With a TTY and no task, `spec ask` opens a multi-turn prompt (`ask>`).
Empty input, `exit`, or `quit` ends the session. Piped stdin without a
task is refused so CI never hangs.

A model must be resolved (`spec model use`, `--model`, or a single
installed Ollama tag). There is no template fallback.

The offered tools are the `ask` row of [`spec tools profiles`](tools.md).
Override for one run with `--tools`.

# bdd model

LLM model discovery and selection. The harness talks to a local
[Ollama](https://ollama.com) — no cloud calls, no tokens — and uses
the model only to *polish* deterministic templates in the generation
commands. Everything works without a model; generation just stays at
template quality.

The model this harness is developed and run against is
`qwen3.8-flash-next:125b-mlx`. Pull it with
`ollama pull qwen3.8-flash-next:125b-mlx`, then persist the choice with
`bdd model use qwen3.8-flash-next:125b-mlx`. Your mileage will vary with
other models: a stronger coding model may improve drafts and
implementations; a model trained for chat, general knowledge, or work
other than development will typically produce weaker specs, steps,
tests, and production code. The harness does not require this specific
model — it uses whatever you configure, or the first model Ollama has
installed.

```text
Usage: bdd model [OPTIONS] <COMMAND>

Commands: list, current, use
```

## How a model is resolved

Highest priority first:

1. **`--model` flag** — this invocation only, never persisted.
2. **Configuration** — the `model` key in `.bdd.toml` under the
   project root, written by `bdd model use`.
3. **Discovery** — the first model installed in Ollama, as a
   session-only default. Nothing is written to disk.

If Ollama is unreachable or has no models, LLM-backed generation falls
back to deterministic templates.

---

## bdd model list

List the models installed in Ollama, marking the one that would
currently be used.

```bash
bdd model list
```

```text
Models available in Ollama:
* qwen3.8-flash-next:125b-mlx   (configured)
  qwen3:8b
  llama3:8b
```

With no configuration, the marker moves to the discovered session
default. If Ollama is down, the command fails with exit status 1 and
says the provider is unreachable.

---

## bdd model current

Show the resolved model and where it came from.

```bash
bdd model current
```

```text
Configured model: qwen3.8-flash-next:125b-mlx
```

With nothing configured but models installed, the first one is the
session default and the output tells you it is not saved:

```text
Model set for this session: qwen3.8-flash-next:125b-mlx (not saved - keep it with: bdd model use qwen3.8-flash-next:125b-mlx).
```

The same announcement appears when the
[interactive shell](../interactive-shell.md) starts.

---

## bdd model use

Persist a model choice in the project's configuration.

```text
Usage: bdd model use [OPTIONS] <MODEL_NAME>
```

```bash
bdd model use qwen3.8-flash-next:125b-mlx
```

```text
Configured model: qwen3.8-flash-next:125b-mlx
Written to /Users/you/code/calculator/.bdd.toml
```

The choice is validated against Ollama's installed models — a name
Ollama does not have is rejected rather than silently saved.

## The [llm] configuration block

Everything model-related lives under `[llm]` in `.bdd.toml`:

```toml
[llm]
model = "qwen3.8-flash-next:125b-mlx"     # persisted by bdd model use
endpoint = "http://localhost:11434"   # the Ollama endpoint
timeout_seconds = 300                 # generation timeout (default 300)
cache_ttl_seconds = 600               # response cache TTL; 0 disables
retry = 3                             # invalid-reply attempts (`--retry` wins)
```

`timeout_seconds` bounds how long one generation call may take. Large
prompts — an implementation attempt carries the requirement, the
failure details, and every project source file — can keep a local
model generating for minutes; when the budget runs out the error names
it explicitly (`no reply within 300s ... set timeout_seconds under
[llm]`). Raise it for big projects or slower models.

## Which commands actually use the model

| Uses the model | Never touches it |
| --- | --- |
| `spec draft` (description wizard, findings rewording) | `test`, `state`, `refactor` |
| `steps generate` | `spec` (other subcommands) |
| `unittest generate` | `feature`, `scenario`, `changes` |
| `implement` | `init`, `inspect`, `validate` |
| `greenfield` (drafting, generation, implementation) | |

## See also

- [Global flags](../global-flags.md) — the `--model` override.
- [`bdd config`](config.md) — dump every key and where it came from.
- [`bdd steps generate`](steps.md#source-template-or-llm) — how LLM
  output is validated before it can stage.

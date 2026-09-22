# Getting started

## Install

Download an installer from the
[latest release](https://github.com/davidparry/spec-driven-agentic/releases/latest)
(macOS Apple Silicon and Intel, Linux x86_64 and arm64, Windows
x86_64), or build from source:

```bash
cd harness
cargo build --release
./target/release/spec --help
```

The shell installer places the binary in `$CARGO_HOME/bin` (usually
`~/.cargo/bin/spec`) and writes an install receipt to
`~/.config/spec-harness/spec-harness-receipt.json`.

## Local LLM (Ollama)

LLM-backed generation uses a local [Ollama](https://ollama.com)
instance. The model this harness is developed and run against is
`qwen3.8-flash-next:125b-mlx`:

```bash
ollama pull qwen3.8-flash-next:125b-mlx
spec model use qwen3.8-flash-next:125b-mlx
```

Your mileage will vary with a different model. A stronger coding model
may draft, generate, and implement better; a model trained for chat,
general knowledge, or work other than development will typically
produce weaker specs, step definitions, tests, and production code.
Without a reachable model the harness still runs — generation falls back
to deterministic templates.

## Your first session

Run bare `spec` in a terminal. You get the help, the banner with the
version, the model status, and an interactive prompt:

```text
$ spec

  ╭──────────────────────────────────╮
  │                                  ▼
  │    > spec  v0.4.0                 │
  │    spec → RED → GREEN → REFACTOR │
  ▲                                  │
  ╰──────────────────────────────────╯

Model set for this session: qwen3.8-flash-next:125b-mlx (not saved - keep it with: spec model use qwen3.8-flash-next:125b-mlx).
Interactive shell - type commands without the spec prefix (e.g. list).
spec>
```

## Two ways to begin a project

**Guided, from zero** — one command runs the whole loop with exactly
two human gates (approving the spec wording and approving generated
tests):

```bash
mkdir calculator && cd calculator
spec greenfield
```

**Step by step** — scaffold, then drive each phase yourself:

```bash
spec init --language rust --name "String Calculator"
spec draft          # describe what to build in plain words; with a
                        # model resolved it proposes title, story, and
                        # criteria for you to edit (manual prompts otherwise)
spec validate       # structure gate
spec refine REQ-001 # wording gate
spec changes commit      # apply the staged spec
spec test                # expect RED
# ...implement...
spec test                # expect GREEN
spec refactor --note "extract parser" --req REQ-001   # does the cleanup
git diff                 # read what it changed
spec test                # still GREEN
spec status              # confirm REQ-001 is ready to mark
spec mark-implemented REQ-001 && spec changes commit
```

This repository’s String Calculator workshop can be finished with the
same commands. The kata-specific recipe (paths under `kata/`, draft
REQ-007, then REQ-003…007 to implemented) is
[This workshop’s String Calculator](workshop.md).

## Where spec keeps its files

Harness files for a project live in one directory, `.spec/`, under the
project root (`--root`, or the current directory). They sit next to
`requirements/`, not loose in the root. `spec init` creates the
directory. The next `spec` run also creates it, and moves an older
root-level file into the matching path below when that new path is
still empty.

```text
.spec/
  config.toml    tracked — LLM, timeouts, per-command tool profiles
  state.json     TDD phase log
  memory.json    discovered language, libraries, and layout
  history        interactive-shell command history
  cache/         cached LLM responses and tool catalogs
  log/           daily diagnostic logs
  staged/        mutations waiting for spec changes commit
```

Only `config.toml` is meant to be committed. `spec init` writes a
gitignore that ignores `.spec/*` and keeps `.spec/config.toml`. The
other children are generated. Deleting `cache/` or `log/` is always
safe. Deleting `state.json` resets the phase to START. Deleting
`staged/` throws away mutations that have not been committed.

## Working against an existing project

Every command takes `--root` (see [Global flags](global-flags.md)), so
you can point the harness at any project:

```bash
spec --root ~/code/my-kata inspect
spec --root ~/code/my-kata validate
```

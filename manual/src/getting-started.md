# Getting started

## Install

Download an installer from the
[latest release](https://github.com/davidparry/tdd-bdd-agentic/releases/latest)
(macOS Apple Silicon and Intel, Linux x86_64 and arm64, Windows
x86_64), or build from source:

```bash
cd harness
cargo build --release
./target/release/bdd --help
```

The shell installer places the binary in `$CARGO_HOME/bin` (usually
`~/.cargo/bin/bdd`) and writes an install receipt to
`~/.config/bdd-harness/bdd-harness-receipt.json`.

## Local LLM (Ollama)

LLM-backed generation uses a local [Ollama](https://ollama.com)
instance. The model this harness is developed and run against is
`qwen3.8-flash-next:125b-mlx`:

```bash
ollama pull qwen3.8-flash-next:125b-mlx
bdd model use qwen3.8-flash-next:125b-mlx
```

Your mileage will vary with a different model. A stronger coding model
may draft, generate, and implement better; a model trained for chat,
general knowledge, or work other than development will typically
produce weaker specs, step definitions, tests, and production code.
Without a reachable model the harness still runs — generation falls back
to deterministic templates.

## Your first session

Run bare `bdd` in a terminal. You get the help, the banner with the
version, the model status, and an interactive prompt:

```text
$ bdd

  ╭──────────────────────────────────╮
  │                                  ▼
  │    > bdd  v0.4.0                 │
  │    spec → RED → GREEN → REFACTOR │
  ▲                                  │
  ╰──────────────────────────────────╯

Model set for this session: qwen3.8-flash-next:125b-mlx (not saved - keep it with: bdd model use qwen3.8-flash-next:125b-mlx).
Interactive shell - type commands without the bdd prefix (e.g. spec list).
bdd>
```

## Two ways to begin a project

**Guided, from zero** — one command runs the whole loop with exactly
two human gates (approving the spec wording and approving generated
tests):

```bash
mkdir calculator && cd calculator
bdd greenfield
```

**Step by step** — scaffold, then drive each phase yourself:

```bash
bdd init --language rust --name "String Calculator"
bdd spec draft          # describe what to build in plain words; with a
                        # model resolved it proposes title, story, and
                        # criteria for you to edit (manual prompts otherwise)
bdd spec validate       # structure gate
bdd spec refine REQ-001 # wording gate
bdd changes commit      # apply the staged spec
bdd test                # expect RED
# ...implement...
bdd test                # expect GREEN
bdd refactor --note "extract parser"
bdd test                # still GREEN
bdd status              # confirm REQ-001 is ready to mark
bdd spec mark-implemented REQ-001 && bdd changes commit
```

This repository’s String Calculator workshop can be finished with the
same commands. The kata-specific recipe (paths under `kata/`, draft
REQ-007, then REQ-003…007 to implemented) is
[This workshop’s String Calculator](workshop.md).

## Working against an existing project

Every command takes `--root` (see [Global flags](global-flags.md)), so
you can point the harness at any project:

```bash
bdd --root ~/code/my-kata inspect
bdd --root ~/code/my-kata spec validate
```

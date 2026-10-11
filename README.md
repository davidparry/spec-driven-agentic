# Spec-Driven with Harness — TDD & BDD in the Agentic Era

[![CI](https://github.com/davidparry/spec-driven-agentic/actions/workflows/ci.yml/badge.svg?branch=trunk)](https://github.com/davidparry/spec-driven-agentic/actions/workflows/ci.yml)
[![Release](https://github.com/davidparry/spec-driven-agentic/actions/workflows/release.yml/badge.svg)](https://github.com/davidparry/spec-driven-agentic/actions/workflows/release.yml)
[![spec harness](https://img.shields.io/github/v/release/davidparry/spec-driven-agentic?label=spec%20harness)](https://github.com/davidparry/spec-driven-agentic/releases/latest)
[![Harness coverage](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fdavidparry%2Fspec-driven-agentic%2Fbadges%2Fcoverage.json)](https://github.com/davidparry/spec-driven-agentic/actions/workflows/ci.yml)
[![Java coverage gate](https://img.shields.io/badge/JaCoCo-100%25%20gate-brightgreen)](pom.xml)
[![Quality gates](https://img.shields.io/badge/SpotBugs%20%7C%20PMD%20%7C%20clippy-enforced-blue)](.github/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/github/license/davidparry/spec-driven-agentic)](LICENSE)

> **Site:** [https://davidparry.github.io/spec-driven-agentic/](https://davidparry.github.io/spec-driven-agentic/)
> Install `spec`, download binaries, open the talk, and read the
> [write-up](https://davidparry.com/blog/2026/08/07/spec-first-was-always-right-agents-just-made-it-fast/).

`spec` is one native binary for a spec-driven loop: requirements, Gherkin,
RED, GREEN, REFACTOR. `spec mcp serve` is the same binary as an MCP server
(22 tools, wire identity `spec-driven-server` / `1.0.0`). The command
manual is [searchable online](https://davidparry.github.io/spec-driven-agentic/manual/);
the harness itself is documented in [`harness/README.md`](harness/README.md).

Generation uses a local coding model ([`spec model`](https://davidparry.github.io/spec-driven-agentic/manual/commands/model.html)).
Optionally, a second and different local model — a *decision* model,
which writes nothing and answers bounded questions with a typed value
and a probability — can judge whether an acceptance criterion is
actually measurable ([`spec judge`](https://davidparry.github.io/spec-driven-agentic/manual/commands/judge.html)).
It is off until you configure it, and a judgment is advice about
wording: it never changes a test result, a requirement's status, or a
deterministic finding. It needs `spec` 0.7.15 or newer; no earlier
release has a decision plane at all.

> **The workshop** — the 60-minute class, the kata, the slides, the student
> guide, and the exercises — is in [`talks/WORKSHOP.md`](talks/WORKSHOP.md).
> Students start at [`student-follow-along.md`](student-follow-docs/student-follow-along.md).

## Where `spec` keeps its files

The harness writes your project's real files — feature files, step definitions, tests, the spec — and git is the undo; `spec greenfield` and `spec deliver` offer a branch before they start. Everything the harness keeps for *itself* lives in one hidden directory, `.spec/`, next to `requirements/`. `spec init` creates it. The next `spec` run also creates it and moves an older root-level name (`.spec.toml`, `.spec-state.json`, `.spec-memory.json`, `.spec-history`, `.spec-cache/`, `.spec-log/`) into the matching path below when that new path is still empty.

```text
.spec/
  config.toml    tracked — LLM, decision model, timeouts, tool profiles
  state.json     TDD phase log
  memory.json    discovered language, libraries, and layout
  history        interactive-shell command history
  cache/         cached LLM responses and discovered tool catalogs
  log/           daily diagnostic logs (spec.log.YYYY-MM-DD)
  .lock          advisory lock serializing concurrent writes
```

Only `config.toml` is meant to be committed. The other children are gitignored. Deleting `cache/` or `log/` is always safe. Deleting `state.json` resets the phase to START.

## Install `spec`

Use the published installer from the [spec site](https://davidparry.github.io/spec-driven-agentic/). It places `spec` on your PATH. You want **0.7.15 or newer** — the `.spec/` directory described above replaced the flat `.spec.toml` in 0.6.0.

macOS and Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/davidparry/spec-driven-agentic/releases/latest/download/spec-harness-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/davidparry/spec-driven-agentic/releases/latest/download/spec-harness-installer.ps1 | iex"
```

Uninstall:

```bash
curl -LsSf https://github.com/davidparry/spec-driven-agentic/releases/latest/download/spec-harness-uninstaller.sh | sh -s -- -y
```

Prebuilt archives and checksums are on that same page.

### Install from this repository

Only do this when you are changing the harness itself. A `cargo build` writes `harness/target/debug/spec` and does **not** put `spec` on PATH. Cursor's `.cursor/mcp.json` runs `"command": "spec"`, which is `~/.cargo/bin/spec` after the installer, so a rebuild alone leaves the old server in place.

From the **repository root** (not from inside `harness/`):

```bash
cargo install --path harness
```

From `harness/` itself use `cargo install --path .` — `--path harness` from there looks for `harness/harness` and fails.

Then reload the MCP server in Cursor (toggle it off/on). To try a debug binary without installing: `harness/target/debug/spec mcp serve --root .`.

### Build a release binary without installing it

To exercise the optimised build the installer ships — timings, spinner behaviour, anything a debug build misreports — without replacing the `spec` on your PATH:

```bash
cargo build --release --manifest-path harness/Cargo.toml
harness/target/release/spec --version
```

Run it by path, `harness/target/release/spec`, for as long as you are testing it. `cargo install --path harness` is the same optimised build, so reach for it once you want the binary on PATH rather than beside it.

### Remove `spec` from this machine

The cargo copy and the installer copy are tracked separately, so removing one can leave the other answering `spec`. Undo the cargo one with the **package** name, `spec-harness`, not the binary name:

```bash
cargo uninstall spec-harness
which -a spec                 # nothing left
```

If `which -a spec` still finds one, it came from the published installer — remove that with the [uninstaller above](#install-spec). A `cargo build` copy is not on PATH at all; deleting `harness/target/` is enough.

## Verify everything is ready for production

Before pushing to `trunk` (which deploys the website) or tagging a
release, run this from the repository root. It builds and gates
everything that ships — the harness, the command manual, and the site:

```bash
(cd harness && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release) \
  && mdbook build manual \
  && scripts/build-pages.sh
```

1. **Harness** — format check, clippy with warnings as errors, the full
   unit + Cucumber suite, and the release binary
   (`harness/target/release/spec`). The same gates CI enforces.
2. **Manual** — regenerates [`docs/manual/`](docs/manual/) from
   [`manual/src`](manual/src). The output is committed, so any
   changes it produces belong in your commit. Must run before the site
   build, which copies the built book.
3. **Website** — assembles `_site/` exactly as the
   [Pages workflow](.github/workflows/pages.yml) does on push to
   `trunk`; a clean local run means a clean deploy.

One-time tools: `cargo install mdbook` and `pip install markdown`.

Two suites are `#[ignore]`d because they need a live local Ollama, so
run them by hand when you have touched what they cover:

```bash
cd harness
cargo test --test decision_live -- --ignored --nocapture   # the decision model and its labeled evaluation
cargo test --test greenfield_e2e -- --ignored --nocapture  # the full generative loop (minutes)
```

`decision_live` skips itself with a printed reason when no
decision-capable model is installed, so it is safe to run anywhere.

If you touched the Java smoke test, also run `mvn -pl smoke-test verify` and
`mvn -f kata/pom.xml test` (the standalone kata). The multi-platform
release binaries are built by the release workflow when a `v*` tag is
pushed (`scripts/release.sh`), not locally. The workshop branches and the
class CI gates are described in [`talks/WORKSHOP.md`](talks/WORKSHOP.md#branches-and-ci).

## License

AGPL-3.0 — see [LICENSE](LICENSE).

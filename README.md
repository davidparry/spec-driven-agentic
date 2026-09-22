# Spec-Driven with Harness — TDD & BDD in the Agentic Era

[![CI](https://github.com/davidparry/spec-driven-agentic/actions/workflows/ci.yml/badge.svg?branch=trunk)](https://github.com/davidparry/spec-driven-agentic/actions/workflows/ci.yml)
[![Release](https://github.com/davidparry/spec-driven-agentic/actions/workflows/release.yml/badge.svg)](https://github.com/davidparry/spec-driven-agentic/actions/workflows/release.yml)
[![spec harness](https://img.shields.io/github/v/release/davidparry/tdd-bdd-agentic?label=spec%20harness)](https://github.com/davidparry/spec-driven-agentic/releases/latest)
[![Harness coverage](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fdavidparry%2Ftdd-bdd-agentic%2Fbadges%2Fcoverage.json)](https://github.com/davidparry/spec-driven-agentic/actions/workflows/ci.yml)
[![Java coverage gate](https://img.shields.io/badge/JaCoCo-100%25%20gate-brightgreen)](pom.xml)
[![Quality gates](https://img.shields.io/badge/SpotBugs%20%7C%20PMD%20%7C%20clippy-enforced-blue)](.github/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/github/license/davidparry/tdd-bdd-agentic)](LICENSE)

> **Site:** [https://davidparry.github.io/spec-driven-agentic/](https://davidparry.github.io/spec-driven-agentic/)
> Install `spec`, download binaries, open the talk, and read the
> [write-up](https://davidparry.com/blog/2026/08/07/spec-first-was-always-right-agents-just-made-it-fast/).

`spec` is one native binary for a spec-driven loop: requirements, Gherkin,
RED, GREEN, REFACTOR. `spec mcp serve` is the same binary as an MCP server
(25 tools, wire identity `spec-driven-server` / `1.0.0`). The command
manual is [searchable online](https://davidparry.github.io/spec-driven-agentic/manual/);
the harness itself is documented in [`harness/README.md`](harness/README.md).

> **The workshop** — the 60-minute class, the kata, the slides, the student
> guide, and the exercises — is in [`talks/WORKSHOP.md`](talks/WORKSHOP.md).
> Students start at [`student-follow-along.md`](student-follow-docs/student-follow-along.md).

## Where `spec` keeps its files

Everything the harness writes for a project lives in one hidden directory, `.spec/`, next to `requirements/`. `spec init` creates it. The next `spec` run also creates it and moves an older root-level name (`.spec.toml`, `.spec-state.json`, `.spec-memory.json`, `.spec-history`, `.spec-cache/`, `.spec-log/`, `.spec-staged/`) into the matching path below when that new path is still empty.

```text
.spec/
  config.toml    tracked — LLM, timeouts, per-command tool profiles
  state.json     TDD phase log
  memory.json    discovered language, libraries, and layout
  history        interactive-shell command history
  cache/         cached LLM responses and discovered tool catalogs
  log/           daily diagnostic logs (spec.log.YYYY-MM-DD)
  staged/        mutations waiting for `spec changes commit`
```

Only `config.toml` is meant to be committed. The other children are gitignored. Deleting `cache/` or `log/` is always safe. Deleting `state.json` resets the phase to START. Deleting `staged/` throws away mutations that have not been committed.

## Install `spec`

Use the published installer from the [spec site](https://davidparry.github.io/spec-driven-agentic/). It places `spec` on your PATH. You want **0.5.4 or newer**.

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

If you touched the Java smoke test, also run `mvn -pl smoke-test verify` and
`mvn -f kata/pom.xml test` (the standalone kata). The multi-platform
release binaries are built by the release workflow when a `v*` tag is
pushed (`scripts/release.sh`), not locally. The workshop branches and the
class CI gates are described in [`talks/WORKSHOP.md`](talks/WORKSHOP.md#branches-and-ci).

## License

AGPL-3.0 — see [LICENSE](LICENSE).

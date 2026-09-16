# bdd config

Print every configuration key the harness uses and whether the value is a
code **default** or was read from the project file (`.bdd.toml`).

```text
Usage: bdd config [OPTIONS]
```

## Flags

| Flag | Description |
| --- | --- |
| `--json` | Machine-readable object: `file` plus `settings` (`key`, `value`, `source`). |
| `--root <ROOT>` | Project root that owns the config file. Defaults to `.`. |

## Output

Tab-separated columns: `key`, effective `value`, then the source:
`(default)`, `(discovered)`, or the absolute path of the file that
supplied it. The first line is `file` and the path that was opened,
`(none)` when no file exists, or the path plus `(invalid TOML)` /
`(unreadable)` when the file could not be used.

When the file names no `llm.model`, Ollama is asked which model a run
would actually use — the same order [`bdd model current`](model.md)
follows — and that name is shown as `(discovered)`. Nothing is
written; persist it with `bdd model use <name>`. A configured model
skips the provider call, and an unreachable or empty Ollama leaves the
key `(unset)`.

The file is read from `--root` only; there is no search of parent
directories. Run `bdd config` from the project root, or point
`--root` at it, or `file` reads `(none)` and every key is a default.

```bash
bdd config
```

```text
file	/Users/you/code/calculator/.bdd.toml
llm.model	qwen3.8-flash-next:125b-mlx	/Users/you/code/calculator/.bdd.toml
llm.endpoint	http://localhost:11434	(default)
llm.timeout_seconds	900	/Users/you/code/calculator/.bdd.toml
llm.cache_ttl_seconds	600	/Users/you/code/calculator/.bdd.toml
llm.retry	3	(default)
tools.max_rounds	12	(default)
tools.confirm	command_run	(default)
tools.discovery_timeout_seconds	10	(default)
tools.call_timeout_seconds	300	(default)
tools.cache_ttl_seconds	86400	(default)
tools.mcp_config	(unset)	(default)
tools.profiles.spec-draft	list_requirements, get_requirement, validate_spec, refine_requirement	(default)
tools.profiles.implement	get_requirement, feature_read, …	(default)
```

In a project that has never run `bdd model use`, the first row is the
name Ollama supplied:

```text
llm.model	qwen3:8b	(discovered)
```

A `[tools.profiles]` list for a caller **replaces** that command's
built-in tools and is attributed to the file. `bdd init` writes every
caller's default list so those rows show as from the file. Callers not
listed keep the code default. `[tools.enabled]` / `[tools.disabled]`
rows appear only when those tables are set.

Optional keys with nothing to show print `(unset)` with source
`(default)`: `tools.mcp_config`, and `llm.model` when Ollama is down
or has no models pulled.

## See also

- [`bdd model`](model.md) — persist `llm.model`.
- [`bdd tools`](tools.md) — per-command profiles and `mcp.json` tools.
- [Global flags](../global-flags.md) — `--model`, `--retry`, `--tools`
  override a value for one run and are not written to the file.

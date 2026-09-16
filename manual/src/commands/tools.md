# bdd tools

Inspect and attach the tools a model call may use. Built-in tools are
listed offline from `bdd mcp serve`. External MCP servers from
`mcp.json` are discovered on demand and must be attached **per command**
with `--for` — never globally.

```text
Usage: bdd tools [OPTIONS] <COMMAND>

Commands:
  list      Catalog (optionally `--for` one caller, `--offline`, `--refresh`)
  profiles  Every caller and its resolved set
  show      One tool's description and schema
  enable    Attach a tool to a caller (`--for` required)
  disable   Remove a tool from a caller (`--for` required)
  refresh   Rediscover external servers and rewrite `.bdd-cache/tools/`
  servers   Registered mcp.json servers and parse problems
```

```bash
bdd tools profiles
# spec-draft          4  list_requirements, get_requirement, validate_spec, refine_requirement
# implement           7  get_requirement, feature_read, …, command_run, changes_show
# status              7  project_root, list_requirements, …, changes_show, changes_validate

bdd tools list --for status          # exactly those seven
bdd tools list --offline             # built-ins only; never connects
bdd tools enable self__validate_spec --for status
bdd tools servers
```

`--for` accepts: `spec-draft`, `spec-reword`, `steps-generate`,
`unittest-generate`, `implement-advice`, `implement`, `status`, `ask`.
Omitting it lists those names and exits nonzero.

Default profiles contain no staging or commit tools. The only mutation
a CLI-side model may request is `command_run` on the `implement`
profile, and that call still asks the human to confirm.

External tools are namespaced `server__tool`. A name in config that is
not in the catalog is a warning, not a hard failure.

## `.bdd.toml` per-command mapping

Built-in defaults live in code and are also written into
`[tools.profiles]` by `bdd init` so you can see what each command
offers the model. If that table names a caller with a list, that list
**replaces** the default for that command. Omitted callers keep the
code defaults. `[tools.enabled]` adds names;
`[tools.disabled]` removes them. `bdd tools enable` / `disable` write
those last two tables.

After you add a server to `mcp.json`, run `bdd tools refresh` and put
the tool on the command that should use it:

```toml
[tools.profiles]
implement = ["get_requirement", "feature_read", "playwright:browser_navigate"]

[tools.enabled]
status = ["playwright:browser_navigate"]
```

### Name collisions

Built-in tools keep their catalog names (`validate_spec`). Tools from
`mcp.json` are catalogued as `server__tool` so they never overwrite a
built-in. When both exist, the **bare** name is the built-in. Pin the
origin when you need to be explicit:

| Written in `.bdd.toml` | Resolves to |
| --- | --- |
| `validate_spec` | built-in |
| `builtin:validate_spec` | built-in, even if an MCP tool shares the short name |
| `self:validate_spec` | mcp.json server `self` |
| `self__validate_spec` | same MCP tool (catalog name) |

`builtin` is reserved for the CLI's own tools. `bdd init` writes
`.bdd.toml` with every key and a live `[tools.profiles]` list for each
caller. [`bdd config`](config.md) prints the resolved set and whether
each value is a default or came from the file.

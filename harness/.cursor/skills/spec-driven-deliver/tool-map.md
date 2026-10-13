# Tool map, verification, and MCP constraints

Reference for which MCP tools drive each delivery stage. Read this when
a stage does not behave as expected or when you need to distinguish a
missing capability from an incorrect call.

## Stage to tool

Each line names a delivery stage and the tools that drive it.

- **Plan resolution** — `list_requirements` for the catalog and its
  prefix; `get_requirement` for an ask's `derived` and `ready`. Prose
  goes through [shaping.md](shaping.md).
- **Branch gate** — no MCP tool. Use git once, before the first write.
- **Scenarios** — `feature_create` when a feature does not exist, then
  `scenario_add`.
- **Step definitions** — `step_definitions_find`, then
  `step_definition_create`, then write the generated bodies. The tool
  scaffolds; it does not implement the bodies.
- **Unit test** — `unit_test_create`, then write the assertions.
- **Test to RED and GREEN** — `run_tests`.
- **Implementation** — no generation tool exists on the MCP surface.
  Edit the production files directly, using `command_run` only when a
  build or dependency command is needed.
- **Refactor** — `start_refactor` opens the phase; make one bounded
  change, then prove the bar with `run_tests`.
- **Mark implemented** — `requirement_mark_implemented`, followed by
  `validate_spec`.
- **Close asks** — `requirement_mark_implemented` on the ask id.

Read-only tools with no stage of their own, worth reaching for:
`project_root`, `project_inspect`, `get_tdd_state`, `git_diff`,
`feature_list`, `feature_read`.

## Verification

Verify each stage by re-reading the observable project state. These four
checks establish whether its gap is closed:

- **Scenario present** — `feature_read` the file and look for the
  `@REQ-xxx` tag. `validate_spec` will not catch a missing one, because
  it only enforces tags for requirements already marked implemented.
- **Steps defined** — `step_definitions_find` returns an empty `missing`.
  This is the same missing-step logic the survey uses, so it is exact
  rather than a substitute.
- **Unit test present** — the `target` path from the `unit_test_create`
  reply, read back off disk.
- **Marked** — `get_requirement` shows `status: "implemented"`, then
  `validate_spec` reports `valid: true`.

A gap still open after its stage wrote stops that requirement. Do not
retry: these tools are deterministic, and a second identical call lands
in the same place. Report what the check saw.

## The gaps, and what to do about each

### Paths are discovered, never assumed

Use `project_inspect`, existing project conventions, and the `target`
fields returned by mutation tools as the authorities for write
locations. Requirement records describe behaviour; do not treat
path-like metadata on them, examples in this skill, or a location from a
previous run as authority for the current project.

When an inspection result names a directory, choose the file by reading
the nearby implementation and extending the component that already owns
the behaviour. When a mutation tool returns a `target`, write only to
that returned target.

`featureLocation` may be absent until a scenario is recorded. Absence is
not permission to invent a conventional location; discover the feature
files first.

### Prose cannot be decomposed by the server

`requirement_ask` has no model. It preserves the words and derives
nothing. You do the breakdown. Covered in [shaping.md](shaping.md).

### `set_feature` has no tool, and needs none

There is no MCP operation for recording the feature file on a
requirement immediately after scenario authoring, and none is needed.
`requirement_mark_implemented` finds the scenario tagged with the
requirement id and records its file as part of marking. A later
`validate_spec` then passes. Some catalogs already carry `featureFile`
on pending requirements, in which case marking has nothing to change.

So do not hand-edit the requirements file to set it, and do not worry
that `featureLocation` is empty between stage 1 and stage 8.

### No source snapshot or restore

The MCP surface does not snapshot or restore source files. Use
`git_diff` to inspect what an attempt landed and git to undo an attempt
that breaks the build. This is why the branch gate matters.

### Code-stage judgments are not on the wire

Only `refine_requirement` exposes model judgments, and those concern
wording. The MCP surface does not provide judgments for whether a step
binds its scenario, a unit test asserts its criterion, an implementation
is complete, or a refactor preserves behaviour.

[tdd-loop.md](tdd-loop.md) expresses those as self-checks. Be honest
about what that means: asking yourself is not a gate. Never report that a
judgment was taken, and never present a self-check as evidence. The
evidence is the test bar.

### No orchestration or implementation tool

The MCP surface exposes deterministic primitives, not a one-call
delivery or implementation operation. The client sequences the tools and
writes the implementation.

## Concurrency

Mutating tools hold a write lock, so parallel calls will not corrupt a
file. What they will corrupt is the **TDD phase**, which is a single
global value owned by the server. Two requirements in flight means one
run's `run_tests` decides the other's phase, and `start_refactor` or
`requirement_mark_implemented` then gate on the wrong bar.

Strictly one requirement at a time. No parallel subagents.

## Errors

A failed tool call comes back as a successful JSON-RPC response with
`is_error` set and the message as text. Most are plain prose. Two are
structured:

- `runtime_missing` — a JSON body naming the runtime and a hint. The
  language runtime is not installed; the state was not touched.
- A decision gate that was configured to enforce and could not get an
  answer — reported as a tool error rather than a finding, because it is
  not wording you can reword.

Read the message. These tools say what is wrong and name the way
through more often than not.

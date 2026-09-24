# spec test

Run the project's tests through its own build tool and update the
persistent RED/GREEN/REFACTOR phase from the results. This is the
heartbeat of the workflow.

```text
Usage: spec test [OPTIONS]
```

MCP tool equivalent: `run_tests`.

## Flags

| Flag | Description |
| --- | --- |
| `--feature <FEATURE>` | Run only one feature (path or name, passed to the runner's filter). |
| `--scenario <SCENARIO>` | Run only one scenario by name. |
| `--root <ROOT>` | Project root. Defaults to `.`. |
| `--model <MODEL>` | Accepted (global flag) but unused — running tests never involves an LLM. |

## How the runner is chosen

The runner follows the detected language and shells out to the
project's own toolchain:

| Language | Command under the hood |
| --- | --- |
| Java | `mvn test` |
| JavaScript / TypeScript | `npm test` (Cucumber-JS) |
| .NET | `dotnet test` |
| Rust | `cargo test` |

If the runtime is missing, the command **refuses instead of
pretending**:

```text
Error: runtime_missing: mvn is not installed. Install Maven (and a JDK) to run tests; the harness reports, it never installs.
```

## Examples

A failing run — the phase moves to RED:

```bash
spec test
```

```json
{
  "phase": "RED",
  "tests": 3,
  "failures": 1,
  "errors": 0,
  "skipped": 0,
  "failureDetails": [
    "Two numbers separated by a comma are summed: expected 3 but was 0"
  ],
  "nextStep": "You are RED. Write just enough production code to make the failing test pass, then run tests again."
}
```

After implementing — GREEN:

```json
{
  "phase": "GREEN",
  "tests": 3,
  "failures": 0,
  "errors": 0,
  "skipped": 0,
  "failureDetails": [],
  "nextStep": "You are GREEN. Refactor with 'spec refactor', or mark the requirement implemented and pick the next one."
}
```

Filtered runs:

```bash
spec test --feature features/string_calculator.feature
spec test --scenario "Two numbers separated by a comma are summed"
```

Filters are forwarded to the underlying runner (e.g. Cucumber's name
filter), so only the selected slice executes — useful while iterating
on one scenario.

## Phase semantics

- Any failure or error ⇒ `RED`.
- All passing ⇒ `GREEN` (also ends a REFACTOR step successfully).
- A failing run during REFACTOR drops you back to RED — the refactor
  broke behavior.

The phase is stored in `.spec/state.json` and read back by
[`spec state`](state.md) and enforced by [`spec refactor`](refactor.md).

## See also

- [The workflow](../workflow.md) — the phase machine in full.
- [`spec inspect`](inspect.md) — check the runtime before expecting a run.

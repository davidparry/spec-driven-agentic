# bdd validate

Validate all Gherkin in the project — committed feature files **and**
staged ones — so a broken scenario never reaches a test run. This is
the cheap gate to run before `bdd changes commit`. MCP clients call the
same check as `changes_validate` (`validate_spec` remains the on-disk
frozen tool).

```text
Usage: bdd validate [OPTIONS]
```

## Flags

Only the [global flags](../global-flags.md) (`--root`, `--model`).

## What is checked

- Every `.feature` file under the root parses as valid Gherkin.
- Every file in the staging area (`.bdd-staged/`) that is a feature
  file parses too — you cannot commit a transaction containing broken
  Gherkin without knowing.
- Scenario requirement tags (`@REQ-...`) refer to ids that exist in
  the spec.

## Examples

Everything clean:

```bash
bdd validate
```

```json
{
  "valid": true,
  "issues": [],
  "nextStep": "Gherkin is clean. Run 'bdd test' or commit staged changes."
}
```

Problems found (the command exits 0; the report carries the verdict):

```json
{
  "valid": false,
  "issues": [
    "features/string_calculator.feature: (5:3) expected a step keyword",
    "staged features/newlines.feature: scenario 'Newlines act as delimiters' is tagged @REQ-009 but the spec has no such requirement"
  ],
  "nextStep": "Fix the listed files (staged ones via their originating command), then validate again."
}
```

## Relation to `bdd spec validate`

| Command | Validates |
| --- | --- |
| `bdd spec validate` | The requirements JSON: shape, ids, statuses, criterion phrasing. |
| `bdd validate` | The Gherkin: feature files on disk and in the stage, plus tag/spec consistency. |

Run both before a commit-and-test cycle; both appear as `nextStep`
suggestions at the appropriate moments.

## See also

- [`bdd changes`](changes.md) — the staged transaction this gate protects.
- [`bdd feature show`](feature.md#bdd-feature-show) — inspect a file that failed to parse.

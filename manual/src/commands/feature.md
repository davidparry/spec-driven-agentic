# spec feature

Feature discovery and creation. Feature files are the BDD altitude of
the workflow: each requirement gets a scenario in one, tagged with its
`@REQ-...` id.

```text
Usage: spec feature [OPTIONS] <COMMAND>

Commands: list, show, create
```

---

## spec feature list

List every feature file under the root with its feature name and
scenario count.

```bash
spec feature list
```

```json
[
  {
    "path": "features/string_calculator.feature",
    "name": "String Calculator",
    "scenarios": 3
  }
]
```

---

## spec feature show

Show one parsed feature file — its name, scenarios, tags, and steps —
as structured JSON rather than raw text.

```text
Usage: spec feature show [OPTIONS] <PATH>
```

The path is relative to `--root`:

```bash
spec feature show features/string_calculator.feature
```

```json
{
  "path": "features/string_calculator.feature",
  "name": "String Calculator",
  "scenarios": [
    {
      "name": "Empty string returns zero",
      "tags": ["@REQ-001"],
      "steps": [
        "Given the input \"\"",
        "When add is called",
        "Then the result is 0"
      ]
    }
  ]
}
```

A file that is not valid Gherkin fails with the parser's diagnosis.

---

## spec feature create

Create a feature file. The file is **staged**, not written to the
working tree — review with [`spec changes show`](changes.md) and apply
with `spec changes commit`.

```text
Usage: spec feature create [OPTIONS] --path <PATH> --name <NAME>
```

| Flag | Description |
| --- | --- |
| `--path <PATH>` | Feature file path relative to `--root` (conventionally under `features/`). |
| `--name <NAME>` | Feature name — the text after `Feature:`. |

```bash
spec feature create --path features/string_calculator.feature --name "String Calculator"
spec changes show
spec changes commit
```

The staged file contains the `Feature:` header ready for scenarios:

```gherkin
Feature: String Calculator
```

Add scenarios with [`spec scenario add`](scenario.md) — don't edit the
staged file by hand.

## See also

- [`spec scenario`](scenario.md) — populate features with tagged scenarios.
- [`spec changes validate`](changes.md#spec-changes-validate) — parse-check all features, staged included.

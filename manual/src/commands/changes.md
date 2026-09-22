# spec changes

Staged-transaction management. Every file mutation the harness authors
lands in `.spec/staged/` first (see [Staged changes](../staged-changes.md));
these subcommands are how you review, apply, or drop the transaction.

```text
Usage: spec changes [OPTIONS] <COMMAND>

Commands: show, commit, discard, validate
```

None of the subcommands take flags beyond the
[global flags](../global-flags.md).

---

## spec changes show

List everything currently staged: the path, whether applying would
create or modify the file, and a one-line summary of the change.

```bash
spec changes show
```

```json
{
  "changes": [
    {
      "path": "features/string_calculator.feature",
      "action": "modify",
      "summary": "append scenario 'Two numbers separated by a comma are summed' tagged @REQ-003"
    },
    {
      "path": "features/step_definitions/string_calculator_steps.js",
      "action": "create",
      "summary": "2 step definitions generated for undefined steps"
    }
  ],
  "nextStep": "Apply with 'spec changes commit' or drop with 'spec changes discard'."
}
```

An empty stage:

```json
{
  "changes": [],
  "nextStep": "Nothing is staged. Authoring commands (feature, scenario, steps, unittest, spec draft) stage their output here."
}
```

To see the full content of a staged file, read it directly under
`.spec/staged/` — the layout mirrors the project tree.

---

## spec changes commit

Apply every staged change to the working tree atomically and clear
the stage. Files marked `create` are written fresh; `modify` replaces
the working copy with the staged version.

```bash
spec changes commit
```

Run [`spec changes validate`](#spec-changes-validate) first when the
transaction contains Gherkin — broken staged Gherkin is reported there
before it can land.

After applying, `commit` re-validates the working tree. Open issues
ride along in the reply as a warning — the commit still happened, but
an invalid spec never lands silently:

```json
{
  "changes": [
    { "path": "requirements/requirements.json", "action": "modify", "summary": "mark REQ-001 implemented" }
  ],
  "issues": [
    "REQ-001: implemented requirements must name their featureFile - rerun spec mark-implemented REQ-001 on GREEN to backfill it"
  ],
  "nextStep": "Staged changes applied, but the working tree does not validate - fix the issues above, then run spec changes validate again."
}
```

A clean commit carries no `issues` field.

---

## spec changes discard

Drop the entire staged transaction. The working tree is untouched; the
stage is emptied. There is no partial discard — the stage is one
transaction by design (a scenario without its step definitions is not
a state worth keeping).

```bash
spec changes discard
```

---

## spec changes validate

Validate all Gherkin in the project — committed feature files **and**
staged ones — so a broken scenario never reaches a test run. This is
the cheap gate to run before `spec changes commit`. MCP clients call
the same check as `changes_validate` (`validate_spec` remains the
on-disk frozen tool).

```text
Usage: spec changes validate [OPTIONS]
```

### What is checked

- Every `.feature` file under the root parses as valid Gherkin.
- Every file in the staging area (`.spec/staged/`) that is a feature
  file parses too — you cannot commit a transaction containing broken
  Gherkin without knowing.
- Scenario requirement tags (`@REQ-...`) refer to ids that exist in
  the spec.

### Examples

Everything clean:

```bash
spec changes validate
```

```json
{
  "valid": true,
  "issues": [],
  "nextStep": "Gherkin is clean. Run 'spec test' or commit staged changes."
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

### Relation to `spec validate`

| Command | Validates |
| --- | --- |
| `spec validate` | The requirements JSON: shape, ids, statuses, criterion phrasing. |
| `spec changes validate` | The Gherkin: feature files on disk and in the stage, plus tag/spec consistency. |

Run both before a commit-and-test cycle; both appear as `nextStep`
suggestions at the appropriate moments.

## A typical review session

```bash
spec scenario add --feature features/calc.feature --req REQ-002 \
    --name "A single number is returned" --step 'Given the input "5"' \
    --step 'When add is called' --step 'Then the result is 5'
spec steps generate
spec changes show                 # one modify + one create
spec changes validate                     # staged Gherkin parses, tags resolve
spec changes commit               # both land together
spec test                         # honest RED
```

## See also

- [Staged changes](../staged-changes.md) — the model and its rationale.
- [`spec validate`](spec.md#spec-validate) — the requirements-spec gate, the other half of the pair.
- [`spec feature show`](feature.md#spec-feature-show) — inspect a file that failed to parse.

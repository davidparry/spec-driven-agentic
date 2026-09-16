# bdd changes

Staged-transaction management. Every file mutation the harness authors
lands in `.bdd-staged/` first (see [Staged changes](../staged-changes.md));
these subcommands are how you review, apply, or drop the transaction.

```text
Usage: bdd changes [OPTIONS] <COMMAND>

Commands: show, commit, discard
```

None of the subcommands take flags beyond the
[global flags](../global-flags.md).

---

## bdd changes show

List everything currently staged: the path, whether applying would
create or modify the file, and a one-line summary of the change.

```bash
bdd changes show
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
  "nextStep": "Apply with 'bdd changes commit' or drop with 'bdd changes discard'."
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
`.bdd-staged/` — the layout mirrors the project tree.

---

## bdd changes commit

Apply every staged change to the working tree atomically and clear
the stage. Files marked `create` are written fresh; `modify` replaces
the working copy with the staged version.

```bash
bdd changes commit
```

Run [`bdd validate`](validate.md) first when the transaction contains
Gherkin — broken staged Gherkin is reported there before it can land.

After applying, `commit` re-validates the working tree. Open issues
ride along in the reply as a warning — the commit still happened, but
an invalid spec never lands silently:

```json
{
  "changes": [
    { "path": "requirements/requirements.json", "action": "modify", "summary": "mark REQ-001 implemented" }
  ],
  "issues": [
    "REQ-001: implemented requirements must name their featureFile - rerun bdd spec mark-implemented REQ-001 on GREEN to backfill it"
  ],
  "nextStep": "Staged changes applied, but the working tree does not validate - fix the issues above, then run bdd validate again."
}
```

A clean commit carries no `issues` field.

---

## bdd changes discard

Drop the entire staged transaction. The working tree is untouched; the
stage is emptied. There is no partial discard — the stage is one
transaction by design (a scenario without its step definitions is not
a state worth keeping).

```bash
bdd changes discard
```

## A typical review session

```bash
bdd scenario add --feature features/calc.feature --req REQ-002 \
    --name "A single number is returned" --step 'Given the input "5"' \
    --step 'When add is called' --step 'Then the result is 5'
bdd steps generate
bdd changes show                 # one modify + one create
bdd validate                     # staged Gherkin parses, tags resolve
bdd changes commit               # both land together
bdd test                         # honest RED
```

## See also

- [Staged changes](../staged-changes.md) — the model and its rationale.
- [`bdd validate`](validate.md) — the gate before `commit`.

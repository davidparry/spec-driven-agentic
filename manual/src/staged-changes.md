# Staged changes

Every command that would write into your project — feature creation,
scenario mutations, step-definition and unit-test generation, spec
drafting, marking a requirement implemented — writes to a staging area
instead: `.bdd-staged/` under the project root. Nothing touches your
working tree until you commit the transaction.

## Why

- **Review before apply.** You (or an agent's human supervisor) can
  inspect exactly what would change.
- **Atomicity.** A multi-file change (say, a feature file plus new
  step definitions) lands together or not at all.
- **Safe agents.** Over MCP, a model can propose file changes without
  ever holding write access to your tree.

## The lifecycle

```bash
bdd feature create --path features/calculator.feature --name "String Calculator"
bdd changes show      # review: one staged "create"
bdd changes commit    # apply to the working tree, clear the stage
# or
bdd changes discard   # drop everything staged, tree untouched
```

`bdd changes show` lists each staged entry with its action and path:

```json
{
  "changes": [
    { "action": "create", "path": "features/calculator.feature" }
  ],
  "nextStep": "Review the staged changes, then 'bdd changes commit' to apply or 'bdd changes discard' to drop them."
}
```

## What stages and what doesn't

| Writes to the stage | Writes directly |
| --- | --- |
| `feature create` | `init` (scaffolding a fresh project) |
| `scenario add` / `update` / `delete` | `model use` (writes `.bdd.toml`) |
| `steps generate` | `test` / `refactor` (phase state file) |
| `unittest generate` | |
| `spec draft` | |
| `spec mark-implemented` | |

Validation (`bdd validate`) checks staged Gherkin too, so you can gate
a transaction before committing it.

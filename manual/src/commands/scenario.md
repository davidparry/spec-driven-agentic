# spec scenario

Scenario mutations. Every scenario is tied to a requirement by a
`@REQ-...` tag, keeping the feature files traceable back to the spec.
All four subcommands write to the [staging area](../staged-changes.md).

```text
Usage: spec scenario [OPTIONS] <COMMAND>

Commands: generate, add, update, delete
```

`generate` writes a requirement's scenarios for you, from its acceptance
criteria. The other three are the typed mutations underneath it, for when
you want to say exactly what goes in.

---

## spec scenario generate

Turn one requirement's acceptance criteria into tagged scenarios — one
scenario per criterion, in order.

```text
Usage: spec scenario generate [OPTIONS] <REQ_ID>
```

| Flag | Description |
| --- | --- |
| `<REQ_ID>` | The requirement whose criteria become scenarios. |
| `--feature <FEATURE>` | Feature file to append to. Defaults to the requirement's own `featureFile`. |

```bash
spec scenario generate REQ-003
```

```json
{
  "feature": "kata/src/test/resources/features/string_calculator.feature",
  "scenarios": [
    "Two numbers separated by a comma are summed",
    "Two larger numbers separated by a comma are summed"
  ],
  "criteria": 2,
  "staged": true,
  "source": "llm",
  "nextStep": "Read the steps against the acceptance criteria, apply with spec changes commit, then run spec steps missing."
}
```

### Where the wording comes from

Acceptance criteria are already `Given …, when …, then …`, so a literal
reading of them is always available and needs no model. That literal
reading is the **template**, and `source` reports `template` when it is
what got staged.

It is correct but rarely idiomatic: it cannot know that the feature file
it is joining opens every scenario on `Given a string calculator`. So
when a model is resolved it is asked to say the same thing in the file's
own vocabulary, with the file and the existing step definitions as
context — the `scenario-generate` tool profile is `get_requirement`,
`feature_read`, `step_definitions_find`. `source` then reports `llm`.

| | The steps for `Given "1,2", when add is called, then the result is 3` |
| --- | --- |
| `template` | `Given "1,2"` / `When add is called` / `Then the result is 3` |
| `llm` | `Given a string calculator` / `When I add "1,2"` / `Then the result is 3` |

The second reuses steps that already have definitions, so
[`spec steps missing`](steps.md) comes back empty. The first is two new
undefined steps.

### What it refuses

The model's reply is only used when it holds **exactly one scenario per
criterion**, every step opens with a Gherkin keyword, every scenario has
a `When` and a `Then`, and no name collides with one already in the file
or with another in the reply. A reply failing any of those is retried
with the reason, and the template is staged if the retries run out —
coverage is never quietly lost to a chatty model.

The command itself refuses two situations outright:

- The requirement already has scenarios tagged `@REQ-...` in that file.
  Running twice would stack a second copy of each. Change them with
  [`spec scenario update`](#spec-scenario-update) or delete them first.
- None of the criteria are Given/When/Then shaped, so there is nothing to
  read. Reword the requirement with [`spec reword`](spec.md#spec-reword).

A requirement with no `featureFile` and no `--feature` is refused too,
naming [`spec set-feature`](spec.md#spec-set-feature) as the remedy;
passing `--feature` also points the requirement at that file, exactly as
`spec scenario add` does.

---

## spec scenario add

Append a tagged scenario to an existing feature file.

```text
Usage: spec scenario add [OPTIONS] --feature <FEATURE> --req <REQ> --name <NAME>
```

| Flag | Description |
| --- | --- |
| `--feature <FEATURE>` | Feature file path relative to `--root`. |
| `--req <REQ>` | Requirement id the scenario implements; becomes the `@REQ-...` tag. |
| `--name <NAME>` | Scenario name. |
| `--step <STEPS>` | One full Gherkin step per flag, repeatable, in order. |

```bash
spec scenario add \
  --feature features/string_calculator.feature \
  --req REQ-003 \
  --name "Two numbers separated by a comma are summed" \
  --step 'Given the input "1,2"' \
  --step 'When add is called' \
  --step 'Then the result is 3'
```

The staged result appended to the feature:

```gherkin
  @REQ-003
  Scenario: Two numbers separated by a comma are summed
    Given the input "1,2"
    When add is called
    Then the result is 3
```

Each `--step` must start with a Gherkin keyword (`Given`, `When`,
`Then`, `And`, `But`); the mutation is validated as real Gherkin
before it stages.

---

## spec scenario update

Replace a scenario's steps and/or its requirement tag. The scenario is
found by feature path + scenario name; omitted parts are kept.

```text
Usage: spec scenario update [OPTIONS] --feature <FEATURE> --name <NAME>
```

| Flag | Description |
| --- | --- |
| `--feature <FEATURE>` | Feature file path relative to `--root`. |
| `--name <NAME>` | Name of the scenario to update. |
| `--req <REQ>` | New requirement id for the tag; omit to keep the current tag. |
| `--step <STEPS>` | New steps (repeatable, full replacement); omit to keep the current steps. |

Retag a scenario without touching its steps:

```bash
spec scenario update \
  --feature features/string_calculator.feature \
  --name "Two numbers separated by a comma are summed" \
  --req REQ-007
```

Rewrite the steps:

```bash
spec scenario update \
  --feature features/string_calculator.feature \
  --name "Two numbers separated by a comma are summed" \
  --step 'Given the input "10,20"' \
  --step 'When add is called' \
  --step 'Then the result is 30'
```

---

## spec scenario delete

Remove a scenario from a feature file.

```text
Usage: spec scenario delete [OPTIONS] --feature <FEATURE> --name <NAME>
```

```bash
spec scenario delete \
  --feature features/string_calculator.feature \
  --name "Two numbers separated by a comma are summed"
```

Deleting a scenario that does not exist fails with exit status 1 and
names the feature searched.

## The full rhythm

```bash
spec scenario generate REQ-002   # or scenario add, to write them yourself
spec changes show      # review the staged modify
spec changes commit    # apply
spec steps missing     # any steps without definitions?
spec test              # expect RED
```

## See also

- [`spec steps`](steps.md) — find and generate the step definitions
  behind these scenarios.
- [`spec changes`](changes.md) — review, apply, or discard the staged mutation.

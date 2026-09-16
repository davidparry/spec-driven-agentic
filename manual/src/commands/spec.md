# bdd spec

Requirements spec tools. The spec at
`requirements/requirements.json` is the project's source of truth;
these subcommands read it, gate it, and mutate it (through the
[staging area](../staged-changes.md)). The root file is a
[catalog](../spec-format.md#the-catalog-splitting-the-spec-across-files):
it may include child spec files, and every subcommand operates on the
merged view of the whole tree.

```text
Usage: bdd spec [OPTIONS] <COMMAND>

Commands: list, show, draft, validate, refine, reword, set-feature, mark-implemented, include
```

MCP tool equivalents: `list_requirements`, `get_requirement`,
`validate_spec`, `refine_requirement`, `requirement_reword`,
`requirement_mark_implemented`.

---

## bdd spec list

List every requirement with its id, title, status, and the spec file
it lives in — requirements from included files merge into one backlog.

```bash
bdd spec list
```

```json
[
  { "id": "REQ-001", "title": "Empty string returns zero", "status": "implemented",
    "file": "requirements/requirements.json" },
  { "id": "REQ-002", "title": "A single number is returned as-is", "status": "pending",
    "file": "requirements/requirements.json" },
  { "id": "REQ-003", "title": "Two numbers separated by a comma are summed", "status": "pending",
    "file": "requirements/core/arithmetic.json" }
]
```

Use it to pick the next `pending` requirement to work on.

---

## bdd spec show

Show one requirement, enriched with file locations and a workflow
hint.

```text
Usage: bdd spec show [OPTIONS] <REQ_ID>
```

```bash
bdd spec show REQ-003
```

```json
{
  "id": "REQ-003",
  "title": "Two numbers separated by a comma are summed",
  "status": "pending",
  "story": "As a user, I want comma-separated numbers to be summed so that I can add multiple values at once.",
  "acceptanceCriteria": [
    "Given \"1,2\", when add is called, then the result is 3",
    "Given \"10,20\", when add is called, then the result is 30"
  ],
  "featureLocation": "features/string_calculator.feature",
  "stepDefinitions": "features/step_definitions/calculator_steps.js",
  "testLocation": "test/calculator.test.js",
  "productionLocation": "src/calculator.js",
  "workflowHint": "Write the Gherkin scenario for this requirement in the feature file first (tag it @REQ-003), reuse or add step definitions, then run tests to see RED."
}
```

An unknown id fails with exit status 1:

```text
Error: no requirement with id REQ-999
```

---

## bdd spec draft

Interactively draft a requirement. Human input drives the spec — the
harness never invents requirements. The draft is validated and
quality-gated in a loop until it is clean, then staged.

```bash
bdd spec draft
```

Non-interactive (no TTY). Repeat `--criterion` for each Given/When/Then
line. The next free id is allocated; the draft is staged.

```bash
bdd spec draft \
  --title "Newlines as delimiters" \
  --story "As a calculator user, I want newlines to separate numbers in addition to commas so that multi-line input just works." \
  --criterion 'Given the input "1\n2,3", when add is called, then the result is 6' \
  --criterion 'Given an empty string "", when add is called, then the result is 0'
```

`--title`, `--story`, and at least one `--criterion` must be given
together. Structural validate must pass; refine findings are reported
in `nextStep` (`bdd spec reword <id>` to address them). A title or
criterion that collides with an existing requirement warns but still
stages.

By default the draft lands in the root `requirements.json`. To draft
into an included spec file instead, name it with `--file` (it must
already be part of the catalog — see
[`bdd spec include`](#bdd-spec-include)):

```bash
bdd spec draft --file requirements/core/arithmetic.json
```

Ids are allocated across the whole catalog, so a requirement drafted
into a child file still gets the next free `REQ-nnn`.

The prompts, first pass (interactive wizard, flags omitted):

```text
REQ-004 title:
REQ-004 story:
Acceptance criteria (Given/When/Then). A blank criterion ends the list:
REQ-004 criterion 1 (leave blank to finish the criteria):
REQ-004 criterion 2 (leave blank to finish the criteria):
```

- The id is allocated automatically (next free `REQ-nnn`).
- A **blank criterion ends the list** — enter at least one first.
- On a color console the bracketed suggestion — the text
  <kbd>Enter</kbd> will use — renders green; the destructive
  `'-' drops it` hint on criterion prompts, model failures, and other
  dead ends render red; the animated dots on `working ...` lines
  render light yellow.
- On a real terminal answers are edited on a `> ` line with full line
  editing — arrow keys move the cursor anywhere in the typed text,
  Home/End jump, and the up arrow recalls this session's answers.

If validation or refinement finds problems, each finding prints with a
concrete suggestion, and the rewording pass shows your prior answers:

```text
REQ-004: acceptance criterion 1 must be phrased Given/When/Then
  try: rephrase as: Given <starting state>, when <action>, then <exact result>

REQ-004 title [Newlines act as delimiters] (Enter keeps it):
REQ-004 story [As a user, I want newlines to work like commas so that input can be multi-line.] (Enter keeps it):
REQ-004 criterion 1 [it should handle newlines] (Enter keeps it, '-' drops it):
Given "1\n2", when add is called, then the result is 3
REQ-004 criterion 2 (leave blank to finish the criteria):
```

- <kbd>Enter</kbd> keeps the prior answer.
- `-` drops a prior criterion.
- New criteria can be appended after the priors.

With a resolved model, `bdd spec draft` runs the same
description-driven wizard as [greenfield](greenfield.md#the-description-driven-wizard),
and findings are sent to the model first: the rewording prompts carry
its corrected proposal instead of your raw prior answers, so
<kbd>Enter</kbd> accepts each fix (for the happy-paths finding, that
includes the edge-case criterion it added):

```text
Findings to address:
  - criteria: only happy paths - add at least one edge case (empty, invalid, or error input)
    try: add an edge case, e.g. Given an empty string "", when add is called, then the result is 0
Asking qwen3.8-flash-next:125b-mlx to address finding 1 of 1 - working ...
The model reworded the draft. Each prompt shows its proposal - Enter accepts it, or type your own wording.
REQ-004 title [Newlines act as delimiters] (Enter keeps it):
REQ-004 criterion 2 [Given an empty string "", when add is called, then the result is 0] (Enter keeps it, '-' drops it):
```

Each finding is its own model call: call 1 addresses finding 1 on your
draft, call 2 addresses finding 2 on the draft call 1 produced, and so
on — the fixes accumulate one finding at a time instead of asking for
everything at once. An unusable reply skips that finding (it stays
yours to fix at the prompts); a model error ends the chain and keeps
whatever fixes already landed.

If the review rejects a wording again, the next model call also
recounts every earlier wording of the draft and the findings each one
produced, so the model never proposes a wording the review already
rejected.

On a terminal the `working ...` lines animate — the trailing dots grow
`.` `..` `...` in light yellow and start over — while the model call
is in flight.

If the model is unreachable or its rewording is unusable, the prompts
fall back to your prior answers exactly as without a model. Nothing is
accepted silently either way — every field still passes through your
hands, and validate + refine rerun on whatever you accept.

When the draft is clean it is staged:

```json
{
  "id": "REQ-004",
  "title": "Newlines act as delimiters",
  "staged": true,
  "nextStep": "Review with 'bdd changes show', apply with 'bdd changes commit', then write the Gherkin scenario."
}
```

---

## bdd spec validate

Validate the whole spec on disk: JSON shape, required fields, unique
ids, status values, and Given/When/Then phrasing of every criterion.
The full include tree is validated as one catalog: missing included
files, include cycles, and duplicate ids across files are each one
actionable issue (a cross-file duplicate names the file that declared
the id first). Only the root document needs a `project` name, and the
merged tree must hold at least one requirement.

```bash
bdd spec validate
```

Valid spec:

```json
{
  "valid": true,
  "issues": [],
  "nextStep": "Pick a pending requirement with 'bdd spec list' and write its scenario."
}
```

Invalid spec (the command still exits 0 — the *report* carries the
verdict):

```json
{
  "valid": false,
  "issues": [
    "REQ-005: acceptance criterion 1 must be phrased Given/When/Then ('the calculator should handle newlines quickly')"
  ],
  "nextStep": "Fix the issues in requirements/requirements.json, then validate again."
}
```

---

## bdd spec refine

Review one requirement's wording for quality: vague words, missing
actor or benefit in the story, compound criteria, non-concrete
outcomes, happy-path-only criteria.

```text
Usage: bdd spec refine [OPTIONS] <REQ_ID>
```

```bash
bdd spec refine REQ-004
```

```json
{
  "id": "REQ-004",
  "clean": false,
  "findings": [
    "acceptance criterion 1 is ambiguous: 'quickly' is not testable",
    "the story is missing the why (no 'so that ...')"
  ],
  "nextStep": "Reword the flagged parts, then refine again until there are no findings."
}
```

Refine until `"clean": true`, then have the developer approve the
wording — that approval is the first human gate of the workflow.

---

## bdd spec reword

Reword an existing backlog item (staged). Interactive wizard is the
same refine → reword loop as draft. Flags skip the wizard:

```bash
bdd spec reword REQ-007
bdd spec reword REQ-007 \
  --title "Newlines as delimiters" \
  --story "As a calculator user, I want newlines to separate numbers in addition to commas so that multi-line input just works." \
  --criterion 'Given the input "1\n2,3", when add is called, then the result is 6' \
  --criterion 'Given an empty string "", when add is called, then the result is 0'
```

Over MCP this is `requirement_reword` (arguments `id`, `title`, `story`,
`acceptance_criteria`). It is how an agent repairs whatever `validate_spec`
or `refine_requirement` reported: the spec file's JSON escaping and
indentation differ from what the read tools return, so a hand-written
string replacement against `requirements.json` will not match.

---

## bdd spec set-feature

Point a requirement at the feature file that was actually written
(staged). Used when a copied spec still names `kata/…` paths that do
not exist on an extracted project. `bdd scenario add` already updates
`featureFile` when it is missing or stale.

```bash
bdd spec set-feature REQ-003 --file src/test/resources/features/string_calculator.feature
```

---

## bdd spec mark-implemented

Flip a requirement's status to `implemented` and record its
`featureFile` — the feature carrying the `@REQ-...` tag — in the same
staged edit, so the spec passes validation. The change is staged, not
applied directly.

```text
Usage: bdd spec mark-implemented [OPTIONS] <REQ_ID>
```

```bash
bdd spec mark-implemented REQ-003
bdd validate
bdd changes commit
```

```json
{
  "id": "REQ-003",
  "status": "implemented",
  "staged": true,
  "nextStep": "Review with changes show, run bdd validate (it checks the @REQ-003 scenario exists), then bdd changes commit."
}
```

The command is gated twice:

- **GREEN only** — it refuses unless the last recorded run passed.
  Do this only when the scenario and tests for the requirement are
  GREEN; it is the last move of the per-requirement rhythm.
- **Tagged scenario only** — it refuses when no committed feature
  file carries a scenario tagged `@<REQ_ID>`; add one with
  `bdd scenario add` and apply it with `bdd changes commit` first.

Re-running it on an already-implemented requirement is safe: on GREEN
it backfills a missing `featureFile`, which is exactly the repair for
a spec that fails validation with
`implemented requirements must name their featureFile`.

Like every mutation, the flip is written back to the spec file the
requirement actually lives in — a requirement declared in an included
file stages that file, never the root.

---

## bdd spec include

Manage the catalog's included spec files
([format reference](../spec-format.md#the-catalog-splitting-the-spec-across-files)).

```text
Usage: bdd spec include add [OPTIONS] <PATH>

Options:
  --from <FILE>  The catalog document declaring the include (default: the root requirements.json)
```

`add` stages two things: the include entry on the parent document,
and — when the included file does not exist anywhere yet — an empty
spec skeleton (`{ "requirements": [] }`) for it.

```bash
bdd spec include add requirements/core/arithmetic.json
```

```json
{
  "file": "requirements/core/arithmetic.json",
  "parent": "requirements/requirements.json",
  "created": true,
  "staged": true,
  "nextStep": "Review with bdd changes show, apply with bdd changes commit, then draft into it with bdd spec draft --file."
}
```

Nest deeper by including from a child instead of the root; the entry
is written relative to that parent's directory:

```bash
bdd spec include add requirements/core/edge-cases.json \
  --from requirements/core/arithmetic.json
```

Including a file that is already part of the catalog is a safe no-op.
Includes must be `.json` files inside the spec directory.

## See also

- [The requirements format](../spec-format.md) — every field of the spec, and the include catalog.
- [The workflow](../workflow.md) — where each subcommand fits.
- [`bdd changes`](changes.md) — reviewing and applying staged spec mutations.

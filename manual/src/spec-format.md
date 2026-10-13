# The requirements format

`requirements/requirements.json` is the spec — the versioned source of
truth every agent implements from. This chapter is the reference for
its shape: what each field means, who consumes it, and how one file
grows into a **catalog** of many.

One thing to keep straight: **Cucumber never reads this file.**
Cucumber runs the `.feature` files. The spec sits *upstream*: the
tooling (`spec`, the MCP server) reads it, agents turn its acceptance
criteria into Gherkin scenarios and unit tests, and those are what the
test runner executes.

```text
requirements.json  →  spec / MCP tools  →  .feature + tests  →  Cucumber/JUnit
```

## The document shape

```json
{
  "project": "String Calculator Kata",
  "description": "What this spec is for, in a sentence or two.",
  "includes": ["core/arithmetic.json"],
  "requirements": [
    {
      "id": "REQ-003",
      "title": "Two numbers separated by a comma are summed",
      "status": "pending",
      "story": "As a user, I want comma-separated numbers to be summed so that I can add multiple values at once.",
      "acceptanceCriteria": [
        "Given \"1,2\", when add is called, then the result is 3",
        "Given \"10,20\", when add is called, then the result is 30"
      ],
      "featureFile": "kata/src/test/resources/features/string_calculator.feature"
    }
  ]
}
```

## Top-level fields

| Field | Required | What it does |
|---|---|---|
| `project` | root only | The project name. `spec validate` fails when it is blank on the root document; included files never need one. Returned by `list_requirements`, and greenfield mode derives production file names from it. |
| `description` | no | Documentation for humans and agents. The tooling never acts on it. |
| `includes` | no | Child spec files merged into this one — see [the catalog](#the-catalog-splitting-the-spec-across-files). |
| `requirements` | see note | The backlog this file contributes. A file may hold an empty array when it only exists to include others, but the *merged* catalog must contain at least one requirement. |

## Requirement fields

| Field | Required | What it does |
|---|---|---|
| `id` | yes | The lookup key, shaped like `REQ-007` (uppercase prefix, dash, number). Must be unique across the **whole catalog** — every included file counts. It is also the tag the workflow expects on the Gherkin scenario (`@REQ-007`); that tag is how a scenario traces back to its requirement. |
| `title` | yes | One line naming the behavior. Shown by `spec list`. |
| `status` | yes | `pending` or `implemented` — nothing else. `spec mark-implemented` flips it on GREEN; the value gates the `featureFile` rules below. |
| `story` | yes | The user story, `As a …, I want … so that …`. `spec refine` reviews it for a missing actor, missing benefit, and ambiguous words. |
| `acceptanceCriteria` | at least one | Each criterion must be phrased Given/When/Then. This is the load-bearing field: agents translate these lines into the Gherkin scenarios and unit tests you then implement. |
| `featureFile` | when implemented | Repo-root-relative path to the feature file carrying the scenario. Optional while `pending`; once `implemented`, validation requires the file to exist *and* to contain a scenario tagged `@<id>`. `spec mark-implemented` records it automatically. |
| `productionFiles` | no | Where this requirement's production code lives, as an array — one requirement's code is not always one file. Declared with [`spec set-production`](commands/spec.md#spec-set-production) and added to by a delivery recording what it wrote. When absent, `spec implement` infers a conventional path from `project` instead. |
| `type` | no | `actionable_requirement` (the default when absent) or `ask_requirement` — see below. A requirement written before the field existed is read, and written back, without it. |

## Asks and the requirements derived from them

A request in the user's own words is kept in the same `requirements`
array as an **ask**: a requirement like any other — id, title, story,
Given/When/Then criteria, status — with `type: "ask_requirement"`, an
`ASK-nnn` id, and the words themselves in `request`. The requirements
derived from it are ordinary `actionable_requirement` entries that name
it as `parent`; the ask lists them in `derived`. Both directions are
checked by `spec validate`. Every field below is omitted when it does
not apply, so a spec without asks is byte for byte what it was.

| Field | On | What it does |
|---|---|---|
| `request` | ask | The request verbatim. Rewording it in meaning (`spec reword ASK-001 --request …`) reopens the decomposition; the derived requirements stand. |
| `derived` | ask | Ids of the requirements derived from it, in the order they were accepted. |
| `decomposition` | ask | `mode` (`automatic` or `interactive`), `state` (`open` or `complete`), `confirmed` (the user said the accepted requirements cover the request — interactive only), `fingerprint` of the request the proposals were made from, and `ready` — derived, see below. |
| `proposals` | ask | Every proposed requirement with a `number`, its wording, a `fingerprint` of that wording, `status` (`pending`, `accepted`, `rejected`, `excluded`), the `requirement` it became once accepted, the `reason` given with a rejection or exclusion, and the review `findings`. |
| `questions` | ask | Clarifications the breakdown raised, each with a `number`, `text`, whether it is `blocking`, and the `answer` once given. Separate from the proposals: a question is never a requirement. |
| `parent` | derived | The ask this requirement derives from. |
| `acceptance` | derived | `by` — `automatic` (the deterministic review passed it) or `user` (a person approved these exact words) — and `wording`, the fingerprint of the title, story, and criteria that were accepted. A requirement whose wording no longer matches is **not accepted**: `spec reword` clears the field when the meaning changes and keeps it when only whitespace does. |

An ask's own title, story, and criteria are derived from its children
(one criterion per accepted requirement plus the refusal of anything
still pending) so the ask passes the same validation and wording review
as every other row; they cannot be set directly.

**Readiness** (`decomposition.ready`, recomputed on every write and
reported by `spec show`, `spec draft`, `spec decide`, and
`get_requirement`) is true only when the decomposition is `complete`,
`derived` is non-empty, every derived requirement exists, names the ask
as `parent`, and carries an `acceptance` whose `wording` matches its
current wording (and whose `by` is `user` in interactive mode), no
blocking question is unanswered, no proposal is `pending` or `rejected`
without having been revised or excluded, and — in interactive mode —
`confirmed` is true. `spec deliver` works an ask through its derived
requirements and refuses one that is not ready; an ask is implemented
once every derived requirement is, and the `spec deliver` run that lands
the last of them marks it so (`spec mark-implemented ASK-nnn` does the
same by hand, and refuses while any derived requirement is pending).

```json
{
  "id": "ASK-001",
  "type": "ask_requirement",
  "title": "Sum numbers from a comma separated string, empty input means zero",
  "status": "pending",
  "story": "As a requester, I want ...",
  "acceptanceCriteria": ["Given REQ-002 ... is accepted, ..."],
  "request": "sum numbers from a comma separated string, empty input means zero",
  "derived": ["REQ-002"],
  "decomposition": { "mode": "automatic", "state": "complete", "confirmed": false, "fingerprint": "…", "ready": false },
  "proposals": [
    { "number": 1, "title": "Comma separated numbers are summed", "story": "…", "acceptanceCriteria": ["…"], "fingerprint": "…", "status": "accepted", "requirement": "REQ-002" },
    { "number": 2, "title": "Empty string returns zero", "story": "…", "acceptanceCriteria": ["…"], "fingerprint": "…", "status": "pending", "findings": ["…"] }
  ],
  "questions": []
}
```

```json
{
  "id": "REQ-002",
  "type": "actionable_requirement",
  "title": "Comma separated numbers are summed",
  "status": "pending",
  "story": "…",
  "acceptanceCriteria": ["…"],
  "parent": "ASK-001",
  "acceptance": { "by": "automatic", "wording": "…" }
}
```

How these are written and decided is in
[`spec draft`, `spec decide`, `spec link`, and `spec delete`](commands/spec.md#spec-draft-request--an-ask-broken-into-requirements).

## The catalog: splitting the spec across files

`requirements/requirements.json` is **always the entry point** — but it
does not have to hold every requirement itself. It is a catalog: it may
carry its own `requirements` *and* an `includes` array of child spec
files. Children use the same document shape and may include further
files, so the tree nests as many levels deep as the backlog needs.

```text
requirements/
├── requirements.json        ← the root catalog (always read first)
├── core/
│   ├── arithmetic.json      ← included by the root
│   └── edge-cases.json      ← included by arithmetic.json
└── validation/
    └── negatives.json       ← included by the root
```

The rules:

- **Paths are relative to the file declaring them.** The root's
  `"core/arithmetic.json"` lives next to the root;
  `arithmetic.json`'s `"edge-cases.json"` lives next to
  `arithmetic.json`. Includes must stay inside the spec directory.
- **Merge order is depth-first**: a file's own requirements first,
  then each include in listed order. `spec list`,
  `list_requirements`, `validate_spec`, and every other tool operate
  on this merged view — one backlog, whatever the file layout.
- **Ids are unique across the tree.** A duplicate id spanning two
  files fails validation naming the file that declared it first.
- **Cycles and missing files are validation issues**, not crashes:
  including a file twice, including a file that does not exist, or
  escaping the spec directory each report a single actionable issue.
- **Mutations write back to the declaring file.** `spec reword`,
  `set-feature`, `set-production`, and `mark-implemented` find the file
  a requirement lives in and stage only that file. `spec draft` appends
  to the root by default, or to a chosen file with `--file`.

### Growing the catalog

```bash
# Write a new (empty) spec file and the include entry on the root:
spec include add requirements/core/arithmetic.json

# Draft directly into it:
spec draft --file requirements/core/arithmetic.json

# Nest deeper: include a file from a child instead of the root:
spec include add requirements/core/edge-cases.json \
  --from requirements/core/arithmetic.json
```

`spec list` names the file each requirement lives in, so the
catalog stays navigable as it grows:

```json
[
  { "id": "REQ-001", "title": "Empty string returns zero", "status": "implemented",
    "file": "requirements/requirements.json" },
  { "id": "REQ-004", "title": "Any amount of numbers is supported", "status": "pending",
    "file": "requirements/core/arithmetic.json" }
]
```

A spec with no `includes` behaves exactly as before — one file, one
backlog. Split it the day it stops fitting in your head.

## See also

- [`spec`](commands/spec.md) — the commands that read, gate, and mutate the spec.
- [The workflow](workflow.md) — where the spec drives the loop.

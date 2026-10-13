# Shaping: plain words to approved requirements

Use this workflow when the target is a plain-language description rather
than an existing requirement. The MCP server preserves and validates the
result; you perform the decomposition and wording.

## The server cannot split prose, and that is not a bug

`requirement_ask` runs with no model and a null prompter. Given prose it
**preserves the words as an ask and derives nothing**, coming back with a
blocking open question that says so:

```json
{
  "ask": "ASK-001",
  "derived": [],
  "pending": [],
  "openQuestions": [{ "number": 1, "text": "No model is resolved and the request is not an enumerated list, so nothing was split. ...", "blocking": true }],
  "ready": false,
  "reasons": ["no derived requirement has been accepted", "question 1 is blocking and unanswered: ..."]
}
```

That is the correct answer, and the question names the way through:
word the requirements yourself under the ask. Do not read `ready: false`
as a failure and do not retry the call.

If the request **was** an enumerated list of two or more items, the
server splits it deterministically into `pending` proposal numbers with
no story and no criteria. Those are titles, not requirements, and they
will not accept themselves.

## The sequence

### 1. Preserve the words

```json
requirement_ask { "request": "<the developer's words, verbatim>" }
```

Pass the words as they were given. This is the record of what was
actually asked for, and every requirement below hangs off it. Note the
`ask` id that comes back.

Use `file` only when the requirement belongs in a specific included spec
document. Omitted, it lands in the catalog covering the working
directory.

### 2. Word each requirement

Read the ask's request and break it into requirements of **one
capability each**. Count the behaviours the sentence names — that is your
number, not a target you pick. Five named behaviours is five
requirements; one clause is one.

For each, call:

```json
requirement_create {
  "title": "<one capability, no conjunction>",
  "story": "As a <role>, I want <capability>, so that <benefit>",
  "acceptance_criteria": [
    "Given <state>, when <action>, then <exact outcome>",
    "Given <edge case>, when <action>, then <exact outcome>"
  ],
  "parent": "<the ask id>",
  "by": "user"
}
```

`by` takes only `"user"` or `"automatic"`. It matters:

- `"user"` accepts the child unconditionally. The developer asked for
  this work, so their authority is the honest one to record.
- `"automatic"` accepts only if the deterministic review has nothing
  blocking. When it does block, the child is written and linked but
  **not accepted**, which leaves the ask not ready and the proposal
  pending.
- Omitting `by` while passing `parent` means `"automatic"`. There is no
  "no decision" option here.

Do not pass `request` and `parent` together — an ask has no parent, and
the tool refuses the pair.

If `requirement_ask` left pending proposals, give the matching child the
**same title** as its proposal and it stands for that proposal. A pending
proposal nothing stands for keeps the ask from being ready; clear it with
`requirement_decide { "id": "ASK-001", "exclude": [{ "number": 2, "reason": "..." }] }`.

### 3. Get the wording clean

For each new id, loop until there is nothing left:

```json
validate_spec {}
refine_requirement { "id": "REQ-001" }
```

`refine_requirement` returns `clean` and `findings`. While `findings` is
non-empty, fix the wording with `requirement_reword` and ask again:

```json
requirement_reword {
  "id": "REQ-001",
  "title": "...",
  "acceptance_criteria": ["...", "..."]
}
```

A non-empty `acceptance_criteria` **replaces the whole list**. An empty
or omitted one keeps what is there. `status` and `featureFile` are kept
either way.

The reply may also carry `judgments`, `judgmentAdvisories`,
`judgmentAction`, or `judgmentNote` when a decision model is configured.
Read them as advice about wording and nothing more:

- `findings` and `clean` are the deterministic review. Fix those first;
  they are not opinions.
- A `FAILS` verdict is a reason to re-read the criterion, not a verdict
  on it. Under `enforce` it is folded into `findings` and you must act;
  under `advisory` it changes nothing.
- `INCONCLUSIVE` landed in the dead band. It is not a yes and not a no.
  Say so and move on.
- `judgmentNote` means no judgment was taken at all. That is not
  approval of anything.

Never claim the wording was judged when it was not.

### 4. Close the decomposition

Clear any blocking question and mark the breakdown finished:

```json
requirement_decide {
  "id": "ASK-001",
  "answers": [{ "number": 1, "answer": "<how the requirements were worded>" }],
  "complete": true
}
```

Then `get_requirement { "id": "ASK-001" }` and confirm `ready` is true.
If it is not, the `reasons` say exactly what is outstanding — a proposal
still pending, a child not accepted, a question unanswered. Fix that
rather than proceeding.

`confirmed: true` requires `by: "user"` and records that the developer
agrees the accepted requirements cover the ask. Set it only after they
actually say so.

### 5. Stop for approval

This is the second checkpoint, after the branch choice. Show the
developer each requirement — title, story, criteria — and the ask it
came from, and get their agreement before any scenario, test, or
production file is written.

The developer stays in control of every engineering decision. The
wording is the decision no gate checks: a requirement can be valid,
clean, judged sound, delivered green, and still be the wrong thing to
build.

After approval, go to [tdd-loop.md](tdd-loop.md) and do not stop again.

## Writing criteria the review will pass

The deterministic rules reject predictable things. Write for them the
first time rather than looping:

- **Every requirement needs at least one edge case.** Criteria covering
  only the happy path are rejected outright. An empty input, an absent
  id, a zero count, an error.
- **The clause after `then` names an exact value, status, or error.** A
  number, a quoted literal, a named error. Not "correctly", not
  "appropriately".
- **No hedge words** anywhere in the criterion: `should`, `handle`,
  `quickly`, `properly`, `appropriately`, `as needed`.
- **One action per criterion.** Two `when` clauses is two criteria.
- **The story needs an actor and a benefit.** "As a developer, I want X"
  with no `so that` is a finding.

One more, worth knowing because it is measured rather than guessed: a
configured decision model reads `then the X is "Y"` as *describing* a
state rather than asserting one, and scores it low. `then the reply
names the verdict "uncovered"` and `then there is 1 uncovered criterion`
score well with the same meaning. If a criterion you believe in gets
flagged, that shape is usually why — but only reword it if the wording
genuinely got sharper. Rewording a sound criterion to move a number is
fitting the work to the measurement.

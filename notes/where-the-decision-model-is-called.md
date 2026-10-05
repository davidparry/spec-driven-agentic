# Where the decision model is called, and by what

**Status.** Implemented and verified. 1185 unit and integration tests,
278 Cucumber scenarios, 97.11% line coverage, `clippy -D warnings`
clean. Verified live against Ollama 0.35.1 with `nimble:latest`
(9.0B, `capabilities: ["decision"]`). The labeled evaluation in
`harness/tests/decision_live.rs` is opt-in and documented in
[Does the question work?](#does-the-question-work).

This is the design record for the decision plane: what calls it, what
does not, and why each of those is a deliberate answer rather than a
place the work stopped.

## What a decision model is here

A second, separate local model that writes nothing. It takes a curated
brief and a bounded question set, and returns a typed answer with a
probability. Ollama 0.35 exposes it at `POST /v1/systemone`, which is a
different route, a different request shape and a different reply shape
from `/api/chat`.

The harness asks exactly one question, `measurable/v1`: *does the clause
after `then` name something a test could assert without a reader first
deciding what the words mean?* The question type is `noul` — a boolean
with a probability and, notably, **no `confidence` field** (only `choice`
and `score` carry one), which is why the policy puts a symmetric dead
band on the probability itself.

It is not a reviewer, a second opinion on code, or a gate. See
[The authority boundary](#the-authority-boundary).

## The call path

There is exactly one line in the codebase that performs a decision round
trip. Everything else is layers deciding whether to reach it and what to
make of the answer.

```
CLI / MCP entry point
  └─ wiring::decision_service(root, flag) -> Option<DecisionService<OllamaDecision>>
       └─ DecisionService::judge_refinement  (workflow)   ─┐
          DecisionService::review_criteria   (per request) ─┤
            └─ DecisionService::judge_criterion            ─┘
                 └─ self.client.decide(&self.model, &request)   ← the only call
                      └─ OllamaDecision::decide
                           └─ POST {endpoint}/v1/systemone
```

The one call site:

```86:97:harness/src/application/decision_service.rs
    pub fn judge_criterion(&self, input: &str, criterion: &str) -> Result<Judgment, DecisionError> {
        let request = Request::single(
            MEASURABLE_ANSWER,
            measurable_question(),
            measurable_state(criterion),
        );
        let provenance = crate::domain::decision::Provenance {
            input: input.to_string(),
            state: request.state_digest(),
            state_bytes: request.state.to_string().len(),
        };
        let outcome = self.client.decide(&self.model, &request)?;
```

`Request::single` is built from `measurable_question()` and
`measurable_state(criterion)`, both pure domain functions. The brief
contains the criterion text and nothing else — no file paths, no spec,
no repository contents — and `state_digest()` hashes exactly what was
sent so the record can be audited later.

### Why `None` is the shipped state

`wiring::decision_service` returns `Option`, and the absence of a
configured model is not an error:

```71:82:harness/src/wiring.rs
pub fn decision_service(
    root: &Path,
    flag: Option<&str>,
) -> Option<DecisionService<OllamaDecision>> {
    let settings = resolved_decision(root, flag);
    let model = settings.model?;
    Some(DecisionService::new(
        model,
        OllamaDecision::with_timeout(settings.endpoint, settings.timeout),
        settings.policy,
    ))
}
```

Every caller holds a `let Some(...) else { return }`. With no
`[decision] model` set, no request is built, no HTTP client is
constructed, and every command behaves byte-for-byte as it did before
the decision plane existed. That is what makes "nothing is on until you
turn it on" a property of the wiring rather than a promise in the docs.

Note what this function deliberately does *not* consult: `mode`. A human
typing `spec judge` gets an answer even when `mode = "off"`, because
`off` suppresses the *automatic* judgments inside the workflow, not the
command whose entire purpose is to ask.

## Which tools use it now

### MCP: one of twenty-five

| Tool | Calls the decision model |
| --- | --- |
| `refine_requirement` | **Yes**, when a decision model is configured |
| the other 24 | No |

`refine_requirement` is the only one, and the tool count stays at 25 —
no 26th tool was added, which keeps the Java smoke test's six
`hasSize(25)` assertions and its `names exactly 25 tools` scenario
intact. Extending a tool's reply was the cheaper option in every sense:
a new tool would have been a breaking change to a contract two other
projects assert on.

**Why `refine_requirement`.** It is the only tool whose job is already
*judging wording*, and the only one with a demonstrable hole a decision
model can fill. Its rules are regexes. One of them asks whether the
clause after `then` looks concrete — a number, a quoted literal, a named
error — and any number anywhere satisfies it. So this criterion earns
**no finding at all**:

> Given the refactored module, when the suite runs, then code quality is
> improved by at least 20%

Nobody measured code quality. The rule asks whether a number is
*present*; it cannot ask whether the number *is* the assertion. The
decision model reads it at 0.038. That gap is the entire justification
for the feature, and it is a gap in exactly one tool.

**Why not the other twenty-four.** Not an oversight in each case, but
the same answer three times over:

- **They have ground truth already.** `run_tests` reports a bar,
  `validate_spec` checks structure, `get_tdd_state` reads a state
  machine, `changes_validate` parses a manifest. A probability added to
  any of these would be strictly worse than the fact they already hold.
  `run_tests` is the sharpest case: a model that can influence a bar
  colour is the single thing this repository exists to argue against.
- **They are gates, and a gate that is sometimes wrong is not a gate.**
  `start_refactor`, `requirement_mark_implemented`, `changes_commit`,
  `command_run`. These are deterministic refusals a human relies on. See
  [The authority boundary](#the-authority-boundary).
- **They are plumbing.** `project_root`, `feature_list`, `feature_read`,
  `changes_show`, `step_definitions_find`, `list_requirements`,
  `get_requirement` — reads with nothing to judge.

`requirement_reword` is the one near miss worth naming. It *generates*
wording, so a judgment could in principle score its output. It does not,
because the reword already lands in staging as a reviewable diff and the
next `refine_requirement` judges the result anyway. Scoring it twice
would add a model call and no new information.

### CLI: two commands

| Command | Calls it | Judgment is |
| --- | --- | --- |
| `spec judge criterion <REQ>` / `--text <wording>` | Yes, always | the entire output |
| `spec refine <REQ>` | Yes, when configured | attached beside the findings |
| `spec judge models` / `current` / `use` | No — `/api/tags`, `/api/show`, config | n/a |
| every other command | No | n/a |

**Why `spec judge criterion`.** The brief asked for "a small command
that runs a real, inspectable decision against supplied or repository
evidence so users can verify setup and understand the typed result."
This is it, and it is the only place a judgment is the product rather
than an aside. It prints the model, question version, verdict, action,
input provenance, the SHA-256 of the brief, and token counts; `--json`
emits the same record as a stable object.

It is also the only entry point that ignores `mode`, for the reason
above: a human typed it.

**Why `spec refine`.** It is the CLI twin of the MCP tool, same hole,
same question. It is also the only place `enforce` can act, because the
exit code is the gate and a human is watching it:

```1662:1684:harness/src/main.rs
fn judge_refinement(
    root: &Path,
    flag: Option<&str>,
    report: &mut spec_harness::application::spec_service::RefinementReport,
) -> anyhow::Result<bool> {
    let Some(service) = wiring::decision_service(root, flag) else {
        return Ok(false);
    };
    let criteria = match spec_service(root).get_requirement(&report.id) {
        Ok(requirement) => requirement.acceptance_criteria,
        // The caller already refined this id successfully, so a failure
        // here is not worth turning into the command's error.
        Err(_) => return Ok(false),
    };
    match service.judge_refinement(report, &criteria) {
        Ok(action) => Ok(action != Transition::Continue),
        // Enforcing mode with no answer. Never silently an approval:
        // the refusal is the command's error.
        Err(error) => Err(anyhow::anyhow!(
            "decision gate refused to pass without an answer - {error}"
        )),
    }
}
```

The `bool` it returns is "the harness is gating on this", which the
caller turns into a nonzero exit *after* printing the reply. The reply
always reaches stdout; the exit code is a separate channel.

### The CLI gates and MCP does not

`refine_requirement` over MCP is forced to advisory regardless of
configuration:

```329:330:harness/src/mcp.rs
            let service = wiring::decision_service(&root, None)?.into_advisory();
            let review = service.review_criteria(&req_id, &criteria);
```

An exit code is something a human watches. A tool reply is something an
agent reads, and an agent reading `"error"` will route around it — so
gating there converts a careful refusal into a retry loop. A project
configured to enforce still enforces on the CLI, where it means
something. `mcp_conformance.rs` pins this: it configures
`mode = "enforce"` and asserts the reply still reports `CONTINUE` with
`mode: advisory`.

`into_advisory` lowers `Enforce` to `Advisory` and leaves `Off` alone,
so `off` still means off over MCP.

## Implementation, layer by layer

Hexagonal, matching the rest of the harness: domain holds no IO, the
application layer takes its collaborators by constructor, and the
adapter is the only thing that knows HTTP exists.

### `domain/decision.rs` — types and policy, no IO

`Question` (`Noul` / `Choice` / `Score`), `Answer`, `Outcome`,
`Judgment`, `Verdict`, `Transition`, `Mode`, `Policy`, `Provenance`, and
`Request` with a `fault()` method that returns the first thing wrong
with a brief. Also `measurable_question()` and `measurable_state()`, so
the exact wording asked is a reviewable constant rather than a string
built at a call site.

The policy is four lines of meaning and no cleverness. For the boolean
question, at or above the threshold reads `Holds`, at or below
`1.0 - threshold` reads `Fails`, anything between is `Inconclusive`.
Then:

| Mode | Verdict | Transition |
| --- | --- | --- |
| `Off`, `Advisory` | any | `Continue` |
| `Enforce` | `Holds` | `Continue` |
| `Enforce` | `Fails` | `Rework` |
| `Enforce` | `Inconclusive` | `Escalate` |

`Verdict` and `Transition` serialize uppercase (`HOLDS`, `REWORK`),
matching the vocabulary the article uses for workflow states. `Mode`
stays lowercase because it round-trips with the `mode = "advisory"` line
in `.spec/config.toml`.

`DEFAULT_MIN_CONFIDENCE` is `0.80`, and the doc comment says why: the
threshold sweep, not a vendor number. See
[Does the question work?](#does-the-question-work).

### `ports.rs` — the port and its failures

```186:192:harness/src/ports.rs
pub trait DecisionModel {
    fn decide(
        &self,
        model: &str,
        request: &crate::domain::decision::Request,
    ) -> Result<crate::domain::decision::Outcome, DecisionError>;
}
```

A different trait from `LlmConversation` on purpose. The two roles are
not interchangeable at either end, and keeping them apart in the type
system is what makes "a generative command cannot pick up a decision
model" a compile-time fact rather than a runtime check.

`DecisionError` has ten variants rather than being a string, because the
policy branches on failure and every variant has to name something the
reader can do: `Invalid`, `Unavailable`, `Unsupported { endpoint }`,
`ModelMissing { model }`, `NotADecisionModel { model, detail }`,
`TooLarge`, `Timeout`, `Malformed`,
`UnexpectedAnswers { missing, unexpected }`, `ScoringFailed`. A test
walks every one and asserts the message contains an action.

### `adapters/ollama_decision.rs` — the HTTP boundary

Serializes the body, checks local faults before paying for a round trip,
POSTs to `{endpoint}/v1/systemone`, and maps status codes and provider
sentences onto the typed errors. It refuses a reply whose answer keys do
not match the question keys, so a malformed response never reaches the
policy as a partial answer. It is `Clone`, which shares the connection
pool rather than copying it — that is what keeps `keep_alive` meaningful
across successive judgments. Twenty-three tests drive it through a
one-shot `TcpListener`, so the request construction and parsing are
tested over a real socket.

### `application/decision_service.rs` — the policy applied

`judge_criterion` is the round trip plus the record. `review_criteria`
is every criterion of one requirement, keeping the strongest transition
and building advisories for the ones that failed. `judge_refinement`
is the workflow entry point and the one place `mode = "off"` is checked,
"so no future caller can forget it".

`apply_review` is a free function taking a `Policy`, not a method:

```202:223:harness/src/application/decision_service.rs
pub fn apply_review(
    policy: Policy,
    report: &mut RefinementReport,
    review: Result<CriteriaReview, DecisionError>,
) -> Result<Transition, DecisionError> {
    let before = report.findings.clone();
    match review {
        Ok(review) => {
            report.judgment_advisories = review.advisories;
            report.judgments = review.judgments;
            report.judgment_action = Some(review.action);
            debug_assert_eq!(before, report.findings, "a judgment never edits a finding");
            Ok(review.action)
        }
        Err(error) if policy.mode == Mode::Enforce => Err(error),
        Err(error) => {
            tracing::warn!(error = %error, "no judgment: carrying on with the deterministic verdict");
            report.judgment_note = Some(format!("no judgment - {error}"));
            Ok(Transition::Continue)
        }
    }
}
```

A free function because the MCP path does its asking on another thread
and comes back holding a review and a policy but no client. Making this
a method would have forced a placeholder client into existence purely to
satisfy a type.

The `debug_assert_eq!` is the invariant in executable form: a judgment
may add keys, never edit one.

### The async constraint, and the bug it caused

`reqwest::blocking` builds and drives its own runtime. Constructing or
calling it on a tokio runtime thread stalls the host and risks a
nested-runtime panic. The first version of the MCP handler did exactly
that and deadlocked the server — found by the conformance suite hanging,
not by review.

The fix moves everything except a config read onto the blocking pool,
and builds the service inside the closure so the client is never even
constructed on a runtime thread:

```319:333:harness/src/mcp.rs
        // Everything else goes to the blocking pool. The decision client
        // is `reqwest::blocking`, which builds and drives its own
        // runtime: constructing or calling it on a runtime thread both
        // stalls the host and risks a nested-runtime panic. The service
        // is therefore built where it is used.
        let root = self.root.clone();
        let req_id = report.id.clone();
        let criteria = requirement.acceptance_criteria;
        let asked = tokio::task::spawn_blocking(move || {
            let _span = tool_call("refine_requirement");
            let service = wiring::decision_service(&root, None)?.into_advisory();
            let review = service.review_criteria(&req_id, &criteria);
            Some((service.policy(), review))
        })
        .await;
```

Two consequences worth knowing before touching this code. A
`tracing::EnteredSpan` is not `Send`, so the handler's span has to be
scoped and a fresh one opened inside the closure — which is why
`refine_requirement`'s handler has three span blocks instead of one. And
a panicked or cancelled blocking task is mapped to
`DecisionError::Unavailable`, because a task that did not finish is not
an answer.

`attach_judgment` is deliberately infallible. Whatever the decision
model does, the host gets the deterministic findings.

## Keeping the two roles apart

Three mechanisms, none of which hardcodes a model name. Outside test
fixtures, `nimble` appears in the harness exactly once — as
`RECOMMENDED_DECISION_MODEL`, used only in a hint and a scaffolded
comment. Nothing treats it as installed or infers a capability from it.

**1. Discovery excludes decision-only models.** Asked in the negative on
purpose:

```55:72:harness/src/application/model_service.rs
/// Whether a model may be handed to generative work.
///
/// The question is asked in the negative on purpose: only a model the
/// provider *positively* reports as decision-capable and not
/// completion-capable is withheld. A provider that cannot answer, or a
/// model it has no capability list for, keeps the behaviour it has
/// always had — a capability probe that fails is not evidence to start
/// excluding models on.
fn generative_candidate<C: ModelCatalog + ?Sized>(catalog: &C, model: &str) -> bool {
    match catalog.capabilities(model) {
        None => true,
        Some(capabilities) => {
            let decides = capabilities.iter().any(|c| c == DECISION_CAPABILITY);
            let completes = capabilities.iter().any(|c| c == COMPLETION_CAPABILITY);
            !decides || completes
        }
    }
}
```

Only the discovery path pays for the `/api/show` probes — a flag or a
configured name returns before them, so the common case costs what it
always did. When the filter empties the list, `SessionModel` reports
`NoGenerativeModel { installed }`, which names what *is* there rather
than claiming nothing is installed.

This closes a real hole, not a theoretical one: `nimble` on `/api/chat`
returns junk rather than refusing, so without the filter a machine with
a decision model listed first would silently use it for code.

**2. `spec model use` refuses one by name.**

```
$ spec model use nimble:latest
Error: 'nimble:latest' is a decision model and cannot do generative work
 - set it as the decision model instead: spec judge use nimble:latest
```

**3. Separate config keys.** `spec judge use` writes `[decision] model`
and prints that the generative model is unchanged. `spec config` reports
both with provenance, and `decision.model` reads `(unset)` until someone
opts in.

## The authority boundary

A judgment may be reported. That is the whole list.

It may not turn a red bar green, certify an implementation, bypass
staging or review, waive a human checkpoint, or override deterministic
validation. Concretely, it never writes to `findings`, `clean`, a test
result, a requirement's status, or the staging area.

`enforce` is the strongest setting and it is still bounded in one
direction: a failing or unsure answer can make `spec refine` exit
nonzero asking for rework or a human. **It can stop work; it cannot
approve it.** There is no configuration in which a judgment lets
something through that the deterministic rules refused.

Defined behaviour for the four awkward cases:

| Case | Advisory (default) | Enforce |
| --- | --- | --- |
| Low confidence | `INCONCLUSIVE`, used for nothing | `ESCALATE`, exit 1 |
| Criteria disagree | strongest transition wins | strongest transition wins |
| Model unavailable | `judgmentNote`, deterministic verdict stands, exit 0 | exit 1, `decision gate refused to pass without an answer` |
| No evidence | no criteria, no judgments | no criteria, no judgments |

All four verified end-to-end, not just unit-tested. A failed decision
request is never read as approval in any mode.

## Does the question work?

A vendor benchmark cannot answer that for this repository's question, so
`harness/tests/decision_live.rs` holds 32 labeled criteria in five
groups — clearly measurable, clearly not, genuinely ambiguous, eight
written to *look* finished, and four lifted from this repository's own
spec — each with a note saying why it is labeled that way. It prints
every answer, a confusion matrix, and a threshold sweep.

Against `nimble:latest` at 0.80: **0 misses, 1 false alarm, 3 unsure.**
All eight adversarial criteria caught, including a prompt-injection
attempt. 48 ms median warm.

Two results shaped the shipped code and are recorded here because both
contradicted a design choice already made.

**The question wording.** The first phrasing asked the open question —
"could a test check this criterion?" — and read **every one** of the
eight adversarial criteria as measurable at p > 0.9, including "the
system achieves 99.9% correctness across all code paths" (0.965) and one
whose opening words were "This criterion is measurable" (0.889).
Narrowing it to the clause after `then` and naming the specific dodges
took that from 8 misses to 0. The lesson is about the question, not the
model: an open question invites a judgment of the sentence's *style*,
and style is what convincing-looking wording gets right.

**The threshold, from 0.70 to 0.80.** Judging this repository's own
criteria turned up a false alarm nobody predicted. `then the verdict is
"covered"` asserts an exact quoted string and is plainly measurable, but
the model weighs the quoted word's meaning over its quotes and scores it
0.09. One round of rewording fixed that case and let an adversarial case
through (0.499 → 0.803), so the tuning stopped and the weakness is
documented instead — in the test with a note, in `judge.md`, and in the
student guide where it is the point of the exercise. The sweep then
showed 0.80 cuts confident false alarms from three to one, adds no
misses, and correctly leaves 2 of 6 ambiguous criteria unsure.

The two error directions do not cost the same, which is the reasoning
that outlives this particular model: a false alarm teaches people to
ignore judgments, while silence costs nothing, because the deterministic
rules are doing their job either way. Hence the wider band, and hence
advisory by default.

`KNOWN_FALSE_ALARMS` in the test is a regression guard set to the
measured count, not a tolerance to grow into. If it rises, either the
question changed or the model is worse at it than the one the default was
set against.

## Reading list

- [`manual/src/commands/judge.md`](../manual/src/commands/judge.md) —
  the user-facing reference, including what a judgment may not do
- [`harness/README.md`](../harness/README.md) — the Decision model
  section and the evaluation summary
- `harness/tests/features/decision_model.feature` — the behaviour, 16
  scenarios, written before the implementation
- `harness/tests/decision_live.rs` — the labeled evaluation, opt-in

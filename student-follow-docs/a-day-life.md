# A Day in the Life — a harness is worn, a jig is clamped down

A harness goes on a person. It catches you when you fall, and you still
decide where to walk. Nobody wears a jig. A jig is clamped to the bench,
and it holds the work so the cut comes out the same every time — which
is exactly why the word matters once the thing being held is a language
model.

The test is whether the model can **skip** it. Anything a model can talk
its way past is a preference, not a rail.

This page takes one requirement through the same server twice, under two
very different amounts of trust, and then applies what is left over to a
business decision that has no test suite to hide behind.

## Two demos, one requirement

Both demos deliver the same sentence:

```text
explain one spec command by name - what it does
```

Same MCP server, same gates, same acceptance criteria. What changes is
who decides the sequence runs.

### Running these yourself

Both demos drive the `harness/` crate, and they need **a checkout
each** — they cannot share one. Three things in the way, and all three
are the design working rather than failing:

- `cargo test` holds an exclusive lock on `target/`, so the second run
  waits instead of running.
- The RED/GREEN bar is one piece of state per project root. Two runs
  reading it, testing for a minute, then writing it back, and the later
  write drops the earlier one.
- Both would draw the same next requirement id out of the same catalog.

So give each its own worktree, and run them one after the other:

```bash
git worktree add ../talk-demo1 -b demo1
git worktree add ../talk-demo2 -b demo2
```

One thing does not come with a worktree: `target/` starts empty, so run
`cargo test` once in each before you begin. Warm, the whole suite is 27
seconds; cold, it is a full build of the crate and everything it
depends on, and you do not want to discover that on the first RED bar.

### Demo 1 — Cursor, a skill, and the MCP server

The loose end of the range. A frontier model in an editor, pointed at
`spec mcp serve`, and sequenced by
[`harness/.cursor/skills/spec-driven-deliver/SKILL.md`](../harness/.cursor/skills/spec-driven-deliver/SKILL.md).

Open the editor on `../talk-demo1/harness`, not on the repository root.
Both the MCP config and the skill live under `harness/.cursor/`, and
the server starts with `--root ${workspaceFolder}` — so from the root
you get no skill and a server pointed at the wrong catalog.

Paste this as one prompt:

```text
Use the spec-driven-deliver skill to deliver:
"explain one spec command by name - what it does"
```

The first line makes activation explicit. The skill itself owns the MCP
workflow: it calls `project_inspect`, `project_root`,
`list_requirements` and `get_tdd_state`, then briefly explains what it
found before the branch choice or any write. That explanation comes from
the catalog and inspected assets rather than from README prose.

Then it stops and hands the branch back to you. It proposes a name and
asks *you* to run `git switch -c`, because creating a branch needs a
shell and the agent has not been given one — then it re-reads
`project_inspect` to confirm the branch moved rather than taking your
word for it. Small moment, and it is the argument in miniature.

It then works the loop itself:

```text
requirement_create          the sentence becomes a draft
validate_spec               structure first
refine_requirement          then wording, and reword until clean
scenario_add                the acceptance criteria become Gherkin
step_definition_create      templates only
unit_test_create            templates only
run_tests                   expect RED
  (the model writes the implementation)
run_tests                   expect GREEN
start_refactor
requirement_mark_implemented
```

No `spec` subcommand runs. Every effect the *agent* has reaches the
work through one of the 27 tools, and nothing else. The one command
typed during the run is your own `git switch -c`, which is the point
rather than an exception to it.

One tool on that surface is not purely deterministic, and it is worth
knowing which. `refine_requirement` runs its deterministic rules first
and always; then, if the project configures a decision model, it
attaches a bounded second opinion on whether each criterion is
measurable. The findings are the rules'. The judgment arrives beside
them, never instead of them. The other twenty-six call nothing.

What holds, even here:

- `start_refactor` refuses on a red bar.
- `requirement_mark_implemented` needs a green bar *and* a scenario
  tagged with that requirement id.
- `command_run` is RED-gated, argv-allowlisted, and runs no shell at
  all.

Those are the same `domain/tdd.rs` and the same service layer the CLI
uses. A frontier model gets the identical refusal you would.

### Demo 2 — the same requirement, no skill on the wire

The tight end. One command, from the other worktree, and the model
never sees a skill:

```bash
spec deliver "explain one spec command by name - what it does"
# ← Enter for the generated branch name, and then it goes quiet
```

The sentence is split into requirements and reviewed before any of them
is accepted; then, per requirement, a scenario, step definitions, a
unit test, a RED bar, and `implement` — the slow one. The sequence is
compiled in. Nothing asks the model whether to run it, in what order, or
whether this step can be skipped just this once.

Two details, because being vague here would undercut the point. That
review is the pipeline's own automatic check on the proposals, not the
interactive reword loop `spec draft` gives you. And the scenario is
built from the acceptance criteria by a **template**, not by a model —
which is why the gate that asks whether a scenario exercises its
criterion never fires on this path. There is nothing generated for it
to read.

Three honest things about watching it, measured against this crate:

- `implement` took **20, 36 and 57 minutes** across three attempts. A
  second run is much faster, and that is the **cache** at
  `.spec/cache/` — not the model warming up. It is keyed on the model,
  the endpoint, the whole conversation and the tool schemas, so change
  one word and you pay full price.
- It can **run out of time** rather than finish, and the partial
  generation is gone. This project raises `[llm] timeout_seconds` to
  3600 for exactly this step; the shipped default is 300, which cuts
  `implement` off mid-generation on a crate this size.
- An attempt can return **nothing usable**, having spent its rounds
  looking instead of writing. The bar is untouched and the next attempt
  is briefed with the failure.

If it falls short it reports what landed and exits nonzero. That is the
jig working, and it is the thing a prompt structurally cannot do: tell
you the truth about work nobody watched.

Three different judges run at every generating stage, and keeping them
apart is the whole design:

- **Deterministic rules** — regex and structural checks over what came
  back.
- **Types** — the return parses into the expected shape, or it is not a
  result at all.
- **A decision model**, asked one bounded question at a time, returning
  a probability rather than prose. Below **0.80** it answers unsure, and
  unsure is not a yes.

Four of those judgments exist only in `deliver`, because only `deliver`
generates code: `STEPS_BIND_SCENARIO`, `UNIT_TEST_ASSERTS`,
`IMPLEMENTATION_COMPLETE` and `REFACTOR_PRESERVES_BEHAVIOUR`. In demo 1
they are simply not on the wire.

Which is four gates and not all of them. Demo 1 did reach one judgment,
through `refine_requirement`, on whether a criterion is measurable. The
difference between the two demos is not that one had no judge. It is
that demo 1 produced no generated code for a judge to read.

All four are advisory: recorded and reported, and they do not stop the
run. That is the same advice this page gives below about your own
gates — a question earns the right to block after you have measured it,
not before.

### A Skill is not a Jig

This is the part worth sitting with, because the two demos look similar
from the outside and are not.

A skill is a **briefing**. It loads when the model decides it is
relevant, and it is followed as far as the model chooses to follow it.
Activation and compliance are both the model's call, so a skill fails
the skip test by construction. Nobody inside a skill has the job of
saying no.

The skill is not the jig. The server is. A model can skip an
instruction; it cannot skip a tool that returns an error instead of a
result.

Which is why the interesting half of MCP here is the inverse of how it
usually gets sold. The value is not reach. It is that the set of things
the model can attempt is finite, typed, and known before it starts — a
**clamp**. And the return value matters as much as the permission,
because a model left to narrate what it just did reports the version of
events it finds most plausible, while a tool reports what happened.

The dial between the two demos is the real decision, and no setting
gives you both ends: strictness is bought with build effort, speed is
bought with tokens and with trust. Reproducibility that has to replay
identically next year wants demo 2. Clearing a backlog on a Thursday
afternoon is fine in demo 1. Work with no settled shape wants neither —
a jig repays its cost through repetition, and exploration is what
general agents are for.

## None of this was about Gherkin

Strip the domain out of both demos and five pieces are left. Not one of
them is about software delivery:

- A **source of truth** that gets validated before anything acts on it
- A **sequence** held in code rather than in a prompt
- A **narrow typed surface** that is the only way to touch the work
- **Evidence** collected before anything is called done
- A **refusal** when the evidence is not there

So the rest of this page applies those same five to a decision most
enterprises already make badly.

> **The loyalty case below is a design, not a shipped tool.** There is
> no loyalty implementation in this repository and nothing here is
> runnable. The commands in its examples are illustrative. What is real
> is the pattern, and the measured claims about the decision model,
> which come from the two demos above.

## The decision

An account comes up for loyalty review. The business wants a tier —
say Bronze, Silver, Gold — and the tier drives real money: discount
band, support SLA, renewal terms, who gets a human account manager.

The policy is not complicated to state:

- Tenure of at least 24 months.
- Annual spend above the band threshold.
- No late payments in the last 12 months.
- Support burden within normal range for the band.
- Not under an active credit hold.

And it is not complicated to apply to the easy 70% of accounts. The
difficulty is entirely in the rest, and everyone who has built this
knows the shape of it: the account that was 23 months but came through
an acquisition. The late payment that was a billing error the company
caused. The support burden that is high because of an outage that was
not their fault. The contract with a bespoke clause someone signed in 2023.

## What we used to build

### A rules engine

Encode the policy as rules, put them in an engine, and let it fire.

It works, right up until it does not, and the way it fails is
predictable. Rules multiply, because every exception is a new rule.
Rules interact, so the firing order becomes load-bearing and nobody
documented why it is that order. Changing one threshold means
regression-testing the entire set, because you cannot reason locally
about which rules a change reaches. And the answer it gives you is a
tier with no account of itself — when the customer asks why they went
from Gold to Silver, somebody reads the rules and reconstructs a story.

The deepest problem is that the hard 30% was never really rules. "The
late payment was our billing error" is a judgement about a sentence in
a dispute note. You can encode a flag for it, and then you have moved
the judgement to whoever sets the flag, with no record of why.

### Or a prompt and a hope

The other thing we build now: hand the policy and the account to a
model and ask for a tier.

It will answer. It will answer fluently, every time, and it will
sometimes be right. What you have no way to know is which time. The
arithmetic may be wrong and stated confidently. A criterion may have
been skipped. The model may have decided a dispute was the company's
fault on no evidence at all, because that reading made the narrative
hang together. And none of it is auditable, because the only artefact
is prose.

Apply the skip test: anything a model can talk its way past
is a preference, not a rail. In the prompt version, **every single
thing** is a preference.

## The jig version

Five pieces, each one the same piece the two demos used.

### 1. The policy is the source of truth, and it gets validated first

The policy is not prose in a Confluence page. It is a document with
structure — criteria, thresholds, bands — and it is structurally
validated before any account is scored. Missing threshold, band with no
upper bound, two criteria that cannot both hold: those are caught when
the policy is edited, not when an account hits them.

Then the wording gets reviewed the same way an acceptance criterion
does, and for the same reason. `refine_requirement`'s deterministic
rules reject hedge words and outcomes with no concrete value, which is
exactly the
defect a loyalty policy accumulates:

- `Support burden within normal range for the band` — not measurable.
  Normal according to what?
- `Given an account in the Gold band, when its support burden is
  evaluated over 12 months, then tickets per month is at most 4` —
  measurable. Someone can compute it and get one answer.

That rewrite is not pedantry; it is the difference between a criterion
a validator can hold and a criterion a model has to interpret. Every
criterion you move across that line is one the model no longer gets a
say in.

### 2. The decision is a state machine

A tier is not assigned; it is arrived at, through states that exist in
code:

```text
GATHERING   evidence is being collected, no tier may be proposed
EVIDENCED   every criterion has evidence or an explicit exception
PROPOSED    a tier is on the table, with its evidence attached
DECIDED     a human or a policy rule has accepted the proposal
REFUSED     the decision cannot be made and says what is missing
```

The transitions are the rails. You cannot propose a tier from
`GATHERING`, the way you cannot refactor on a red bar. You cannot reach
`DECIDED` from `GATHERING` at all. A model that would like to skip
straight to a tier finds there is no transition to take — not a
reprimand, an absence.

### 3. A narrow typed surface is the only way to touch the work

The model gets no database, no shell, no query language. It gets a
handful of typed, read-only tools:

```text
account_read(account_id)            tenure, band, contract type
spend_summary(account_id, months)   totals, by period
payment_history(account_id, months) payments, late flags, disputes
support_summary(account_id, months) ticket counts, severities
credit_holds(account_id)            active holds with reasons
```

This is the inverse of how MCP is usually sold. The value is not reach;
it is that the set of things the model can attempt is finite and known
before it starts, and every attempt arrives as a call something can
permit, refuse, or record. A narrow list is a clamp.

It is also how truth reaches the model. Left to its own account of what
it did, a model reports the version of events it finds most plausible.
A tool returns what actually happened — the real ticket count, the real
dispute note, the hold that is actually active.

### 4. Evidence before verdict

Every policy criterion has to be answered with evidence that names its
source before a tier can be proposed. Not "tenure is fine" but "tenure
is 31 months, from `account_read`". The criterion with no evidence is
visible as a gap, reported rather than assumed — the same move
`validate_spec` and `refine_requirement` make on a requirement before
anyone writes a test against it.

What falls out of this is the thing the rules engine never gave you.
The decision is self-describing. "Silver, because tickets per month was
6.2 against a Gold ceiling of 4, measured over the 12 months to
September, from `support_summary`" is an answer you can put in front of
a customer, an auditor, or a regulator.

### 5. Missing evidence is a refusal, not a guess

If `payment_history` cannot reach the billing system, the decision does
not proceed on four criteria out of five. It lands in `REFUSED` and
names what is missing.

This is the hardest one to sell internally and the one that pays for
the whole build. A rules engine returns a tier when its inputs are
incomplete, confidently and silently. So does a prompt. A jig says it
cannot decide, which is the correct answer and the one nobody ever
builds.

## Where each model sits

Three different things judge, and the whole design depends on not
confusing them. It is the division demo 2 makes, unchanged:

**Static validators hold the arithmetic, the thresholds, and the
transitions.** Months of tenure. Sum of spend. Count of late payments.
Tickets per month. Whether 6.2 exceeds 4. Which state may follow which.
None of this is ever a model's job, and this is most of the decision by
volume. A model asked to add up twelve monthly figures will usually get
it right, which is precisely the problem: usually.

**The writing model reads what no validator can read.** The dispute
note that says the late payment followed a billing error the company
made. The acquisition that explains a 23-month tenure. The contract
clause with a bespoke support allowance. It proposes: this criterion is
met, this one is not, this one has an exception and here is the sentence
it rests on.

**The judgment model answers one bounded question at a time.** Does
this evidence support this criterion — and nothing else. It returns a
probability, not prose, and answers below the confidence floor come
back as unsure rather than as a yes. Demo 2 sets that floor at
0.80.

Two properties of that last one carry over exactly, and both are
measured rather than hoped for:

- **A judgment can never approve anything.** It can only object. A
  criterion that clears every judgment still has to have its evidence
  and its arithmetic.
- **A judgment that could not be taken is not agreement.** If the
  decision model is unreachable, the reply says so and the
  deterministic result stands. Silence is never read as a yes.

And the same warning applies here: measure the question on
your own criteria before you let it gate anything. Measured against
this repository's own spec, the decision question read 14 of 73
acceptance criteria as unmeasurable when they plainly were not — every
one of them a quoted literal or a count. A gate wired to that blocks a
fifth of the work it was built to check.

That measurement was taken when the catalog held 73 criteria. It holds
111 now and the figure has not been re-measured, which is the warning
arriving on schedule: a number like this describes a question, a model,
and a spec on one particular day, and it goes stale without anybody
touching the gate. Start `advisory`, read what it says for a month, and
turn it up only when the numbers are yours and current.

## A walkthrough

The account from the hard 30%: 23 months of tenure, one late payment,
high support volume, and a Gold-band spend.

```text
GATHERING
  account_read            tenure 23 months, band Gold, contract enterprise
  spend_summary(12)       above the Gold threshold
  payment_history(12)     1 late payment, flagged, dispute attached
  support_summary(12)     7.4 tickets per month, Gold ceiling 4
  credit_holds            none active

  writing model proposes:
    tenure          not met on its face; the contract names an
                    acquisition date 14 months earlier
    late payment    disputed; the note says the invoice went to a
                    retired address the company held on file
    support burden  not met; 7.4 against 4

  judgment model, one question each:
    does the contract clause support an earlier tenure date?   0.91
    does the dispute note support "not the customer's fault"?  0.64  unsure
    does the evidence support the support-burden criterion?     n/a, arithmetic

  validators:
    tenure, counted from the acquisition date   37 months, met
    tickets per month                           7.4 > 4, not met

EVIDENCED
PROPOSED   Silver, on the support-burden criterion alone
REFUSED?   no - every criterion has evidence or an exception
```

Read what happened there. The tier is **Silver**, and the reason is one
criterion with arithmetic behind it that nobody can argue with. The
tenure question went the customer's way on a contract clause a model
read and a validator then recomputed. And the dispute came back
**unsure at 0.64**, under the floor — so it was not used, and it is
sitting in the record as the one thing a human should look at.

That last line is the whole value. The decision is not "Silver, trust
me". It is "Silver, here is the arithmetic, and here is the single
judgement call I could not make."

## The frontier model is not the requirement

Add up what the model actually did in that walkthrough. It read a
contract clause and a dispute note, and proposed which criteria were
met. The arithmetic was validators. The thresholds were validators. The
state transitions were the state machine. The confidence floor threw
away the one answer it was not sure about.

That is a narrow job, and a narrow job is what makes a small model
viable. The argument in one line: you are not choosing
between a rigid rules engine and an expensive model you have to trust.
Narrow the job far enough, hold everything mechanical in code, and the
model's remaining share is small enough that a model on your own
hardware does it — with no key, no per-decision bill, no customer
financial data leaving the building, and the same decision reproducible
next year when somebody asks how this account got its tier.

The rules engine could not read the dispute note. The prompt could not
be trusted with the arithmetic. The jig gives each of them the half it
is actually good at, and refuses when it has neither.

## Where this is going

Three honest limits, since this is a design rather than a product:

- **It costs more than a prompt**, and the thing that justifies the
  build is repetition. A decision you make four times a year does not
  need a jig. A decision you make four thousand times does.
- **The policy becomes the work.** Most of the effort lands in moving
  criteria across the line from "a model interprets this" to "a
  validator holds this", and that is a business conversation, not an
  engineering one.
- **It cannot tell you the policy is wrong.** Every gate here is about
  whether the policy was applied correctly. Whether the policy is the
  right policy has no state machine behind it and no second model
  either — which is the same gap the wording review leaves open: a
  requirement can pass structure and wording and still be the wrong
  requirement, and nothing in the jig will say so.

## Now the one that is yours

Everything above is a jig for a developer's own working day, and that is
the easy half. The harder question, and the one you are actually
answerable for, is how AI enters the product your business sells.

If that product is on the JVM, the pieces already exist, and it is worth
being precise about which piece does what.

**Embabel holds the sequence.** It is an agent framework from Rod
Johnson, written in Kotlin, Java-friendly, and built on top of Spring
AI. An `@Agent` is an ordinary Spring bean. The part that matters here
is how a plan gets made: an `@Action`'s parameter types are its
preconditions and its return type is its effect, so the sequence is
*derived* from types rather than written into a prompt. The planner is
Goal-Oriented Action Planning borrowed from game AI — an A\* search, in
`AStarGoapPlanner` — which means it is **deterministic, and it is not a
model**. The plan can be inspected before it executes, `@Condition`
methods are re-evaluated after every step, and actions that need no
model at all are just injected Spring services.

That is piece two of the five, in a framework, with the planner on the
deterministic side of the line where it belongs.

**Spring AI holds the judges.** The structured-output path is the type
check from demo 2: `ChatClient` can return
`.entity(Type.class, spec -> spec.validateSchema())`, which validates
the response against a JSON schema and retries with the validation
error fed back, and `.useProviderStructuredOutput()` pushes the shape
down to the provider as an API-level constraint rather than a request.
A response that will not parse into the type is not a result.

For the narrow typed surface, `@Action(toolGroups = …)` scopes tools to
the action that needs them, and progressive tool disclosure means only
the tools actually discovered for a step are put in front of the model.
That is the clamp, not reach.

For the bounded judgment question, `FactCheckingEvaluator(context,
claim).isPass()` asks one thing — is this claim supported by this
evidence — and Spring AI's own documentation shows it running against a
small local model through Ollama rather than a frontier one. Which is
the point the loyalty case has been making all along: narrow the
question far enough and the expensive model stops being a requirement.

**And refusal is an absence, not a reprimand.** This is the piece people
skip, and it falls out of the planner for free: a `@Condition` that
stays false leaves no applicable action, so there is no path to the
goal. The agent does not get told off for trying. There is nothing to
try — the same shape as being unable to refactor on a red bar.

Rod Johnson's phrase for the posture is "trust, then verify". That is
the whole argument in three words, and it is the opposite of both things
we usually ship: the rules engine that verifies without trusting
anything, and the prompt that trusts without verifying anything.

Build the jig so the business gets the judgment and keeps the
determinism. That is what we are responsible for.

## Grading your own run

Neither demo has to be taken on trust, which is the whole argument
applied to itself. From the repository root:

```bash
scripts/verify-harness-morning.sh check HARNESS-025
```

It checks the row that matters on the run's own terms rather than
against a recorded solution: a scenario carrying the tag, no undefined
steps, a unit test that compiles and has no leftover placeholder,
production files that actually differ from the base branch, a GREEN
bar, the status flipped, and the whole suite green. Hand it the JSON
`spec deliver` printed as a third argument and it also tables every
judgment the decision plane recorded, gate by gate.

## See also

- [Workshop follow-along](student-follow-along.md) — the same loop on
  the String Calculator kata, with commands you can run yourself.
- [The Jig](https://davidparry.com/blog/2026/09/25/the-jig/) — the
  argument this page applies.

# A Day in the Life — presenter notes

Stage script for the seven-slide cut, `slides/index.html?day`. Two live
demos deliver the same sentence —
`explain one spec command by name - what it does` — once through an
editor sequenced by a skill, once through a compiled command that never
shows the model a skill at all. Everything below is the presenter's
copy; the attendee version is
[a-day-life.md](../student-follow-docs/a-day-life.md).

Seven slides across 60 minutes means the slides are not the talk. The
demos are, and the slides are the places you stop to say what just
happened.

**The one piece of staging that matters:** `implement` in demo 2 took
20, 36 and 57 minutes across three measured attempts. It does not fit
inside a segment. So you start demo 2 *first*, let it go quiet, and run
all of demo 1 on top of it. Get this wrong and the talk has a
twenty-minute hole in it.

Every command below runs from **inside `harness/`**. There is no
`--root` flag anywhere in this script, and that is deliberate: `spec`
walks up from the working directory to the nearest enclosing project.
`cd harness` once and the tool finds the catalog from anywhere beneath
it. If you find yourself typing `--root`, you are in the wrong
directory.

## Before you walk on

```bash
cd <repo>
git switch trunk && git pull
git switch -c talk-$(date +%Y%m%d)         # never demo on trunk
scripts/preflight.sh                       # model, binary, toolchains
cd harness                                 # everything after this runs from here
spec validate                              # must be valid: true
spec list                                  # 23 requirements, 0 pending
spec status                                # nextId: HARNESS-025
```

Two terminals, both already in `harness/`. One is for demo 2 and you
will not type in it again after the first command. The other is spare,
for `git diff` and `spec status`.

Have the editor open with the MCP server configured and
`.cursor/skills/spec-driven-deliver/SKILL.md` present. Confirm the tool
list is live before you walk on — a server that is not connected is the
one failure the room cannot help you recover from.

Decide now whether demo 2 runs warm or cold. `.spec/cache/` is keyed on
the prompt, so a rehearsal with the identical sentence makes the real
run much faster. That is legitimate, but say so from the stage. Claiming
model speed for a cache hit is the kind of thing this talk is against.

## Slide 1 — A Day in the Life (0-6 min)

The vocabulary first, because everything else hangs off it.

A harness goes on a person. It catches you when you fall and you still
decide where to walk. Nobody wears a jig — a jig is clamped to the
bench, and it holds the work so the cut comes out the same every time.

Then the test, and give it a beat: **can the model skip it?** Anything a
model can talk its way past is a preference, not a rail. Invite them to
apply it to their own setup while you talk. Most people realise during
demo 1 that everything they currently call a guardrail fails it.

Name the sentence both demos will deliver, and say plainly that the
server, the gates and the acceptance criteria are identical in both. The
only variable is who decides the sequence runs.

**Start demo 2 now**, before you leave this slide:

```bash
spec deliver "explain one spec command by name - what it does"
# ← Enter for the generated branch name, and then it goes quiet
```

Say what you just did, then leave that terminal alone.

## Slide 2 — Demo 1, live (6-30 min)

The long segment. Paste this as one prompt:

```text
Use the spec-driven-deliver skill to deliver:
"explain one spec command by name - what it does"
```

The point to make out loud: the prompt only activates the skill and
names the work. The skill owns everything else. Its preflight calls
`project_root`, `project_inspect`, `list_requirements`, `validate_spec`
and `get_tdd_state`, briefly explains what those tools found, then
offers the branch before any write. You can watch each call.

Let it work:

```text
requirement_create          the sentence becomes a draft
validate_spec               structure first
refine_requirement          then wording, and reword until clean
scenario_add                acceptance criteria become Gherkin
step_definition_create      templates only
unit_test_create            templates only
run_tests                   expect RED
  (the model writes the implementation)
run_tests                   expect GREEN
start_refactor
requirement_mark_implemented
```

No `spec` subcommand runs in this demo. Every effect lands through one
of the 27 tools and nothing else — worth saying twice, because the room
will assume you are shelling out.

Three gates to point at as they go past, and if the model does not trip
one, say what would have happened:

- `start_refactor` refuses on a red bar.
- `requirement_mark_implemented` needs a green bar **and** a scenario
  tagged with that requirement id.
- `command_run` is RED-gated, argv-allowlisted, and spawns no shell at
  all.

Same `domain/tdd.rs`, same service layer the CLI uses. A frontier model
gets the identical refusal a person would.

Then the turn, and this is the slide's real payload. **A Skill is not a
Jig.** A skill is a briefing: it loads when the model decides it is
relevant, and it is followed as far as the model chooses to follow it.
Activation and compliance are both the model's call, so a skill fails
the skip test by construction. Nobody inside a skill has the job of
saying no.

The line to land: the skill is not the jig, the server is. A model can
skip an instruction. It cannot skip a tool that returns an error
instead of a result.

If there is time, add the inversion about MCP: the value here is not
reach, it is that the set of things the model can attempt is finite,
typed, and known before it starts. A clamp. And the return value matters
as much as the permission, because a model narrating what it just did
reports the most plausible version of events, while a tool reports what
happened.

## Slide 3 — Demo 2, back to the terminal (30-42 min)

Go to the terminal you started on slide 1. Whatever state it is in is
the demo.

Walk the stages: draft, wording review, scenario, step definitions, unit
test, RED, then `implement`. The sequence is compiled in. Nothing asked
the model whether to run it, in what order, or whether this step could
be skipped just this once.

The three judges, which is the thing to be precise about:

- **Deterministic rules** — regex and structural checks over what came
  back.
- **Types** — the return parses into the expected shape, or it is not a
  result at all.
- **A decision model**, one bounded question at a time, answering with a
  probability rather than prose. Below **0.80** it answers unsure, and
  unsure is not a yes.

Four judgments exist only on this path, because only this path generates
code: `STEPS_BIND_SCENARIO`, `UNIT_TEST_ASSERTS`,
`IMPLEMENTATION_COMPLETE`, `REFACTOR_PRESERVES_BEHAVIOUR`. In demo 1
they were simply not on the wire. Say that explicitly — it is the
sharpest difference between the two demos and it is invisible unless you
name it.

Be honest about the rough edges, all three:

- Measured attempts: **20, 36 and 57 minutes** on `implement`. A second
  run is much faster and that is the prompt-keyed cache at
  `.spec/cache/`, not the model warming up. Change one word and you pay
  full price.
- It can **run out of time** rather than finish. `timeout_seconds` is
  3600 for exactly this step, and the partial generation is gone.
- An attempt can return **nothing usable**, having spent its rounds
  looking instead of writing. The bar is untouched and the next attempt
  is briefed with the failure.

**If it failed on stage, you have the better demo.** It reports what
landed and exits nonzero. That is the thing a prompt structurally cannot
do: tell you the truth about work nobody watched. Do not apologise for
it.

Close the segment on the dial: strictness is bought with build effort,
speed with tokens and with trust, and no setting gives you both.
Reproducibility that has to replay identically next year wants demo 2.
Clearing a backlog on a Thursday afternoon is fine in demo 1. Work with
no settled shape wants neither — a jig repays its cost through
repetition, and exploration is what general agents are for.

Before you leave this slide, one aside, and keep it an aside. The
command exists to run a repeatable workflow. A useful side effect is
that the same sequence walks entry-level developers through how the
team builds, with the checks and the feedback in the open. TDD is the
example on this stage. The encoded workflow can be whatever the team
already uses to build and review work. Mentoring and the engineering
decisions stay with a person. Then turn to slide 4.

## Slide 4 — None of this was about Gherkin (42-46 min)

The hinge. Strip the domain out of both demos and five pieces are left,
and not one is about software delivery: a validated source of truth, a
sequence in code rather than a prompt, a narrow typed surface, evidence
before anything is called done, and a refusal when the evidence is not
there.

Then turn: an account comes up for loyalty review. Bronze, Silver, Gold,
and the tier drives discount band, support SLA, renewal terms, and who
gets a human account manager.

Two ways we build that today, briefly, because the room has built both.
A rules engine: every exception is a new rule, the firing order quietly
becomes load-bearing, one threshold change means regression-testing all
of it, and the tier arrives with no account of itself. Or a prompt: it
answers fluently every time and you have no way to know which time it
was right.

The skip test again, on the prompt version: every single thing in it is
a preference.

## Slide 5 — Who judges what (46-54 min)

Three judges, and the design depends on never confusing them. Validators
hold the arithmetic, the thresholds and the transitions — most of the
decision by volume, and never a model's job. The writing model reads
what no validator can: the dispute note, the acquisition date, the
bespoke contract clause. The judgment model answers one bounded question
at a time as a probability, with the same 0.80 floor.

Walk the account: 23 months of tenure, one late payment, 7.4 tickets a
month against a Gold ceiling of 4. The contract clause supports an
earlier tenure date at 0.91, and a validator then recomputes tenure at
37 months from the acquisition date. The dispute note comes back at
**0.64** — under the floor, so it was not used, and it is sitting in the
record as the one thing a human should look at.

The verdict is Silver, on one criterion with arithmetic nobody can argue
with. The line to land: the decision is not "Silver, trust me", it is
"Silver, here is the arithmetic, and here is the single judgement call I
could not make."

Say the disclaimer out loud, not just on the slide: the loyalty case is
a design, there is no implementation in the repository, and nothing in
it is runnable. What is measured is everything about the decision model.

If you have a minute, add the caveat that protects you from your own
argument: measure the decision question on your own criteria before it
gates anything. This repository's own question reads 14 of its 73
acceptance criteria as unmeasurable when they plainly are not. Start
`advisory`, read it for a month, turn it up only when the numbers are
yours.

## Slide 6 — Now the one that is yours (54-59 min)

The close, and the only thing you need them to carry out of the room.

Everything so far builds a jig for a developer's own working day, and
that is the easy half. The question they are answerable for is how AI
enters the product their business sells.

On the JVM the pieces exist. Be precise about which does what, because
vagueness here undoes the whole talk:

**Embabel holds the sequence.** An `@Action`'s parameter types are its
preconditions and its return type is its effect, so the plan is derived
from types rather than written into a prompt. The planner is
Goal-Oriented Action Planning — an A\* search in `AStarGoapPlanner` —
which is **deterministic and is not a model**. The plan can be inspected
before it runs, `@Condition` methods are re-evaluated after every step,
and actions needing no model are plain injected Spring services.

**Spring AI holds the judges.**
`.entity(Type.class, spec -> spec.validateSchema())` validates against a
JSON schema and retries with the validation error fed back;
`.useProviderStructuredOutput()` pushes the shape down as a
provider-level constraint. `@Action(toolGroups = …)` plus progressive
tool disclosure is the clamp. `FactCheckingEvaluator(context,
claim).isPass()` is the bounded question, and Spring AI's own docs show
it running on a small local model through Ollama.

**Refusal falls out for free.** A condition that stays false leaves no
applicable action, so there is no path to the goal. Nothing to be talked
past — the same shape as being unable to refactor on a red bar.

Rod Johnson's phrase is "trust, then verify", which is the opposite of
both things we normally ship: the rules engine that verifies without
trusting anything, and the prompt that trusts without verifying
anything.

Last line, and then stop: build the jig so the business gets the
judgment and keeps the determinism. That is what we are responsible for.

## Slide 7 — Thank you (59-60 min)

Leave the two QR codes on screen for questions. The left one goes to
`davidparry.com`; the right one goes directly to the repository site and
the workshop material.

## If something breaks

**The MCP server is not connected.** Nothing in demo 1 works and the
room cannot help. Check the editor's tool list first, before the
sentence. `spec mcp tools` from `harness/` confirms the server itself is
fine, which at least tells you the problem is the host.

**Demo 1's model goes off-script** — writes a file directly, or marks
work implemented early. This is the talk's thesis arriving early. Let it
happen, then show the refusal. A model that skips the skill is the
demonstration that a skill is not a rail.

**Demo 2 is still running at slide 6.** Fine. Leave it. Say what it is
doing and move on; the point of an unwatched run is that you are not
watching it. Come back to it in questions if it lands.

**Demo 2 failed.** Better outcome, covered above. Show the nonzero exit
and the report of what landed.

**You are behind.** Cut the MCP inversion on slide 2 and the measurement
caveat on slide 5. Do not cut slide 6 — the close is the reason the talk
exists, and the loyalty case without it is just an anecdote.

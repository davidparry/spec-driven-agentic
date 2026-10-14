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

**The one piece of staging that matters:** the two demos run one after
the other, in **two separate worktrees**, and they never overlap. Demo 1
finishes before demo 2 starts.

That is not a stylistic choice. Both demos drive the same crate, and a
single project root cannot hold two of them:

- `cargo test` takes an exclusive lock on `target/`, so the second
  process waits on the first rather than running.
- The RED/GREEN bar is one `currentSession` in `.spec/state.json`, and
  `run_tests` reads it, runs the suite for minutes, then writes. Two
  overlapping runs and the later write silently drops the earlier one.
- Both would allocate `HARNESS-025` from the same catalog, and the
  second blocks on `.spec/.lock` for the length of the first's model
  call.

Two worktrees give each demo its own `target/`, its own `.spec/`, and
its own branch. The cost is that demo 2 no longer has demo 1's segment
to run underneath, so it **must** go into the talk cache-warm. The
night-before setup below is what makes that true, and it is not
optional.

Every command below runs from **inside a worktree's `harness/`**. There
is no `--root` flag anywhere in this script, and that is deliberate:
`spec` walks up from the working directory to the nearest enclosing
project. `cd harness` once and the tool finds the catalog from anywhere
beneath it. If you find yourself typing `--root`, you are in the wrong
directory.

## The night before — build the two worktrees

This takes an hour of wall clock, most of it waiting on a build and a
rehearsal run. Do it the night before, not the morning of: step 3 seeds
a cache with a 24-hour life, and that clock is the one thing here you
cannot hurry.

```bash
cd <repo>
git switch trunk && git pull
git worktree add ../talk-demo1 -b demo1-$(date +%Y%m%d)
git worktree add ../talk-demo2 -b demo2-$(date +%Y%m%d)
```

**1. Warm both `target/` directories.** A fresh worktree shares no build
artifacts with the repo it came from. Cold, the first `run_tests` of
either demo is a full build of the crate before a single test runs.
Warm, the whole suite is 27 seconds.

```bash
(cd ../talk-demo1/harness && cargo test)
(cd ../talk-demo2/harness && cargo test)
```

Both must come back green. A pre-existing failure stops demo 1 dead:
the skill's loop treats a bar that was already red on unrelated tests as
a foreign failure and refuses to implement against it.

**2. Rehearse demo 1** in `../talk-demo1`, end to end, through the
editor. Then put the worktree back:

```bash
cd ../talk-demo1 && git restore . && git clean -fd harness/tests
rm -f harness/.spec/state.json
```

**3. Rehearse demo 2** in `../talk-demo2`, with the sentence typed
**byte for byte** as it appears on stage:

```bash
cd ../talk-demo2/harness
spec deliver "explain one spec command by name - what it does"
```

This is the run that pays the 20 to 57 minutes. Its replies land in
`.spec/cache/`, keyed on a hash of the model, the endpoint, the full
message history and the tool schemas — so a sentence that differs by one
character, a different model, or an edited config is a cache miss and
you pay full price on stage.

**4. Reset demo 2's worktree, and mind which files you keep.**

```bash
cd ../talk-demo2
git restore . && git clean -fd harness/tests
rm -f harness/.spec/state.json    # or the demo opens on the rehearsal's GREEN bar
# do NOT touch harness/.spec/cache/ — that is the whole point of step 3
```

Both `state.json` and `cache/` are gitignored, so `git restore` leaves
them alone and you have to be deliberate about each one: keeping the
state file starts the demo on a green bar it did not earn, and deleting
the cache throws away the hour you just spent.

## On the morning

```bash
cd <repo>
scripts/preflight-day.sh                   # models, binary, both worktrees
```

It checks what this talk actually needs: both Ollama models pulled and
answering, the `spec` binary serving its 27 tools, `CRITERION_MEASURABLE`
held advisory, and for each worktree a valid catalog, a green suite and
a warm `target/` — plus a seeded cache for demo 2 and a clean slate for
demo 1. Fix anything it reports before you go further. `SKIP_SUITE=1`
re-runs it without rebuilding both worktrees, which is what you want
after fixing one thing.

That one gate is worth understanding before someone asks. The plane is
at the shipped `enforce`, and five of the six gates ship at an advisory
ceiling they cannot be raised past — so `enforce` arms exactly one
question, whether a criterion is measurable, and that is the question
this repository has measured as wrong about one criterion in five of
its own. It is held down by name in `harness/.spec/config.toml`. If it
were armed, demo 1 would hit a false alarm it cannot reword its way out
of, because the skill is told to reword until clean. The config is the
slide-5 advice applied to itself, and it is a good answer to give.

Two terminals. One sits in `../talk-demo2/harness` and you will type in
it exactly once, at slide 3. The other is spare, for `git diff` and
`spec status`.

**Open the editor on `../talk-demo1/harness`, not on the repository
root.** This matters more than it looks: the MCP config and the skill
both live under `harness/.cursor/`, and the server is launched with
`--root ${workspaceFolder}`. Open the repo root instead and the editor
finds no skill and points the server at the kata catalog — demo 1 then
delivers into the wrong project, if it starts at all. Confirm the tool
list is live before you walk on; a server that is not connected is the
one failure the room cannot help you recover from.

**Say the cache out loud.** Demo 2 replays a rehearsal from
`.spec/cache/`, and that is legitimate, but the room has to hear it.
Claiming model speed for a cache hit is exactly the kind of thing this
talk is against. The honest framing is the one that is also true: the
20, 36 and 57 minutes were real, you paid them last night, and what
they are watching is the second run.

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

Nothing to start here. Demo 2 waits until slide 3, in its own worktree,
because the two of them cannot share a checkout — and that is worth one
sentence from the stage rather than none. The same crate, two drivers,
one `target/` lock and one bar: the collision is itself an example of
why the sequence belongs somewhere a model cannot negotiate with it.

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

The branch is where it hands back to you. It asks for a name and then
asks **you** to run `git switch -c`, because creating a branch needs a
shell and the agent is not given one. Type it, tell it you have, and
watch it re-read `project_inspect` to confirm rather than taking your
word for it. Small moment, and it is the whole argument in miniature.

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

No `spec` subcommand runs in this demo. Every effect the *agent* has
lands through one of the 27 tools and nothing else — worth saying
twice, because the room will assume you are shelling out. The one
command typed on stage is your own `git switch -c`, which is the point
rather than an exception to it.

One honest footnote while `refine_requirement` is on screen: it is the
single tool on this surface that can call a model. The deterministic
rules run first and always, and then — because this project configures
a decision model — it attaches a bounded second opinion on whether each
criterion is measurable. Everything else in the 27 is pure. Say it
rather than letting someone find it later; the talk is about being
precise over who decides what.

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

## Slide 3 — Demo 2, the other worktree (30-42 min)

Go to the second terminal, the one sitting in `../talk-demo2/harness`,
and start it now:

```bash
spec deliver "explain one spec command by name - what it does"
# ← Enter for the generated branch name, and then it goes quiet
```

One question and then nothing. Say what the single question was — a
branch name — and that it is the last time the run asks you anything.

Then say the cache out loud, before anyone wonders. You ran this
sentence last night and paid between twenty minutes and an hour for it;
what is replaying now came from `.spec/cache/`. The run is real and the
cost was real, and you are showing them the second one because an hour
does not fit in a slot. Do not let this land as a reveal later.

Walk the stages as they go past: the sentence is split into
requirements and reviewed before anything is accepted, then per
requirement a scenario, step definitions, a unit test, a RED bar, and
`implement`. The sequence is compiled in. Nothing asked the model
whether to run it, in what order, or whether this step could be skipped
just this once.

Two details worth having right if someone asks. The scenario here is
built from the acceptance criteria by a **template**, not by a model —
which is why the scenario gate that exists elsewhere in the codebase
never fires on this path. And the wording review is the pipeline's own
automatic check on the proposals, not the interactive reword loop
`spec draft` gives you.

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

Be exact about "not on the wire", because it is four gates and not all
of them. Demo 1 did reach one judgment, on whether a criterion is
measurable, through `refine_requirement`. What demo 1 never reached is
the four above — the ones that read generated code — because nothing on
the MCP surface generates code. The difference is not that demo 1 had
no judge. It is that demo 1 had no code for a judge to read.

These four are advisory: they are recorded and reported, and they do not
stop the run. That is deliberate and it is the same advice you give the
room on slide 5 — a gate goes to blocking after you have measured it on
your own criteria, not before.

Be honest about the rough edges, all three:

- Measured attempts: **20, 36 and 57 minutes** on `implement`. A second
  run is much faster and that is the cache at `.spec/cache/`, not the
  model warming up. It is keyed on the model, the endpoint, the whole
  conversation and the tool schemas, so change one word and you pay
  full price.
- It can **run out of time** rather than finish, and the partial
  generation is gone. This project raises `[llm] timeout_seconds` to
  3600 for exactly this step; the shipped default is 300, which cuts
  `implement` off mid-generation on a crate this size.
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

## Slide 5 — Past the rules engine: splitting a decision responsibly (46-54 min)

Open on the correction. `if`, `switch`, and a rules engine are the same
kind of thing: they hold arithmetic over facts that are already
structured. A dispute note is not one of those facts. What we actually
did was not stop — we added a flag, and the judgment moved to whoever
set it, with no record of why. A model can be asked to read the note.
On this account it reads the contract clause at 0.91, and a validator
then recomputes tenure. The dispute note comes back at 0.64, under the
floor, so it is not used. That is the responsible split: the model gets
the reading, not the numbers and not the tier, and unsure is never a yes.

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
gates anything. Measured against this repository's own spec, the
question read 14 of 73 acceptance criteria as unmeasurable when they
plainly were not — every one a quoted literal or a count. That was
taken when the catalog held 73 criteria; it holds 111 now, and the
figure has not been re-measured since, which is itself the point. Start
`advisory`, read it for a month, turn it up only when the numbers are
yours and current.

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

**The MCP server is not connected, or there is no skill.** Nothing in
demo 1 works and the room cannot help. Nine times in ten the editor is
open on the repository root instead of `../talk-demo1/harness`, which
is where both `.cursor/mcp.json` and the skill live. Check the tool
list first, before the sentence. `spec mcp tools` from that `harness/`
confirms the server itself is fine, which at least tells you the
problem is the host.

**Demo 1's model goes off-script** — writes a file directly, or marks
work implemented early. This is the talk's thesis arriving early. Let it
happen, then show the refusal. A model that skips the skill is the
demonstration that a skill is not a rail.

**Demo 1 is still grinding at 30 minutes.** It will not be the suite.
Warm, the whole crate runs in **27 seconds**, and 35 after a source edit
forces a rebuild — so three or four `run_tests` calls are two minutes of
the segment, not twenty. What runs long is the model: reading the
catalog, writing Rust, and re-reading failures. Stop it at the green
bar. `start_refactor` and `requirement_mark_implemented` are the two
gates worth showing and both are instant, so skip the refactor edit
itself and go straight to marking. Do not let it eat slide 3 — demo 2 is
the half of the argument the room cannot guess at.

**Demo 2 is slow, meaning the cache missed.** The sentence has to match
last night's byte for byte, and the config has to be untouched. If it is
clearly going to run long, leave it running and move to slide 4; say
what it is doing and that the point of an unwatched run is that nobody
watches it. Come back to it in questions if it lands. You lose the
walked output, not the argument.

**Demo 2 failed.** Better outcome, covered above. Show the nonzero exit
and the report of what landed.

**You are behind.** Cut the MCP inversion on slide 2 and the measurement
caveat on slide 5. Do not cut slide 6 — the close is the reason the talk
exists, and the loyalty case without it is just an anecdote.

## Afterwards

Grade the run rather than describing it. From the repository root:

```bash
scripts/verify-harness-morning.sh check HARNESS-025
```

It reads whichever worktree you point it at and checks the row that
matters: a scenario tagged with the id, no undefined steps, a unit test
that compiles and carries no leftover placeholder, production files that
actually differ from the base branch, a GREEN bar, the status flipped,
and the whole suite green. Hand it demo 2's JSON report as a third
argument and it also tables every judgment the decision plane recorded,
per gate — which is the most convincing artefact in the whole talk if a
question goes that way.

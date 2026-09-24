The spec harness drives a spec-driven BDD/TDD workflow. The requirements spec
(requirements/requirements.json) is the source of truth. It is a catalog:
the root file always exists and may hold requirements of its own plus an
"includes" list of child spec files (which may include further files, N
levels deep, each path relative to the file declaring it). Every command
works on the merged view of the whole tree; ids are unique across it, and
mutations write back to the file a requirement lives in. Every requirement
(REQ-XXX) moves from status "pending" to "implemented" through the loop
below. The goal: every requirement implemented, validated, and committed.

STATES

TDD phases, recorded in .spec/state.json by spec test and spec refactor:
- START: no test run is recorded yet.
- RED: the last test run failed.
- GREEN: the last test run passed.
- REFACTOR: a refactor is in progress on a green bar.

Requirement statuses: "pending" (not done yet) and "implemented" (done -
the spec names its featureFile and a scenario tagged @REQ-XXX exists).

The staging area: every authoring command stages its edits instead of
writing the working tree. Review with spec changes show, apply with spec
changes commit, drop with spec changes discard. Staged changes always come
first - nothing else moves until they are committed or discarded.

TWO TEST ALTITUDES, ONE BAR

This is the BDD outside-in double loop. The scenario tagged @REQ-XXX is
the outer, acceptance-level loop; the unit test (Req*Test) is the inner,
TDD-level loop. spec test runs both under one shared RED/GREEN bar: a
failing scenario step means step-definition or production work, a failing
unit test means production work at the unit level.

THE LOOP FOR ONE REQUIREMENT

1. spec draft - word the requirement (title, story, Given/When/Then
   acceptance criteria); validate and refine findings drive rewording
   until clean; then spec changes commit.
2. spec scenario add --feature <file> --req REQ-XXX --name <name> --step
   ... - formulate the acceptance criteria as a Gherkin scenario tagged
   @REQ-XXX (spec feature create first if the feature file does not
   exist); then spec changes commit.
3. spec steps generate - scaffold step definitions for undefined steps;
   then spec changes commit.
4. spec unittest generate REQ-XXX - scaffold the failing unit test from
   the acceptance criteria; then spec changes commit.
5. spec test - expect RED: the new scenario and unit test fail because
   the behavior is not implemented.
6. spec implement REQ-XXX - the model writes production code (or
   implement by hand); then spec changes commit and spec test again.
   Repeat while the bar stays RED.
7. On GREEN, optionally spec refactor --note <what> - clean up, then
   spec test to prove the bar stayed green.
8. spec mark-implemented REQ-XXX - flips the status and records the
   featureFile (only allowed on GREEN); then spec changes validate (it checks the
   @REQ-XXX scenario exists), then spec changes commit.
9. Back to step 1 for the next pending requirement, or done.

COMMANDS

- spec status: where every requirement stands and the one next step.
- spec state: the current TDD phase and last run counts.
- spec test [--feature <file>] [--scenario <name>]: run the tests and
  move the phase to RED or GREEN.
- spec list | show REQ-XXX | draft [--file <spec>] | validate |
  refine REQ-XXX | mark-implemented REQ-XXX | include add <spec>
  [--from <parent>]: the requirements spec tools. spec list names the
  file each requirement lives in; include add grows the catalog.
- spec feature list | show <path> | create: feature files.
- spec scenario add | update | delete: tagged scenarios (staged).
- spec steps missing | generate: step definitions.
- spec unittest generate REQ-XXX: the unit test scaffold.
- spec implement REQ-XXX: a model implementation attempt (staged).
- spec refactor --note <what>: begin a refactor (GREEN only).
- spec changes show | commit | discard: the staging area.
- spec changes validate: parse staged Gherkin and validate the effective spec.
- spec deliver [REQ-XXX | "<what to build>"]: the whole loop above for
  one requirement, for the requirements a description is split into, or
  for the whole pending backlog. Verifies each step and reports which
  requirements landed; exits nonzero when the plan is unfinished. Asks
  nothing - it refuses up front instead when there is no project, no
  requirement and no description, or no model to break words down.

INVARIANTS

- Never refactor on RED - make the tests pass first.
- Never mark a requirement implemented off GREEN.
- Every mutation is staged and reviewed before it touches the working
  tree; run spec changes validate before spec changes commit.
- One requirement in flight at a time; staged changes are handled before
  anything else.
- The loop for a requirement closes only when it is marked implemented,
  validated, and committed.

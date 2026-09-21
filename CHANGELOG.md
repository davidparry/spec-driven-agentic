# Changelog

## Unreleased

- `spec draft`'s splitter now rejects a reply whose criteria are all
  happy paths and asks again, so the draft the wizard walks you through
  already carries an edge case. The prompt had asked for one since the
  beginning, as the fourth clause of a compound rule about criteria, and
  models dropped it often enough that the pattern was reliable: the
  wording review then raised `criteria: only happy paths`, the repair
  loop ran, and the author reviewed the same requirement twice — once
  before the edge case existed and once after. The rule that finding
  comes from is deterministic, so it is now applied to the model's reply
  the moment it arrives, where a retry costs seconds the author never
  waits through instead of a second pass over wording they already read.
  The edge-case clause is a rule of its own in the prompt as well, with
  the vocabulary and two examples.

  The check goes lenient on the last attempt rather than failing the
  split: a happy-path draft is worth keeping, and losing one to a
  stubborn model would drop the author into manual drafting — worse than
  the second pass this removes. When the retries cannot win it, the
  findings round still asks, exactly as before.

- `spec scenario generate <REQ>` writes a requirement's scenarios from
  its acceptance criteria, one per criterion, staged and tagged. The BDD
  altitude was the only one without a generator: `spec unittest generate`
  has always written the unit test from the criteria and `spec steps
  generate` the step definitions, while the scenario — the most
  mechanical of the three, since criteria are already stored as
  Given/When/Then — could only be typed out a `--step` at a time. The
  agent path never had this gap, because an agent reads the criteria and
  calls `scenario_add` itself; the CLI exposed that tool's writing half
  and not its authoring half, so the human did the model's job.

  A literal reading of the criteria is always available and needs no
  model, and `source` reports `template` when it is what got staged. It
  is correct but foreign — it cannot know the feature file it is joining
  opens every scenario on `Given a string calculator` — so a resolved
  model is given the requirement, the feature file, and the existing step
  definitions, and asked for the same behaviour in the file's own
  vocabulary (`source: "llm"`, profile `scenario-generate`:
  `get_requirement`, `feature_read`, `step_definitions_find`). On the
  kata that difference is the difference between zero undefined steps and
  two. The reply is only used when it holds exactly one scenario per
  criterion with valid keywords and no colliding names, so coverage
  cannot be lost to a chatty model. Re-running against a requirement that
  already has tagged scenarios is refused rather than stacking a second
  copy of each.

## 0.5.4

Two rounds of defects found by re-walking the documented paths rather than
by running the kata again. Nothing here is a new feature. The first round
is correctness and safety — things that were wrong or unsafe and quiet
about it; the second is interaction — things that were correct and
unbearable to sit in front of.

- `spec validate` exits non-zero when the spec is invalid. It printed
  `"valid": false` and exited 0, so any CI gate scripted on it passed on
  a duplicate id, a circular include, or a criterion that is not phrased
  Given/When/Then — which is the whole population of things the command
  exists to catch. The binary also disagreed with itself, since `spec list`
  has always exited 1 on a circular include. The report is unchanged; only
  the status is new, so a caller that reads the JSON sees exactly what it
  saw before.

- `spec scenario add` no longer deletes the content following the last
  scenario in a feature file. `0.5.2` taught the parser to round-trip the
  header comments and the `As a / I want / So that` narrative, and the
  same defect at the other end of the file went unnoticed: Gherkin allows
  nothing but comments and blank lines after the last scenario, so the
  trailing block had nowhere to be stored and was dropped on the first
  append. The kata's feature file ends in a two-line note telling the
  student that `REQ-003+` scenarios are written live during the workshop
  from the acceptance criteria — which is to say Exercise 2 deleted its own
  instructions on its first tool call. A `trailing` field on `FeatureDoc`
  carries it now, and `render` writes every append *above* it, so a note
  that points at the end of the file keeps pointing there.

- Staging is safe across concurrent processes. The `0.5.2` lock covered
  one process; an advisory lock belongs to the open file rather than to
  the process, so nothing covered two. Six concurrent `spec scenario add`
  invocations against one feature file produced five `"staged": true`
  replies, one hard crash reading a half-written manifest, and three
  surviving scenarios: two callers were told they had staged and had not.
  The manifest is written atomically — scratch file in the same directory,
  then renamed over the real one, so a reader sees the old bytes or the new
  ones and never a mixture — and the staging directory is guarded by an
  advisory lock on `.spec-staged/.lock` held for the whole
  read-modify-write. Four details are deliberate. Each writer gets a
  scratch file of its own, because writers sharing one rename each other's
  half-written bytes into place, which is the crash this started as. The
  in-process claim is taken *before* the file lock, because a second
  handle on the same lock file blocks its own process exactly as hard as
  it blocks a stranger. The lock is re-checked by inode after it is taken,
  because `changes commit` deletes the whole staging directory including
  the lock file, and a handle can otherwise end up holding an inode that
  is no longer the lock. And the kernel releases the lock when the file
  closes, so there is no stale lock to clear after a panic or a kill.
  Threads cannot prove any of this — every thread in one process shares
  the in-process half — so `harness/tests/staging_concurrency.rs` drives
  real `spec` child processes.

- `spec implement`'s confirmation prompt is no longer hidden behind the
  spinner. The command asks before it runs a shell command the model
  requested, and the animation redrawing its own line over the question
  left the run looking hung at the exact moment it was waiting on a human.
  Spinners now hush for the duration of any prompt, decided once at the
  composition root rather than in each wizard, so a prompt written later
  inherits it.

- `spec implement` no longer stages files the model returned unchanged. A
  byte-identical "modify" is not an edit, and staging it gave the reviewer
  a diff with nothing in it to review. If every file comes back unchanged
  there is nothing to review at all, and the reply says so.

- `spec model use` preserves the comments and the key order in
  `.spec.toml`. It parsed, mutated, and re-serialised, which threw away
  every comment in the file — including the block that documents
  `server:tool` and the commented-out defaults. That file is mostly prose,
  and rewriting the key is now a line edit against it: a commented-out
  `# model = ...` is a comment and not the key, so the real assignment is
  the one that moves.

- `spec draft`'s partial-flag error names the flags you supplied. It
  picked whichever flag it happened to check first, so `--title X` on its
  own was answered by demanding `--title` — naming a flag the developer
  had already typed and not the two they had not.

- CLI `nextStep` text no longer names MCP tools. The services word their
  advice for the agent, which calls tools; none of those names is a
  command, so a student who followed the advice literally typed something
  that does not run. Every reply the shell prints is rewritten into the
  shell's dialect at the one place it prints one — 23 tools translate to
  their commands, and the tools with no command are deliberately left
  alone, because a visible tool name reads better than an invented
  command.

- Refactor refusals are worded for the phase the developer is in.
  "Never refactor on a red bar" is the right sentence on RED and nonsense
  anywhere else: at START there is no bar yet, and in REFACTOR there is
  already one open. Each phase gets its own second sentence, and the first
  one still names the rule.

- `spec state` leads with the phase. `instructions` is roughly 900
  characters of unchanging guidance on how to read the phase log, and it
  came first, so the command a stuck student is sent to answered "what
  phase am I in?" with a page of prose before the one word they wanted.
  Field order is reading order now and `instructions` is last. Every field
  is still there, so an agent reading this over MCP loses nothing.

- The non-TTY stdin warning is scoped to the commands that run a wizard.
  The warning's point is that a wizard ending in "Stage this?" declines on
  a spent pipe and therefore stages nothing. `spec implement`,
  `spec unittest generate`, and `spec steps generate` have no wizard and
  stage regardless — they printed it too, which told a scripted run its
  staged work had been thrown away when it had not.

The second round is about what the commands feel like to use.

- The prompter distinguishes the end of the input from an empty line. A
  read past the end of a pipe returns an empty string and pressing Enter
  returns an empty string, and conflating the two is what made
  `spec reword` never terminate: the wording review asks "[r]eword again,
  [m]anual, [a]ccept [Enter for r]", read the end of the pipe as `r`,
  reworded, found the same finding, and asked again — forever. A wizard
  whose answers have run out now stops asking, declines, and says why
  once on stderr; confirmations answer no, which is the safe terminal
  action, so a wizard ending in "Stage this?" reaches its own declined
  outcome and reports it rather than erroring out with a report nobody
  sees. Ctrl+D during a terminal wizard takes the same path: the declined
  report, and exit 0, where it used to exit 1 with an error. Decorated at
  the composition root next to the spinner-hushing prompter and for the
  same reason — a wizard several layers down should not have to know where
  its answers come from.

- `spec unittest generate` and `spec steps generate` narrate the wait.
  Both call a model, both showed nothing for the duration, and a rejected
  reply costs another full call — so three silent minutes were
  indistinguishable from a hang. They now say who is being asked and what
  for while the model works, and name each rejected reply with the reason
  and the attempt number. The template path has no wait and says nothing,
  which is the point.

Anything describing `spec validate` as exiting 0 on a bad spec, a feature
file losing its trailing comment, concurrent staging as single-process
only, or `spec reword` as hanging on a pipe is stale against this release.

## 0.5.3

Defects found by running the full workshop end to end through the `pi`
agent against 0.5.2. Nothing here is driven by a new feature; each item is
something the documented path walks into on its own.

- Generated unit-test methods no longer collide. Two acceptance criteria
  differing only in punctuation slugged to the same Java method name and
  the test class stopped compiling with `method ... is already defined in
  class StringCalculatorTest`. The workshop reaches this without doing
  anything unusual, because a custom-delimiter requirement produces
  criteria that differ only by the delimiter character. Every template
  that slugs free text into an identifier now claims its name from a
  `MemberNames` allocator: the first claimant keeps the bare slug and
  later collisions take `_2`, `_3`, and so on in order. Uniqueness holds
  both within one generation batch and against the members the target file
  already declares, so appending to a class that has `foo()` produces
  `foo_2()`. Names are allocated in criterion order and stay the same
  across a regenerate, so re-running the command does not churn the diff.
  The suffix disambiguates the identifier and nothing else — the criterion
  is already carried verbatim beside the member, in the `@DisplayName`, in
  the comment above the body, and in the `TODO` the placeholder fails
  with. This was never Java-only: the .NET and Rust unit-test templates
  slug the same way, and so does step-definition generation in all three
  languages, where two step texts differing only in punctuation collide
  identically. JavaScript and TypeScript are structurally immune and are
  left alone — their test names are string literals and their step
  definitions are anonymous functions, so there is no identifier to
  collide.

- Spec JSON files end in a newline. `requirements/requirements.json` and
  every child spec file stopped at `}`, leaving a `\ No newline at end of
  file` marker in students' diffs and reading inconsistently against the
  generated Java and Gherkin, which have always ended in one. Every writer
  of a spec document now renders through a single `model::render`, so one
  place decides what a spec file looks like on disk instead of three that
  have to agree.

- `refine_requirement` can no longer report a false `clean: true`. This is
  the one worth reading twice. `requirement_reword` stages its edit, while
  refinement read the committed copy, so a deliberately vague story that
  had just been staged came back `{"clean": true, "findings": []}` — the
  tool passed judgement on text the developer had already replaced and
  told them their wording was fine. The same split stopped the documented
  iterate-until-clean loop from converging: rewording changed nothing the
  next pass could see, so identical findings came back indefinitely unless
  an agent inserted an undocumented `changes_commit` between passes.
  Refinement now resolves staged-first, and the reply carries a `source`
  field reading `"staged"` or `"working tree"` so the behaviour is never
  silent. The loop no longer needs a commit between passes — reword and
  refine until the findings are gone, then commit once. `validate_spec`
  deliberately still reads the committed spec, because it is the frozen
  workshop tool and `changes_validate` is its staged-aware twin, but it
  now discloses that: its `nextStep` names `changes_validate` whenever a
  spec edit is waiting in staging.

- Catalog-structure errors name a remedy that exists. A duplicate id and a
  spec file included more than once were both answered with advice to call
  the reword tool, alongside a blanket "never edit the requirements file
  by hand". Rewording fixes neither — a duplicate id needs a requirement
  object deleted from one of the files declaring it, a repeated include
  needs an entry removed from an `includes` array, and no tool performs
  either edit — so the model was told to do something impossible and
  forbidden from doing the only thing that would work. These two classes
  now get their own guidance, which names the file edit and states that
  the hand-editing rule covers wording, not catalog structure; the
  exception is written down rather than left as a rule the reader has to
  quietly break. Mixed issues keep both remedies, with the prohibition
  intact for the wording half. The same wrong advice also lived in
  `changes_validate`'s `nextStep` and in the `requirement_reword` tool
  description, which claimed it repairs whatever validation reported; both
  now agree with the rest.

- `changes_show` counts every edit it elides. The six-edit case that
  prompted the investigation was in fact correct — `(1 earlier edit(s))`
  plus five named edits is six, and nothing was lost. The real defect
  started at the eighth edit: the `(N earlier edit(s))` marker became an
  ordinary element of the summary on the next merge and was re-counted as
  a single dropped edit, so the total pegged at 2 and understated the
  batch for as long as the run went on. The count now round-trips, and
  the phrasing states the total outright, reading
  `(9 edits in all, 4 not shown); ...` — because `changes_show` is the
  human review checkpoint and a reviewer should not have to add a prefix
  to a list to learn what they are approving. The cap stays at five named
  edits, so one file's review line still cannot grow without bound.

- The MCP `list_requirements` tool reports the spec file each requirement
  lives in, matching the `file` field `spec list` has always returned.
  Once the workshop splits the catalog across included files, an agent
  driving over MCP could see that a requirement existed but not which
  document held it. The field is declared last, so the existing `id`,
  `title`, and `status` keep their names and their wire order and nothing
  downstream shifts.

The six fixes are covered by 802 unit tests, 228 Cucumber scenarios, and
23 MCP conformance tests. Anything describing the old `refine_requirement`
loop — a `changes_commit` between refinement passes in particular — the
old `changes_show` phrasing, or generated member names without a `_2`
suffix is stale against this release.

## 0.5.2

The version the student follow-alongs and the two records in `notes/` are
written against. Check with `spec --version`; a binary reporting anything
lower does not have the generation fixes below.

- `spec steps generate` and `spec unittest generate` send the model only
  the newly generated class members, never the file they are spliced into.
  The append itself is deterministic Rust (`splice_step_definitions`,
  `splice_unit_tests`), so every byte outside the insertion point is
  carried over rather than retyped, and the model cannot rename a field or
  an existing step method on the way past. Measured for one new step
  definition: 7 added lines, 0 removed, against a 48-line whole-file
  baseline. A reply that hands back a whole file, alters a generated step
  expression, or drops a definition is refused, and the deterministic
  members are staged instead (`"source": "template"`). The pre-existing
  whole-file gate still runs on the assembled result as defence in depth.
  Greenfield generation, which has no existing file to protect, still goes
  through the whole-file polish pass.

- Appended members are separated from whatever the class already declares
  by exactly one blank line, collapsing a pre-existing trailing blank so a
  second append never leaves two.

- Spec text is backslash- and control-character-escaped wherever it crosses
  into generated source, through one `escape_literal(text, quote)` helper
  parameterised on the quote character. Only double quotes were escaped
  before, at seventeen interpolation points across the Java, Rust, C#, and
  JS/TS templates. A criterion holding the two-character `\n` escape —
  REQ-005 does — broke the Maven failure message across two lines; a
  criterion holding a real newline turned the `// criterion` comment into
  stray Java and failed compilation outright. The Rust step-definition arm
  was worse: it interpolated step text raw, so any step with a quoted
  argument emitted a `todo!` that could not compile.

- `extract_patterns` un-escapes symmetrically with `escape_literal`,
  including `\\`. Unknown escapes are deliberately left alone, so the `\d`
  in a hand-written regex step stays a regex atom. Without this a generated
  pattern would not match itself, would read as missing, and the next
  `spec steps generate` would append a duplicate definition — which makes
  Cucumber refuse every scenario that uses it.

- A staging lock serialises concurrent mutating MCP tool calls. Staging a
  mutation is a read-modify-write spread across a service and an adapter,
  so a host that batches tool calls in parallel could interleave two of
  them, let the second write win, and lose one edit while both calls
  reported success.

- `changes_show` summaries accumulate when several edits hit one file: two
  `scenario_add` calls on the same feature file now produce one entry
  naming both scenarios. The summary used to be overwritten, understating
  the review surface at the exact moment a human is asked to approve it.

- Feature-file header comments and the `As a / I want / So that` narrative
  under `Feature:` survive `scenario_add`. The Gherkin parser discards
  comments and the description was never round-tripped, so both vanished on
  the first append.

- Model replies are re-escaped before they are staged, so a reworded
  requirement keeps the two-character `\n` the canonical spec uses instead
  of deserialising it into a real newline.

- The `command_run` confirmation no longer renders its `[y/N]` suffix
  twice.

- The wizards warn on stderr when stdin is not a terminal, and the reply's
  `nextStep` says that nothing was staged. A read past the end of a pipe is
  indistinguishable from pressing Enter, so `spec reword` used to take
  defaults for what it could not read, decline at the final confirmation,
  and exit 0 without explaining itself.

- Polished fragments are guaranteed to end in a newline and to keep the
  template's leading indent, both of which the splice point relies on and
  code-fence stripping removes.

- `scripts/verify-workshop-run.sh check` resolves the spec catalog's
  `includes` recursively, with a circular-include guard, so a requirement
  moved into a child spec file is still graded.

One note on version strings, since it is the reason this bump exists. Four
of the items above — the staging lock, the feature-header preservation, the
recursive `includes` in the verifier, and the first version of the
piped-stdin warning — landed one commit before the bump, while the crate
still reported `0.5.0`. Two materially different builds therefore answer
`spec --version` with `0.5.0`.

It then happened a second time at `0.5.1`. That version was built and
installed from an uncommitted working tree and never committed, so
`git log -S 'version = "0.5.1"'` finds nothing — but binaries reporting
`0.5.1` were installed and used, and they predate the generation fixes
above. So a `0.5.1` in the wild is not a phantom; it is a build from
before this entry. If a symptom recorded in
`notes/workshop-validation-runs.md` reappears, check the version first and
rebuild anything below `0.5.2`.

## 0.2.6 – 0.5.0

These releases were never split into per-version entries. Three headline
changes in the range can be dated from the crate version at the time:

- `0.4.0` consolidated the workshop MCP server into the binary, deleting the
  Java `mcp-server/` module and renaming `mcp-client/` to `smoke-test/`.
- `0.4.1` renamed `cli/` to `harness/` and the crate to `bdd-harness`.
- `0.5.0` renamed the binary to `spec` and the crate to `spec-harness`.

Everything from `0.2.6` through `0.3.3` was tagged and released without
changelog entries, and those releases are not reconstructed here. The list
below is the accumulated backlog for the whole range, roughly newest first.

- The binary is now `spec` and the crate is `spec-harness`. What the tool
  does is author, gate, and drive a requirements spec; `bdd` named the
  altitude of one of the two test loops it runs, which was always the
  narrower half of the story. This is a hard cutover — there is no `bdd`
  shim and no alias. Installing `spec` leaves an older `bdd` on PATH
  untouched, so uninstall that separately.

  The `bdd spec …` group is promoted to the top level, because
  `spec spec draft` is absurd: `spec list`, `spec show`, `spec draft`,
  `spec validate`, `spec refine`, `spec reword`, `spec set-feature`,
  `spec mark-implemented`, and `spec include add`. Only `validate`
  collided, and the requirements spec won it, so the Gherkin gate that
  was `bdd validate` is now `spec changes validate` — which also finally
  matches its MCP name, `changes_validate`. Every other command keeps its
  own name behind the new one.

  On-disk state is renamed with the tool: `.spec.toml`,
  `.spec-state.json`, `.spec-memory.json`, `.spec-staged/`,
  `.spec-cache/`, `.spec-log/` (holding `spec.log`), `.spec-history`, and
  the home-directory MCP registry at `~/.spec/mcp.json`. Those names were
  scattered string literals and are now constants in `domain`, declared
  once. Nothing migrates a project carrying the old names: rename them,
  or let the harness recreate what it needs.

  Also renamed: `BDD_MCP_CONFIG` to `SPEC_MCP_CONFIG` and the `BDD_E2E_*`
  knobs to `SPEC_E2E_*`; release assets to `spec-harness-*` with the
  receipt at `~/.config/spec-harness/spec-harness-receipt.json`;
  `RUST_LOG` filters on `spec_harness::…`; the interactive shell prompt to
  `spec>`, forgiving a pasted leading `spec` where it used to forgive
  `bdd`; and the smoke test's `-Dbdd.binary` to `-Dspec.binary`.

  Two things deliberately did not move. The 25 MCP tool names and the
  `spec-driven-server` key are unchanged, so an MCP client needs nothing
  but the new `"command": "spec"`. And `bddFramework` stays `bddFramework`
  in `project_inspect` output and `.spec-memory.json`, because that field
  names the project's BDD framework — Cucumber-JVM, cucumber-rs — and has
  never referred to this tool.

- The `cli/` directory is now `harness/` and the crate is `bdd-harness`:
  what ships is a harness for the whole spec-driven loop — commands and
  the embedded MCP server — not only a command line. The binary is still
  `bdd` and no command, flag, or tool name changed. What does change:
  release assets are `bdd-harness-*` (installer, archives, and
  `bdd-harness-uninstaller.sh`), the install receipt is
  `~/.config/bdd-harness/bdd-harness-receipt.json`, `RUST_LOG` filters on
  `bdd_harness::…`, the CI job is `harness`, and the site serves the
  harness page at `/harness/`. An existing install keeps working, but a
  receipt written by an older installer is only understood by the old
  `bdd-cli-uninstaller.sh`.

- `scripts/verify-workshop-run.sh check` grades Exercise 1 on its own
  terms. It used to demand that REQ-007 match the `complete` branch word
  for word, which no correct run could satisfy: the wording is authored
  live by the student and their agent, and the recorded one predates the
  custom-delimiter prompt. It now asks `bdd spec validate` and
  `bdd spec refine` — the same deterministic checks Exercise 1 runs — and
  that a criterion covers the `//` declaration.

- Exercise 2 is graded the same way, so the verifier no longer reads the
  `complete` branch at all. It used to require REQ-003's scenarios to
  match that branch character for character and the unit test to contain a
  method named `twoCommaSeparatedNumbersAreSummed`, which a harness-driven
  run cannot produce: `bdd unittest generate` names one method per
  acceptance criterion. A green run was failed for naming. Both checks now
  ask whether every one of REQ-003's acceptance criteria is covered by a
  scenario tagged `@REQ-003` and asserted by a `@Test` that names the
  requirement — the criteria are the bar, the wording is the run's. A
  missing scenario is still caught, and named: `bdd validate` only
  requires that *one* tagged scenario exist, so a run that wrote a
  scenario for one of two criteria used to pass every gate.

- The deck can change cuts from inside the deck: a switch in the
  bottom-left corner and the <kbd>t</kbd> key move between the 60- and
  30-minute tracks, and `?60` now forces the long cut so the switch also
  works from the published `/talk30/` path. Until now the cut was decided
  by the URL alone, so opening `slides/index.html` — what the README tells
  you to do — left no way to reach the short track but to retype the
  address. The links that were supposed to offer it were broken in the
  same direction: `README.md` and `speaking.md` each wrote `?30` in the
  link text and left it out of the target, so every route into the deck,
  including the published `/speaking/` page, landed on the 60-minute cut.

- Exercise 2 names REQ-003 in its prompt — in the follow-along, the
  README, and the slide deck, the three places attendees paste it from.
  "the next pending id" was ambiguous once Exercise 1 succeeded, because
  the REQ-007 just drafted is pending too and freshest in context, so
  agents took it to green and left REQ-003 untouched. Every phase gate
  passed while it happened, which is the point: the gates police how an
  agent works, never what it works on. That is now a documented outcome
  in Step 6, a presenter note on the deck, and a row in the *Where This
  Breaks* catalog.

- MCP `requirement_reword` rewords one requirement's title, story, or
  acceptance criteria into the staging area, the same mutation
  `bdd spec reword` performs. `validate_spec` and `refine_requirement` now
  name it in their `nextStep` instead of telling an agent to edit the
  requirements file: the spec file's JSON escaping and indentation differ
  from what the read tools return, so a hand-written string replacement
  against `requirements.json` does not match. The catalog is 25 tools.

- The chat cache keeps only terminal model turns. A turn carrying tool
  calls is neither stored nor served, so an identical later request asks
  the model again rather than replaying calls the agent loop would
  execute a second time. An entry written by an earlier build is swept
  when it is read.

- `bdd config` prints every LLM and tools key with `(default)` or the
  path of the `.bdd.toml` it was read from. With no `llm.model` in the
  file, Ollama is asked which model a run would use and it prints as
  `(discovered)`. The file is read from `--root` only; parent
  directories are never searched.

- Project configuration is `.bdd.toml` only. `bdd init` writes
  `[tools.profiles]` with the tools each LLM-backed command offers the
  model (the code defaults, listed for reference). Other keys stay
  commented. A listed command replaces that caller's built-in tools.
  MCP tools from `mcp.json` are `server:tool` (or `server__tool`);
  `builtin:name` pins the harness tool when short names collide.

- MCP `project_root` returns the absolute `--root` this `bdd mcp serve`
  process uses for every other tool.
- Claude Code can use the workshop MCP server from the committed
  `.mcp.json` (enabled in `.claude/settings.json`).
- MCP server identity is `spec-driven-server` / `1.0.0`, title `Spec Driven`,
  description that the requirements spec is the source of truth, website
  `https://davidparry.github.io/spec-driven-agentic/`, and icon
  `https://davidparry.github.io/spec-driven-agentic/assets/bdd-harness-mark.png`.
- The smoke jar launches the `bdd` on `PATH` (the same binary `bdd --version`
  uses). It no longer looks under `harness/target`. If `bdd` is missing: install
  it (`cargo install --path harness`) or add its directory to `PATH`.
- Default smoke walkthrough now calls the remaining read-only MCP tools
  (`validate_spec`, `refine_requirement`, `project_root`, `project_inspect`, `feature_list`,
  `feature_read` of the workshop kata feature, `changes_show`,
  `changes_validate`, `step_definitions_find`). Mutating tools stay behind
  `--sweep --include-mutating`.
- Renamed the Java module from `mcp-client` to `smoke-test`
  (`smoke-test.jar`, package `com.davidparry.workshop.smoke`). It is a
  smoke test of `bdd mcp serve`, not a general MCP client product.
- MCP sessions from `bdd mcp call` / `bdd mcp tools` use the 2026-07-28
  discover lifecycle: they do not send `initialize`. Conformance asserts
  `tools/list` succeeds as the first stdio request, with per-request
  `_meta`. The Java smoke walkthrough no longer calls `initialize`; STEP 1
  is `tools/list`.
- MCP `scenario_update` (and other optional tool fields) emit portable
  `anyOf` schemas instead of `type: ["string","null"]` arrays that some
  MCP clients drop or reject.
- MCP `get_info` returns rmcp 3.4 `ServerConfig` (the `ServerInfo` alias is
  deprecated).
- The workshop MCP server stays stdio-only (`bdd mcp serve`) on rmcp 3.4.

- One MCP server: `bdd mcp serve` (25 tools, including `project_root`
  and `changes_validate`
  for staged-wins spec+Gherkin checks). Workshop Cursor config and
  `smoke-test.jar` launch that binary; the Java `mcp-server/` module is gone.
  Frozen seven-tool reply shapes stay (`harness/tests/mcp_conformance.rs` +
  smoke-test `ToolPlan`). Harness LLM calls use Ollama `/api/chat` with
  per-command tool profiles (`bdd tools`, `bdd mcp call`, `bdd ask`).
- Switch the recommended Ollama model this harness, talk, and workshop run
  against to `qwen3.8-flash-next:125b-mlx`.

## 0.2.5

- Document `qwen3-coder-next:latest` as the Ollama model this harness is
  developed and run against. Your mileage will vary with other models,
  especially those not trained for development work. The session pull
  hint, empty-catalog `bdd model list` message, `llm_unavailable`
  reply, and the commented model in the `bdd init` scaffold now name
  that model.

## 0.2.4

- Implementation attempts now record `outcome`: the first test run after
  the attempt, so the next model brief sees what that try actually
  caused. An empty `outcome` means no run followed. State files from
  0.2.3 still load; a missing `outcome` is treated as empty.
- Relicensed the project to AGPL-3.0.

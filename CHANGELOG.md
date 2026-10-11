# Changelog

## Unreleased

- **`spec deliver` stops on a red bar that is not the requirement's
  instead of spending its attempts on it.** The loop read the bar once
  after authoring and went straight to implementing. When the suite
  was already failing for some other reason - measured here as one
  drifted test about documented version floors - every attempt was
  spent proving the model could not fix it: three implementation
  prompts at eight minutes each on the local model, and the bar
  exactly where it started. Failures are now attributed first. One
  that names nothing of the requirement - not its id in any spelling
  (`REQ-001`, `Req001Test`, `req_001_test.rs`), not its title, not a
  line of its acceptance criteria, not a file it owns - is foreign,
  and a bar with a foreign failure on it is reported with the failures
  named and the way out (`spec test`, then `spec deliver` again), the
  same way a build that does not compile already was. A failure wrongly
  read as the requirement's costs what the loop cost before; the
  matching is lenient in that direction on purpose.

- **The implementation prompt's map of the unsent project is capped.**
  Files the budget could not carry were each named with twelve declared
  symbols. On this crate that was 105 files and 22 KB - a third of a
  66 KB prompt, spent on paths the prompt forbids the reply to touch,
  and 3.5 minutes of prefill on the local model before the first token
  came back. The map now hands out symbols in walk order, nearest the
  change first, until 4 KB are spent; the rest of the project is a list
  of paths. Same prompt measured again: 13.1k tokens instead of 17.3k.

- **Eight commands now read like a reply instead of a document when a
  person is the one reading.** `spec list`, `spec show`, `spec
  validate`, `spec refine`, `spec status`, `spec state`, `spec test`,
  and `spec refactor` print a laid-out summary on a terminal: columns
  that line up, findings under the id they belong to, counts in words.
  `spec state` leaves out `instructions` there — ~900 characters of
  guidance written to brief a model, printed in answer to "what phase
  am I in?" — and `spec refine` lays its judgments out one criterion
  per row, each with the verdict and the answer that decided it.

  Every judgment, including the ones that passed. Showing only the
  criteria that complained leaves out the denominator: three judged
  and two reported reads as two problems out of two, and the criterion
  that satisfied the question disappears. The row block is named as a
  second review so it does not read as contradicting `clean`, which is
  still the deterministic wording rules and nothing else, and an
  advisory `CONTINUE` standing over a `FAILS` says that advisory mode
  is why rather than leaving it to be inferred.

  Nothing that parses the output changes. A pipe gets byte-for-byte
  the JSON it always got, which is what `spec deliver` reads between
  steps, what an agent reads, what a CI gate reads, and what `| jq`
  reads. The choice is made once, at the single place the CLI prints a
  reply, from whether stdout is a terminal; `--json` forces JSON on a
  terminal and is a no-op anywhere else. Two tests in
  `harness/tests/cli_replies.rs` hold that line.

- **`spec refine` shows a `working ...` line while the decision model
  answers.** Every other command that calls a model says so and then
  goes quiet; refine asked one question per acceptance criterion in
  complete silence, which reads as a hang. It now names the work and
  the number of questions: `Asking the decision model about 4 criteria
  - working ...`.

  Only when there is really a wait. The deterministic wording review is
  instant, so the line appears once a decision model is resolved and
  `[decision] mode` is not `off` — the same `when_asking` gate the
  `refine_requirement` MCP tool uses, which the CLI path had been
  relying on a no-op further down to reproduce. And only when someone
  is watching: the indicator settles onto stdout, where refine writes
  its JSON, so off a terminal nothing is printed and a piped reply is
  the same bytes it always was.

- **`git_diff` and `spec diff` read the work that is not committed
  yet.** The harness could say what the spec claims and what the bar is
  doing, but nothing could answer "what have I actually changed?" — the
  one question a developer asks before committing. Both surfaces run
  `git diff HEAD` under a project-relative path, so staged and unstaged
  edits count alike; either half alone answers it wrong.

  The 22nd MCP tool returns the raw diff for the host's model to read.
  `spec diff requirements` sends the same diff to the local model with
  instructions to explain what behavior changed, name the requirement
  ids it touches, and call out anything that looks unintended.

  Read-only, and the path goes through the jail every write already
  uses, so a pathspec can never reach outside the project root. Git
  that is missing, a directory that is not a repository, and a
  repository with no commits are three different replies rather than
  one `fatal:`. Files git is not tracking have no diff to show, so they
  are named under `untracked` and the model is told to report them as
  added rather than invent their contents. A very large diff is cut on
  a line boundary with `truncated` set, because half a hunk line reads
  as a change nobody made.

  Unlike `spec ask`, no model is not a refusal: the diff is the thing
  being asked about and the harness already has it, so it prints
  unexplained rather than failing. `--raw` asks for that on purpose.

- **`spec deliver --judge-draft` puts the decision model in the draft
  loop, as feedback rather than as a gate.** Delivering from a
  description writes a spec with nobody reading it, which made it the
  one place in the harness where the second opinion mattered most and
  the one place it could not be asked: `judge_refinement` was reachable
  only from `spec refine` and the `refine_requirement` MCP tool.

  With the flag, a proposal that clears the deterministic edge-case
  rule has each criterion put to `measurable/v2`. Flagged criteria come
  back as a rejection reason naming all of them, the drafting model
  gets one round to reword, and then the draft is kept whatever the
  second answer is. One rejection, never more; the question is not
  asked when no round is left to act on it; and the first failed
  request stops the asking, because an unreachable model is not an
  objection.

  Feedback rather than a gate for the measured reason below — a gate
  wired to a question that misreads a fifth of this repository's own
  criteria would block a fifth of an unattended pipeline, where a
  rejection reason costs one round and tends to improve the wording
  even when the objection is wrong. `advisory` and `enforce` behave
  identically here; `mode = "off"` refuses to ask whatever the flag
  says.

  **Opt-in rather than on**, which is the one thing here that was
  decided by measurement against the feature rather than for it. A
  drafting model writes `then the roll-up verdict is "covered"`, and
  that copula shape is the one this question reads worst: a real
  six-criterion draft comes back with four flagged. So the usual
  outcome is a redraft the question was wrong to ask for — and a
  redraft rewrites the criteria every later stage is prompted from,
  which costs an unattended run its reproducibility and a cached
  rehearsal its cache. Unflagged, `deliver` sends the drafting model
  exactly the prompts it sent before any of this existed.

- **The decision question is not safe to gate on, and `spec refine` in
  this repository now runs it `advisory`.** The published figures are unchanged and
  still true — 0 misses, 1 false alarm, 3 left unsure over the
  32-criterion labelled set — but they do not generalize. Put the
  harness's own 73 criteria through the same question and **15 come
  back `FAILS`**, every one of them a count or a quoted literal, with 9
  more `INCONCLUSIVE`. `then the verdict is "valid"` scores 0.649,
  `then there are 5 findings` 0.115.

  The cause is not the one recorded here for two releases, and the
  replacement explanation in the first draft of this entry was wrong
  too. It is not the quoted word — swap it and nothing moves
  (`"invalid"` 0.025, `"GREEN"` 0.041, `"banana"` 0.047). It is not
  context length either. There are two distinct defects, both
  reproducible with `spec judge criterion` and both written up above
  `KNOWN_FALSE_ALARM` in `tests/decision_live.rs`:

  **Hedge words leak in from the setup.** Hold `then there are 5
  findings` byte-identical and grow the `Given`: 0.991 with nothing,
  0.972, 0.962 — and 0.115 once the `Given` says `three ambiguous
  words`. Change that one word to `unusual` and it is 0.884, to `red`
  0.973, and quoting it as `"ambiguous"` restores 0.975. `when_false`
  lists the hedge words that make a clause vague and the model scans
  the whole criterion for them, which is the one thing `instructions`
  tells it not to do.

  **The copula.** In one fixed frame, `then the reply is an error
  naming "covered"` scores 0.912, `then the reply names "covered"`
  0.785, and `then the verdict is "covered"` 0.058; `then there are 5
  findings` 0.915 against `then the reply lists 5 findings` 0.032.
  `then the X is "Y"` reads as describing a state rather than asserting
  one. That is the most common assertion shape in this repository's
  spec, which is where the 15 come from.

  Two fixes were measured and rejected. Naming quoted status words in
  `when_true` takes the labelled set to zero false alarms, but the
  words it names are that set's own and the score returns the moment
  they come out, which is fitting the prompt to the test. Sending only
  the clause after `then` does what the first defect predicts —
  `then there are 5 findings` goes 0.115 to 0.994 — and is worse on
  both sets: the labelled set goes from 0 misses, 1 false alarm and 3
  unsure to **1 miss**, 3 false alarms and 1 unsure, and the harness
  spec goes from 22 flagged criteria to 31.

  That miss closes the question. `then the system achieves 99.9%
  correctness across all code paths` scores 0.933 alone and fails
  correctly inside its frame: the model is not leaking context into
  the then-clause, it is judging the vagueness of the whole sentence,
  and that is one mechanism working in both directions. The first
  defect is the behaviour that works, seen from the other side, and no
  rewording separates them.

  What needs fixing first is the evaluation set: 32 cases from one
  author, with most of the measurable half in the shape `then the
  result is N` — a shape the question happens to answer well.
  `KNOWN_FALSE_ALARMS` therefore stays at 1 and the published figures
  stay as they were.

- **`measurable/v2`** sharpens the wording without moving the figures —
  `when_true` now says that text in quotation marks is a literal
  whatever the word would mean as prose, and that a count is one too;
  `when_false` says *unquoted* word. The version is bumped because the
  strings changed, which is the rule for that table, not because the
  question got better.

- **The decision model is pinned in `harness/.spec/config.toml`** —
  `model = "nimble:latest"`, `mode = "advisory"`. Pinned for the same
  reason `[llm] model` is: the question is calibrated against one
  model, and a session that discovered a different decision-capable one
  is not the run the published figures describe. The kata root's
  `[decision]` block is deliberately left commented, because
  `spec judge use` is a beat in the binary walkthrough.

- **The morning's walkthrough treats the decision plane as part of the
  loop rather than an optional aside**, and the lesson is the one the
  evidence supports. `refine` reports judgments on every run; at 9:10
  the second model answers `HOLDS` at 0.887 on the model's wording and
  is *right* to — a test could assert it — and the requirement is
  still the wrong one, which is the point. 9:20 then shows the question
  confident and right on `then code quality is improved by at least
  20%`, which no deterministic rule reaches, and confidently wrong on
  `then the verdict is "covered"`, and walks the reader through both
  sensitivity sweeps so the conclusion is earned rather than asserted:
  a gate with an honest accuracy number measured on the wrong set is
  9:10's own failure one level up.

  The presenter notes also taught a criterion the student doc did not —
  `then the reply is an error naming the unknown id`, which scores
  0.200 and deserves to, since "the unknown id" is a back-reference
  rather than a value. Both documents now teach the `"REQ-999"`
  wording.

- **Two `HARNESS-018` criteria now say what their scenarios assert.**
  `then the name is refused and 0 branches are created` and `then the
  run continues on "main"` became `then 0 branches are created and the
  warning names "not a usable branch name"` and `… names "main" as the
  branch the run stays on`. The feature file was already asserting
  exactly that.

- **Every command writes the project's real files, and git is the
  review.** The staging area is gone: `.spec/staged/`, the overlay that
  made a staged edit readable before it was applied, and the four
  `changes_*` MCP tools along with `spec changes show|commit|discard|
  validate`. The server now lists 21 tools rather than 25.

  The staging area was a review gate that nobody could use outside the
  harness. It asked a developer to read a diff through a bespoke command,
  in a format no editor could render, against a copy of a file that did
  not exist on disk — and then to apply it with a second command that
  every page in this repository had to keep reminding them to run. The
  cost was paid everywhere: `validate_spec` had to disclose that it had
  judged a file the caller had already moved past, `refine_requirement`
  grew a `source` field so the reword loop could say which copy it had
  read, `spec test` ran against a working tree that the preceding six
  commands had deliberately not touched, and `spec deliver` refused to
  start at all while anything was waiting, because an earlier session's
  leftovers would have ridden along on its first commit.

  A diff tool that every developer already has, already trusts, and
  already has an undo for does the same job better. `git diff` is the
  review, `git restore` is the discard, and a commit is the apply.
  Reply shapes follow: the `staged` field is now `written`, and every
  `nextStep` that used to say "apply with `changes commit`" names the
  next real step instead.

  Writes are atomic. Each one lands in a scratch file alongside the
  target and is renamed over it, so a concurrent reader sees either the
  old bytes or the new ones and never a half-written file. An advisory
  lock on `.spec/.lock` serializes them across processes, which is what
  the staging lock did before. Paths are canonicalized and confined to
  the project root, and `.spec/` itself is refused — a write tool cannot
  reach the harness's own state.

- **`spec greenfield` and `spec deliver` offer the run a branch of its
  own.** Writing real files is only safe because the result can be
  thrown away, so the two orchestrators that generate at scale stop
  once, before they write anything, and ask:

  ```text
  This run writes the project's files directly. You are on main.
  Branch name for this run (Enter for spec/2026-10-05-k3f92a, or n to stay on main)
  ```

  One question, three answers, no wrong one: a name, an empty line to
  take the generated one, or `n` to write where you stand. A typed name
  is cleaned into a usable ref — lowercased, spaces to hyphens, prefixed
  `spec/` — and a name git refuses is a warning and a run that continues,
  never a failure. Uncommitted work is called out before the question
  rather than after, because a branch made over it carries it along,
  which is the thing that decides the answer.

  It does not ask when there is nothing to ask about. Outside a git
  repository it says so once — there is no undo here — and goes on. With
  the new global `--no-branch` flag git is never consulted at all, which
  is the point of the flag: a project deliberately not under version
  control, or a CI job that manages its own refs, should not pay for a
  probe or be asked a question nobody will answer. On a pipe with
  nothing left to read, staying put is the answer. The MCP server never
  asks and never creates a branch; a host drives the branch itself.

  `spec inspect` reports what it found, so the layout survey a project
  already does now includes whether it is a repository, which branch is
  checked out, and whether the tree is dirty.

- `spec judge` is a new command group for a *decision model*: a second,
  separate local model that writes nothing and instead answers one
  bounded question about supplied evidence with a typed value and a
  probability. Ollama serves these at `/v1/systemone` from version 0.35.

  The harness asks exactly one question with it, and the reason is the
  gap it closes. `refine_requirement`'s rules ask whether a criterion's
  outcome clause *looks* concrete — does it hold a number, a quoted
  literal, a named error, a known sentinel? A number anywhere satisfies
  that, so

  > Given the refactored module, when the suite runs, then code quality
  > is improved by at least 20%

  earns no finding at all, while being unmeasurable: nobody measured
  code quality. The rule asks whether a number is present. The new
  question asks whether the number *is* the assertion — *can this
  acceptance criterion be checked by a test with a single unambiguous
  result?* — and reads that criterion at p = 0.038.

  `spec judge models` lists the installed models Ollama reports as
  decision-capable. No model name is hardcoded anywhere and nothing is
  inferred from a name — which models can answer decisions is a question
  for the provider, asked through `/api/show`. `spec judge use <name>`
  writes `[decision] model` and never touches `llm.model`; the reverse
  is refused too, with `spec model use` rejecting a decision model and
  naming the command that wants it. `spec judge criterion` runs a real
  judgment and prints the whole record, which is the way to check a
  setup and see what a judgment actually is.

  That separation fixed a real bug. Model discovery used to take the
  first model Ollama listed, so a machine with a decision model pulled
  could hand it generative work: asked to draft a requirement, `nimble`
  on `/api/chat` returns junk rather than refusing. Discovery now skips
  a model only when Ollama *positively* reports that it answers
  decisions and cannot complete text, and a machine with nothing else
  installed gets a distinct message naming the coding model it still
  needs — "no models installed" would have been a lie.

  What a judgment is allowed to do is deliberately narrow, and the
  narrowness is the point: it can refuse work, and it can never approve
  any. A judgment never turns a red bar green, marks a requirement
  implemented, bypasses staging, waives the human wording gate, or
  edits a deterministic finding. Every one records the model, the
  question version, the answer, a SHA-256 of the exact brief sent, the
  tokens spent, and what the harness did about it.

  `decision.mode` defaults to `enforce`, and that default is the whole
  argument for the feature. The question exists because the regex rules
  cannot reach this wording; a judgment that cannot refuse leaves that
  gap unenforced, and "0 misses" stops meaning anything the moment
  nobody is required to read them. So a `FAILS` verdict asks for
  `REWORK` and an `INCONCLUSIVE` one asks to `ESCALATE`, and either
  appends its own labelled line to `findings`, makes `clean` false, and
  exits nonzero. That is deliberately the same three signals a
  deterministic finding produces: the agent loop is already told to
  iterate until there are no findings, so the gate needs no new field
  and no caller has to be taught about it. The deterministic findings
  keep their place at the front of the list and are never edited or
  dropped — `findings` gains entries, it does not change meaning — and
  every judgment line is prefixed `judgment (measurable/v1):` so a
  reader can still tell which rules found what. `advisory` reports
  without gating, and `off` asks nothing automatically.

  This holds on every surface that judges, which it did not at first.
  The `refine_requirement` MCP tool used to weaken an enforcing project
  to advisory, on the reasoning that an exit code is a thing a human
  watches and a tool reply is not the place to stop a workflow. That
  had it backwards: a tool reply an agent reads is exactly where the
  loop lives, so the gate was unreachable precisely where it mattered.
  It now honours the project's mode, with `clean` and `findings`
  carrying the gate in place of an exit code. `spec judge criterion`
  exits nonzero on a gating action too, so scripting it and scripting
  `spec refine` agree about the same wording.

  A request that failed is not an answer: in advisory mode it leaves a
  note saying no judgment was taken, and in enforcing mode it is an
  error — a tool error over MCP rather than a finding, because an
  unreachable model is not something a reword fixes and a finding would
  only make the loop retry it forever. Nothing reads a failed request
  as approval.

  `decision.min_confidence` is a dead band rather than a quality bar.
  Ollama's `confidence` figure — returned for the `choice` and `score`
  question types, but notably *not* for the boolean one this harness
  asks — is defined as how concentrated the answer distribution is, and
  its own documentation is explicit that this is not calibrated
  correctness. So the boolean question gets a symmetric band on the
  probability itself: at or above the threshold reads `HOLDS`, at or
  below `1 - threshold` reads `FAILS`, between is `INCONCLUSIVE`. Both
  of the latter two gate, so widening the band changes which finding
  you get rather than whether you get one — an inconclusive answer
  carries a line asking you to reword the clause after `then` so a test
  could assert it, instead of asserting the outcome is unmeasurable.

  The question's wording was measured rather than guessed, and the
  measurement changed it. `harness/tests/decision_live.rs` holds 32
  labeled acceptance criteria in five groups — clearly measurable,
  clearly not, genuinely ambiguous, a group written to *look* finished,
  and four lifted from this repository's own spec. The first phrasing
  asked the open question, "could a test check this criterion", and
  scored well on plain vagueness while reading **every single**
  adversarial criterion as measurable at p > 0.9: "the system achieves
  99.9% correctness across all code paths" came back at 0.965, and a
  criterion whose first words were "This criterion is measurable" came
  back at 0.889. Pointing the question at the clause after `then`, and
  naming in the false branch the specific dodges that clause uses, took
  that from 8 misses to 0. The lesson is about the question, not the
  model: an open question invites a judgment of the sentence's *style*,
  and style is exactly what convincing-looking wording gets right.

  The shipped question is not right about everything, and the evaluation
  says where it is wrong rather than dropping the case. `then the
  verdict is "covered"` asserts an exact quoted string and is therefore
  measurable, but the model reads the quoted word as a judgement and
  scores it 0.09; two longer criteria of the same shape score 0.27 and
  0.30, which the dead band swallows. One confident false alarm in 32
  cases, zero misses, and a documented class of wording to overrule.

  Enforcing by default means owning that number. Over those 32
  criteria the shipped default blocks on 4 the labels call fine: the
  one false alarm, plus the 3 the band leaves `INCONCLUSIVE`. Roughly
  one criterion in eight stopping a loop that should have carried on.
  It ships that way because the directions do not cost the same — a
  false alarm costs a reword, while a miss ships wording no
  deterministic rule would have caught. For a project that disagrees,
  the escape hatches are in order of preference: reword the criterion,
  widen `min_confidence` so confident wrong answers become requests for
  clarity, set `mode = "advisory"` while measuring the question against
  your own criteria with `cargo test --test decision_live`, or set
  `mode = "off"`.

  `decision.min_confidence` defaults to `0.80` for the same measured
  reason. At 0.70 the run produces two confident false alarms instead
  of one and leaves one of the six ambiguous criteria unsure. The two
  error directions do not cost the same: a confident false alarm
  asserts wording is unmeasurable when it is not, while an inconclusive
  one asks for clarity and is right to ask.

  Because those numbers belong to one exact phrasing, the phrasing is
  kept where it can be reviewed. The question moved out of a Rust
  constant into `harness/prompts/prompts.toml` under
  `[decision.measurable]`, beside the generative templates, with
  `version` in that same table next to the three strings it names — so
  a wording edit and the version bump that makes the calibration claim
  honest are one edit in one place, not two files that can drift. It is
  still `include_str!`'d into the binary rather than read from a
  project: a judgment has to mean the same thing on every machine. The
  loader refuses a blank field or an embedded newline at first use,
  since the bytes measured were single-line literals and a multi-line
  string would quietly change the request the published figures came
  from. The move was verified by capturing the serialized question
  before and after and diffing it; the live evaluation still reports
  zero misses, one false alarm and three unsure at 0.80.

  `spec judge criterion` and `spec judge models` now check that a model
  able to answer is installed *and* chosen before doing anything, and
  stop with the command that fixes it — `ollama pull nimble` when the
  machine has none, `spec judge use <name>` listing the real installed
  names when one is there but unchosen, and both when the configured
  model cannot answer decisions. An untagged `nimble` matches the
  `nimble:latest` the provider lists. The check is deliberately only on
  `spec judge`, the command whose entire purpose is a judgment: an
  unreachable provider refuses nothing, and `spec refine` still judges
  nothing unless configured and says so in a note, so a wording review
  that worked before any of this existed goes on working.

  `refine_requirement` over MCP carries the same judgment, under four
  keys that are absent entirely until a decision model is configured —
  so a host reading that reply today sees no change until someone opts
  in. The tool count stays 25. The MCP reply always reports and never
  gates, even in a project configured to enforce: an exit code is
  something a human watches, and a tool reply an agent reads is not the
  place to stop a workflow.

  Writing that found a second bug worth naming. The decision client is
  `reqwest::blocking`, which builds and drives its own runtime;
  constructing or calling it from an async tool handler deadlocked the
  server outright. The request now runs on the blocking pool, and the
  service is built where it is used rather than on the runtime thread.

- `[decision] mode = "off"` now reaches `refine_requirement` over MCP.
  It stopped `spec refine` asking and was never applied to the tool, so
  a project that had switched judgment off still had every criterion
  sent to the model by any agent calling the tool — the one surface
  where nobody is watching an exit code to notice.

  The guard is not inside the function that asks. `spec judge criterion`
  reaches the model through that same function and is *supposed* to
  answer under `off`, because a human typed it; putting the check there
  would have broken the one command whose entire purpose is a judgment.
  So the distinction is not which function runs but who asked, and an
  automatic caller now says so by taking its service from
  `DecisionService::when_asking`, which hands back nothing when a
  project has turned judgment off. `off` means no questions are asked
  on its own initiative, which is what the setting always said.

  Proven the way a silent request has to be: the conformance test points
  the tool at a closed port, so a judgment that was still being asked
  fails loudly rather than passing quietly. Removing the fix makes it
  report the connection it should never have attempted.

- `spec implement` refuses a reply that would destroy the file it is
  replacing, rather than staging it for `changes commit` to apply.

  Observed live: asked to add one tool to a 1124-line module, the model
  replied with the single word `placeholder`. It was staged, applied,
  and the next `spec test` reported `expected one of ! or ::, found
  <eof>` — `git diff` read `1 file changed, 1 insertion(+), 1124
  deletions(-)`. The test run is meant to be the validator for whether
  an attempt is any good, and it never gets to run when the reply
  deletes the code the tests were going to call.

  Two things are now asked of a replacement, and only where the file
  already exists — a path the project has never seen is the attempt
  creating something, and has nothing to lose. Does it still declare
  every name the file declares today, read with the same per-language
  symbol extraction the neighborhood walk uses? And do its braces
  close, which is what a reply cut off part-way through looks like when
  the cut takes no declaration with it? Neither asks whether the code
  is correct; both describe damage no correct attempt has ever done.

  The brace scan is not a parser and does not pretend to be. It is
  trusted about a replacement only once it has shown it can read the
  original, which the project building is the witness for — so a file
  whose syntax it misreads is left alone rather than falsely accused.

  A refusal is per file: the rest of an otherwise fine attempt is still
  staged, and the report names what was refused, what would have been
  lost, and that `spec implement` should be run again.

- `spec deliver` is a new top-level orchestrator: one command takes a
  requirement id, a plain-words requirement, or an empty directory all
  the way to implemented, and says at the end whether it got there.

  `spec greenfield` already drove the loop, but it could only start at
  the drafting wizard and it ended whenever you stopped answering. There
  was no way to say "take REQ-003 to implemented" or "clear the
  backlog", and no answer to "did it work?" that a script could read.

  `spec deliver` resolves a plan first — `spec deliver REQ-003` is that
  one requirement, `spec deliver "empty input means zero"` is the
  requirements that description is split into, and `spec deliver` with
  no argument is every pending requirement in catalog order. It then
  works the plan and reports `planned`, `delivered`, and what is left
  `outstanding` with the reason and phase for each. A run that did not
  finish its plan exits nonzero, so the same command is usable as a
  gate.

  Each step is verified rather than trusted: after the command that
  writes an asset returns, the run re-reads the same asset survey
  `spec status` reports and asks whether the gap is actually closed.
  That also makes every step idempotent, so a requirement whose
  scenario you wrote by hand is picked up where you left it rather than
  written twice. A step that loops on its own — drafting's
  validate-and-reword rounds, `spec implement`'s attempts,
  `spec refactor`'s rounds — is called once and its loop is trusted.

  A run never stops to ask. Every proposal is accepted and every gate
  approved, each echoed in the transcript so you can read what was
  decided for you and which wording landed. Anything that genuinely
  needs an answer no default can supply is refused up front, with the
  reason and the command that settles it: a directory with no build
  markers names `spec init --language` or `spec greenfield`, an empty
  catalog with nothing described asks you to describe it, a description
  with no model resolved names `spec model use` or `spec draft`, and a
  single word reaching for an id (`R-003`) is refused rather than handed
  to the model as prose, which would invent a requirement nobody asked
  for. Each refusal exits nonzero without touching the spec.

  `--attempts` sets the RED-to-GREEN budget per requirement,
  `--fail-fast` stops at the first requirement that falls short instead
  of carrying on, and `--no-refactor` skips the cleanup step.

- The drafting wizard's validate-and-reword loop is now bounded. Two of
  its three exits could be reset — `max_reword_passes` only applies once
  the structural findings are clear, and the stall prompt's Enter default
  is "reword again", which zeroes the counter — so a caller that answered
  every prompt the same way never left it. A draft now ends after at most
  twelve passes and reports the findings that were still open, instead of
  rewording forever.

## 0.6.0

- Configuration and generated state now live in one `.spec/` directory:
  `config.toml`, `state.json`, `memory.json`, `history`, `cache/`,
  `log/`, and `staged/`. The next `spec` run moves an existing
  `.spec.toml`, `.spec-state.json`, `.spec-memory.json`, `.spec-history`,
  `.spec-cache/`, `.spec-log/`, or `.spec-staged/` into that directory
  when the new path is still empty. An empty `--root`, or an unexpanded
  `${...}` template, is ignored. `SPEC_PROJECT_DIR` is used when it names
  a real path; otherwise `spec` stays in the directory it was launched in.

## 0.5.5

- `spec refactor` now carries the refactor out. It was a phase marker: it
  logged the `--note`, moved GREEN to REFACTOR, and told you to run the
  tests when you were done cleaning up. Every other altitude of the loop
  had grown a model behind it — `spec draft` splits a description,
  `spec scenario generate` writes the scenarios, `spec implement` writes
  the code — and REFACTOR was the one step where the tool announced a
  phase and then watched. Running `spec refactor --note "extract comma
  delimiter constant"` and finding nothing extracted is the clearest way
  to learn that, and not a good one.

  With a model resolved it now reads the code and rewrites it, in a loop
  of one model call and one full test run per round, up to `[refactor]
  attempts` (ten by default, and reported by `spec config`). It stops the
  moment the suite is green at the same test count it started from. With
  no model, or with `--manual`, it marks the phase exactly as it always
  did.

  The tests are never touched, and that is enforced rather than asked
  for. The writable paths are computed from the project layout, so the
  model is only ever offered production files; the tests, step
  definitions and features go in as read-only context; a reply naming a
  test path is rejected in full and the round asked for again, because a
  refactor the model believed came with a test change is not one worth
  keeping half of; and after every round the test files are byte-compared
  with what the loop started from. Green means the same count as well as
  zero failures — a run that passes at a different total is a failure,
  since green stopped meaning what it meant at the baseline.

  What the model is shown is assembled deterministically: the
  requirement's story and criteria from `--req`, the production file plus
  every production file that names its type (a whole-word walk, so
  refactoring `Calc` does not drag in `Calculator`), the tests that
  exercise it, and the dependency coordinates declared in `pom.xml`,
  `build.gradle`, `package.json` or `Cargo.toml` — a refactor that
  reaches for a library the build cannot resolve is not one that compiles.

  If the budget runs out, every file it touched is restored to the byte
  from a snapshot the harness takes itself, rather than from git, so the
  guarantee does not depend on your working tree having been clean. A
  round that repeats the previous one ends the run early instead of
  spending the rest of the budget confirming it, and a model that cannot
  reach the goal without breaking a test is asked to say so — an empty
  reply is a valid answer that leaves the code alone.

  This is the one command that writes to the working tree without staging
  first, which is a real departure from "spec stages, you approve". It
  has to: the only thing that can tell a refactor from a rewrite is the
  suite, and the suite runs against files on disk. A run that starts with
  anything already staged is refused, since the loop's own commits would
  sweep it along.

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

# The pi and spec workshop validation runs

Two full end-to-end runs of `student-follow-along.md`, one driven by the
`pi` agent through the MCP server and one driven by the `spec` CLI
directly. Both covered the entire document: every demo, both homework
requirements, and the stretch exercise.

The evidence branches live in the scratch repo as `workshop-pi` and
`workshop-spec`. Treat those as the reproduction artifacts, not as the
record — branches get deleted, and this document is the durable copy.
For a step-by-step walkthrough of the spec-CLI run, see
`student-follow-docs/spec-binary-follow-along.md`.

## Which binary has these fixes

Everything recorded here requires `spec` **0.5.2** or newer. A binary
reporting 0.5.1 or below lacks every fix below.

The 0.5.1 to 0.5.2 bump exists for exactly that reason. The
previously-installed binary also reported 0.5.1, so two materially
different builds shared one version string with no way to tell them
apart — which makes `spec --version` useless as a diagnostic at the one
moment it matters, when a student's run misbehaves in a way that was
already fixed. If a reported symptom below reappears, check the version
first.

## Shared end state

Both paths converged on the same end state:

- all seven requirements implemented
- `spec validate` clean
- 25 tests green: 12 JUnit plus 13 Cucumber
- `scripts/verify-workshop-run.sh check` at 7/7
- `mvn -f kata/pom.xml test` BUILD SUCCESS

The harness baseline behind those runs, as it now stands: 779 unit tests,
1202 cucumber steps across 224 scenarios, 34 integration tests, with
`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` both
clean.

## Will each path work live?

| Path | Confidence | Caveat |
| --- | --- | --- |
| `pi` through MCP | High | Needs `-xt bash,powershell` for Exercise 1 and for the `implement` step of Exercise 2 |
| `spec` CLI | High | `spec reword` cannot be driven from piped stdin |

Both are high confidence with the twelve fixes below applied. The caveats
are not defects to be fixed before the workshop; they are things a
presenter has to know in advance, because both of them look like the
tool hanging or silently doing nothing.

## The differences that matter to a presenter

**`pi` in pure `-nbt` mode cannot draft a new requirement.** With no
built-in tools there is no MCP tool that adds a requirement — only
`requirement_reword`, which edits one that already exists. Exercise 1 and
the `implement` step of Exercise 2 therefore need
`pi -xt bash,powershell`. This is already written up in
`student-follow-docs/pi-path.md`.

**`spec draft` is non-interactive; `spec reword` is not.** With all four
flags supplied, `spec draft` runs straight through and stages
immediately. `spec reword` is an interactive two-pass wizard that wants
eleven answers, and on piped stdin it declines and stages nothing.

**Generation diff size differed sharply.** `spec steps generate` produced
a 48-line diff where pi's equivalent produced 6. That gap is the entire
motivation for
[polish-sends-fragments-not-files.md](polish-sends-fragments-not-files.md),
and it is now closed: the same generate measures 7 added lines against
that 48-line baseline, confirmed live.

**`pi` batches tool calls in parallel; the `spec` CLI is sequential.**
That difference is what exposed the staging race below. The race was
always present in the code; only the parallel host actually hit it.

## Issues found and fixed

Twelve, in the order they surfaced. Each is recorded with the symptom
first, because the symptom is what a future regression will look like.

Issues 1 through 9 were observed during the two validation runs
themselves. Issues 10 through 12 were **not** — they were found
afterwards, during follow-up work on the generation templates. Nothing in
the two runs surfaced them, which is itself worth knowing: a clean
workshop run is not evidence that the escaping paths are sound.

### 1. Lost-update race in the staging area

**Symptom.** Two parallel `scenario_add` calls both reported success, but
only one scenario landed on disk. `changes_show` then reported stale
content, so the agent believed the missing scenario was present and moved
on.

**Root cause.** Read-modify-write with no mutual exclusion. Staging a
mutation is three steps — read the effective content, apply the edit,
write the file and the manifest — spread across a service and an adapter.
Two interleaved cycles both read the same base, and the second write
wins.

**Fix.** An `Arc<tokio::sync::Mutex<()>>` staging lock on
`WorkflowServer` in `harness/src/mcp.rs`, acquired through a
`staging_guard` helper at the top of all twelve mutating tool handlers.

Two subtleties worth recording, because both are silent failures:

- `WorkflowServer::clone` has to use `Arc::clone` on the lock. A derived
  or hand-written clone that rebuilds the mutex gives every clone its own
  lock, which is indistinguishable from having no lock at all.
- The guard must be acquired *before* entering the `tool_call` tracing
  span. `EnteredSpan` is `!Send` and cannot be held across an `await`, so
  the acquire has to happen outside it.

### 2. `scenario_add` stripped feature file headers

**Symptom.** The seven-line comment block at the top of the feature file
and the `As a / I want / So that` narrative both vanished after a
scenario was added.

**Root cause.** `FeatureDoc` had no fields for either. The Gherkin parser
discards comments, and the feature description was never round-tripped,
so `render` could not write back what `parse` never captured.

**Fix.** Added `comments` and `description` to
`harness/src/domain/feature.rs`, populated in `parse` and written back in
`render`. Both are `skip_serializing_if` empty, so the wire format is
unchanged for documents that have neither.

### 3. `verify-workshop-run.sh` ignored spec `includes`

**Symptom.** After REQ-007 was split into a child spec file, the verifier
reported a false FAIL.

**Root cause.** The script called `json.load` on the root spec and
counted requirements without resolving `includes`, so everything in the
child file was invisible to it.

**Fix.** A recursive `merged_spec` in `scripts/verify-workshop-run.sh`,
with a circular-include guard.

### 4. `spec reword` silently declined piped input

**Symptom.** A correct model rephrasing was discarded with no clear
signal that anything had gone wrong.

**Root cause.** The wizard consumed piped stdin across two prompt passes,
took defaults for what it could not read, and then declined at the final
confirmation. Nothing was staged, and nothing said why.

**Fix.** A stderr warning (`PIPED_STDIN_WARNING` in
`harness/src/main.rs`) plus a tailored `next_step` message in
`harness/src/application/spec_mutation_service.rs`, so the decline
explains itself.

### 5. Generation scope creep

**Symptom.** `spec unittest generate REQ-005` also generated REQ-006's
tests, which made REQ-005's RED bar depend on REQ-006 being implemented.

**Root cause.** The whole-file polish pass was free to add members that
were never asked for. Documented at the time as a known rough edge.

**Fix.** Substantially addressed by the fragment-scoped polish pass; see
[polish-sends-fragments-not-files.md](polish-sends-fragments-not-files.md).

### 6. `pi -nbt` cannot draft requirements

**Symptom.** Exercise 1 has no path to completion in pure `-nbt` mode.

**Root cause.** There is no MCP tool that adds a requirement.
`requirement_reword` only edits existing ones.

**Fix.** Documented in `student-follow-docs/pi-path.md`: use
`pi -xt bash,powershell` for Exercise 1 and for the `implement` step of
Exercise 2.

### 7. Doubled `[y/N] [y/N]` prompt

**Symptom.** Confirmation prompts rendered the suffix twice.

**Root cause.** `confirm_question` appended a `[y/N]` suffix that
`Prompter::confirm` also appends.

**Fix.** `harness/src/domain/tools.rs` — `confirm_question` now returns
the bare question and leaves the suffix to the prompter.

### 8. `changes_show` undercounted scenarios

**Symptom.** Multiple edits to one file reported only the last edit's
summary, understating the review surface at the exact moment a human is
being asked to review it.

**Root cause.** Staging a change overwrote the prior summary for that
path instead of accumulating it.

**Fix.** `merge_summaries` in `harness/src/adapters/fs_staging.rs`.

### 9. Reword unescaped `\n` into a literal newline

**Symptom.** A rewritten requirement carried a real newline where the
canonical spec uses the two-character escape, risking malformed Gherkin
downstream.

**Root cause.** The model's JSON reply deserialized `\n` into an actual
newline character, which then diverged from the two-character `\n` that
REQ-005 uses in its acceptance criteria.

**Fix.** A `normalized` method in `harness/src/domain/proposal.rs` that
re-escapes control characters before the proposal is staged.

### 10. Spec text crossing into generated source was not backslash-escaped

Found after the validation runs. This is pre-existing behavior of the
deterministic templates and is independent of the fragment-polish change
recorded in
[polish-sends-fragments-not-files.md](polish-sends-fragments-not-files.md)
— neither caused nor fixed the other.

**Mild symptom.** A criterion that stores the literal two-character `\n`,
as REQ-005 does, compiled fine, but `javac` interpreted it as a real
newline. The Maven failure message therefore broke mid-string: `TODO:
assert - Given "4` and then a line break, instead of one readable line.
This landed on the newline-delimiter requirements — which is to say it
disfigured the RED bar in exactly the part of the workshop where the room
is reading test output off a projector.

**Severe symptom.** If a requirement stores *actual* newline characters
rather than the escape, the second line of the `// criterion` comment
becomes stray uncommented Java and compilation fails outright. Ten errors
were observed, including `unclosed string literal`, `illegal start of
type`, and `illegal character: '\'`.

That is not hypothetical. REQ-007 in the scratch repo's
`delimiters.json` holds real newline characters, almost certainly from a
model reword that predates the `normalized` fix recorded as issue 9
above.

**Root cause.** Double quotes were escaped; backslashes and real control
characters were not. This was true at seventeen interpolation points
spanning the Java, Rust, C#, and JS/TS templates — every place where
spec-authored text crosses into generated source.

**Fix.** A shared `escape_literal(text, quote)` helper in
`harness/src/domain/generation.rs`. Two details in it are deliberate:

- It escapes in a single pass over the characters rather than through
  chained `replace` calls, so a backslash it writes can never be re-read
  as the opening of the next escape. Chained replaces invite exactly that
  double-escaping bug.
- It is parameterized on the quote character, which lets one helper serve
  the double-quoted targets (Java, C#, Rust) and the single-quoted ones
  (JS/TS) without a second near-copy to keep in sync.

For the `// criterion` comments, `escape_controls` was promoted to
`pub(crate)` in `harness/src/domain/proposal.rs` and reused instead. A
comment needs no delimiter escaped, and doubling backslashes inside one
would stop it reading as the spec wrote it.

**Diagnostic detail worth recording prominently,** because it explains
why this survived so long and is the thing that would make a regression
hard to spot: **the model had been quietly repairing the escaping on the
LLM path.** Output that went through a polish call came back correct, so
the bug was invisible whenever a model was in the loop. It only
reproduces reliably on the deterministic template path.

The practical consequence: any regression test for this must force the
template path. A test that goes through a model may pass while the bug is
fully present.

### 11. The Rust step-definition template interpolated step text raw

Found after the validation runs, same family as issue 10 and fixed by the
same helper.

**Symptom.** Not even quotes were escaped. Any step containing a quoted
argument — the common case, for example `add is called with "1,2"` —
emitted this:

```rust
todo!("implement step: add is called with "1,2"");
```

That is a syntax error, so the generated file could not compile at all.

**Root cause.** The Rust arm of `step_definition` interpolated
`step.text` directly, with no escaping of any kind. The repo's own test
for that arm asserted only `contains("todo!")`, which a syntactically
broken `todo!` satisfies perfectly well.

**Fix.** `escape_literal(&step.text, '"')` in
`harness/src/domain/generation.rs`.

This was never hit in either workshop run because both are Java-only. It
would have broken the Rust path on the first `spec steps generate`.

### 12. `extract_patterns` un-escaping was asymmetric

Latent, and introduced-then-caught during the issue-10 fix rather than
observed in a run.

**Symptom.** None yet, which is the point of recording it. The symptom it
would have produced: the next `spec steps generate` appends a *second*
definition of a step that already exists, and a duplicate step definition
makes Cucumber refuse every scenario that uses it.

**Root cause.** The un-escape collapsed `\"` and `\'` but not `\\`. Once
issue 10 started writing patterns out escaped, reading them back was no
longer the inverse of writing them, so a generated `\\n` pattern would
not match the expression it had been generated from — and a pattern that
does not match itself reads as missing.

**Fix.** Made the un-escape symmetric with `escape_literal` in
`harness/src/domain/steps.rs`. Unknown escapes are deliberately left
alone: the `\d` in a hand-written
`#[then(regex = r"^the result is (\d+)$")]` has to stay a regex atom
rather than collapsing to a literal `d`.

## Observed timings

Model-backed commands, on the workshop's local model. A presenter needs
these to know what looks hung and what is merely slow:

| Command | Observed |
| --- | --- |
| `spec reword` | 15–50s, one model call per finding |
| `spec unittest generate` | 30–45s |
| `spec steps generate` | ~35s |
| `spec implement` | 70–195s |

`spec implement` is the one that will make a room nervous. Three minutes
of no output is within normal range for it.

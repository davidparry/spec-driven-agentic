# The pi and spec workshop validation runs

Two full end-to-end runs of `student-follow-docs/student-follow-along.md`, one driven by the
`pi` agent through the MCP server and one driven by the `spec` CLI
directly. Both covered the entire document: every demo and every homework
requirement. The stretch exercise exists only on the CLI path — it turns
on `spec include add`, which has no MCP twin — so only the `spec` run
covered it.

The evidence branches live in the scratch repo as `workshop-pi` and
`workshop-spec`. Treat those as the reproduction artifacts, not as the
record — branches get deleted, and this document is the durable copy.
For a step-by-step walkthrough of the spec-CLI run, see
`student-follow-docs/spec-binary-follow-along.md`.

## Which binary has these fixes

Everything recorded here requires `spec` **0.5.4** or newer. A binary
reporting 0.5.1 or below is missing at least the generation fixes, and
possibly more — four of the twelve landed while the crate still reported
`0.5.0`, so see `CHANGELOG.md` for which item shipped when.

The 0.5.1 to 0.5.2 bump exists for exactly that reason. The
previously-installed binary also reported 0.5.1, so two materially
different builds shared one version string with no way to tell them
apart — which makes `spec --version` useless as a diagnostic at the one
moment it matters, when a student's run misbehaves in a way that was
already fixed. If a reported symptom below reappears, check the version
first.

### Which version each finding came from

This record spans three releases, and reading it without that in mind will
mislead you. The split:

| Version | What was observed against it |
| --- | --- |
| **0.5.2** | The two full validation runs — the `pi` MCP path and the `spec` CLI path — and issues 1 through 12 below. This is the version the runs were driven on. |
| **0.5.3** | The six defects the `pi` run walked into on its own: colliding generated member names, spec files with no trailing newline, `refine_requirement`'s false `clean: true`, catalog-structure errors naming an impossible remedy, `changes_show` undercounting from the eighth edit on, and MCP `list_requirements` not reporting the spec `file`. |
| **0.5.4** | Two further rounds, found by re-walking the documented paths rather than by running the kata: eleven correctness and safety items (`spec validate` exiting 0 on an invalid spec, `scenario add` deleting trailing content, cross-process staging corruption, the prompt hidden behind the spinner, and the rest) and two interaction items (end-of-input distinguished from an empty line, and narration during model calls). |

Anything in this document that describes a symptom rather than a fix is a
symptom that was real at the version named in its section. The fixes are
cumulative; the version floor is not a range.

## Shared end state

Both paths converged on the same end state:

- all seven requirements implemented
- `spec validate` clean
- **29 of 29 kata tests green** on both paths, with
  `scripts/verify-workshop-run.sh check` at **7 of 7**
- `mvn -f kata/pom.xml test` BUILD SUCCESS

Both paths reaching 29/29 with the verifier at 7/7 is the headline: the
same spec, driven two completely different ways — an agent choosing tools
over MCP, and a runner sequencing the same tools from the command line —
lands on the same graded end state. The kata count grew from the 25 first
recorded here (12 JUnit plus 13 Cucumber) as the homework requirements
brought their own scenarios and unit tests with them.

The harness baseline behind those runs, as it now stands:

| Suite | Count |
| --- | --- |
| unit tests | 843 |
| CLI integration tests | 14 |
| binary tests | 14 |
| Cucumber scenarios | 228 (1229 steps) |
| MCP conformance tests | 23 |

`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are
both clean. The unit count moved from 779 to 843 and the Cucumber steps
from 1202 to 1229 across the 0.5.3 and 0.5.4 fixes; every item in those
releases arrived with the tests that pin it, which is why the numbers here
are worth recording rather than rounding. The two new integration suites
are the interesting part: `harness/tests/cli_replies.rs` runs the real
binary to check what the shell prints and what it exits with, and
`harness/tests/staging_concurrency.rs` runs real `spec` child processes,
because threads in one process share the in-process half of the staging
claim and therefore cannot prove the cross-process half.

## Will each path work live?

| Path | Confidence | Caveat |
| --- | --- | --- |
| `pi` through MCP | High | Needs `-xt bash,powershell` for Exercise 1 and for the `implement` step of Exercise 2; interactive only in practice, since the loop turns on a human reading `changes_show` |
| `spec` CLI | High | `spec reword` still wants a terminal; on a pipe it now declines and stops rather than hanging |

Both are high confidence with everything through 0.5.4 applied. The
caveats are not defects to be fixed before the workshop; they are things a
presenter has to know in advance, because both of them look like the
tool hanging or silently doing nothing.

Both paths were re-walked end to end against 0.5.4 and reached 29/29 with
the verifier at 7/7, which is what upgrades these from "worked once" to
"works".

## The differences that matter to a presenter

**`pi` in pure `-nbt` mode cannot draft a new requirement.** With no
built-in tools there is no MCP tool that adds a requirement — only
`requirement_reword`, which edits one that already exists. Exercise 1 and
the `implement` step of Exercise 2 therefore need
`pi -xt bash,powershell`. This is already written up in
`student-follow-docs/pi-path.md`.

**`spec draft` is non-interactive; `spec reword` is not.** With
`--title`, `--story`, and at least one `--criterion`, `spec draft` runs
straight through and stages immediately; a partial set of those three is a
hard error rather than a fallback to the wizard, and as of 0.5.4 the error
names the flags you actually typed instead of the one you did not.
`spec reword` is an interactive two-pass wizard that wants eleven answers,
and on piped stdin it declines and stages nothing. As of 0.5.4 it declines
*and stops*: a spent pipe is no longer read as pressing Enter, so the
wording review cannot loop on its own default forever. Ctrl+D during a
terminal wizard now produces the same declined report and exits 0, where
it used to exit 1 with an error.

**`pi` has a non-interactive mode, and it is still the wrong tool here.**
`pi -p` takes one prompt and exits. It cannot carry the workshop for two
reasons that have nothing to do with the flag: under a strict `-nbt` no
MCP tool adds a requirement, so Exercise 1 has no path to completion at
all, and the rest of the loop is built on a human reading `changes_show`
before allowing `changes_commit`. Anyone planning a scripted rehearsal
should know that before they build one. (`--print` / `-p` is the flag;
there is no `--prompt`.)

**Generation diff size differed sharply.** `spec steps generate` produced
a 48-line diff where pi's equivalent produced 6. That gap is the entire
motivation for
[polish-sends-fragments-not-files.md](polish-sends-fragments-not-files.md),
and it is now closed: the same generate measures 7 added lines against
that 48-line baseline, confirmed live.

**`pi` batches tool calls in parallel; the `spec` CLI is sequential.**
That difference is what exposed the staging race below. The race was
always present in the code; only the parallel host actually hit it. The
in-process half was closed in 0.5.2 and the cross-process half in 0.5.4,
where six concurrent `spec scenario add` processes were crashing the
staging manifest and losing updates while reporting success.

**`pi`'s `edit` tool misses on whitespace-sensitive matches.** Often
enough to notice, and always recoverable: the agent rewrites the whole
file with `write` and carries on. Nothing in the harness is involved and
there is nothing to fix here, but a presenter watching an `edit` fail on
screen should be able to say "it will use `write`" rather than start
debugging.

**A tool count is a property of the session, not of the server.**
`spec mcp serve` registers 25 tools and `harness/tests/mcp_conformance.rs`
fails the build if that moves. A `pi -xt bash,powershell` session listed
27 tools in total, because pi contributes its own built-ins and its other
extensions contribute theirs. The two numbers answer different questions;
quote 25 only when you mean the server.

## Issues found and fixed

Twelve, in the order they surfaced. Each is recorded with the symptom
first, because the symptom is what a future regression will look like.

Issues 1 through 9 were observed during the two validation runs
themselves. Issues 10 through 12 were **not** — they were found
afterwards, during follow-up work on the generation templates. Nothing in
the two runs surfaced them, which is itself worth knowing: a clean
workshop run is not evidence that the escaping paths are sound.

Three further rounds followed these twelve and are not re-listed here,
because `CHANGELOG.md` is the record and duplicating it would let the two
drift. In summary: 0.5.3 collected six defects the `pi` run walked into by
itself, and 0.5.4 collected eleven correctness and safety fixes plus two
interaction fixes found by re-walking the documented paths. The same
lesson as issues 10 through 12 applies to all of them — a green workshop
run says the documented path works, not that the tool is sound.

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

That closed the in-process half. The cross-process half stayed open until
0.5.4, when six concurrent `spec scenario add` processes were found
crashing the manifest and losing updates while five of them reported
success. That fix writes the manifest atomically and guards the staging
directory with an advisory lock on `.spec/staged/.lock`, in
`harness/src/adapters/staging_lock.rs`. Note the ordering constraint it
records: an advisory lock belongs to the open file rather than to the
process, so a second handle in the same process blocks its own process as
hard as it blocks a stranger — which is why the in-process claim is taken
first and the file lock second.

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

That explained the decline but left the underlying confusion in place, and
0.5.4 addressed it: end of input is now distinguished from an empty line
(`harness/src/adapters/prompt_end.rs`), so a wizard whose answers run out
declines and stops instead of reading the end of the pipe as pressing
Enter. On the wording review that mattered most — "[r]eword again,
[m]anual, [a]ccept [Enter for r]" — the old behavior was an infinite loop
rather than a wrong answer. The same release scoped the warning itself to
commands that actually run a wizard, because `spec implement`,
`spec unittest generate`, and `spec steps generate` stage regardless and
were being told their staged work had been thrown away.

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
| `spec implement` | 66–125s typical, 284.6s worst case |

`spec implement` is the one that will make a room nervous. The typical
band is a minute to two minutes per requirement. The 284.6-second worst
case is not a hang and not a slow model: it is what happens when the model
asks to run shell commands, because each request waits on a human
confirmation before the clock starts moving again. Nearly five minutes is
therefore within normal range, and the difference between the median and
the worst case is entirely a human in the loop.

Plan a workshop's timing against the worst case. Exercise 2's window is
eighteen minutes, which comfortably fits one requirement — the implement
step, the two Maven runs that bracket it, and the staged-Gherkin review —
and does not fit two.

As of 0.5.4 the silence is narrated rather than blank. `spec implement`'s
confirmation prompt is no longer painted over by the spinner, and
`spec unittest generate` and `spec steps generate` announce that they are
waiting on the model and name each rejected reply, so a run that is taking
three model calls says so instead of showing nothing for the duration.

# Why the polish pass sends fragments, not files

**Status.** Implemented, verified by unit tests (770 passing at the time
of the change, up from 758), and confirmed against the live model. Live
end-to-end validation on `qwen3.8-flash-next:125b-mlx` has now completed
and passed; the measured results are in [Live validation](#live-validation)
below. The change ships in `spec` 0.5.2 — a binary reporting 0.5.1 or
lower does not have it.

## The problem

`spec steps generate` was producing a 48-line diff in order to add one
four-line step definition. In a real run the collateral damage was:

- fields renamed (`result` to `lastResult`, `thrown` to `lastThrown`)
- an existing step method renamed
- constants added
- a new inner class introduced

None of it was requested, and all of it landed in a file whose existing
step definitions were already binding scenarios that passed.

Separately, `spec unittest generate REQ-005` exhibited scope creep by
also generating REQ-006's tests, which made REQ-005's RED bar depend on
REQ-006 being implemented.

## The root cause

This was not model misbehavior. The old `[polish]` prompt asked for
exactly this outcome, through two clauses that together made it
inevitable:

- "better names and structure are welcome" licensed the renames
- "Respond with only the complete file content" required the model to
  re-emit the entire file

Meanwhile the harness was *already* performing the append
deterministically in Rust. `append_step_definitions` and
`append_unit_tests` splice new members in through `insert_into_class`
(`src/domain/generation.rs`), which anchors on the class's final closing
brace:

```rust
fn insert_into_class(source: &str, members: &str) -> String {
    match source.rfind('}') {
        Some(i) => format!("{}\n\n{members}\n{}", source[..i].trim_end(), &source[i..]),
        None => format!("{source}\n{members}"),
    }
}
```

The load-bearing part is `rfind('}')`: the insertion point is found
structurally, not by matching text the model has to reproduce. (The
blank-line separator in that format string came later — see
[The blank-line separator](#the-blank-line-separator).)

So the file was already correct before the model ran. The polish pass
took a correct file, regenerated it in full, and that round-trip was the
entire source of the damage.

For a 49-line `StringCalculatorSteps.java`, roughly 91% of the output
tokens were re-transcription of code that was already correct — and that
re-transcription was the whole attack surface. Every byte the model
retypes is a byte it can get wrong; bytes it never sees, it cannot
damage.

## What the field says

Five independent sources converge on the same shape, and the answer is
not "write a better prompt":

- Anthropic's file-editor tool design exposes `insert` for additions and
  `str_replace` for targeted changes, reserving `create` for genuinely
  new files. Rewriting a file in order to add to it is not the intended
  shape of the operation.
- The Geometric edit-format benchmark found that AST- and node-addressed
  edits were the only format where models reached 100%, because the
  applier locates the target structurally rather than by matching text.
- Cascaded editing work — a large model emits a small edit sketch, a
  cheap deterministic applier merges it — reports both lower cost and
  better quality than whole-file regeneration.
- Recent work on edit locality found that diff-style regimes win
  specifically when the change is short and spatially localized.
  Appending members to the end of a class is about as localized as an
  edit gets.
- OpenAI's `apply_patch` guidance explicitly says to nudge models toward
  minimal targeted edits, calling whole-file rewrites for small changes
  "expensive and error-prone."

## Why a diff format was nevertheless rejected

This is the important nuance, and it is the reason the design is not
simply "adopt V4A."

`apply_patch` and the V4A grammar work because significant training
compute went into making those models fluent in that grammar. This
workshop runs `qwen3.8-flash-next:125b-mlx` on a laptop. Aider's
benchmark data is consistent on this point: weaker models degrade badly
on diff formats and do *better* with whole-file output, which is
precisely why whole-file exists as an option at all.

Asking a local model for a context-anchored patch would therefore trade
one failure mode for a worse one: "renames things I didn't ask for"
becomes "patch fails to apply." A silent-but-wrong edit is bad; a hard
failure on every generate is worse, because the workshop stops.

The chosen design sidesteps the tradeoff rather than picking a side.
Send neither the file nor a patch. Send only the new members. There is
no anchor for the model to get wrong, because the model is not being
asked to locate anything — Rust already knows where the members go.

## What was implemented

Four files changed:

1. **`prompts/prompts.toml`** — a new `[polish_fragment]` section.
2. **`src/domain/prompts.rs`** — `polish_fragment` registered in
   `SECTIONS`, which is now nine entries and is asserted at startup to
   carry both a system and a user template.
3. **`src/domain/generation.rs`** — both append paths split into a
   fragment builder plus a splice: `step_definitions_fragment` +
   `splice_step_definitions`, and `unit_test_fragment` +
   `splice_unit_tests`.
4. **`src/application/generation_service.rs`** — a new `polish_fragment`
   pass that both generators route through when appending.

`append_step_definitions` and `append_unit_tests` kept their signatures
by composing the new pieces, so every existing caller and test came
through unchanged:

```rust
pub fn append_step_definitions(
    existing: &str,
    language: Language,
    missing: &[MissingStep],
) -> String {
    match step_definitions_fragment(existing, language, missing) {
        Some(fragment) => splice_step_definitions(existing, language, &fragment),
        None => existing.to_string(),
    }
}
```

### The prompt

Quoted verbatim from `harness/prompts/prompts.toml`, including the
comment block that documents the contract:

```toml
# `spec steps generate` / `spec unittest generate` when the target file
# already exists. Only the newly generated members are shown - the file
# they will be spliced into is deliberately kept out of the context, so
# the model cannot rewrite code it was never given. Context: framework,
# language, practices, count, members (the generated fragment).
[polish_fragment]
system = '''
You are improving {{ count }} newly generated {{ framework }} ({{ language }}) member(s)
that will be inserted into an existing class. You are seeing only the new members,
not the file around them.
{{ language }} best practices to follow:
{{ practices }}
Rules:
- Improve the method names and formatting only.
- Keep every annotation, step expression, and failing placeholder byte-for-byte as written.
- Do NOT emit a package declaration, an import, or a class/interface declaration.
- Do NOT add, remove, or merge members: reply with exactly the {{ count }} member(s) given.
Respond with only the member definitions, no surrounding class, no explanation.
'''
user = '''
{{ members }}
'''
```

The `count` is computed by `count_members`, which tallies the annotation
markers in the generated fragment. Naming the number in the prompt gives
the model an explicit budget, and the gate below enforces it
independently of whether the model honors it.

### Validation gates

Both gates fall back to the deterministic template fragment on any
failure, reported as `source: "template"` in the `GenerationReport`.

**Step definitions — `looks_like_step_fragment`.** The reply must yield
exactly the cucumber expression set it was given: equal length *plus*
every expression present, so neither dropping an expression nor
inventing one passes.

```rust
pub fn looks_like_step_fragment(language: Language, fragment: &str, expected: &[String]) -> bool {
    if fragment.trim().is_empty() || declares_enclosing_scope(language, fragment) {
        return false;
    }
    let kept = extract_patterns(language, fragment);
    kept.len() == expected.len() && expected.iter().all(|pattern| kept.contains(pattern))
}
```

The reply must also contain no `package`, `import`, `using`, or
class/interface/enum/record declaration, since a fragment is spliced
*inside* an existing class and any of those would either nest illegally
or duplicate what the file already has. `declares_enclosing_scope` peels
modifiers first, so `public final class Foo` is caught as readily as a
bare `class Foo`.

**Unit tests — `looks_like_unit_test_fragment`.** The same `@Test` count,
and every `fail("TODO: assert …")` placeholder intact.

This second gate matters pedagogically, not just mechanically. The TODO
placeholder *is* the exercise: the developer is supposed to sharpen the
assertion by hand and watch the bar go red. A model that helpfully
writes the assertion has taken the RED bar away from the developer. That
reply is now refused rather than staged, and the template's failing
placeholder stands.

### The whole-file gate is kept as defense-in-depth

The pre-existing whole-file gate still runs on the assembled result,
including the `declared` check in `steps_generate` that every step
pattern the file previously declared survives:

```rust
let declared = existing
    .map(|file| extract_patterns(self.language, &file.content))
    .unwrap_or_default();
let class_name = production_type_name(&target);
let valid_file = |code: &str| {
    looks_like_step_definitions(self.language, code, class_name.as_deref())
        && package_line
            .as_deref()
            .map(|package| code.contains(package.trim_end_matches(';')))
            .unwrap_or(true)
        && {
            let kept = extract_patterns(self.language, code);
            declared.iter().all(|pattern| kept.contains(pattern))
        }
};
```

The fragment design makes it structurally impossible for that check to
fire, because the bytes outside the splice point are carried over by
`splice_step_definitions` rather than retyped by a model. It was kept
deliberately rather than deleted — a second line of defence that is
expected never to trigger is exactly the one worth having.

Be precise on one point: there is no literal "output contains the
original as a prefix" guard anywhere in the code. The pattern-survival
check on `declared` is the real guard.

### Greenfield generation is deliberately unchanged

`step_definitions_template` and `unit_test_template` still go through the
whole-file `polish` pass with the original `[polish]` prompt. There is no
pre-existing content to protect in that case, and that is precisely the
case the original prompt was written for. A test pins this: a bare
fragment reply on the greenfield path is *refused*, because there the
class declaration is required.

### One implementation wrinkle worth preserving

`strip_code_fences` trims the reply. That trim costs the first member its
indentation and the last member its trailing newline — both of which the
deterministic template carries and the splice point relies on.
`shaped_like` restores the template fragment's leading indent and
guarantees the trailing newline across all four return paths of
`polish_fragment`:

```rust
fn shaped_like(template: &str, polished: &str) -> String {
    let indent: String = template
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut shaped = String::new();
    if !polished.starts_with([' ', '\t']) {
        shaped.push_str(&indent);
    }
    shaped.push_str(polished);
    ending_in_newline(shaped)
}
```

### The blank-line separator

`insert_into_class` now separates appended members from whatever the
class already declares by exactly one blank line. Before, the new member
butted directly against the one above it.

The `trim_end()` in that format string matters as much as the added
newline. The unit-test template's file already ends with a blank line
before its closing brace, so a naive `\n` would produce two blank lines
there while producing one everywhere else. Trimming first makes the
separator uniform regardless of how the existing file ends.

No existing test pinned the separator, which is why the cramped output
survived as long as it did. There is now a test asserting the separator
on a first append and the absence of `\n\n\n` on a second.

## Result

770 tests passing, up from 758 — twelve new.

One pre-existing test was inverted rather than kept.
`polish_that_drops_an_existing_step_pattern_is_refused` had a second half
that fed a whole file on the append path and asserted `source == "llm"`.
That is now exactly what gets refused, so the test became
`a_fragment_reply_that_rewrites_the_whole_file_is_refused` and asserts
the fallback instead. The old shape is worth keeping in mind: that test
was previously *documenting* the behavior this change removes.

Measured outcome for one new step definition against the 48-line
pre-fix baseline: 6 added lines, 0 removed, from the deterministic
splice. Live, with the blank-line separator, it is 7 — see below.

## Live validation

Run against `qwen3.8-flash-next:125b-mlx`, five clean-cache repeats of
each command.

**Diff size.** Live `spec steps generate` for one new step definition:
**7 added lines, 0 removed**, against the 48-line pre-fix baseline. Six
of those are the predicted content; the seventh is the blank-line
separator described above.

**The fallback never fired.** `source: "llm"` on **12 of 12 runs**. This
was the open question the design took on faith, and it resolved
favorably: the fragment gates are tight enough to refuse a whole-file
rewrite, but not so tight that the model fails them on ordinary reasonable
replies. Had the template fallback been firing routinely, the polish pass
would have been decoration — a model call whose output is always thrown
away — and the honest conclusion would have been to delete it. It is
doing real work.

**Real work, specifically.** The model named the method
`anIllegalArgumentExceptionIsThrown`. The raw deterministic template
produces `anIllegalargumentexceptionIsThrown`, because the template's
casing pass has no way to know that `IllegalArgumentException` is three
words. That readability gain is the justification for keeping a model in
the loop at all; a smaller diff alone would not be.

**Stability of what must not change.** Annotation text was byte-identical
across all six `steps generate` runs. Only the method name varied, and
only once — `thenIllegalArgumentExceptionIsThrown` on one run. The
cucumber expression, which is the part that binds the scenario, never
moved.

**Scope.** `spec unittest generate REQ-005` stayed scoped to REQ-005
across five runs, with every placeholder intact. The only occurrence of
`REQ-006` in the output is the pre-existing `// REQ-005 .. REQ-006:`
comment, not a generated test.

**The RED bars arrived as expected.** Maven reported 25 tests / 1 error
for the steps case (the new `PendingException`) and 15 tests / 2 failures
for the unittest case (the two placeholders).

### Known limitation: generated unit tests are not byte-stable

Unit-test generation is not byte-stable across runs. One run returned 13
added lines instead of 15, because the model dropped the `// criterion`
comment that sits above each placeholder.

The gates check the `@Test` count and placeholder survival; they do not
check that comment. That is a deliberate decision rather than an
oversight. Tightening the gate to require the comment would buy cosmetic
consistency and pay for it in real template fallbacks — and a template
fallback costs the readability gain documented above, which is the whole
reason the pass exists. The `@DisplayName` annotation carries the same
criterion text either way, so nothing is actually lost from the output a
developer reads.

Worth knowing before demoing it live: the line count on a unit-test
generate can vary by a couple of lines between runs, and that is expected.


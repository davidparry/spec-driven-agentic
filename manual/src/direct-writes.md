# Direct writes

Every command that writes into your project — feature creation,
scenario mutations, step-definition and unit-test generation, spec
drafting, marking a requirement implemented — writes the real file.
There is no staging area, no transaction to commit, and nothing to
remember to apply. When `spec scenario add` answers, the scenario is in
the feature file.

## Why

- **One source of truth.** Your editor, your test runner, your build and
  the harness are all looking at the same bytes. A scenario the harness
  reports as added is one `mvn test` will run.
- **No forgotten work.** The old staging area could hold an edit the
  developer never applied, and a later command would quietly build on a
  file that did not exist yet.
- **Review where review already happens.** `git diff` shows exactly what
  changed, in the tool you already use for every other change.

## How a write lands

Each write goes to a scratch file beside the target and is then renamed
over it. A rename is atomic, so a reader racing the write sees either
the old file or the new one, never a half-written mixture. Concurrent
`spec` processes serialize their read-modify-write cycles on an advisory
lock at `.spec/.lock`.

A write is confined to the project. A path that escapes the root —
absolute, `..`-relative, or through a symlink — is refused, as is any
path under `.spec/`, which is the harness's own state.

## Undo

Git is the undo. That is why the two commands that generate at scale,
[`spec greenfield`](commands/greenfield.md) and
[`spec deliver`](commands/deliver.md), offer to put the run on a branch
of its own before they write anything — see
[the branch gate](branch-gate.md). For a single command, `git diff` and
`git restore` are the review and the undo.

## What writes and what doesn't

| Writes project files | Writes harness state |
| --- | --- |
| `feature create` | `init` (scaffolding a fresh project) |
| `scenario add` / `update` / `delete` | `model use` (writes `.spec/config.toml`) |
| `steps generate` | `test` / `refactor` (phase state file) |
| `unittest generate` | |
| `spec draft` / `spec reword` | |
| `spec mark-implemented` | |
| `implement`, `greenfield`, `deliver` | |

`spec validate` reads the spec and the Gherkin as they stand on disk, so
running it after a write tells you what the next command will see.

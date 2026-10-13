# The branch gate

The harness [writes your project's real files](direct-writes.md). That
is only safe because you can throw the result away, and git is what
makes it throwable. So the two commands that generate at scale —
[`spec greenfield`](commands/greenfield.md) and
[`spec deliver`](commands/deliver.md) — stop exactly once, before they
write anything, and offer to put the run on a branch of its own.

## The one question

```text
This run writes the project's files directly. You are on main.
Branch name for this run (Enter for spec/2026-10-05-k3f92a, or n to stay on main):
```

Three answers, none of them wrong:

| You type | What happens |
| --- | --- |
| A name | The branch is created and checked out. A bare name is prefixed `spec/`; a name with a `/` in it is taken as you typed it. |
| Enter | A generated name is used: `spec/<today>-<six characters>`. |
| `n` | Nothing is created. The run writes on the branch you are already on. |

Spaces and underscores in a typed name are folded to `-`, so
`add newline support` becomes `spec/add-newline-support`. A name git
would refuse outright — one containing `..`, `~`, `^`, `:` or a control
character — is reported and the run continues where it stands.

## When it does not ask

- **`--no-branch`.** Git is not consulted at all: no probe, no
  question, no branch. The run writes the project where it is. This is
  the flag for CI, for a project deliberately outside version control,
  and for any script that manages its own refs.
- **Outside a git repository.** There is nothing to branch from, so the
  run says so once and carries on:

  ```text
  This project is not a git repository, so there is no branch to work on
  and no undo for what this run writes. Continuing here.
  ```

- **Over MCP.** The server never prompts and never creates a branch. An
  agent-driven session is the caller's to arrange; put yourself on a
  branch before you start it.
- **Any other command.** `spec scenario add`, `spec implement`,
  `spec steps generate` and the rest write one or two files and never
  ask. They are small enough to read in a `git diff`.

## Uncommitted work

If the working tree is dirty when the gate runs, it says so before it
asks, because it changes the answer:

```text
There is uncommitted work here. A new branch carries it along, so it
will be mixed in with what this run writes.
```

Commit or stash first if you want the run's output on its own.

## Throwing the run away

When a branch was created, the gate tells you how to undo the whole
thing in one line:

```bash
git switch main && git branch -D spec/2026-10-05-k3f92a
```

## It never stops the run

The gate is advisory. An unusable name, a git that refuses to create
the branch, or piped input with no answer left all produce a warning
and a run that continues on the current branch. Nothing the gate does
can fail a delivery.

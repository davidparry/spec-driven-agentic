# spec diff

Explain the work in the tree that is not committed yet. The harness runs
`git diff HEAD` under the path you name, then asks the local model to
turn that diff into prose: what behavior changed, what it means for the
requirements it touches, and anything that looks unintended.

```text
Usage: spec diff [OPTIONS] [PATH]
```

```bash
spec diff requirements     # just the spec catalog
spec diff                  # the whole project
spec diff requirements --raw
spec diff --json
```

`PATH` is relative to the project root and may not reach outside it —
`..`, absolute paths, and `~` are refused before git runs. Omit it to
diff everything.

## What it compares

`HEAD` against the files on disk, so **staged and unstaged edits both
count**. That is what someone asking "what have I changed?" means, and
either half alone would answer it wrong.

Files git is not tracking have no diff to show, so they are listed by
name under `untracked` and the model is told to report them as added
rather than describe contents it was never given.

A very large diff is cut on a line boundary and `truncated` is true. Ask
again with a narrower path to see the rest.

## When there is no model

Unlike [`spec ask`](ask.md), a missing model is not a refusal. The diff
is the thing you asked about and the harness already has it, so `spec
diff` prints the diff and the suggested next step instead of failing.
`--raw` asks for that on purpose, and skips the model even when one is
configured.

## Requirements

A `git` on PATH and a project inside a work tree. Without either, the
reply names which one is missing. A repository with no commits yet has
no `HEAD` to compare against, and the reply says so.

The offered tools are the `diff` row of [`spec tools
profiles`](tools.md). Override for one run with `--tools`.

The same reading is available to an agent over MCP as the `git_diff`
tool — see [spec mcp](mcp.md).

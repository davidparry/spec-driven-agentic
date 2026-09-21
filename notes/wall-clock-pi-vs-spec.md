# Wall clock per phase: `pi` + MCP vs the `spec` binary

One measured run of each path to the workshop bar — Exercise 1 drafts
REQ-007, Exercise 2 takes REQ-003 to `implemented`. Both runs ended
`7/7 PASS` on `scripts/verify-workshop-run.sh check`, so the phases below
are five equivalent units of work, not two different amounts of it.

Timings only. For what the two paths produced, see the artifact
comparison; for what the `spec` side costs against its own documented
budgets, see the table in
`student-follow-docs/spec-binary-follow-along.md`.

## Conditions

Both runs were driven from isolated git worktrees cut from the same
commit (`16219d0`), one after the other on the same laptop, against a
model already resident in Ollama.

| | |
| --- | --- |
| Date | 2026-09-21 |
| Model | `qwen3.8-flash-next:125b-mlx` (Ollama 0.34.2, local) |
| `spec` | 0.5.5 |
| `pi` | 0.85.1, `pi-mcp-extension`, against `spec mcp serve` |
| Java / Maven | 21.0.2 / 3.9.16 |
| Branches | `run-spec-binary`, `run-pi-mcp` |

## Per phase

| Phase | `spec` binary | `pi` + MCP | Ratio |
| --- | ---: | ---: | ---: |
| Draft REQ-007 | 28.1 s | 230.7 s | 8.2× |
| Gherkin + unit test | 15.3 s | 177.6 s | 11.6× |
| Commit + RED | 2.7 s | 28.0 s | 10.4× |
| Implement | 70.9 s | 173.7 s | 2.4× |
| Refactor + mark implemented | 59.7 s | 32.6 s | 0.5× |
| **Total** | **176.7 s** (2 m 57 s) | **642.6 s** (10 m 43 s) | **3.6×** |

## What each phase contains

| Phase | `spec` binary | `pi` + MCP |
| --- | --- | --- |
| Draft REQ-007 | `spec draft` wizard, 28.1 s | one agent turn under `-xt bash,powershell`, 230.7 s |
| Gherkin + unit test | `spec scenario generate` 3.7 s, `spec steps missing` <0.1 s, `spec unittest generate` 11.6 s | first attempt 145.7 s (rejected at the checkpoint and discarded) + redo 31.9 s |
| Commit + RED | `spec changes commit` <0.1 s, `spec test` 2.6 s | one agent turn wrapping `changes_commit` + `run_tests`, 28.0 s |
| Implement | `spec implement` 70.9 s | file writes 135.3 s + `run_tests` and self-repair 38.4 s |
| Refactor + mark implemented | `spec refactor` 56.9 s, `spec test` 2.6 s, `spec mark-implemented` + `spec changes commit` <0.2 s | one agent turn: `start_refactor`, edit, `run_tests`, `requirement_mark_implemented`, `changes_commit`, 32.6 s |

## Reading the numbers

The `pi` column is honest about two costs a clean run would not pay, and
both are left in because both were caused by the path rather than by bad
luck:

- **145.7 s of the Gherkin phase was thrown away.** The first batch
  invented new step wordings and created `PendingException` stubs, so it
  was rejected at the review checkpoint. Only the 31.9 s redo survived.
- **38.4 s of the implement phase was recovery.** The wholesale `write`
  fallback emitted an invalid static import; `run_tests` caught the
  compile break and the agent repaired it.

Excluding both, `pi` finishes in 458.5 s (7 m 39 s) — still 2.6× the
`spec` binary.

The one phase where `pi` wins is the last one, and the reason is
structural rather than a speed difference: `spec refactor` runs a full
model call **and** a full test run per round, up to `[refactor] attempts`,
and it judges the result itself. On the `pi` path `start_refactor` only
flips the phase — the edit and the single `run_tests` that follows are
the agent's, with no round budget and no revert.

Two `pi` measurements are excluded from the table because they are not
workshop steps: a 48 s session that listed the tool surface under `-nbt`,
and a 67 s session that did the same under `-xt bash,powershell`.

Timings are a single run each on one laptop. They establish the shape of
the gap, not a benchmark.

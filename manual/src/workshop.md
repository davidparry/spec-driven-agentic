# This workshop’s String Calculator (harness)

The 60-minute class uses Cursor against `spec mcp serve` — one Rust binary
that is both the harness and the MCP server. (The Java module in the repo
is a smoke-test *client* of that server, not a server of its own.) The same
kata can be finished with `spec` commands instead, or with a free local agent
through the [pi path](../../../student-follow-docs/pi-path.md). The student
recipe — install, branch, draft REQ-007, then drive REQ-003…007 to
`implemented` — lives in
[student-follow-docs/harness-path.md](../../../student-follow-docs/harness-path.md).

Do not edit [student-follow-along.md](../../../student-follow-docs/student-follow-along.md)
for this path. Do not implement the kata on `trunk`.

Kata files `spec` must reuse (not parallel `Req00NTest` / `Kata.java`
classes):

- `kata/src/test/resources/features/string_calculator.feature`
- `kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java`
- `kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java`
- `kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java`

Commit staged files **before** [`spec test`](commands/test.md). The
runner executes Maven on the working tree. [`spec mark-implemented`](commands/spec.md)
stays GREEN-gated.

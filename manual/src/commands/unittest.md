# spec unittest

Unit-test generation — the TDD altitude beneath the Gherkin scenarios.
Where a scenario proves the behavior end to end, the unit test pins
down the fine-grained contract of the production code.

```text
Usage: spec unittest [OPTIONS] <COMMAND>

Commands: generate
```

MCP tool equivalent: `unit_test_create`.

---

## spec unittest generate

Generate a unit test from a requirement's acceptance criteria and
stage it. Each Given/When/Then criterion becomes one test case with
the Given as setup, the When as the action, and the Then as the
assertion.

```text
Usage: spec unittest generate [OPTIONS] <REQ_ID>
```

```bash
spec unittest generate REQ-003
```

```json
{
  "target": "src/test/java/StringCalculatorTest.java",
  "staged": true,
  "source": "template",
  "summary": "Unit test for REQ-003 with 2 cases from its acceptance criteria.",
  "nextStep": "Review with 'spec changes show', apply with 'spec changes commit', then 'spec test' to see RED."
}
```

The target file and framework follow the detected language:

| Language | Test framework | Typical target |
| --- | --- | --- |
| Java | JUnit | `src/test/java/<Name>Test.java` |
| JavaScript | node test runner | `test/<name>.test.js` |
| TypeScript | node test runner + ts | `test/<name>.test.ts` |
| .NET | xUnit-style via the test project | `<Name>Tests.cs` |
| Rust | `#[test]` | `tests/<name>_test.rs` |

`source` works exactly as in
[`spec steps generate`](steps.md#source-template-or-llm): deterministic
template by default, `"llm"` only when a model's polished version
passed validation, with the session language's best practices pinned
in the prompt. Generated assertions fail honestly until the
production code exists — the point is a real RED.

An unknown requirement id fails with exit status 1.

## Where it fits

```bash
spec show REQ-003          # read the criteria
spec unittest generate REQ-003  # stage the test
spec changes commit
spec test                       # RED at both altitudes
```

## See also

- [`spec show`](spec.md#spec-show) — the criteria being turned into cases.
- [`spec test`](test.md) — run the generated test.

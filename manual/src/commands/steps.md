# spec steps

Step-definition discovery and generation — the glue between Gherkin
scenarios and real code.

```text
Usage: spec steps [OPTIONS] <COMMAND>

Commands: missing, generate
```

MCP tool equivalents: `step_definitions_find`, `step_definition_create`.

---

## spec steps missing

Report steps used by scenarios that have no matching definition
(undefined), and steps matched by more than one definition
(ambiguous). Detection is language-aware: Cucumber-JVM annotations for
Java, Cucumber-JS functions for JavaScript/TypeScript, Reqnroll
bindings for .NET, cucumber-rs attributes for Rust.

```bash
spec steps missing
```

```json
{
  "language": "javascript",
  "framework": "cucumber-js",
  "missing": [
    { "step": "Given the input \"1,2\"", "keyword": "Given" },
    { "step": "Then the result is 3", "keyword": "Then" }
  ],
  "nextStep": "Generate skeletons with 'spec steps generate', then implement their bodies."
}
```

An empty `missing` array means every step in every scenario is bound.

---

## spec steps generate

Generate step definitions for the undefined steps and stage them.
Already-defined steps are never regenerated — only the gap is filled.

```bash
spec steps generate
```

```json
{
  "target": "features/step_definitions/string_calculator_steps.js",
  "staged": true,
  "source": "template",
  "summary": "2 step definitions generated for undefined steps.",
  "nextStep": "Review with 'spec changes show', apply with 'spec changes commit', implement the bodies, then 'spec test'."
}
```

### `source`: template or llm

- `"template"` — the deterministic skeleton: correct annotations and
  signatures, bodies that fail honestly (throw / `panic!` /
  `PendingStepException`) so the first run is genuinely RED.
- `"llm"` — a model polished the skeleton and the result passed
  validation. Requires a resolved model (see [`spec model`](model.md));
  when no model is reachable, generation silently falls back to the
  template. The LLM output must parse and compile-shape-check or the
  template is used instead — a model can never stage broken code.

The polish prompt pins the session language's best practices — package
naming for Java, `const`/`let` and strict equality for JavaScript,
typed exports for TypeScript, folder-mirroring namespaces for .NET,
snake_case modules for Rust — so a polished file follows the
ecosystem's conventions, not just the step expressions.

Generated skeleton (JavaScript flavor):

```javascript
Given('the input {string}', function (input) {
  throw new Error('Pending: implement this step');
});
```

## See also

- [`spec scenario`](scenario.md) — where the steps come from.
- [`spec changes`](changes.md) — apply the staged definitions.
- [`spec test`](test.md) — run and watch the honest RED.

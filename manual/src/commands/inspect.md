# bdd inspect

Detect the project's languages, build system, BDD framework, and —
critically — whether each language's runtime is actually installed.
The harness only executes tests when the runtime is present, so `inspect`
tells you up front what will run and what will refuse.

```text
Usage: bdd inspect [OPTIONS]
```

## Flags

Only the [global flags](../global-flags.md) (`--root`, `--model`).

## Detection rules

| Marker in the root | Language | Runtime probed |
| --- | --- | --- |
| `pom.xml` | Java (Maven + Cucumber-JVM) | `mvn` |
| `package.json` + `tsconfig.json` | TypeScript (Cucumber-JS) | `node` |
| `package.json` | JavaScript (Cucumber-JS) | `node` |
| `*.csproj` | .NET (Reqnroll) | `dotnet` |
| `Cargo.toml` | Rust (cucumber-rs) | `cargo` |

## Examples

A Rust project with the toolchain installed:

```bash
bdd inspect
```

```json
{
  "languages": [
    {
      "language": "rust",
      "bddFramework": "cucumber-rs",
      "runtime": "cargo",
      "runtimePresent": true,
      "runtimeVersion": "cargo 1.97.0"
    }
  ],
  "nextStep": "The runtime is present. 'bdd test' will execute the suite."
}
```

A Java project without Maven on the PATH:

```json
{
  "languages": [
    {
      "language": "java",
      "bddFramework": "cucumber-jvm",
      "runtime": "mvn",
      "runtimePresent": false,
      "note": "Install Maven (and a JDK) to execute tests; the harness reports, it never installs."
    }
  ],
  "nextStep": "Install the missing runtime before 'bdd test'; authoring commands still work."
}
```

An empty directory reports no languages and points you at
[`bdd init`](init.md).

## Project memory

Session start (`bdd` shell, `bdd mcp serve`), [`bdd init`](init.md),
[`bdd greenfield`](greenfield.md), and every LLM command refresh
`.bdd-memory.json` in the project root: language, BDD framework, build
tool, libraries parsed from the manifest, and a short layout outline.
A language chosen at greenfield/init is kept even if other marker files
appear later. That file is project identity (commit it); every model
system prompt opens with a compact brief of its contents.

## Notes

- Authoring commands (spec, feature, scenario, steps, unittest) work
  without any runtime; only [`bdd test`](test.md) and the test-running
  parts of [`bdd greenfield`](greenfield.md) require one.
- With multiple markers present (a polyglot root), every detected
  language is listed.

//! Deterministic code generation: step-definition and unit-test
//! templates per supported ecosystem, plus the validation applied to LLM
//! output before it may replace a template. Everything here is pure text
//! transformation.

use crate::domain::language::Language;
use crate::domain::model::Requirement;
use crate::domain::prompts::{RenderedPrompt, render};
use crate::domain::steps::{MissingStep, step_to_expression};
use crate::domain::tdd::{ImplementAttempt, StateEntry};

/// Where generated step definitions are staged, by ecosystem convention.
pub fn steps_target_path(language: Language) -> &'static str {
    match language {
        // Default package on purpose: the production class is staged at
        // the default-package path src/main/java/<Project>.java, and Java
        // forbids a named package from referencing a default-package
        // class - steps in a "steps" package could never compile against
        // the production code the implement loop writes.
        Language::Java => "src/test/java/GeneratedSteps.java",
        Language::JavaScript => "features/step_definitions/generated.steps.js",
        Language::TypeScript => "features/step_definitions/generated.steps.ts",
        Language::DotNet => "StepDefinitions/GeneratedSteps.cs",
        Language::Rust => "tests/steps/generated.rs",
    }
}

/// Where a generated unit test for one requirement is staged.
pub fn unit_test_target_path(language: Language, req_id: &str) -> String {
    let pascal = pascal_case(req_id);
    match language {
        Language::Java => format!("src/test/java/{pascal}Test.java"),
        Language::JavaScript => format!("test/{}.test.js", snake_case(req_id)),
        Language::TypeScript => format!("test/{}.test.ts", snake_case(req_id)),
        Language::DotNet => format!("Tests/{pascal}Test.cs"),
        Language::Rust => format!("tests/{}_test.rs", snake_case(req_id)),
    }
}

/// A pending step-definition file covering every missing step. Steps
/// whose texts collapse to the same cucumber expression (e.g. "the
/// result is 3" and "the result is 5" both become "the result is
/// {int}") share one definition - duplicates would make the runner
/// refuse every scenario as ambiguous.
pub fn step_definitions_template(language: Language, missing: &[MissingStep]) -> String {
    let mut seen: Vec<String> = Vec::new();
    let definitions: Vec<String> = missing
        .iter()
        .filter(|step| {
            let expression = step_to_expression(&step.text);
            if seen.contains(&expression) {
                false
            } else {
                seen.push(expression);
                true
            }
        })
        .map(|step| step_definition(language, step))
        .collect();
    let body = definitions.join("\n");
    match language {
        Language::Java => format!(
            "import io.cucumber.java.PendingException;\n\
             import io.cucumber.java.en.Given;\n\
             import io.cucumber.java.en.Then;\n\
             import io.cucumber.java.en.When;\n\n\
             public class GeneratedSteps {{\n\n{body}}}\n"
        ),
        Language::JavaScript => {
            format!("const {{ Given, When, Then }} = require('@cucumber/cucumber');\n\n{body}")
        }
        Language::TypeScript => {
            format!("import {{ Given, When, Then }} from '@cucumber/cucumber';\n\n{body}")
        }
        Language::DotNet => format!(
            "using Reqnroll;\n\n\
             namespace StepDefinitions;\n\n\
             [Binding]\n\
             public class GeneratedSteps\n{{\n{body}}}\n"
        ),
        Language::Rust => format!(
            "use cucumber::{{given, then, when}};\n\n\
             use crate::World;\n\n{body}"
        ),
    }
}

fn step_definition(language: Language, step: &MissingStep) -> String {
    let expression = step_to_expression(&step.text);
    let placeholders = count_placeholders(&expression);
    match language {
        Language::Java => {
            let params = parameter_list(&expression, |i, kind| match kind {
                "{int}" => format!("int arg{i}"),
                _ => format!("String arg{i}"),
            });
            format!(
                "    @{keyword}(\"{expr}\")\n    public void {name}({params}) {{\n        throw new PendingException();\n    }}\n",
                keyword = step.keyword,
                expr = expression.replace('"', "\\\""),
                name = camel_case(&name_source(&step.text)),
            )
        }
        Language::JavaScript | Language::TypeScript => {
            let params: Vec<String> = (0..placeholders).map(|i| format!("arg{i}")).collect();
            format!(
                "{keyword}('{expr}', function ({params}) {{\n  return 'pending';\n}});\n",
                keyword = step.keyword,
                expr = expression.replace('\'', "\\'"),
                params = params.join(", "),
            )
        }
        Language::DotNet => {
            let params = parameter_list(&expression, |i, kind| match kind {
                "{int}" => format!("int arg{i}"),
                _ => format!("string arg{i}"),
            });
            format!(
                "    [{keyword}(\"{expr}\")]\n    public void {name}({params})\n    {{\n        throw new PendingStepException();\n    }}\n",
                keyword = step.keyword,
                expr = expression.replace('"', "\\\""),
                name = pascal_case(&name_source(&step.text)),
            )
        }
        Language::Rust => {
            let params = parameter_list(&expression, |i, kind| match kind {
                "{int}" => format!(", arg{i}: i64"),
                _ => format!(", arg{i}: String"),
            });
            format!(
                "#[{keyword}(expr = \"{expr}\")]\nfn {name}(_world: &mut World{params}) {{\n    todo!(\"implement step: {text}\");\n}}\n",
                keyword = step.keyword.to_lowercase(),
                expr = expression.replace('"', "\\\""),
                name = snake_case(&name_source(&step.text)),
                text = step.text,
            )
        }
    }
}

/// A failing (RED) unit-test file with one test per acceptance criterion.
pub fn unit_test_template(language: Language, requirement: &Requirement) -> String {
    let tests: Vec<String> = requirement
        .acceptance_criteria
        .iter()
        .map(|criterion| unit_test_case(language, criterion))
        .collect();
    let body = tests.join("\n");
    let header = format!("Generated from {}: {}", requirement.id, requirement.title);
    match language {
        Language::Java => format!(
            "import org.junit.jupiter.api.Test;\n\n\
             import static org.junit.jupiter.api.Assertions.fail;\n\n\
             /** {header} */\n\
             class {name}Test {{\n\n{body}}}\n",
            name = pascal_case(&requirement.id),
        ),
        Language::JavaScript => format!(
            "// {header}\n\
             const test = require('node:test');\n\
             const assert = require('node:assert');\n\n{body}"
        ),
        Language::TypeScript => format!(
            "// {header}\n\
             import test from 'node:test';\n\
             import assert from 'node:assert';\n\n{body}"
        ),
        Language::DotNet => format!(
            "using Xunit;\n\n\
             namespace Tests;\n\n\
             /// <summary>{header}</summary>\n\
             public class {name}Test\n{{\n{body}}}\n",
            name = pascal_case(&requirement.id),
        ),
        Language::Rust => format!("// {header}\n\n{body}"),
    }
}

fn unit_test_case(language: Language, criterion: &str) -> String {
    match language {
        Language::Java => format!(
            "    @Test\n    void {name}() {{\n        // {criterion}\n        fail(\"TODO: assert - {escaped}\");\n    }}\n",
            name = snake_case(criterion),
            escaped = criterion.replace('"', "\\\""),
        ),
        Language::JavaScript | Language::TypeScript => format!(
            "test('{name}', () => {{\n  // {criterion}\n  assert.fail('TODO: assert - {escaped}');\n}});\n",
            name = criterion.replace('\\', "\\\\").replace('\'', "\\'"),
            escaped = criterion.replace('\\', "\\\\").replace('\'', "\\'"),
        ),
        Language::DotNet => format!(
            "    [Fact]\n    public void {name}()\n    {{\n        // {criterion}\n        Assert.Fail(\"TODO: assert - {escaped}\");\n    }}\n",
            name = pascal_case(criterion),
            escaped = criterion.replace('"', "\\\""),
        ),
        Language::Rust => format!(
            "#[test]\nfn {name}() {{\n    // {criterion}\n    unimplemented!(\"TODO: assert - {escaped}\");\n}}\n",
            name = snake_case(criterion),
            escaped = criterion.replace('"', "\\\""),
        ),
    }
}

/// Where the project's production code lives, by ecosystem convention.
/// Named after the project - the spec's `project` field.
pub fn implementation_target_path(language: Language, project: &str) -> String {
    let pascal = pascal_case(project);
    match language {
        Language::Java => format!("src/main/java/{pascal}.java"),
        Language::JavaScript => format!("src/{}.js", snake_case(project)),
        Language::TypeScript => format!("src/{}.ts", snake_case(project)),
        Language::DotNet => format!("{pascal}.cs"),
        Language::Rust => "src/lib.rs".into(),
    }
}

/// The session's craft notes for the detected language: every
/// code-producing model call carries these so generated tests and
/// implementations follow the ecosystem's conventions - package
/// naming for Java, snake_case modules for Rust - instead of merely
/// compiling. The language is captured once per session (detected or
/// chosen at scaffold time) and the hints flow from it.
pub fn best_practices(language: Language) -> &'static str {
    match language {
        Language::Java => {
            "- Package names are lowercase and mirror the directory: code under \
             src/main/java and tests under src/test/java declare matching packages.\n\
             - Classes are PascalCase; methods and fields are camelCase; constants \
             are UPPER_SNAKE_CASE.\n\
             - Test methods carry descriptive behavior names and assert one \
             behavior each.\n\
             - Prefer explicit imports over wildcards."
        }
        Language::JavaScript => {
            "- Use const/let (never var), strict equality (===), and camelCase names.\n\
             - One module per concern with explicit exports.\n\
             - Test names read as behavior sentences and assert one behavior each."
        }
        Language::TypeScript => {
            "- Type every exported function and avoid any; let inference handle locals.\n\
             - Use const/let (never var), strict equality (===), and camelCase names.\n\
             - Test names read as behavior sentences and assert one behavior each."
        }
        Language::DotNet => {
            "- Namespaces mirror the folder structure; namespaces, classes, methods, \
             and properties are PascalCase; locals and parameters are camelCase.\n\
             - Test methods carry descriptive behavior names and assert one \
             behavior each.\n\
             - Prefer expression-bodied members only when they stay readable."
        }
        Language::Rust => {
            "- Modules, functions, and file names are snake_case; types and traits \
             are PascalCase.\n\
             - Return Result in library code instead of panicking; reserve unwrap \
             and expect for tests.\n\
             - Borrow instead of cloning where a reference suffices, and keep \
             rustfmt-clean formatting."
        }
    }
}

/// One file the model wants to write during an implementation attempt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct FileUpdate {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
}

/// One prerequisite surveyed by the implement preflight: what it is,
/// where it should live, and whether it exists right now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ImplementAsset {
    pub role: String,
    pub path: String,
    pub present: bool,
}

/// The strict instructions for a workflow advice call: given the
/// requirement, the preflight findings, the asset survey, and the last
/// failures, the model says whether `spec implement` can succeed right
/// now and names the exact next command to run. Rendered from the
/// `[advice]` templates.
pub fn advice_prompt(
    language: Language,
    requirement: &Requirement,
    findings: &[String],
    assets: &[ImplementAsset],
    failures: &[String],
) -> RenderedPrompt {
    render(
        "advice",
        minijinja::context! {
            workflow => crate::domain::workflow::WORKFLOW_PROCESS,
            language => language.display(),
            id => requirement.id,
            title => requirement.title,
            story => requirement.story,
            criteria => requirement.acceptance_criteria,
            assets,
            findings,
            failures,
        },
    )
}

/// One prior implementation attempt, as the `[implementation]` user
/// template consumes it: the paths it wrote (already joined), the
/// failures it was trying to fix, and what the run after it reported -
/// both briefed to their first lines.
#[derive(serde::Serialize)]
struct AttemptContext {
    targets: String,
    failures: Vec<String>,
    outcome: Vec<String>,
}

/// How many prior attempts the implementation prompt recounts, and how
/// much of each prior failure survives. An unbounded history grows the
/// prompt with every RED attempt (13 attempts reached 188KB of prompt,
/// 84% of it old stack traces) until the model outlasts its timeout.
/// The current failures keep full detail; old ones are context, not
/// the assignment.
const PROMPT_HISTORY_ATTEMPTS: usize = 3;
const PROMPT_FAILURE_BRIEF_CHARS: usize = 300;

/// Project files the failures point at, in the order they appear in
/// `files`. Named by file name when the stack mentions one; otherwise
/// every step-definition source when Cucumber reports an undefined step.
fn implicated_paths<'a>(failures: &[String], files: &'a [(String, String)]) -> Vec<&'a str> {
    let failure_text = failures.join("\n");
    let named: Vec<&str> = files
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| failure_text.contains(name))
        })
        .collect();
    if !named.is_empty() {
        return named;
    }
    if !is_undefined_step_failure(&failure_text) {
        return named;
    }
    files
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| is_step_definition_path(path))
        .collect()
}

fn is_undefined_step_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("is undefined")
        || lower.contains("undefinedstepexception")
        || lower.contains("you can implement this step using the snippet")
}

fn is_step_definition_path(path: &str) -> bool {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    name.contains("Steps")
        || name.contains("steps.")
        || lower.contains("/step_definitions/")
        || lower.contains("/steps/")
}

/// A prior failure, briefed for the prompt: its first line, capped.
fn brief_failure(failure: &str) -> String {
    let first_line = failure.lines().next().unwrap_or("").trim();
    let brief: String = first_line
        .chars()
        .take(PROMPT_FAILURE_BRIEF_CHARS)
        .collect();
    if first_line.chars().count() > PROMPT_FAILURE_BRIEF_CHARS {
        format!("{brief} ...")
    } else {
        brief
    }
}

/// One project file, as the `[implementation]` user template consumes it.
#[derive(serde::Serialize)]
struct FileContext<'a> {
    path: &'a str,
    content: &'a str,
}

/// One dated TDD state, as the `[implementation]` user template consumes
/// it: timestamp, phase, and last-run counts — no stack traces.
#[derive(serde::Serialize)]
struct StateContext {
    timestamp: String,
    phase: String,
    tests: u32,
    failures: u32,
    errors: u32,
    skipped: u32,
    refactor_log: Vec<String>,
}

/// The strict instructions for an implementation attempt: make the
/// failing tests pass by writing production code and replacing the
/// TODO placeholders in the test scaffolding with real bodies. The
/// prompt carries the full failure details (stack traces included),
/// every prior attempt with the failures it was addressing, and only
/// the three latest dated TDD states. Rendered from the
/// `[implementation]` templates.
pub fn implementation_prompt(
    language: Language,
    requirement: &Requirement,
    failures: &[String],
    history: &[ImplementAttempt],
    states: &[StateEntry],
    files: &[(String, String)],
    production_path: &str,
) -> RenderedPrompt {
    let omitted = history.len().saturating_sub(PROMPT_HISTORY_ATTEMPTS);
    let history_context: Vec<AttemptContext> = history[omitted..]
        .iter()
        .map(|attempt| AttemptContext {
            targets: attempt.targets.join(", "),
            failures: attempt.failures.iter().map(|f| brief_failure(f)).collect(),
            outcome: attempt.outcome.iter().map(|f| brief_failure(f)).collect(),
        })
        .collect();
    let omitted_states = states
        .len()
        .saturating_sub(crate::domain::tdd::LLM_STATE_ENTRIES);
    let states_context: Vec<StateContext> = states[omitted_states..]
        .iter()
        .map(|entry| StateContext {
            timestamp: entry.timestamp.clone(),
            phase: entry.phase.to_string(),
            tests: entry.last_run.tests,
            failures: entry.last_run.failures,
            errors: entry.last_run.errors,
            skipped: entry.last_run.skipped,
            refactor_log: entry.refactor_log.clone(),
        })
        .collect();
    let files_context: Vec<FileContext> = files
        .iter()
        .map(|(path, content)| FileContext { path, content })
        .collect();
    // The project files the current failures name (by file name, which
    // covers bare stack-frame names and absolute paths alike). Observed
    // live: 147 straight attempts rewrote only the production file while
    // every stack trace pointed at a step-definition file - the prompt
    // must say where the failing code lives, not just show it.
    // Undefined Cucumber steps never name the glue file (the engine
    // never entered it), so those failures implicate the step-definition
    // sources even when the stack is only Cucumber internals.
    let implicated = implicated_paths(failures, files);
    let attempt = history.len() + 1;
    tracing::debug!(requirement = %requirement.id, attempt, omitted, "implementation prompt");
    tracing::debug!(files = %files_context.len(), implicated = %implicated.len(), "prompt files");
    render(
        "implementation",
        minijinja::context! {
            implicated,
            language => language.display(),
            practices => best_practices(language),
            production_path,
            id => requirement.id,
            title => requirement.title,
            story => requirement.story,
            criteria => requirement.acceptance_criteria,
            failures,
            attempt => history.len() + 1,
            omitted,
            history => history_context,
            instructions => crate::domain::tdd::STATE_INSTRUCTIONS,
            states => states_context,
            files => files_context,
        },
    )
}

/// The hybrid polish instructions of `steps generate` and `unittest
/// generate`: improve the deterministic scaffold without touching its
/// contract. Rendered from the `[polish]` templates.
pub fn polish_prompt(language: Language, scaffold: &str) -> RenderedPrompt {
    render(
        "polish",
        minijinja::context! {
            framework => language.bdd_framework(),
            language => language.display(),
            practices => best_practices(language),
            file => scaffold,
        },
    )
}

/// Parse the model's implementation reply. Elements without a path or
/// content are dropped; an unparseable reply is an empty list.
pub fn parse_file_updates(reply: &str) -> Vec<FileUpdate> {
    let body = strip_code_fences(reply);
    // Models sometimes reply with one bare object instead of the asked-for
    // array - accept both shapes rather than discarding a usable attempt.
    let updates = serde_json::from_str::<Vec<FileUpdate>>(&body)
        .or_else(|_| serde_json::from_str::<FileUpdate>(&body).map(|update| vec![update]))
        .unwrap_or_default();
    let updates: Vec<FileUpdate> = updates
        .into_iter()
        .filter(|update| !update.path.trim().is_empty() && !update.content.trim().is_empty())
        .collect();
    if updates.is_empty() {
        tracing::debug!(reply_chars = %reply.len(), "LLM reply held no usable file updates");
    } else {
        tracing::debug!(count = updates.len(), "parsed file updates from LLM reply");
    }
    updates
}

/// Parse with a reason when the reply held no usable file update.
pub fn parse_file_updates_checked(reply: &str) -> Result<Vec<FileUpdate>, String> {
    let updates = parse_file_updates(reply);
    if updates.is_empty() {
        Err("the reply was not a JSON array of {path, content} file updates".into())
    } else {
        Ok(updates)
    }
}

/// Strip a surrounding Markdown code fence, which LLMs love to add,
/// and drop a leading `<think>...</think>` block some models emit
/// before the payload.
pub fn strip_code_fences(response: &str) -> String {
    let trimmed = strip_think_block(response.trim());
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let after_language = rest.split_once('\n').map_or("", |(_, body)| body);
    after_language
        .rsplit_once("```")
        .map_or(after_language, |(body, _)| body)
        .trim()
        .to_string()
}

pub fn strip_think_block(text: &str) -> String {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("<think>") else {
        return trimmed.to_string();
    };
    match rest.split_once("</think>") {
        Some((_, after)) => after.trim().to_string(),
        None => trimmed.to_string(),
    }
}

/// Would this code plausibly be a step-definition file for the language?
/// The gate before LLM output may replace the deterministic template.
pub fn looks_like_step_definitions(language: Language, code: &str) -> bool {
    !code.trim().is_empty()
        && match language {
            Language::Java => {
                (code.contains("@Given") || code.contains("@When") || code.contains("@Then"))
                    // The file is staged at GeneratedSteps.java; a polish
                    // pass that renames the public class (observed live:
                    // "class StringCalculatorSteps is public, should be
                    // declared in a file named ...") can never compile.
                    && code.contains("public class GeneratedSteps")
            }
            Language::JavaScript | Language::TypeScript => {
                code.contains("Given(") || code.contains("When(") || code.contains("Then(")
            }
            Language::DotNet => {
                code.contains("[Given(") || code.contains("[When(") || code.contains("[Then(")
            }
            Language::Rust => {
                code.contains("#[given") || code.contains("#[when") || code.contains("#[then")
            }
        }
}

/// Would this code plausibly be a unit-test file for the language?
pub fn looks_like_unit_test(language: Language, code: &str) -> bool {
    !code.trim().is_empty()
        && match language {
            Language::Java => code.contains("@Test"),
            Language::JavaScript | Language::TypeScript => code.contains("test("),
            Language::DotNet => code.contains("[Fact]") || code.contains("[Theory]"),
            Language::Rust => code.contains("#[test]"),
        }
}

/// Tightened polish gate for a brownfield test class: keep `@Test`, and
/// refuse a reply that calls a different type than the one the project
/// already constructs (`Calculator.add` when production is
/// `StringCalculator`). A `fail(` TODO is always acceptable.
pub fn looks_like_unit_test_for(
    language: Language,
    code: &str,
    production_type: Option<&str>,
) -> bool {
    if !looks_like_unit_test(language, code) {
        return false;
    }
    let Some(ty) = production_type else {
        return true;
    };
    match language {
        Language::Java => {
            let wrong_static = code.contains("Calculator.add") && ty != "Calculator";
            (code.contains(ty) || code.contains("fail(")) && !wrong_static
        }
        _ => true,
    }
}

/// Append failing tests for `requirement` onto an existing test class
/// (the workshop `StringCalculatorTest` shape). Greenfield still uses
/// [`unit_test_template`].
pub fn append_unit_tests(existing: &str, language: Language, requirement: &Requirement) -> String {
    let methods: String = requirement
        .acceptance_criteria
        .iter()
        .map(|criterion| unit_test_case_for(language, requirement, criterion))
        .collect();
    match language {
        Language::Java => append_java_methods(existing, &methods),
        _ => format!("{}\n{methods}", existing.trim_end()),
    }
}

fn unit_test_case_for(language: Language, requirement: &Requirement, criterion: &str) -> String {
    match language {
        Language::Java => format!(
            "    @Test\n    @DisplayName(\"{id}: {title}\")\n    void {name}() {{\n        // {criterion}\n        fail(\"TODO: assert - {escaped}\");\n    }}\n",
            id = requirement.id,
            title = criterion.replace('"', "\\\""),
            name = snake_case(criterion),
            escaped = criterion.replace('"', "\\\""),
        ),
        _ => unit_test_case(language, criterion),
    }
}

fn append_java_methods(existing: &str, methods: &str) -> String {
    let with_imports = ensure_java_import(
        &ensure_java_import(existing, "import org.junit.jupiter.api.DisplayName;"),
        "import static org.junit.jupiter.api.Assertions.fail;",
    );
    match with_imports.rfind('}') {
        Some(i) => format!("{}{}\n{}", &with_imports[..i], methods, &with_imports[i..]),
        None => format!("{with_imports}\n{methods}"),
    }
}

fn ensure_java_import(source: &str, import: &str) -> String {
    if source.contains(import.trim_end_matches(';')) {
        return source.to_string();
    }
    if source.starts_with("package ")
        && let Some(end) = source.find(';')
    {
        return format!(
            "{}\n\n{}\n{}",
            &source[..=end],
            import,
            source[end + 1..].trim_start()
        );
    }
    format!("{import}\n{source}")
}

fn count_placeholders(expression: &str) -> usize {
    expression.matches('{').count()
}

/// Build a parameter list from the expression's placeholders, one entry
/// per placeholder, formatted by `format_param(index, kind)`.
fn parameter_list(expression: &str, format_param: impl Fn(usize, &str) -> String) -> String {
    let mut params = Vec::new();
    let mut rest = expression;
    while let Some(open) = rest.find('{') {
        let tail = &rest[open..];
        let close = tail
            .find('}')
            .expect("expressions come from step_to_expression");
        params.push(format_param(params.len(), &tail[..=close]));
        rest = &tail[close + 1..];
    }
    params.join(", ")
}

/// The text an identifier is derived from: quoted arguments carry data,
/// not meaning, so they are dropped before casing.
fn name_source(text: &str) -> String {
    let mut out = String::new();
    let mut in_quotes = false;
    for c in text.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            out.push(c);
        }
    }
    out
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn snake_case(text: &str) -> String {
    let name = words(text).join("_");
    if name.is_empty() {
        "step".to_string()
    } else if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{name}")
    } else {
        name
    }
}

fn pascal_case(text: &str) -> String {
    let name: String = words(text)
        .iter()
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .expect("words are non-empty")
        })
        .collect();
    if name.is_empty() {
        "Step".to_string()
    } else if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("N{name}")
    } else {
        name
    }
}

fn camel_case(text: &str) -> String {
    let pascal = pascal_case(text);
    let mut chars = pascal.chars();
    chars
        .next()
        .map(|c| c.to_lowercase().collect::<String>() + chars.as_str())
        .expect("pascal_case never returns an empty string")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn missing(keyword: &str, text: &str) -> MissingStep {
        MissingStep {
            feature: "features/calc.feature".into(),
            scenario: "Adds".into(),
            keyword: keyword.into(),
            text: text.into(),
        }
    }

    fn requirement() -> Requirement {
        Requirement {
            id: "REQ-001".into(),
            title: "Empty string returns zero".into(),
            status: "pending".into(),
            story: "As a user, I want zero so that sums start clean.".into(),
            acceptance_criteria: vec![
                "Given an empty string \"\", when add is called, then the result is 0".into(),
            ],
            feature_file: None,
        }
    }

    #[test]
    fn every_language_has_a_steps_target_path_in_its_convention() {
        assert_eq!(
            steps_target_path(Language::Java),
            "src/test/java/GeneratedSteps.java"
        );
        assert_eq!(
            steps_target_path(Language::JavaScript),
            "features/step_definitions/generated.steps.js"
        );
        assert_eq!(
            steps_target_path(Language::TypeScript),
            "features/step_definitions/generated.steps.ts"
        );
        assert_eq!(
            steps_target_path(Language::DotNet),
            "StepDefinitions/GeneratedSteps.cs"
        );
        assert_eq!(
            steps_target_path(Language::Rust),
            "tests/steps/generated.rs"
        );
    }

    #[test]
    fn unit_test_paths_carry_the_requirement_id() {
        assert_eq!(
            unit_test_target_path(Language::Java, "REQ-001"),
            "src/test/java/Req001Test.java"
        );
        assert_eq!(
            unit_test_target_path(Language::JavaScript, "REQ-001"),
            "test/req_001.test.js"
        );
        assert_eq!(
            unit_test_target_path(Language::TypeScript, "REQ-001"),
            "test/req_001.test.ts"
        );
        assert_eq!(
            unit_test_target_path(Language::DotNet, "REQ-001"),
            "Tests/Req001Test.cs"
        );
        assert_eq!(
            unit_test_target_path(Language::Rust, "REQ-001"),
            "tests/req_001_test.rs"
        );
    }

    #[test]
    fn every_language_carries_its_own_best_practices() {
        assert!(best_practices(Language::Java).contains("Package names are lowercase"));
        assert!(best_practices(Language::JavaScript).contains("const/let (never var)"));
        assert!(best_practices(Language::TypeScript).contains("avoid any"));
        assert!(best_practices(Language::DotNet).contains("Namespaces mirror the folder"));
        assert!(best_practices(Language::Rust).contains("snake_case"));
    }

    #[test]
    fn a_java_step_definition_is_a_pending_annotated_method() {
        let code = step_definitions_template(
            Language::Java,
            &[missing("When", "add is called with \"1,2\"")],
        );
        assert!(code.contains("import io.cucumber.java.en.When;"));
        assert!(code.contains("@When(\"add is called with {string}\")"));
        assert!(code.contains("public void addIsCalledWith(String arg0)"));
        assert!(code.contains("throw new PendingException();"));
    }

    #[test]
    fn a_java_int_placeholder_becomes_an_int_parameter() {
        let code = step_definitions_template(Language::Java, &[missing("Then", "the result is 3")]);
        assert!(code.contains("@Then(\"the result is {int}\")"));
        assert!(code.contains("public void theResultIs3(int arg0)"));
    }

    #[test]
    fn steps_that_share_an_expression_share_one_definition() {
        let code = step_definitions_template(
            Language::Java,
            &[
                missing("Then", "the result is 3"),
                missing("Then", "the result is 5"),
                missing("When", "add is called with \"1,2\""),
            ],
        );
        assert_eq!(
            code.matches("@Then(\"the result is {int}\")").count(),
            1,
            "duplicate expressions make cucumber refuse every scenario:\n{code}"
        );
        assert!(code.contains("@When(\"add is called with {string}\")"));
    }

    #[test]
    fn javascript_and_typescript_definitions_return_pending() {
        for (language, first_line) in [
            (
                Language::JavaScript,
                "const { Given, When, Then } = require('@cucumber/cucumber');",
            ),
            (
                Language::TypeScript,
                "import { Given, When, Then } from '@cucumber/cucumber';",
            ),
        ] {
            let code = step_definitions_template(language, &[missing("Given", "a calculator")]);
            assert!(code.starts_with(first_line), "got: {code}");
            assert!(code.contains("Given('a calculator', function () {"));
            assert!(code.contains("return 'pending';"));
        }
    }

    #[test]
    fn a_dotnet_definition_is_a_reqnroll_binding() {
        let code = step_definitions_template(
            Language::DotNet,
            &[
                missing("Then", "the result is 3"),
                missing("When", "add is called with \"1,2\""),
            ],
        );
        assert!(code.contains("using Reqnroll;"));
        assert!(code.contains("[Binding]"));
        assert!(code.contains("[Then(\"the result is {int}\")]"));
        assert!(code.contains("public void TheResultIs3(int arg0)"));
        assert!(code.contains("[When(\"add is called with {string}\")]"));
        assert!(code.contains("public void AddIsCalledWith(string arg0)"));
        assert!(code.contains("throw new PendingStepException();"));
    }

    #[test]
    fn a_rust_definition_is_a_cucumber_rs_attribute_fn() {
        let code = step_definitions_template(
            Language::Rust,
            &[missing("When", "add is called with \"1,2\"")],
        );
        assert!(code.contains("use cucumber::{given, then, when};"));
        assert!(code.contains("#[when(expr = \"add is called with {string}\")]"));
        assert!(code.contains("fn add_is_called_with(_world: &mut World, arg0: String)"));
        assert!(code.contains("todo!"));
    }

    #[test]
    fn rust_int_placeholders_become_i64_parameters() {
        let code = step_definitions_template(Language::Rust, &[missing("Then", "the result is 3")]);
        assert!(code.contains("fn the_result_is_3(_world: &mut World, arg0: i64)"));
    }

    #[test]
    fn unit_tests_fail_red_in_every_language() {
        let requirement = requirement();
        let expectations = [
            (Language::Java, "fail(\"TODO: assert -"),
            (Language::JavaScript, "assert.fail('TODO: assert -"),
            (Language::TypeScript, "assert.fail('TODO: assert -"),
            (Language::DotNet, "Assert.Fail(\"TODO: assert -"),
            (Language::Rust, "unimplemented!(\"TODO: assert -"),
        ];
        for (language, marker) in expectations {
            let code = unit_test_template(language, &requirement);
            assert!(
                code.contains(marker),
                "{language:?} missing {marker}: {code}"
            );
            assert!(
                code.contains("Generated from REQ-001: Empty string returns zero"),
                "{language:?} missing header"
            );
            assert!(
                looks_like_unit_test(language, &code),
                "{language:?} fails own gate"
            );
        }
    }

    #[test]
    fn every_template_passes_its_own_validation_gate() {
        let steps = [missing("Given", "a calculator")];
        for language in Language::ALL {
            let code = step_definitions_template(language, &steps);
            assert!(
                looks_like_step_definitions(language, &code),
                "{language:?} template fails its own gate: {code}"
            );
        }
    }

    #[test]
    fn java_steps_validation_rejects_a_renamed_public_class() {
        // Seen live: the polish pass renamed the class while the file
        // stays GeneratedSteps.java, which can never compile.
        let renamed = "import io.cucumber.java.en.Given;\n\
             public class StringCalculatorSteps {\n\
                 @Given(\"a calculator\") public void polished() {}\n\
             }\n";
        assert!(!looks_like_step_definitions(Language::Java, renamed));
        let kept = renamed.replace("StringCalculatorSteps", "GeneratedSteps");
        assert!(looks_like_step_definitions(Language::Java, &kept));
    }

    #[test]
    fn validation_rejects_empty_and_unrecognizable_output() {
        for language in Language::ALL {
            assert!(!looks_like_step_definitions(language, "   "));
            assert!(!looks_like_step_definitions(
                language,
                "I cannot help with that."
            ));
            assert!(!looks_like_unit_test(language, ""));
            assert!(!looks_like_unit_test(language, "Sure! Here is an essay."));
        }
    }

    #[test]
    fn java_polish_rejects_a_wrong_production_type() {
        let polished = "@Test void two() { Calculator.add(\"1,2\"); }";
        assert!(looks_like_unit_test(Language::Java, polished));
        assert!(!looks_like_unit_test_for(
            Language::Java,
            polished,
            Some("StringCalculator")
        ));
        let todo = "@Test void two() { fail(\"TODO\"); }";
        assert!(looks_like_unit_test_for(
            Language::Java,
            todo,
            Some("StringCalculator")
        ));
    }

    #[test]
    fn appending_java_tests_keeps_the_class_and_names_the_requirement() {
        let existing = "package com.example;\n\n\
             import org.junit.jupiter.api.Test;\n\n\
             class StringCalculatorTest {\n    private final StringCalculator calculator = new StringCalculator();\n}\n";
        let requirement = Requirement {
            id: "REQ-003".into(),
            title: "Two comma-separated numbers".into(),
            status: "pending".into(),
            story: "As a user, I want sums so that I can add.".into(),
            acceptance_criteria: vec!["Given a, when b, then 3".into()],
            feature_file: None,
        };
        let code = append_unit_tests(existing, Language::Java, &requirement);
        assert!(code.contains("package com.example;"));
        assert!(code.contains("class StringCalculatorTest"));
        assert!(code.contains("@DisplayName(\"REQ-003: Given a, when b, then 3\")"));
        assert!(code.contains("fail(\"TODO: assert -"));
        assert!(code.contains("import org.junit.jupiter.api.DisplayName"));
        assert!(code.contains("Assertions.fail"));
        assert!(
            code.matches('}').count() >= 2,
            "class brace is preserved: {code}"
        );
    }

    #[test]
    fn implementation_paths_follow_each_ecosystem_and_carry_the_project_name() {
        assert_eq!(
            implementation_target_path(Language::Java, "String Calculator"),
            "src/main/java/StringCalculator.java"
        );
        assert_eq!(
            implementation_target_path(Language::JavaScript, "String Calculator"),
            "src/string_calculator.js"
        );
        assert_eq!(
            implementation_target_path(Language::TypeScript, "String Calculator"),
            "src/string_calculator.ts"
        );
        assert_eq!(
            implementation_target_path(Language::DotNet, "String Calculator"),
            "StringCalculator.cs"
        );
        assert_eq!(
            implementation_target_path(Language::Rust, "String Calculator"),
            "src/lib.rs"
        );
    }

    #[test]
    fn the_implementation_prompt_carries_the_context_and_the_strict_rules() {
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &["Req001Test.case: TODO: assert".into()],
            &[],
            &[],
            &[(
                "src/test/java/Req001Test.java".into(),
                "class Req001Test {}".into(),
            )],
            "src/main/java/Kata.java",
        );
        assert!(prompt.user.contains("REQ-001: Empty string returns zero"));
        assert!(prompt.user.contains("Req001Test.case: TODO: assert"));
        assert!(
            prompt
                .user
                .contains("--- src/test/java/Req001Test.java ---")
        );
        assert!(
            prompt
                .system
                .contains("Write the production code at src/main/java/Kata.java")
        );
        assert!(prompt.system.contains("ONLY a JSON array"));
        assert!(prompt.system.contains("never delete or weaken one"));
        assert!(
            prompt.system.contains("Java best practices to follow:")
                && prompt.system.contains("Package names are lowercase"),
            "the system prompt pins the language's best practices"
        );
        assert!(
            !prompt.user.contains("prior attempt"),
            "a first attempt has no history section"
        );
        assert!(
            !prompt.user.contains("How to interpret the TDD state"),
            "no dated states means no state section"
        );
    }

    #[test]
    fn the_implementation_prompt_points_at_the_files_the_failures_name() {
        // Seen live: every stack trace pointed at GeneratedSteps.java
        // while 147 attempts rewrote only the production file.
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &[
                "expected: <1> but was: <0>\n\tat GeneratedSteps.itReturns(GeneratedSteps.java:34)"
                    .into(),
            ],
            &[],
            &[],
            &[
                (
                    "src/test/java/GeneratedSteps.java".into(),
                    "public class GeneratedSteps {}".into(),
                ),
                (
                    "src/test/java/Req001Test.java".into(),
                    "class Req001Test {}".into(),
                ),
            ],
            "src/main/java/Kata.java",
        );
        assert!(
            prompt
                .user
                .contains("The failure output names these project files")
        );
        assert!(
            prompt
                .user
                .contains("\n- src/test/java/GeneratedSteps.java\n")
        );
        assert!(
            !prompt.user.contains("\n- src/test/java/Req001Test.java\n"),
            "files the failures never name are not listed as implicated"
        );
        assert!(
            prompt
                .user
                .contains("rewriting src/main/java/Kata.java alone cannot fix a wiring bug")
        );
    }

    #[test]
    fn a_prompt_with_failures_naming_no_project_file_has_no_implicated_section() {
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &["Build failed before tests could run".into()],
            &[],
            &[],
            &[(
                "src/test/java/Req001Test.java".into(),
                "class Req001Test {}".into(),
            )],
            "src/main/java/Kata.java",
        );
        assert!(
            !prompt
                .user
                .contains("The failure output names these project files")
        );
    }

    #[test]
    fn an_undefined_cucumber_step_implicates_the_step_definition_file() {
        // Seen live: 57 attempts rewrote only StringCalculator.java while
        // Cucumber reported 'the result is 0.' undefined. The stack never
        // names GeneratedSteps.java because the engine never entered it.
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &[
                "RunCucumberTest.case 1: The step 'the result is 0.' is undefined.\n\
You can implement this step using the snippet(s) below:\n\
@Then(\"the result is {int}.\")\n\
io.cucumber.junit.platform.engine.UndefinedStepException: The step 'the result is 0.' is undefined."
                    .into(),
            ],
            &[],
            &[],
            &[
                (
                    "src/main/java/StringCalculator.java".into(),
                    "public class StringCalculator {}".into(),
                ),
                (
                    "src/test/java/GeneratedSteps.java".into(),
                    "@Then(\"the result is {int}\") void theResultIs(int n) {}".into(),
                ),
                (
                    "src/test/java/Req001Test.java".into(),
                    "class Req001Test {}".into(),
                ),
            ],
            "src/main/java/StringCalculator.java",
        );
        assert!(
            prompt
                .user
                .contains("The failure output names these project files")
        );
        assert!(
            prompt
                .user
                .contains("\n- src/test/java/GeneratedSteps.java\n")
        );
        assert!(
            !prompt.user.contains("\n- src/test/java/Req001Test.java\n"),
            "unit tests are not implicated by an undefined Gherkin step"
        );
        assert!(
            prompt.system.contains("step is undefined"),
            "the system prompt must tell the model glue cannot be fixed in production code"
        );
    }

    #[test]
    fn an_html_encoded_undefined_step_still_implicates_the_glue_file() {
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &["The step &apos;the result is 0.&apos; is undefined.&#10;You can implement this step using the snippet(s) below:"
                .into()],
            &[],
            &[],
            &[(
                "src/test/java/GeneratedSteps.java".into(),
                "public class GeneratedSteps {}".into(),
            )],
            "src/main/java/Kata.java",
        );
        assert!(
            prompt
                .user
                .contains("\n- src/test/java/GeneratedSteps.java\n")
        );
    }

    #[test]
    fn the_implementation_prompt_recounts_every_prior_attempt() {
        let history = vec![
            ImplementAttempt {
                requirement: "REQ-001".into(),
                targets: vec!["src/main/java/Kata.java".into()],
                failures: vec!["Req001Test.case: TODO: assert\nat Req001Test.java:12".into()],
                outcome: vec!["Req001Test.case: expected 0 but was 1\nat Req001Test.java:9".into()],
            },
            ImplementAttempt {
                requirement: "REQ-001".into(),
                targets: vec![
                    "src/main/java/Kata.java".into(),
                    "src/test/java/Req001Test.java".into(),
                ],
                failures: vec!["Req001Test.case: expected 0 but was 1".into()],
                outcome: Vec::new(),
            },
        ];
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &["Req001Test.case: cannot find symbol Kata".into()],
            &history,
            &[],
            &[],
            "src/main/java/Kata.java",
        );
        assert!(
            prompt
                .user
                .contains("This is attempt 3 on this requirement")
        );
        assert!(
            prompt
                .user
                .contains("Attempt 1 wrote: src/main/java/Kata.java\n")
        );
        assert!(
            prompt.user.contains("- Req001Test.case: TODO: assert\n"),
            "the prior failure's first line is recounted"
        );
        assert!(
            !prompt.user.contains("at Req001Test.java:12"),
            "prior stack traces are briefed away"
        );
        assert!(
            prompt.user.contains(
                "The run after attempt 1 reported:\n- Req001Test.case: expected 0 but was 1\n"
            ),
            "each attempt's actual result guides the next try: {}",
            prompt.user
        );
        assert!(
            !prompt.user.contains("at Req001Test.java:9"),
            "outcome stack traces are briefed away too"
        );
        assert!(
            prompt.user.contains(
                "Attempt 2 wrote: src/main/java/Kata.java, src/test/java/Req001Test.java"
            )
        );
        assert!(
            prompt
                .user
                .contains("No test run followed attempt 2 - its changes were never verified."),
            "an unverified attempt is called out: {}",
            prompt.user
        );
        assert!(prompt.user.contains("what remains AFTER the last attempt"));
    }

    #[test]
    fn a_long_history_is_capped_to_the_most_recent_attempts() {
        let history: Vec<ImplementAttempt> = (1..=5)
            .map(|i| ImplementAttempt {
                requirement: "REQ-001".into(),
                targets: vec![format!("src/main/java/Kata{i}.java")],
                failures: vec![format!("failure of attempt {i}")],
                ..Default::default()
            })
            .collect();
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &["current failure".into()],
            &history,
            &[],
            &[],
            "src/main/java/Kata.java",
        );
        assert!(
            prompt
                .user
                .contains("This is attempt 6 on this requirement")
        );
        assert!(prompt.user.contains("(2 earlier attempts omitted.)"));
        assert!(!prompt.user.contains("Attempt 1 wrote"));
        assert!(!prompt.user.contains("Attempt 2 wrote"));
        assert!(
            prompt
                .user
                .contains("Attempt 3 wrote: src/main/java/Kata3.java"),
            "the kept attempts keep their true numbering"
        );
        assert!(
            prompt
                .user
                .contains("Attempt 5 wrote: src/main/java/Kata5.java")
        );
    }

    #[test]
    fn the_implementation_prompt_briefs_only_the_three_latest_states() {
        use crate::domain::tdd::{STATE_INSTRUCTIONS, TddPhase};
        let states: Vec<StateEntry> = (1..=5)
            .map(|i| StateEntry {
                timestamp: format!("2026-08-0{i}T12:00:00Z"),
                phase: TddPhase::Red,
                last_run: crate::domain::model::TestRunSummary {
                    tests: i,
                    failures: 1,
                    failure_details: vec!["stack from day {i}".replace("{i}", &i.to_string())],
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect();
        let prompt = implementation_prompt(
            Language::Java,
            &requirement(),
            &["current failure".into()],
            &[],
            &states,
            &[],
            "src/main/java/Kata.java",
        );
        assert!(
            prompt
                .user
                .contains("How to interpret the TDD state below:")
        );
        assert!(prompt.user.contains(STATE_INSTRUCTIONS));
        assert!(
            prompt
                .user
                .contains("The 3 most recent TDD state(s), oldest first:")
        );
        assert!(!prompt.user.contains("2026-08-01T12:00:00Z"));
        assert!(!prompt.user.contains("2026-08-02T12:00:00Z"));
        assert!(
            prompt
                .user
                .contains("2026-08-03T12:00:00Z RED tests=3 failures=1")
        );
        assert!(
            prompt
                .user
                .contains("2026-08-05T12:00:00Z RED tests=5 failures=1")
        );
        assert!(
            !prompt.user.contains("stack from day"),
            "historical stack traces stay out of the state brief"
        );
        assert!(
            !prompt.user.contains("prior attempt"),
            "a first attempt still has no history section"
        );
    }

    #[test]
    fn a_long_prior_failure_is_briefed_to_its_capped_first_line() {
        let long_line = "x".repeat(400);
        assert_eq!(
            brief_failure(&long_line),
            format!("{} ...", "x".repeat(300))
        );
        assert_eq!(
            brief_failure("expected 0 but was 1\nat Kata.java:9\nat Runner.java:3"),
            "expected 0 but was 1"
        );
    }

    #[test]
    fn the_polish_prompt_carries_the_framework_practices_and_the_scaffold() {
        let prompt = polish_prompt(Language::Java, "@Given(\"a calculator\") void a() {}");
        assert!(prompt.system.contains("Cucumber-JVM"));
        assert!(prompt.system.contains("Java best practices to follow:"));
        assert!(prompt.system.contains("Package names are lowercase"));
        assert!(
            prompt
                .system
                .contains("only the complete file content, no explanation")
        );
        assert_eq!(prompt.user, "@Given(\"a calculator\") void a() {}");
    }

    #[test]
    fn the_advice_prompt_surveys_assets_findings_failures_and_the_rules() {
        let assets = vec![
            ImplementAsset {
                role: "tagged scenario".into(),
                path: "features/calc.feature".into(),
                present: true,
            },
            ImplementAsset {
                role: "unit test".into(),
                path: "src/test/java/Req001Test.java".into(),
                present: false,
            },
        ];
        let prompt = advice_prompt(
            Language::Java,
            &requirement(),
            &["The unit test does not exist - run spec unittest generate REQ-001.".into()],
            &assets,
            &["Req001Test.case: TODO: assert".into()],
        );
        assert!(prompt.user.contains("REQ-001: Empty string returns zero"));
        assert!(
            prompt
                .user
                .contains("- tagged scenario: features/calc.feature (present)")
        );
        assert!(
            prompt
                .user
                .contains("- unit test: src/test/java/Req001Test.java (missing)")
        );
        assert!(
            prompt
                .user
                .contains("Workflow findings blocking an implementation attempt:")
        );
        assert!(prompt.user.contains("- Req001Test.case: TODO: assert"));
        assert!(prompt.system.contains("at most four short sentences"));
        assert!(prompt.system.contains("spec unittest generate <REQ-ID>"));
        assert!(
            prompt.system.contains("THE LOOP FOR ONE REQUIREMENT"),
            "the workflow process briefs the advice call"
        );
    }

    #[test]
    fn an_advice_prompt_without_findings_or_failures_omits_those_sections() {
        let prompt = advice_prompt(Language::Java, &requirement(), &[], &[], &[]);
        assert!(!prompt.user.contains("Workflow findings"));
        assert!(!prompt.user.contains("The last test run's failures"));
    }

    #[test]
    fn file_updates_parse_from_json_with_or_without_fences() {
        let reply = r#"[{"path": "src/lib.rs", "content": "pub fn add() {}"}]"#;
        assert_eq!(parse_file_updates(reply).len(), 1);
        let fenced = format!("```json\n{reply}\n```");
        assert_eq!(parse_file_updates(&fenced)[0].path, "src/lib.rs");
    }

    #[test]
    fn a_single_object_reply_parses_as_one_update() {
        let reply =
            r#"{"path": "src/main/java/BddTest.java", "content": "public class BddTest {}"}"#;
        let updates = parse_file_updates(reply);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].path, "src/main/java/BddTest.java");
        let fenced = format!("```json\n{reply}\n```");
        assert_eq!(parse_file_updates(&fenced).len(), 1);
    }

    #[test]
    fn unusable_file_update_replies_are_an_empty_list() {
        assert!(parse_file_updates("Sure! Here is the code:").is_empty());
        assert!(parse_file_updates(r#"[{"path": "", "content": "x"}]"#).is_empty());
        assert!(parse_file_updates(r#"[{"path": "src/lib.rs", "content": "  "}]"#).is_empty());
    }

    #[test]
    fn code_fences_are_stripped_with_and_without_a_language_tag() {
        assert_eq!(strip_code_fences("```java\nclass A {}\n```"), "class A {}");
        assert_eq!(strip_code_fences("```\ncode\n```"), "code");
        assert_eq!(strip_code_fences("plain code"), "plain code");
        assert_eq!(strip_code_fences("```rust\nunclosed"), "unclosed");
        assert_eq!(
            strip_code_fences("<think>scratch</think>\n[{\"title\":\"T\"}]"),
            "[{\"title\":\"T\"}]"
        );
    }

    #[test]
    fn identifier_casing_handles_leading_digits_and_empty_text() {
        assert_eq!(snake_case("the result is 3"), "the_result_is_3");
        assert_eq!(snake_case("3 numbers"), "_3_numbers");
        assert_eq!(snake_case("!!!"), "step");
        assert_eq!(pascal_case("3 numbers"), "N3Numbers");
        assert_eq!(pascal_case("!!!"), "Step");
        assert_eq!(camel_case("the result"), "theResult");
    }

    #[test]
    fn an_unclosed_think_block_is_kept() {
        assert_eq!(
            strip_think_block("<think>still going"),
            "<think>still going"
        );
        assert_eq!(strip_think_block("plain"), "plain");
    }

    #[test]
    fn looks_like_unit_test_rejects_empty_and_unrecognizable_code() {
        assert!(!looks_like_unit_test(Language::Java, "   "));
        assert!(!looks_like_unit_test(Language::Java, "class X {}"));
        assert!(looks_like_unit_test(Language::Java, "@Test void t() {}"));
    }

    #[test]
    fn looks_like_unit_test_for_accepts_missing_type_and_non_java() {
        assert!(looks_like_unit_test_for(
            Language::Java,
            "@Test void t() {}",
            None
        ));
        assert!(looks_like_unit_test_for(
            Language::Rust,
            "#[test] fn t() {}",
            Some("Foo")
        ));
        assert!(!looks_like_unit_test_for(
            Language::Java,
            "class X {}",
            Some("Kata")
        ));
    }
}

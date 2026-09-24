//! Deterministic code generation: step-definition and unit-test
//! templates per supported ecosystem, plus the validation applied to LLM
//! output before it may replace a template. Everything here is pure text
//! transformation.

use crate::domain::language::Language;
use crate::domain::model::Requirement;
use crate::domain::prompts::{RenderedPrompt, render};
use crate::domain::proposal::escape_controls;
use crate::domain::steps::{MissingStep, extract_patterns, step_to_expression};
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

/// What a generated unit test for one requirement is called, without a
/// directory: the resolved layout decides where it goes.
pub fn unit_test_file_name(language: Language, req_id: &str) -> String {
    let pascal = pascal_case(req_id);
    match language {
        Language::Java => format!("{pascal}Test.java"),
        Language::JavaScript => format!("{}.test.js", snake_case(req_id)),
        Language::TypeScript => format!("{}.test.ts", snake_case(req_id)),
        Language::DotNet => format!("{pascal}Test.cs"),
        Language::Rust => format!("{}_test.rs", snake_case(req_id)),
    }
}

/// Where a generated unit test for one requirement is staged in a
/// project with no layout of its own yet.
pub fn unit_test_target_path(language: Language, req_id: &str) -> String {
    let name = unit_test_file_name(language, req_id);
    match language {
        Language::Java => format!("src/test/java/{name}"),
        Language::JavaScript | Language::TypeScript => format!("test/{name}"),
        Language::DotNet => format!("Tests/{name}"),
        Language::Rust => format!("tests/{name}"),
    }
}

/// A pending step-definition file covering every missing step. Steps
/// whose texts collapse to the same cucumber expression (e.g. "the
/// result is 3" and "the result is 5" both become "the result is
/// {int}") share one definition - duplicates would make the runner
/// refuse every scenario as ambiguous.
pub fn step_definitions_template(language: Language, missing: &[MissingStep]) -> String {
    let body =
        step_definitions(language, missing, Vec::new(), &mut MemberNames::in_file("")).join("\n");
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

/// One definition per missing step whose cucumber expression `seen` does
/// not already cover, each named through `names` so no two definitions in
/// the finished file declare the same member.
fn step_definitions(
    language: Language,
    missing: &[MissingStep],
    mut seen: Vec<String>,
    names: &mut MemberNames,
) -> Vec<String> {
    missing
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
        .map(|step| step_definition(language, step, names))
        .collect()
}

fn step_definition(language: Language, step: &MissingStep, names: &mut MemberNames) -> String {
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
                expr = escape_literal(&expression, '"'),
                name = names.claim(camel_case(&name_source(&step.text))),
            )
        }
        Language::JavaScript | Language::TypeScript => {
            let params: Vec<String> = (0..placeholders).map(|i| format!("arg{i}")).collect();
            format!(
                "{keyword}('{expr}', function ({params}) {{\n  return 'pending';\n}});\n",
                keyword = step.keyword,
                expr = escape_literal(&expression, '\''),
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
                expr = escape_literal(&expression, '"'),
                name = names.claim(pascal_case(&name_source(&step.text))),
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
                expr = escape_literal(&expression, '"'),
                name = names.claim(snake_case(&name_source(&step.text))),
                text = escape_literal(&step.text, '"'),
            )
        }
    }
}

/// A failing (RED) unit-test file with one test per acceptance criterion.
pub fn unit_test_template(language: Language, requirement: &Requirement) -> String {
    let mut names = MemberNames::in_file("");
    let tests: Vec<String> = requirement
        .acceptance_criteria
        .iter()
        .map(|criterion| unit_test_case(language, criterion, &mut names))
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

fn unit_test_case(language: Language, criterion: &str, names: &mut MemberNames) -> String {
    let commented = escape_controls(criterion);
    match language {
        Language::Java => format!(
            "    @Test\n    void {name}() {{\n        // {commented}\n        fail(\"TODO: assert - {escaped}\");\n    }}\n",
            name = names.claim(snake_case(criterion)),
            escaped = escape_literal(criterion, '"'),
        ),
        // The JavaScript and TypeScript case name is a string, not an
        // identifier, so two criteria that read alike stay legal - there
        // is no slug here to collide.
        Language::JavaScript | Language::TypeScript => format!(
            "test('{name}', () => {{\n  // {commented}\n  assert.fail('TODO: assert - {escaped}');\n}});\n",
            name = escape_literal(criterion, '\''),
            escaped = escape_literal(criterion, '\''),
        ),
        Language::DotNet => format!(
            "    [Fact]\n    public void {name}()\n    {{\n        // {commented}\n        Assert.Fail(\"TODO: assert - {escaped}\");\n    }}\n",
            name = names.claim(pascal_case(criterion)),
            escaped = escape_literal(criterion, '"'),
        ),
        Language::Rust => format!(
            "#[test]\nfn {name}() {{\n    // {commented}\n    unimplemented!(\"TODO: assert - {escaped}\");\n}}\n",
            name = names.claim(snake_case(criterion)),
            escaped = escape_literal(criterion, '"'),
        ),
    }
}

/// Where the project's production code lives, by ecosystem convention.
/// Named after the project - the spec's `project` field.
pub fn implementation_target_path(language: Language, project: &str) -> String {
    let name = implementation_file_name(language, project);
    match language {
        Language::Java => format!("src/main/java/{name}"),
        Language::JavaScript | Language::TypeScript | Language::Rust => format!("src/{name}"),
        Language::DotNet => name,
    }
}

/// What the production file is called, without a directory.
pub fn implementation_file_name(language: Language, project: &str) -> String {
    let pascal = pascal_case(project);
    match language {
        Language::Java => format!("{pascal}.java"),
        Language::JavaScript => format!("{}.js", snake_case(project)),
        Language::TypeScript => format!("{}.ts", snake_case(project)),
        Language::DotNet => format!("{pascal}.cs"),
        Language::Rust => "lib.rs".into(),
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
pub(crate) fn brief_failure(failure: &str) -> String {
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

/// The fragment-scoped polish instructions used when the target file
/// already exists: the model is shown only the newly generated members,
/// never the file they will be spliced into. Rendered from the
/// `[polish_fragment]` templates.
pub fn polish_fragment_prompt(language: Language, members: &str, count: usize) -> RenderedPrompt {
    render(
        "polish_fragment",
        minijinja::context! {
            framework => language.bdd_framework(),
            language => language.display(),
            practices => best_practices(language),
            count => count,
            members => members,
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

/// The first JSON value in `body` opening with `open`. Models often
/// emit a complete payload and then commentary (or a second copy);
/// `from_str` rejects trailing data and would discard a usable reply.
pub(crate) fn decode_json<T: serde::de::DeserializeOwned>(body: &str, open: char) -> Option<T> {
    if let Ok(parsed) = serde_json::from_str(body) {
        return Some(parsed);
    }
    let start = body.find(open)?;
    let mut deserializer = serde_json::Deserializer::from_str(&body[start..]);
    T::deserialize(&mut deserializer).ok()
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
///
/// `class_name` is the target file's stem, where the ecosystem requires
/// the public type to match it. A polish pass that renames the class can
/// never compile (observed live: "class StringCalculatorSteps is public,
/// should be declared in a file named ..."), so the rename is rejected
/// and the deterministic template stands.
pub fn looks_like_step_definitions(
    language: Language,
    code: &str,
    class_name: Option<&str>,
) -> bool {
    !code.trim().is_empty()
        && match language {
            Language::Java => {
                (code.contains("@Given") || code.contains("@When") || code.contains("@Then"))
                    && class_name
                        .map(|name| code.contains(&format!("public class {name}")))
                        .unwrap_or(true)
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

/// Does this fragment try to declare the scope it is meant to be spliced
/// into? A polished fragment is inserted *inside* an existing class, so a
/// package line, an import, or a type declaration in the reply would
/// either nest illegally or duplicate what the file already has.
fn declares_enclosing_scope(language: Language, fragment: &str) -> bool {
    const MODIFIERS: [&str; 7] = [
        "public",
        "private",
        "protected",
        "final",
        "abstract",
        "static",
        "sealed",
    ];
    const DECLARATIONS: [&str; 5] = ["class", "interface", "enum", "record", "namespace"];
    fragment.lines().any(|line| {
        let mut rest = line.trim();
        if rest.starts_with("package ") || rest.starts_with("import ") || rest.starts_with("using ")
        {
            return true;
        }
        if matches!(language, Language::Rust) && rest.starts_with("mod ") {
            return true;
        }
        // Peel modifiers so `public final class Foo` is caught as readily
        // as a bare `class Foo`.
        while let Some(tail) = MODIFIERS
            .iter()
            .find_map(|word| rest.strip_prefix(word)?.strip_prefix(' '))
        {
            rest = tail.trim_start();
        }
        DECLARATIONS
            .iter()
            .any(|word| rest.strip_prefix(word).is_some_and(|t| t.starts_with(' ')))
    })
}

/// Polish gate for a step-definition *fragment*: the reply must declare
/// exactly the cucumber expressions it was given - no additions, no
/// losses, no edits - and must not wrap them in a class of its own.
pub fn looks_like_step_fragment(language: Language, fragment: &str, expected: &[String]) -> bool {
    if fragment.trim().is_empty() || declares_enclosing_scope(language, fragment) {
        return false;
    }
    let kept = extract_patterns(language, fragment);
    kept.len() == expected.len() && expected.iter().all(|pattern| kept.contains(pattern))
}

/// Every `fail("TODO: assert ...")` placeholder a generated unit test
/// fragment carries. These are the assertions the developer is meant to
/// sharpen by hand, so the polish pass may not quietly resolve them.
pub fn todo_placeholders(code: &str) -> Vec<String> {
    const OPEN: &str = "\"TODO: assert";
    code.match_indices(OPEN)
        .filter_map(|(start, _)| {
            let tail = &code[start + 1..];
            tail.find('"').map(|end| tail[..end].to_string())
        })
        .collect()
}

/// How many test cases a fragment declares.
fn count_test_cases(language: Language, code: &str) -> usize {
    let marker = match language {
        Language::Java => "@Test",
        Language::JavaScript | Language::TypeScript => "test(",
        Language::DotNet => "[Fact]",
        Language::Rust => "#[test]",
    };
    code.matches(marker).count()
}

/// Polish gate for a unit-test *fragment*: same number of test cases,
/// every TODO placeholder intact, and no class of its own. The
/// placeholders are the contract here - a reply that "helpfully" writes
/// the assertion has taken the exercise away from the developer.
pub fn looks_like_unit_test_fragment(
    language: Language,
    fragment: &str,
    expected_placeholders: &[String],
    expected_cases: usize,
) -> bool {
    if fragment.trim().is_empty() || declares_enclosing_scope(language, fragment) {
        return false;
    }
    if count_test_cases(language, fragment) != expected_cases {
        return false;
    }
    let kept = todo_placeholders(fragment);
    kept.len() == expected_placeholders.len()
        && expected_placeholders.iter().all(|todo| kept.contains(todo))
}

/// Append failing tests for `requirement` onto an existing test class
/// (the workshop `StringCalculatorTest` shape). Greenfield still uses
/// [`unit_test_template`].
pub fn append_unit_tests(existing: &str, language: Language, requirement: &Requirement) -> String {
    splice_unit_tests(
        existing,
        language,
        &unit_test_fragment(existing, language, requirement),
    )
}

/// The new test methods on their own, with no surrounding file.
///
/// Split out of [`append_unit_tests`] so the polish pass can be handed
/// the generated members alone: a model that never sees the rest of the
/// file cannot rename or reflow the tests already in it. `existing` is
/// still read - for the member names it spends, so the fragment cannot
/// redeclare a method the class already has.
pub fn unit_test_fragment(existing: &str, language: Language, requirement: &Requirement) -> String {
    let mut names = MemberNames::in_file(existing);
    requirement
        .acceptance_criteria
        .iter()
        .map(|criterion| unit_test_case_for(language, requirement, criterion, &mut names))
        .collect()
}

/// Insert `fragment` - freshly generated or polished - into an existing
/// test class. Bytes outside the splice point are carried over untouched.
pub fn splice_unit_tests(existing: &str, language: Language, fragment: &str) -> String {
    match language {
        Language::Java => append_java_methods(existing, fragment),
        _ => format!("{}\n{fragment}", existing.trim_end()),
    }
}

fn unit_test_case_for(
    language: Language,
    requirement: &Requirement,
    criterion: &str,
    names: &mut MemberNames,
) -> String {
    match language {
        Language::Java => format!(
            "    @Test\n    @DisplayName(\"{id}: {title}\")\n    void {name}() {{\n        // {commented}\n        fail(\"TODO: assert - {escaped}\");\n    }}\n",
            id = requirement.id,
            title = escape_literal(criterion, '"'),
            name = names.claim(snake_case(criterion)),
            commented = escape_controls(criterion),
            escaped = escape_literal(criterion, '"'),
        ),
        _ => unit_test_case(language, criterion, names),
    }
}

fn append_java_methods(existing: &str, methods: &str) -> String {
    let with_imports = ensure_java_import(
        &ensure_java_import(existing, "import org.junit.jupiter.api.DisplayName;"),
        "import static org.junit.jupiter.api.Assertions.fail;",
    );
    insert_into_class(&with_imports, methods)
}

/// Add members to the last class in a brace-delimited source, one blank
/// line below whatever the class already declares.
fn insert_into_class(source: &str, members: &str) -> String {
    match source.rfind('}') {
        Some(i) => format!("{}\n\n{members}\n{}", source[..i].trim_end(), &source[i..]),
        None => format!("{source}\n{members}"),
    }
}

/// Add definitions for `missing` to a file that already holds step
/// definitions, skipping every cucumber expression the file declares.
///
/// Generated steps have to join the existing file rather than start a
/// new one: two files declaring the same pattern is what Cucumber
/// refuses as a duplicate step definition.
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

/// The new step definitions on their own, with no surrounding file, or
/// `None` when `existing` already declares every missing expression.
///
/// Split out of [`append_step_definitions`] so the polish pass can be
/// handed the generated definitions alone: a model that never sees the
/// rest of the file cannot rename the definitions already binding
/// scenarios that pass today.
pub fn step_definitions_fragment(
    existing: &str,
    language: Language,
    missing: &[MissingStep],
) -> Option<String> {
    let definitions = step_definitions(
        language,
        missing,
        extract_patterns(language, existing),
        &mut MemberNames::in_file(existing),
    );
    if definitions.is_empty() {
        return None;
    }
    Some(definitions.join("\n"))
}

/// Insert `fragment` - freshly generated or polished - into a file that
/// already holds step definitions, adding whatever imports it needs.
/// Bytes outside the splice point are carried over untouched.
pub fn splice_step_definitions(existing: &str, language: Language, fragment: &str) -> String {
    match language {
        Language::Java | Language::DotNet => {
            insert_into_class(&ensure_cucumber_imports(existing, language), fragment)
        }
        _ => format!(
            "{}\n\n{fragment}",
            ensure_cucumber_imports(existing, language).trim_end()
        ),
    }
}

/// The imports a step definition needs, added only when absent.
fn ensure_cucumber_imports(source: &str, language: Language) -> String {
    match language {
        Language::Java => [
            "import io.cucumber.java.PendingException;",
            "import io.cucumber.java.en.Given;",
            "import io.cucumber.java.en.Then;",
            "import io.cucumber.java.en.When;",
        ]
        .iter()
        .fold(source.to_string(), |text, import| {
            ensure_java_import(&text, import)
        }),
        Language::DotNet => ensure_leading_line(source, "using Reqnroll;"),
        Language::JavaScript => ensure_leading_line(
            source,
            "const { Given, When, Then } = require('@cucumber/cucumber');",
        ),
        Language::TypeScript => ensure_leading_line(
            source,
            "import { Given, When, Then } from '@cucumber/cucumber';",
        ),
        Language::Rust => ensure_leading_line(source, "use cucumber::{given, then, when};"),
    }
}

fn ensure_leading_line(source: &str, line: &str) -> String {
    if source.contains(line) {
        return source.to_string();
    }
    format!("{line}\n{source}")
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

/// Spec text as the body of a generated string literal delimited by
/// `quote` - the one place criteria, step texts, and cucumber
/// expressions cross into source code.
///
/// The spec writes an input newline as the two characters `\n` (REQ-005's
/// `"1\n2,3"`), so a literal that escapes only the quote hands the
/// compiler a real newline escape: the assertion message arrives broken
/// across two lines at runtime. A criterion holding a real control
/// character instead - which a model reword introduces, see
/// [`crate::domain::proposal::escape_controls`] - ends the literal
/// mid-string and stops the file compiling at all.
///
/// Escaping in one pass over the characters is what makes the order
/// safe: a backslash written here is never re-read as the opening of the
/// next escape, which is the double-escaping that chained `replace`
/// calls invite.
pub(crate) fn escape_literal(text: &str, quote: char) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c == quote => {
                escaped.push('\\');
                escaped.push(c);
            }
            c => escaped.push(c),
        }
    }
    escaped
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

/// The member names one generated file may still spend.
///
/// Java, C#, and Rust names are slugged out of free text by dropping
/// everything that is not alphanumeric, so two texts that differ only in
/// punctuation collapse to one identifier. REQ-007 walks straight into
/// it: `//*` and `//;` are different delimiters and the same slug, and
/// the class stops compiling on "method ... is already defined". The
/// first claimant keeps the slug; every later one takes `_2`, `_3`, ...
/// in order, which is legal in all three languages and reads the same
/// way in each.
///
/// The suffix says nothing about what distinguishes the members, on
/// purpose. Folding the punctuation into the slug instead would rename
/// every generated member rather than only the colliding ones, and the
/// text is already carried verbatim beside the member - in Java's
/// `@DisplayName`, in the comment above the body, and in the TODO the
/// placeholder fails with.
struct MemberNames {
    taken: Vec<String>,
}

impl MemberNames {
    /// The names `source` already spends, so members appended to it join
    /// without redeclaring one of them. A fresh file is `in_file("")`.
    ///
    /// An identifier followed by `(` is claimed whether it declares a
    /// member or merely calls one: over-claiming costs a suffix nobody
    /// reads, under-claiming costs the build.
    fn in_file(source: &str) -> Self {
        let mut taken = Vec::new();
        let mut word = String::new();
        let mut candidate: Option<String> = None;
        for character in source.chars() {
            if character.is_alphanumeric() || character == '_' {
                word.push(character);
                continue;
            }
            if !word.is_empty() {
                candidate = Some(std::mem::take(&mut word));
            }
            match character {
                '(' => taken.extend(candidate.take()),
                c if c.is_whitespace() => {}
                _ => candidate = None,
            }
        }
        Self { taken }
    }

    /// `name`, or the first free `name_2`, `name_3`, ... after it.
    fn claim(&mut self, name: String) -> String {
        let mut claimed = name.clone();
        let mut ordinal = 1u32;
        while self.taken.contains(&claimed) {
            ordinal += 1;
            claimed = format!("{name}_{ordinal}");
        }
        self.taken.push(claimed.clone());
        claimed
    }
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
                looks_like_step_definitions(language, &code, Some("GeneratedSteps")),
                "{language:?} template fails its own gate: {code}"
            );
        }
    }

    #[test]
    fn appended_steps_join_the_existing_class_and_skip_what_it_declares() {
        let existing = "package com.example.kata;\n\n\
             import io.cucumber.java.en.Given;\n\n\
             public class CalculatorSteps {\n\
             \x20   @Given(\"a calculator\")\n\
             \x20   public void aCalculator() {}\n\
             }\n";
        let appended = append_step_definitions(
            existing,
            Language::Java,
            &[
                missing("Given", "a calculator"),
                missing("Then", "the result is 3"),
            ],
        );
        // The package and class survive, so the module still compiles.
        assert!(appended.starts_with("package com.example.kata;"));
        assert_eq!(appended.matches("public class CalculatorSteps").count(), 1);
        // A pattern the file already declares is not repeated - two
        // definitions of one expression is what Cucumber refuses.
        assert_eq!(appended.matches("@Given(\"a calculator\")").count(), 1);
        assert!(appended.contains("@Then(\"the result is {int}\")"));
        // The imports the new definition needs are added, once.
        assert_eq!(
            appended
                .matches("import io.cucumber.java.en.Given;")
                .count(),
            1
        );
        assert!(appended.contains("import io.cucumber.java.en.Then;"));
        assert!(appended.contains("import io.cucumber.java.PendingException;"));
    }

    #[test]
    fn the_step_fragment_holds_the_new_definitions_and_nothing_around_them() {
        let existing = "package com.example.kata;\n\n\
             public class CalculatorSteps {\n\
             \x20   @Given(\"a calculator\")\n\
             \x20   public void aCalculator() {}\n\
             }\n";
        let fragment = step_definitions_fragment(
            existing,
            Language::Java,
            &[
                missing("Given", "a calculator"),
                missing("Then", "the result is 3"),
            ],
        )
        .expect("one definition is missing");
        // Only the missing step, and none of the scaffolding that would
        // let a polish pass reach the rest of the file.
        assert!(fragment.contains("@Then(\"the result is {int}\")"));
        assert!(!fragment.contains("a calculator"), "{fragment}");
        assert!(!fragment.contains("package"), "{fragment}");
        assert!(!fragment.contains("class"), "{fragment}");

        // Splicing it back is exactly what appending does.
        assert_eq!(
            splice_step_definitions(existing, Language::Java, &fragment),
            append_step_definitions(
                existing,
                Language::Java,
                &[
                    missing("Given", "a calculator"),
                    missing("Then", "the result is 3"),
                ],
            )
        );
    }

    #[test]
    fn appended_members_are_separated_from_the_member_above_by_a_blank_line() {
        let existing = "public class CalculatorSteps {\n\n\
             \x20   @Given(\"a calculator\")\n\
             \x20   public void aCalculator() {\n\
             \x20   }\n\
             }\n";
        let once = append_step_definitions(
            existing,
            Language::Java,
            &[missing("Then", "the result is 3")],
        );
        assert!(
            once.contains("    }\n\n    @Then(\"the result is {int}\")"),
            "{once}"
        );

        // Appending again separates by one blank line, not two.
        let twice = append_step_definitions(
            &once,
            Language::Java,
            &[missing("When", "the numbers are added")],
        );
        assert!(!twice.contains("\n\n\n"), "{twice}");
        assert!(
            twice.contains("    }\n\n    @When(\"the numbers are added\")"),
            "{twice}"
        );
    }

    #[test]
    fn a_file_declaring_every_missing_step_yields_no_fragment() {
        let existing = "public class Steps {\n\
             \x20   @Given(\"a calculator\")\n    public void a() {}\n}\n";
        assert!(
            step_definitions_fragment(
                existing,
                Language::Java,
                &[missing("Given", "a calculator")]
            )
            .is_none()
        );
    }

    #[test]
    fn the_step_fragment_gate_holds_the_expression_set_exactly() {
        let expected = vec!["the result is {int}".to_string()];
        let good = "    @Then(\"the result is {int}\")\n    public void resultIs(int n) {}\n";
        assert!(looks_like_step_fragment(Language::Java, good, &expected));

        // Renaming the method is the point; editing the expression is not.
        let altered =
            "    @Then(\"the result is {word}\")\n    public void resultIs(String n) {}\n";
        assert!(!looks_like_step_fragment(
            Language::Java,
            altered,
            &expected
        ));

        // Nor is inventing a definition that was not asked for.
        let extra = "    @Then(\"the result is {int}\")\n    public void a(int n) {}\n\
             \x20   @Given(\"a calculator\")\n    public void b() {}\n";
        assert!(!looks_like_step_fragment(Language::Java, extra, &expected));

        // A fragment is spliced inside a class, so it may not bring one.
        for wrapped in [
            "package com.example;\n    @Then(\"the result is {int}\")\n    public void a(int n) {}\n",
            "public class Steps {\n    @Then(\"the result is {int}\")\n    public void a(int n) {}\n}\n",
            "import io.cucumber.java.en.Then;\n    @Then(\"the result is {int}\")\n    public void a(int n) {}\n",
        ] {
            assert!(
                !looks_like_step_fragment(Language::Java, wrapped, &expected),
                "should be refused: {wrapped}"
            );
        }
        assert!(!looks_like_step_fragment(Language::Java, "  \n", &expected));
    }

    #[test]
    fn the_unit_test_fragment_gate_keeps_the_todo_placeholders() {
        let todos = vec!["TODO: assert - sums two numbers".to_string()];
        let good = "    @Test\n    void sums() {\n        fail(\"TODO: assert - sums two numbers\");\n    }\n";
        assert!(looks_like_unit_test_fragment(
            Language::Java,
            good,
            &todos,
            1
        ));

        // Writing the assertion takes the RED bar away from the developer.
        let resolved =
            "    @Test\n    void sums() {\n        assertEquals(3, c.add(\"1,2\"));\n    }\n";
        assert!(!looks_like_unit_test_fragment(
            Language::Java,
            resolved,
            &todos,
            1
        ));

        // Dropping or inventing a case is refused on the count alone.
        let two = format!("{good}{good}");
        assert!(!looks_like_unit_test_fragment(
            Language::Java,
            &two,
            &todos,
            1
        ));

        let wrapped = format!("class Test {{\n{good}}}\n");
        assert!(!looks_like_unit_test_fragment(
            Language::Java,
            &wrapped,
            &todos,
            1
        ));
    }

    #[test]
    fn todo_placeholders_are_read_back_out_of_generated_tests() {
        let code = "fail(\"TODO: assert - one\");\n  fail(\"TODO: assert - two\");\n";
        assert_eq!(
            todo_placeholders(code),
            vec!["TODO: assert - one", "TODO: assert - two"]
        );
        assert!(todo_placeholders("assertEquals(1, 1);").is_empty());
    }

    #[test]
    fn appending_nothing_new_leaves_the_file_untouched() {
        let existing = "import io.cucumber.java.en.Given;\n\
             public class Steps {\n\
             \x20   @Given(\"a calculator\")\n\
             \x20   public void aCalculator() {}\n\
             }\n";
        assert_eq!(
            append_step_definitions(
                existing,
                Language::Java,
                &[missing("Given", "a calculator")]
            ),
            existing
        );
    }

    #[test]
    fn appended_steps_keep_each_ecosystem_compiling() {
        for language in Language::ALL {
            let first = step_definitions_template(language, &[missing("Given", "a calculator")]);
            let appended =
                append_step_definitions(&first, language, &[missing("Then", "the result is 3")]);
            assert!(
                appended.contains("a calculator"),
                "{language:?} lost the existing step: {appended}"
            );
            assert!(
                looks_like_step_definitions(language, &appended, Some("GeneratedSteps")),
                "{language:?} append fails the gate: {appended}"
            );
        }
    }

    #[test]
    fn java_steps_validation_rejects_a_class_renamed_away_from_its_file() {
        // Seen live: the polish pass renamed the class while the file
        // kept its name, which can never compile.
        let renamed = "import io.cucumber.java.en.Given;\n\
             public class StringCalculatorSteps {\n\
                 @Given(\"a calculator\") public void polished() {}\n\
             }\n";
        assert!(!looks_like_step_definitions(
            Language::Java,
            renamed,
            Some("GeneratedSteps")
        ));
        // The same code is fine when that *is* the target file's name,
        // which is what appending to a discovered step file needs.
        assert!(looks_like_step_definitions(
            Language::Java,
            renamed,
            Some("StringCalculatorSteps")
        ));
        let kept = renamed.replace("StringCalculatorSteps", "GeneratedSteps");
        assert!(looks_like_step_definitions(
            Language::Java,
            &kept,
            Some("GeneratedSteps")
        ));
    }

    #[test]
    fn validation_rejects_empty_and_unrecognizable_output() {
        for language in Language::ALL {
            assert!(!looks_like_step_definitions(language, "   ", None));
            assert!(!looks_like_step_definitions(
                language,
                "I cannot help with that.",
                None
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

    // ---- member-name uniqueness ---------------------------------------

    /// The two REQ-007 custom-delimiter criteria, which differ only in
    /// the delimiter character, and the slug they both reduce to.
    const DELIMITER_CRITERIA: [&str; 2] = [
        r#"Given "//*\n1*2*3", when add is called, then the result is 6"#,
        r#"Given "//;\n1;2;3", when add is called, then the result is 6"#,
    ];
    const DELIMITER_SLUG: &str = "given_n1_2_3_when_add_is_called_then_the_result_is_6";

    /// The Java methods a generated file or fragment declares.
    fn declared_members(code: &str) -> Vec<String> {
        code.lines()
            .filter_map(|line| line.trim().strip_prefix("void "))
            .filter_map(|rest| rest.split_once('('))
            .map(|(name, _)| name.to_string())
            .collect()
    }

    #[test]
    fn two_criteria_differing_only_in_punctuation_get_distinct_member_names() {
        // Observed live on REQ-007: both criteria slugged to one name,
        // `run_tests` came back tests=0 errors=1, and javac said the
        // method was already defined in StringCalculatorTest.
        let code = unit_test_fragment("", Language::Java, &requirement_with(&DELIMITER_CRITERIA));
        assert_eq!(
            declared_members(&code),
            vec![DELIMITER_SLUG.to_string(), format!("{DELIMITER_SLUG}_2")],
            "{code}"
        );
        // The suffix disambiguates the identifier and nothing else: each
        // criterion still reaches the reader whole.
        assert!(code.contains(r#"//*\n1*2*3"#), "{code}");
        assert!(code.contains(r#"//;\n1;2;3"#), "{code}");
        assert_eq!(code.matches("@DisplayName").count(), 2, "{code}");
    }

    #[test]
    fn a_criterion_never_redeclares_a_method_the_class_already_has() {
        let existing = "import org.junit.jupiter.api.Test;\n\n\
             class StringCalculatorTest {\n\n\
             \x20   @Test\n\
             \x20   void given_1_2_when_add_is_called_then_the_result_is_3() {\n\
             \x20   }\n\
             }\n";
        let appended = append_unit_tests(
            existing,
            Language::Java,
            &requirement_with(&[r#"Given "1,2", when add is called, then the result is 3"#]),
        );
        assert_eq!(
            declared_members(&appended),
            vec![
                "given_1_2_when_add_is_called_then_the_result_is_3",
                "given_1_2_when_add_is_called_then_the_result_is_3_2",
            ],
            "{appended}"
        );
    }

    #[test]
    fn a_three_way_collision_numbers_the_second_and_the_third() {
        let mut criteria = DELIMITER_CRITERIA.to_vec();
        criteria.push(r#"Given "//|\n1|2|3", when add is called, then the result is 6"#);
        let code = unit_test_fragment("", Language::Java, &requirement_with(&criteria));
        assert_eq!(
            declared_members(&code),
            vec![
                DELIMITER_SLUG.to_string(),
                format!("{DELIMITER_SLUG}_2"),
                format!("{DELIMITER_SLUG}_3"),
            ],
            "{code}"
        );
    }

    #[test]
    fn generating_the_same_criteria_twice_names_them_the_same_way() {
        let requirement = requirement_with(&DELIMITER_CRITERIA);
        // Greenfield, and again onto a class that already holds the
        // members of a previous run: the same inputs must give the same
        // names, or a regenerate churns the diff for no reason.
        let class = "class StringCalculatorTest {\n}\n";
        let once = append_unit_tests(class, Language::Java, &requirement);
        assert_eq!(once, append_unit_tests(class, Language::Java, &requirement));
        assert_eq!(
            unit_test_fragment(&once, Language::Java, &requirement),
            unit_test_fragment(&once, Language::Java, &requirement)
        );
        // Appending the same criteria to the file they already produced
        // still keeps every member apart.
        let twice = append_unit_tests(&once, Language::Java, &requirement);
        let members = declared_members(&twice);
        let mut unique = members.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(members.len(), 4, "{twice}");
        assert_eq!(unique.len(), members.len(), "{twice}");
    }

    #[test]
    fn every_language_that_slugs_a_criterion_keeps_the_members_apart() {
        let requirement = requirement_with(&DELIMITER_CRITERIA);
        for (language, first, second) in [
            (
                Language::Java,
                format!("void {DELIMITER_SLUG}()"),
                format!("void {DELIMITER_SLUG}_2()"),
            ),
            (
                Language::DotNet,
                "public void GivenN123WhenAddIsCalledThenTheResultIs6()".to_string(),
                "public void GivenN123WhenAddIsCalledThenTheResultIs6_2()".to_string(),
            ),
            (
                Language::Rust,
                format!("fn {DELIMITER_SLUG}()"),
                format!("fn {DELIMITER_SLUG}_2()"),
            ),
        ] {
            let code = unit_test_template(language, &requirement);
            assert!(
                code.contains(&first),
                "{language:?} missing {first}:\n{code}"
            );
            assert!(
                code.contains(&second),
                "{language:?} missing {second}:\n{code}"
            );
        }
        // JavaScript and TypeScript name a case with a string, not an
        // identifier, so there is no slug to collide and both criteria
        // keep their own words.
        for language in [Language::JavaScript, Language::TypeScript] {
            let code = unit_test_template(language, &requirement);
            assert_eq!(code.matches("test('").count(), 2, "{language:?}: {code}");
            assert!(code.contains(r#"//*\n1*2*3"#), "{language:?}: {code}");
            assert!(code.contains(r#"//;\n1;2;3"#), "{language:?}: {code}");
        }
    }

    #[test]
    fn two_step_texts_that_slug_alike_get_distinct_definition_names() {
        // Two different cucumber expressions - so both definitions are
        // generated - that reduce to one identifier: the quoted argument
        // and the bare asterisk are both dropped before casing.
        let steps = [
            missing("Then", "the delimiter is \";\""),
            missing("Then", "the delimiter is *"),
        ];
        let code = step_definitions_template(Language::Java, &steps);
        assert!(
            code.contains("public void theDelimiterIs(String arg0)"),
            "{code}"
        );
        assert!(code.contains("public void theDelimiterIs_2()"), "{code}");
    }

    #[test]
    fn an_appended_step_definition_never_redeclares_a_method_the_file_has() {
        let existing = "import io.cucumber.java.en.Then;\n\n\
             public class GeneratedSteps {\n\n\
             \x20   @Then(\"the delimiter is {string}\")\n\
             \x20   public void theDelimiterIs(String arg0) {}\n\
             }\n";
        let appended = append_step_definitions(
            existing,
            Language::Java,
            &[missing("Then", "the delimiter is *")],
        );
        assert_eq!(
            appended.matches("public void theDelimiterIs(").count(),
            1,
            "{appended}"
        );
        assert!(
            appended.contains("public void theDelimiterIs_2()"),
            "{appended}"
        );
    }

    /// What `javac` makes of the body of a string literal: the value the
    /// generated test prints at runtime. Asserting on this is what pins
    /// the property the workshop actually reads off the projector.
    fn java_string_value(body: &str) -> String {
        let mut value = String::new();
        let mut characters = body.chars();
        while let Some(character) = characters.next() {
            if character != '\\' {
                value.push(character);
                continue;
            }
            match characters.next() {
                Some('n') => value.push('\n'),
                Some('r') => value.push('\r'),
                Some('t') => value.push('\t'),
                Some(other) => value.push(other),
                None => panic!("literal ends on a dangling backslash: {body}"),
            }
        }
        value
    }

    /// The body of the literal `open` introduces, ending where javac ends
    /// it: at the first quote no backslash escapes.
    fn literal_body<'a>(code: &'a str, open: &str) -> &'a str {
        let start = code
            .find(open)
            .unwrap_or_else(|| panic!("no {open} in:\n{code}"))
            + open.len();
        let rest = &code[start..];
        let mut escaped = false;
        for (index, character) in rest.char_indices() {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => return &rest[..index],
                '\n' => panic!("literal ran off the end of its line:\n{code}"),
                _ => {}
            }
        }
        panic!("unterminated literal in:\n{code}")
    }

    fn requirement_with(criteria: &[&str]) -> Requirement {
        Requirement {
            acceptance_criteria: criteria.iter().map(|c| (*c).into()).collect(),
            ..requirement()
        }
    }

    #[test]
    fn the_escape_in_a_criterion_reaches_java_as_the_two_characters_the_spec_wrote() {
        // REQ-005 verbatim: the spec writes the input newline as the two
        // characters `\n`. Escaping only the quote left javac reading a
        // real newline, and the RED bar the room is watching broke across
        // two lines mid-message.
        let criterion = r#"Given "4\n5\n6", when add is called, then the result is 15"#;
        let code = unit_test_fragment("", Language::Java, &requirement_with(&[criterion]));
        assert!(
            code.contains(r#"fail("TODO: assert - Given \"4\\n5\\n6\", when add is called, then the result is 15");"#),
            "{code}"
        );
        assert_eq!(
            java_string_value(literal_body(&code, "fail(\"")),
            format!("TODO: assert - {criterion}"),
            "what Java prints must be the criterion as the JSON writes it"
        );
        // One line each, or the assertion message wraps on the projector.
        for line in code.lines() {
            assert!(line.len() < 200 || !line.contains("TODO"), "{code}");
        }
        assert_eq!(code.matches("TODO: assert").count(), 1, "{code}");
    }

    #[test]
    fn a_quote_a_backslash_before_a_quote_and_both_together_round_trip() {
        for criterion in [
            r#"Given "1,2", when add is called, then the result is 3"#,
            r#"Given a trailing escape "1,2\", when add is called, then an error is raised"#,
            r#"Given "1\n2", when add is called, then the message is "ok""#,
        ] {
            let code = unit_test_fragment("", Language::Java, &requirement_with(&[criterion]));
            assert_eq!(
                java_string_value(literal_body(&code, "fail(\"")),
                format!("TODO: assert - {criterion}"),
                "round trip failed for {criterion:?}:\n{code}"
            );
            assert_eq!(
                java_string_value(literal_body(&code, "@DisplayName(\"")),
                format!("REQ-001: {criterion}"),
                "the display name must carry the criterion too:\n{code}"
            );
        }
    }

    #[test]
    fn a_real_control_character_in_a_criterion_never_escapes_its_literal_or_its_comment() {
        // Not hypothetical: REQ-007 in the workshop spec holds real
        // newlines, put there by a model reword that ProposedRequirement
        // ::normalized did not get to. Interpolated raw, the comment
        // above the placeholder ends mid-criterion and the rest of the
        // line is stray Java - a compile error, not a cosmetic one.
        let criterion = "Given the input \"//+\n1+2\", when add is called, then the result is 3";
        let code = unit_test_fragment("", Language::Java, &requirement_with(&[criterion]));
        assert!(
            code.contains(
                r#"        // Given the input "//+\n1+2", when add is called, then the result is 3"#
            ),
            "{code}"
        );
        for line in code.lines() {
            let trimmed = line.trim();
            assert!(
                trimmed.is_empty()
                    || trimmed.starts_with("//")
                    || trimmed.starts_with('@')
                    || trimmed.starts_with("void ")
                    || trimmed.starts_with("fail(")
                    || trimmed == "}",
                "a criterion's newline split the file open:\n{code}"
            );
        }
        assert_eq!(
            java_string_value(literal_body(&code, "fail(\"")),
            format!("TODO: assert - {criterion}"),
            "a real newline still round trips - it is simply written as an escape"
        );
    }

    #[test]
    fn every_language_escapes_the_backslash_a_criterion_carries() {
        let requirement =
            requirement_with(&[r#"Given "1\n2,3", when add is called, then the result is 6"#]);
        for language in Language::ALL {
            let code = unit_test_template(language, &requirement);
            assert!(
                code.contains(r"1\\n2,3"),
                "{language:?} left the backslash unescaped:\n{code}"
            );
            assert!(
                !code.lines().any(|line| line.ends_with(r"1\n2,3")),
                "{language:?} still writes a bare escape into a literal:\n{code}"
            );
            assert!(
                looks_like_unit_test(language, &code),
                "{language:?} fails its own gate:\n{code}"
            );
        }
    }

    #[test]
    fn a_step_definition_escapes_the_expression_and_the_text_it_quotes() {
        // The Rust template interpolates the whole step text into a
        // todo!, quotes and all - unescaped it never compiled.
        let code = step_definitions_template(
            Language::Rust,
            &[missing("When", r#"add is called with "1\n2""#)],
        );
        assert!(
            code.contains(r#"todo!("implement step: add is called with \"1\\n2\"");"#),
            "{code}"
        );

        // An expression only keeps a backslash when it falls outside the
        // quoted span step_to_expression collapses to {string}.
        for (language, expected) in [
            (Language::Java, r#"@Then("the delimiter is \\n")"#),
            (Language::DotNet, r#"[Then("the delimiter is \\n")]"#),
            (Language::Rust, r#"#[then(expr = "the delimiter is \\n")]"#),
            (Language::JavaScript, r"Then('the delimiter is \\n'"),
            (Language::TypeScript, r"Then('the delimiter is \\n'"),
        ] {
            let code =
                step_definitions_template(language, &[missing("Then", r"the delimiter is \n")]);
            assert!(
                code.contains(expected),
                "{language:?} wants {expected}:\n{code}"
            );
        }
    }

    #[test]
    fn an_escaped_expression_is_read_back_as_the_expression_it_was_generated_from() {
        // Emitting `\\n` is only half of it: the next steps generate
        // reads the file back to see what it already declares. If the
        // escape did not collapse again the pattern would look missing
        // and a second, ambiguous definition would be appended.
        let step = missing("Then", r"the delimiter is \n");
        let generated = step_definitions_template(Language::Java, std::slice::from_ref(&step));
        assert_eq!(
            extract_patterns(Language::Java, &generated),
            vec![r"the delimiter is \n".to_string()]
        );
        assert!(
            step_definitions_fragment(&generated, Language::Java, &[step]).is_none(),
            "a step the file already declares must not be generated twice:\n{generated}"
        );
    }

    #[test]
    fn escape_literal_escapes_the_delimiter_in_use_and_nothing_else() {
        assert_eq!(escape_literal(r#"a "b" c"#, '"'), r#"a \"b\" c"#);
        assert_eq!(escape_literal(r#"a "b" c"#, '\''), r#"a "b" c"#);
        assert_eq!(escape_literal(r"a 'b' c", '\''), r"a \'b\' c");
        // Backslash first: a doubled backslash must not swallow the
        // quote that follows it.
        assert_eq!(escape_literal(r#"trailing\"#, '"'), r"trailing\\");
        assert_eq!(escape_literal(r#"\"#, '"'), r"\\");
        assert_eq!(escape_literal("tab\there", '"'), r"tab\there");
        assert_eq!(escape_literal("cr\r\nlf", '"'), r"cr\r\nlf");
        assert_eq!(escape_literal("plain", '"'), "plain");
        assert_eq!(escape_literal("", '"'), "");
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

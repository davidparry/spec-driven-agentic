//! Filling in the step definitions a generated file still leaves
//! pending, as a fragment: the definitions come out of the file, go to
//! the model with the production code they should call, and go back in
//! where they were. The rest of the file is never in the conversation.
//!
//! Why a fragment and not the file. The step-definition file of a
//! project that has been at this for a while is the largest file in
//! it - this crate's `tests/cucumber.rs` is 5,600 lines - and a prompt
//! that asks for the whole file back gets nothing back at all. Three
//! measured `spec deliver` runs on this crate each spent every
//! implementation attempt on the production file while the bar stayed
//! RED on three `todo!()` step bodies the model was never going to
//! return a 5,600-line file to fill. The pending definitions themselves
//! are a few dozen lines. That is the brief.

use std::sync::LazyLock;

use regex::Regex;

use crate::domain::generation::{best_practices, is_pending_step_body, looks_like_step_fragment};
use crate::domain::language::Language;
use crate::domain::model::Requirement;
use crate::domain::prompts::{RenderedPrompt, render};
use crate::domain::steps::{compile_pattern, definition_regex, split_step};

/// How much of the file above the first step definition travels with
/// the fragment: the imports, the `World` struct, the helpers a step
/// body is expected to use. Cut on a line boundary.
pub const PREAMBLE_BUDGET: usize = 48 * 1024;

/// One step definition in a source file: its pattern and the byte span
/// of its complete text, attribute through closing brace, with any
/// comment lines directly above it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionSpan {
    pub pattern: String,
    pub start: usize,
    pub end: usize,
}

impl DefinitionSpan {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// Every step definition in `source`, with where it is.
///
/// The span runs from the first comment line directly above the
/// attribute to the end of the line that closes the body. A body whose
/// braces cannot be matched - a file mid-edit - yields no span rather
/// than a wrong one, so nothing downstream splices over a neighbour.
pub fn definition_spans(language: Language, source: &str) -> Vec<DefinitionSpan> {
    definition_regex(language)
        .captures_iter(source)
        .filter_map(|captures| {
            let whole = captures.get(0)?;
            let pattern = captures
                .iter()
                .skip(1)
                .flatten()
                .next()
                .map(|m| crate::domain::steps::unescape_literal(m.as_str()))?;
            let start = span_start(source, whole.start());
            let end = span_end(language, source, whole.end())?;
            Some(DefinitionSpan {
                pattern,
                start,
                end,
            })
        })
        .collect()
}

/// The definitions in `source` whose body is still a generated
/// placeholder and whose pattern matches one of the requirement's
/// scenario lines (`step_lines`, keywords stripped). In file order.
///
/// Scoped to the requirement's own steps on purpose: another
/// requirement's stub is not this attempt's to fill, and a step the
/// developer wrote by hand is not pending however it reads.
pub fn pending_step_definitions(
    language: Language,
    source: &str,
    step_lines: &[String],
) -> Vec<DefinitionSpan> {
    definition_spans(language, source)
        .into_iter()
        .filter(|span| is_pending_step_body(language, span.text(source)))
        .filter(|span| {
            compile_pattern(&span.pattern)
                .is_some_and(|matcher| step_lines.iter().any(|line| matcher.is_match(line)))
        })
        .collect()
}

/// The step texts of every scenario step in `steps`, keyword dropped -
/// the form a step-definition pattern is matched against.
pub fn step_texts(steps: &[String]) -> Vec<String> {
    steps
        .iter()
        .map(|step| split_step(step).1.to_string())
        .collect()
}

/// The file above its first step definition, cut to [`PREAMBLE_BUDGET`]
/// on a line boundary. What a step body needs to see to be written
/// against the file it lives in: the world type and the fixtures.
pub fn step_file_preamble(language: Language, source: &str) -> &str {
    let first = definition_spans(language, source)
        .first()
        .map_or(source.len(), |span| span.start);
    let preamble = &source[..first];
    if preamble.len() <= PREAMBLE_BUDGET {
        return preamble;
    }
    let cut = preamble[..PREAMBLE_BUDGET]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    &preamble[..cut]
}

/// `source` with each of `spans` replaced by the definition in
/// `filled` that declares the same pattern. Spans are replaced last to
/// first so earlier offsets stay true; bytes outside them are carried
/// over untouched. A pattern `filled` does not declare keeps its
/// original text.
pub fn replace_step_definitions(
    language: Language,
    source: &str,
    spans: &[DefinitionSpan],
    filled: &str,
) -> String {
    let replacements = definition_spans(language, filled);
    let mut out = source.to_string();
    let mut ordered: Vec<&DefinitionSpan> = spans.iter().collect();
    ordered.sort_by_key(|span| std::cmp::Reverse(span.start));
    for span in ordered {
        if let Some(replacement) = replacements.iter().find(|r| r.pattern == span.pattern) {
            out.replace_range(span.start..span.end, replacement.text(filled));
        }
    }
    out
}

/// Whether a reply to the fill prompt is usable: exactly the patterns
/// it was given, no enclosing scope of its own, and no body still
/// pending. A reply that kept a placeholder declined the question for
/// that step, and that is the one thing this pass exists to refuse.
pub fn looks_like_filled_steps(language: Language, reply: &str, expected: &[String]) -> bool {
    looks_like_step_fragment(language, reply, expected)
        && definition_spans(language, reply)
            .iter()
            .all(|span| !is_pending_step_body(language, span.text(reply)))
}

/// What the fill prompt is about: the requirement's scenarios, the
/// pending definitions, the head of the file they live in, the
/// production code they should reach, and the last run's failures.
pub struct FillBrief<'a> {
    pub scenarios: &'a str,
    pub fragment: &'a str,
    pub count: usize,
    pub preamble: &'a str,
    pub production: &'a [(String, String)],
    pub failures: &'a [String],
}

/// The `[fill_steps]` prompt.
pub fn fill_steps_prompt(
    language: Language,
    requirement: &Requirement,
    brief: &FillBrief<'_>,
) -> RenderedPrompt {
    #[derive(serde::Serialize)]
    struct File<'a> {
        path: &'a str,
        content: &'a str,
    }
    let production: Vec<File<'_>> = brief
        .production
        .iter()
        .map(|(path, content)| File { path, content })
        .collect();
    render(
        "fill_steps",
        minijinja::context! {
            language => language.display(),
            framework => language.bdd_framework(),
            practices => best_practices(language),
            id => requirement.id,
            title => requirement.title,
            story => requirement.story,
            criteria => requirement.acceptance_criteria,
            scenarios => brief.scenarios,
            fragment => brief.fragment,
            count => brief.count,
            preamble => brief.preamble,
            production,
            failures => brief.failures,
        },
    )
}

/// The world type a step file's definitions share state through, and
/// where its body is - the Rust struct deriving `World`, or the step
/// class for Java and C#. `body_start..body_end` is the text between
/// the braces; `fields` is the names already declared in it.
///
/// A Rust world the template left as a unit struct (`struct W;`) has
/// no body yet: `body_start == body_end` sits on the `;`, and
/// [`insert_fields`] gives it braces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSpan {
    pub name: String,
    pub body_start: usize,
    pub body_end: usize,
    pub fields: Vec<String>,
    unit: bool,
}

/// What a fill reply carried besides the definitions: the new world
/// fields it asked for, and the definitions with that fragment taken
/// out so the usual readers see only definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldFragment {
    pub definitions: String,
    pub fields: Vec<String>,
}

/// Where the world type is declared in `source`, when the language has
/// one and the file declares it. The JS family keeps state in `this`
/// and has nothing to declare, so it is `None` there.
pub fn world_span(language: Language, source: &str) -> Option<WorldSpan> {
    let (name, after_name) = match language {
        Language::Rust => {
            let captures = crate::domain::generation::WORLD_DERIVE.captures(source)?;
            let name = captures.get(1)?;
            (name.as_str().to_string(), name.end())
        }
        Language::Java | Language::DotNet => {
            let captures = STEP_CLASS.captures(source)?;
            let name = captures.get(1)?;
            (name.as_str().to_string(), name.end())
        }
        Language::JavaScript | Language::TypeScript => return None,
    };
    let rest = &source[after_name..];
    let next = after_name + rest.len() - rest.trim_start().len();
    match source.as_bytes().get(next)? {
        b';' if language == Language::Rust => Some(WorldSpan {
            name,
            body_start: next,
            body_end: next,
            fields: Vec::new(),
            unit: true,
        }),
        b'{' => {
            let close = closing_brace(source, next)?;
            let body = &source[next + 1..close];
            let fields = body
                .lines()
                .filter_map(|line| field_name(language, line))
                .collect();
            Some(WorldSpan {
                name,
                body_start: next + 1,
                body_end: close,
                fields,
                unit: false,
            })
        }
        _ => None,
    }
}

/// The reply to a fill prompt, split into the definitions and the new
/// world fields it declares, if any. The world block - `struct Name {
/// ... }` or `class Name { ... }`, attributes above it included - comes
/// out whole; what it held must be fields, every one of them new.
///
/// Refused with a reason the model can act on when the block holds a
/// method or another member, or when the reply declares a type that
/// is neither a definition nor the world: a second struct cannot be
/// spliced anywhere.
pub fn split_world_fragment(
    language: Language,
    reply: &str,
    world: &WorldSpan,
) -> Result<WorldFragment, String> {
    let keyword = match language {
        Language::Rust => "struct",
        Language::Java | Language::DotNet => "class",
        Language::JavaScript | Language::TypeScript => {
            return Ok(WorldFragment {
                definitions: reply.to_string(),
                fields: Vec::new(),
            });
        }
    };
    let declaration = Regex::new(&format!(
        r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+|public\s+|final\s+|static\s+)*{keyword}\s+{}\b",
        regex::escape(&world.name)
    ))
    .expect("valid regex");
    let Some(found) = declaration.find(reply) else {
        return Ok(WorldFragment {
            definitions: reply.to_string(),
            fields: Vec::new(),
        });
    };
    let open = found.end()
        + reply[found.end()..]
            .find('{')
            .ok_or_else(|| format!("the {} fragment in the reply has no body", world.name))?;
    let close = closing_brace(reply, open)
        .ok_or_else(|| format!("the {} fragment in the reply never closes", world.name))?;
    let mut fields = Vec::new();
    for line in reply[open + 1..close].lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("///") {
            continue;
        }
        let Some(name) = field_name(language, line) else {
            return Err(format!(
                "the {} fragment may hold only its new fields, but it held {trimmed:?}; \
                 declare fields there and nothing else",
                world.name
            ));
        };
        if world.fields.contains(&name)
            || fields
                .iter()
                .any(|f: &String| field_name(language, f) == Some(name.clone()))
        {
            continue;
        }
        fields.push(normalized_field(language, trimmed));
    }
    let start = declaration_start(reply, found.start());
    let end = reply[close..]
        .find('\n')
        .map_or(reply.len(), |n| close + n + 1);
    let mut definitions = String::with_capacity(reply.len());
    definitions.push_str(&reply[..start]);
    definitions.push_str(&reply[end..]);
    let definitions = definitions.trim_start_matches('\n').to_string();
    if language == Language::Rust && OTHER_STRUCT.is_match(&definitions) {
        return Err(
            "the reply declared another struct; only the definitions asked for and new \
             fields on the world type can be used"
                .into(),
        );
    }
    Ok(WorldFragment {
        definitions,
        fields,
    })
}

/// `source` with `fields` added to the world's body, one per line at
/// the body's indentation. A Rust struct body is all fields, so they
/// go at its end; a class body holds methods too, so they go after
/// the last field already there, or first when there is none. A unit
/// struct gets braces.
pub fn insert_fields(
    language: Language,
    source: &str,
    world: &WorldSpan,
    fields: &[String],
) -> String {
    if fields.is_empty() {
        return source.to_string();
    }
    let body = &source[world.body_start..world.body_end];
    let closing_indent = body.rfind('\n').map_or("", |n| &body[n + 1..]);
    let closing_indent = if closing_indent.chars().all(char::is_whitespace) {
        closing_indent
    } else {
        ""
    };
    let indent = body
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| &line[..line.len() - line.trim_start().len()])
        .filter(|indent| !indent.is_empty())
        .map_or_else(|| format!("{closing_indent}    "), str::to_string);
    let block: String = fields
        .iter()
        .map(|field| format!("{indent}{field}\n"))
        .collect();
    let (replace_from, replace_to, new_text) = if world.unit {
        (
            world.body_start,
            world.body_end + 1,
            format!(" {{\n{block}}}"),
        )
    } else if body.trim().is_empty() {
        (
            world.body_start,
            world.body_end,
            format!("\n{block}{closing_indent}"),
        )
    } else if language == Language::Rust {
        (
            world.body_start,
            world.body_end,
            format!("{}\n{block}{closing_indent}", body.trim_end()),
        )
    } else {
        // After the last field line, or at the top of the body.
        let mut after = world.body_start + 1;
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            if field_name(language, line).is_some() {
                after = world.body_start + offset + line.len();
            }
            offset += line.len();
        }
        let at = after.min(world.body_end);
        let lead = if source[..at].ends_with('\n') {
            ""
        } else {
            "\n"
        };
        (at, at, format!("{lead}{block}"))
    };
    let mut out = String::with_capacity(source.len() + new_text.len());
    out.push_str(&source[..replace_from]);
    out.push_str(&new_text);
    out.push_str(&source[replace_to..]);
    out
}

/// The step class of a Java or C# step file: the first `class` declared.
static STEP_CLASS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)").expect("valid regex"));

/// A struct declaration at the start of a line - what a Rust reply may
/// not carry once the world block is out.
static OTHER_STRUCT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?struct\s+[A-Za-z_]").expect("valid regex")
});

/// A Rust field: `name: Type` with an optional `pub` and trailing comma.
static RUST_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][A-Za-z0-9_]*)\s*:\s*[^,]+,?\s*$")
        .expect("valid regex")
});

/// The name a field line declares, or `None` when the line is not a
/// field: a method, an attribute, a nested type.
pub fn field_name(language: Language, line: &str) -> Option<String> {
    let trimmed = line.trim();
    match language {
        Language::Rust => RUST_FIELD
            .captures(trimmed)
            .map(|captures| captures[1].to_string()),
        Language::Java | Language::DotNet => {
            let statement = trimmed.strip_suffix(';')?;
            let declaration = statement.split('=').next()?.trim();
            if declaration.contains('(') || declaration.contains('{') {
                return None;
            }
            let name = declaration.rsplit(|c: char| c.is_whitespace()).next()?;
            let is_name = !name.is_empty()
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                && declaration.split_whitespace().count() >= 2;
            is_name.then(|| name.to_string())
        }
        Language::JavaScript | Language::TypeScript => None,
    }
}

/// A field line as the file will carry it: Rust fields end in a comma.
fn normalized_field(language: Language, line: &str) -> String {
    match language {
        Language::Rust if !line.ends_with(',') => format!("{line},"),
        _ => line.to_string(),
    }
}

/// Where a definition's text begins: the start of the attribute's
/// line, or of the first comment line in the run directly above it.
fn span_start(source: &str, attribute_at: usize) -> usize {
    line_run_start(source, attribute_at, |line| {
        line.starts_with("//") || line.starts_with("/*") || line.starts_with('*')
    })
}

/// Where a type declaration's text begins: its line, or the first
/// attribute or comment line in the run directly above it.
fn declaration_start(source: &str, declaration_at: usize) -> usize {
    line_run_start(source, declaration_at, |line| {
        line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
            || line.starts_with("#[")
            || line.starts_with('[')
    })
}

/// The start of the line holding `at`, moved up over every directly
/// preceding line `leads` accepts.
fn line_run_start(source: &str, at: usize, leads: impl Fn(&str) -> bool) -> usize {
    let mut start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    while let Some(previous_end) = start.checked_sub(1) {
        let previous_start = source[..previous_end].rfind('\n').map_or(0, |i| i + 1);
        let line = source[previous_start..previous_end].trim();
        if !leads(line) {
            break;
        }
        start = previous_start;
    }
    start
}

/// Where a definition's text ends: the end of the line that closes the
/// body opened by the first `{` after the attribute. Strings and line
/// comments are skipped so a brace inside them does not count. For the
/// JS family the body is an argument, so the call's `)` and `;` are
/// taken along with it.
fn span_end(language: Language, source: &str, after_attribute: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let open = after_attribute + source[after_attribute..].find('{')?;
    let close = closing_brace(source, open)?;
    let mut end = close + 1;
    if matches!(language, Language::JavaScript | Language::TypeScript) {
        while end < bytes.len() && matches!(bytes[end], b')' | b';' | b' ') {
            end += 1;
        }
    }
    Some(
        source[end..]
            .find('\n')
            .map_or(bytes.len(), |n| end + n + 1),
    )
}

/// The index of the `}` closing the `{` at `open`, skipping strings
/// and line comments so a brace inside them does not count. `None`
/// when the braces never balance.
fn closing_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index)?,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |n| index + n);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// The index just past the closing quote of the string literal opening
/// at `open`, honouring backslash escapes.
fn skip_string(bytes: &[u8], open: usize) -> Option<usize> {
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST_FILE: &str = r#"use cucumber::{World, given, then, when};

#[derive(Debug, Default, World)]
struct KataWorld {
    result: Option<i32>,
}

fn helper() {}

#[given(expr = "an input {string}")]
fn an_input(world: &mut KataWorld, input: String) {
    world.result = Some(input.len() as i32);
}

/// A stub the generator left.
#[when(expr = "add is called")]
fn add_is_called(_world: &mut KataWorld) {
    todo!("implement step: add is called");
}

#[then(expr = "the result is {int}")]
fn the_result_is(_world: &mut KataWorld, expected: i32) {
    todo!("implement step: the result is 3 } not a brace");
}

#[then(expr = "another requirement's step")]
fn another(_world: &mut KataWorld) {
    todo!("implement step: another requirement's step");
}
"#;

    fn lines(steps: &[&str]) -> Vec<String> {
        steps.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_definition_is_found_with_its_whole_text() {
        let spans = definition_spans(Language::Rust, RUST_FILE);
        let patterns: Vec<&str> = spans.iter().map(|s| s.pattern.as_str()).collect();
        assert_eq!(
            patterns,
            [
                "an input {string}",
                "add is called",
                "the result is {int}",
                "another requirement's step"
            ]
        );
        let add = spans[1].text(RUST_FILE);
        assert!(
            add.starts_with("/// A stub the generator left.\n#[when"),
            "{add}"
        );
        assert!(
            add.ends_with("    todo!(\"implement step: add is called\");\n}\n"),
            "{add}"
        );
    }

    #[test]
    fn a_brace_inside_a_string_does_not_end_the_body() {
        let spans = definition_spans(Language::Rust, RUST_FILE);
        let result = spans[2].text(RUST_FILE);
        assert!(result.ends_with("not a brace\");\n}\n"), "{result}");
    }

    #[test]
    fn only_pending_definitions_bound_to_the_requirement_are_selected() {
        let steps = lines(&[
            "Given an input \"1,2\"",
            "When add is called",
            "Then the result is 3",
        ]);
        let pending = pending_step_definitions(Language::Rust, RUST_FILE, &step_texts(&steps));
        let patterns: Vec<&str> = pending.iter().map(|s| s.pattern.as_str()).collect();
        assert_eq!(patterns, ["add is called", "the result is {int}"]);
    }

    #[test]
    fn the_preamble_is_the_file_above_the_first_definition() {
        let preamble = step_file_preamble(Language::Rust, RUST_FILE);
        assert!(preamble.contains("struct KataWorld"), "{preamble}");
        assert!(preamble.ends_with("fn helper() {}\n\n"), "{preamble:?}");
        assert!(!preamble.contains("#[given"), "{preamble}");
    }

    #[test]
    fn a_preamble_over_budget_is_cut_on_a_line_boundary() {
        let long: String = (0..4000)
            .map(|n| format!("fn helper_{n}() {{}}\n"))
            .collect();
        let file = format!("{long}#[given(expr = \"x\")]\nfn x(_w: &mut W) {{}}\n");
        let preamble = step_file_preamble(Language::Rust, &file);
        assert!(preamble.len() <= PREAMBLE_BUDGET);
        assert!(preamble.ends_with('\n'));
    }

    #[test]
    fn filled_definitions_replace_the_pending_ones_in_place_and_nothing_else() {
        let steps = lines(&["When add is called", "Then the result is 3"]);
        let pending = pending_step_definitions(Language::Rust, RUST_FILE, &step_texts(&steps));
        let filled = r#"#[when(expr = "add is called")]
fn add_is_called(world: &mut KataWorld) {
    world.result = Some(kata::add(world.result.unwrap_or(0)));
}

#[then(expr = "the result is {int}")]
fn the_result_is(world: &mut KataWorld, expected: i32) {
    assert_eq!(world.result, Some(expected));
}
"#;
        let out = replace_step_definitions(Language::Rust, RUST_FILE, &pending, filled);
        assert!(out.contains("kata::add(world.result"), "{out}");
        assert!(
            out.contains("assert_eq!(world.result, Some(expected));"),
            "{out}"
        );
        assert!(!out.contains("implement step: add is called"), "{out}");
        assert!(!out.contains("implement step: the result"), "{out}");
        // Untouched: the hand-written step, the other requirement's
        // stub, the preamble, the doc comment carried with the stub.
        assert!(
            out.contains("world.result = Some(input.len() as i32);"),
            "{out}"
        );
        assert!(
            out.contains("implement step: another requirement's step"),
            "{out}"
        );
        assert!(
            out.starts_with("use cucumber::{World, given, then, when};"),
            "{out}"
        );
        assert!(!out.contains("/// A stub the generator left."), "{out}");
        let before: Vec<String> = definition_spans(Language::Rust, RUST_FILE)
            .into_iter()
            .map(|s| s.pattern)
            .collect();
        let after: Vec<String> = definition_spans(Language::Rust, &out)
            .into_iter()
            .map(|s| s.pattern)
            .collect();
        assert_eq!(after, before);
    }

    #[test]
    fn a_reply_that_keeps_a_placeholder_or_drops_a_pattern_is_not_filled() {
        let expected = lines(&["add is called", "the result is {int}"]);
        let still_pending = "#[when(expr = \"add is called\")]\nfn a(_w: &mut KataWorld) { todo!(\"x\") }\n#[then(expr = \"the result is {int}\")]\nfn b(_w: &mut KataWorld, n: i32) { assert_eq!(n, 3); }\n";
        assert!(!looks_like_filled_steps(
            Language::Rust,
            still_pending,
            &expected
        ));
        let one_short = "#[then(expr = \"the result is {int}\")]\nfn b(_w: &mut KataWorld, n: i32) { assert_eq!(n, 3); }\n";
        assert!(!looks_like_filled_steps(
            Language::Rust,
            one_short,
            &expected
        ));
        let good = "#[when(expr = \"add is called\")]\nfn a(w: &mut KataWorld) { w.result = Some(3); }\n#[then(expr = \"the result is {int}\")]\nfn b(w: &mut KataWorld, n: i32) { assert_eq!(w.result, Some(n)); }\n";
        assert!(looks_like_filled_steps(Language::Rust, good, &expected));
    }

    #[test]
    fn java_pending_methods_are_found_and_replaced_inside_the_class() {
        let java = "package kata;\n\nimport io.cucumber.java.en.*;\nimport io.cucumber.java.PendingException;\n\npublic class KataSteps {\n    private int result;\n\n    @When(\"add is called\")\n    public void addIsCalled() {\n        throw new PendingException();\n    }\n\n    @Then(\"the result is {int}\")\n    public void theResultIs(int expected) {\n        org.junit.jupiter.api.Assertions.assertEquals(expected, result);\n    }\n}\n";
        let steps = lines(&["When add is called", "Then the result is 3"]);
        let pending = pending_step_definitions(Language::Java, java, &step_texts(&steps));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].pattern, "add is called");
        let filled = "    @When(\"add is called\")\n    public void addIsCalled() {\n        result = new Kata().add(\"1,2\");\n    }\n";
        let out = replace_step_definitions(Language::Java, java, &pending, filled);
        assert!(out.contains("result = new Kata().add(\"1,2\");"), "{out}");
        assert!(!out.contains("PendingException();"), "{out}");
        assert!(out.contains("assertEquals(expected, result);"), "{out}");
        assert!(out.ends_with("    }\n}\n"), "{out}");
    }

    #[test]
    fn a_javascript_definition_ends_with_its_call() {
        let js = "const { Given, When } = require('@cucumber/cucumber');\n\nWhen('add is called', function () {\n  return 'pending';\n});\n\nWhen('done', function () {});\n";
        let spans = definition_spans(Language::JavaScript, js);
        assert_eq!(spans.len(), 2);
        assert!(
            spans[0].text(js).ends_with("});\n"),
            "{:?}",
            spans[0].text(js)
        );
        let pending =
            pending_step_definitions(Language::JavaScript, js, &lines(&["add is called"]));
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn the_fill_prompt_carries_the_fragment_the_production_code_and_the_preamble() {
        let requirement = Requirement {
            id: "REQ-001".into(),
            title: "Adds".into(),
            story: "As a user, I want sums so that I can add.".into(),
            acceptance_criteria: vec!["Given \"1,2\", when add is called, then 3".into()],
            ..Default::default()
        };
        let production = [("src/lib.rs".to_string(), "pub fn add() {}".to_string())];
        let failures = ["When add is called: FAILED".to_string()];
        let prompt = fill_steps_prompt(
            Language::Rust,
            &requirement,
            &FillBrief {
                scenarios: "  @REQ-001\n  Scenario: adds\n    When add is called\n",
                fragment: "#[when(expr = \"add is called\")]\nfn a(_w: &mut KataWorld) { todo!() }\n",
                count: 1,
                preamble: "struct KataWorld {}\n",
                production: &production,
                failures: &failures,
            },
        );
        assert_eq!(prompt.section, "fill_steps");
        for needle in [
            "REQ-001",
            "add is called",
            "struct KataWorld {}",
            "pub fn add() {}",
            "src/lib.rs",
            "Scenario: adds",
            "When add is called: FAILED",
        ] {
            assert!(
                prompt.user.contains(needle),
                "missing {needle:?} in {}",
                prompt.user
            );
        }
        assert!(prompt.system.contains("1 "), "{}", prompt.system);
        assert!(prompt.system.contains("cucumber-rs"), "{}", prompt.system);
    }

    /// A brace in a comment, an escaped string, or an inner block is
    /// not the end of the definition. Counting any of them would splice
    /// the replacement over the next function.
    #[test]
    fn braces_inside_comments_strings_and_inner_blocks_stay_in_the_body() {
        let source = r#"#[when(expr = "add is called")]
fn add(w: &mut W) {
    let s = "a \" } b";
    // } still inside
    if w.ready {
        w.result = 1;
    }
    w.done = true;
}
"#;
        let spans = definition_spans(Language::Rust, source);
        assert_eq!(spans.len(), 1);
        let text = spans[0].text(source);
        assert!(text.contains("w.done = true;"));
        assert!(text.contains("w.result = 1;"));
        assert!(text.ends_with("}\n"));
    }

    /// A body whose braces never close yields no span. Guessing an end
    /// would eat whatever follows it in the file.
    #[test]
    fn a_body_whose_braces_never_close_is_not_a_span() {
        let source = "#[when(expr = \"add is called\")]\nfn add(_w: &mut W) {\n    let n = 1;\n";
        assert!(definition_spans(Language::Rust, source).is_empty());
    }

    #[test]
    fn the_rust_world_is_the_struct_deriving_world_with_its_fields() {
        let world = world_span(Language::Rust, RUST_FILE).unwrap();
        assert_eq!(world.name, "KataWorld");
        assert_eq!(world.fields, ["result"]);
        assert_eq!(
            &RUST_FILE[world.body_start..world.body_end],
            "\n    result: Option<i32>,\n"
        );
    }

    #[test]
    fn a_unit_struct_world_gets_braces_when_fields_are_added() {
        let source = "use cucumber::World;\n\n#[derive(Debug, Default, World)]\npub struct GeneratedWorld;\n\n#[given(expr = \"a\")]\nfn a(_w: &mut GeneratedWorld) {}\n";
        let world = world_span(Language::Rust, source).unwrap();
        assert!(world.fields.is_empty());
        let out = insert_fields(
            Language::Rust,
            source,
            &world,
            &["kata: Option<Kata>,".into(), "result: Option<u32>,".into()],
        );
        assert!(
            out.contains(
                "pub struct GeneratedWorld {\n    kata: Option<Kata>,\n    result: Option<u32>,\n}\n\n#[given"
            ),
            "{out}"
        );
    }

    #[test]
    fn fields_are_added_at_the_end_of_an_existing_body() {
        let world = world_span(Language::Rust, RUST_FILE).unwrap();
        let out = insert_fields(
            Language::Rust,
            RUST_FILE,
            &world,
            &["report: Option<Report>,".into()],
        );
        assert!(
            out.contains(
                "struct KataWorld {\n    result: Option<i32>,\n    report: Option<Report>,\n}\n"
            ),
            "{out}"
        );
        // Everything else is as it was.
        assert_eq!(
            definition_spans(Language::Rust, &out).len(),
            definition_spans(Language::Rust, RUST_FILE).len()
        );
    }

    #[test]
    fn a_reply_with_a_new_fields_only_world_block_is_split_from_its_definitions() {
        let world = world_span(Language::Rust, RUST_FILE).unwrap();
        let reply = "#[derive(Debug, Default, World)]\nstruct KataWorld {\n    result: Option<i32>,\n    report: Option<Report>\n    proven: Vec<String>,\n}\n\n#[when(expr = \"add is called\")]\nfn add_is_called(w: &mut KataWorld) {\n    w.report = Some(kata::report());\n}\n";
        let split = split_world_fragment(Language::Rust, reply, &world).unwrap();
        // The field the head already shows is dropped; the comma the
        // model forgot is supplied.
        assert_eq!(
            split.fields,
            ["report: Option<Report>,", "proven: Vec<String>,"]
        );
        assert!(
            split.definitions.starts_with("#[when(expr"),
            "{:?}",
            split.definitions
        );
        assert!(!split.definitions.contains("struct KataWorld"));
        let spans = definition_spans(Language::Rust, &split.definitions);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn a_reply_without_a_world_block_has_no_fields_to_add() {
        let world = world_span(Language::Rust, RUST_FILE).unwrap();
        let reply =
            "#[when(expr = \"add is called\")]\nfn a(w: &mut KataWorld) { w.result = Some(3); }\n";
        let split = split_world_fragment(Language::Rust, reply, &world).unwrap();
        assert!(split.fields.is_empty());
        assert_eq!(split.definitions, reply);
    }

    #[test]
    fn a_world_block_holding_a_method_or_a_second_struct_is_refused() {
        let world = world_span(Language::Rust, RUST_FILE).unwrap();
        let with_method = "struct KataWorld {\n    fn reset(&mut self) {}\n}\n#[when(expr = \"add is called\")]\nfn a(w: &mut KataWorld) {}\n";
        let error = split_world_fragment(Language::Rust, with_method, &world).unwrap_err();
        assert!(error.contains("only its new fields"), "{error}");
        let second = "struct KataWorld {\n    report: Option<Report>,\n}\nstruct Report { ok: bool }\n#[when(expr = \"add is called\")]\nfn a(w: &mut KataWorld) {}\n";
        let error = split_world_fragment(Language::Rust, second, &world).unwrap_err();
        assert!(error.contains("another struct"), "{error}");
    }

    #[test]
    fn the_java_world_is_the_step_class_and_its_fields() {
        let java = "import io.cucumber.java.en.*;\n\npublic class GeneratedSteps {\n    private int result;\n\n    @When(\"add is called\")\n    public void addIsCalled() {\n        throw new PendingException();\n    }\n}\n";
        let world = world_span(Language::Java, java).unwrap();
        assert_eq!(world.name, "GeneratedSteps");
        assert_eq!(world.fields, ["result"]);
        let reply = "public class GeneratedSteps {\n    private Kata kata;\n    private int result;\n}\n\n    @When(\"add is called\")\n    public void addIsCalled() {\n        result = kata.add(\"1,2\");\n    }\n";
        let split = split_world_fragment(Language::Java, reply, &world).unwrap();
        assert_eq!(split.fields, ["private Kata kata;"]);
        assert_eq!(
            definition_spans(Language::Java, &split.definitions).len(),
            1
        );
        let out = insert_fields(Language::Java, java, &world, &split.fields);
        assert!(
            out.contains("    private int result;\n    private Kata kata;\n\n    @When"),
            "{out}"
        );
        assert!(out.ends_with("    }\n}\n"), "{out}");
    }

    #[test]
    fn an_empty_java_class_body_gets_its_first_field() {
        let java = "public class GeneratedSteps {\n\n}\n";
        let world = world_span(Language::Java, java).unwrap();
        let out = insert_fields(Language::Java, java, &world, &["private Kata kata;".into()]);
        assert_eq!(
            out,
            "public class GeneratedSteps {\n    private Kata kata;\n}\n"
        );
    }

    #[test]
    fn the_js_family_has_no_world_to_extend() {
        assert_eq!(
            world_span(
                Language::JavaScript,
                "const { Given } = require('@cucumber/cucumber');"
            ),
            None
        );
    }

    /// An unclosed quote runs to the end of the file, so the body has
    /// no trustworthy close either.
    #[test]
    fn an_unclosed_string_is_not_a_span() {
        let source = "#[when(expr = \"add is called\")]\nfn add(_w: &mut W) { let s = \"oops; }\n";
        assert!(definition_spans(Language::Rust, source).is_empty());
    }
}

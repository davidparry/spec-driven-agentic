//! The prompt catalog: every LLM call's system and user prompt lives in
//! `prompts/prompts.toml` (embedded at compile time), written as MiniJinja
//! templates. The prompt builders render a section with their dynamic
//! context; the Rust source holds no prompt wording.

use std::sync::OnceLock;

use minijinja::Environment;
use serde::Serialize;

/// The embedded prompt catalog - the single source of prompt wording.
const PROMPTS_TOML: &str = include_str!("../../prompts/prompts.toml");

/// The sections the catalog must hold, one per LLM call.
pub const SECTIONS: [&str; 12] = [
    "proposal",
    "rewording",
    "scenario",
    "polish",
    "polish_fragment",
    "implementation",
    "refactor",
    "advice",
    "next_step",
    "ask",
    "diff",
    "layout",
];

/// One LLM call's prompts: the system prompt carries the model's role
/// and rules, the user prompt carries the call's data. `section` names
/// the catalog entry that rendered the pair (e.g. `proposal`), so every
/// log line about the call can say which template produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPrompt {
    pub section: String,
    pub system: String,
    pub user: String,
}

#[derive(serde::Deserialize)]
struct CatalogEntry {
    #[serde(default)]
    system: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    brief: String,
}

/// The templates, registered as `<section>.system` / `<section>.user`,
/// plus `project_memory.brief` for the shared project-memory snippet.
/// The catalog is a compile-time asset: a malformed file or template is
/// a build defect, surfaced loudly on first use and covered by tests.
fn environment() -> &'static Environment<'static> {
    static ENV: OnceLock<Environment<'static>> = OnceLock::new();
    ENV.get_or_init(|| {
        let catalog: std::collections::BTreeMap<String, CatalogEntry> =
            toml::from_str(PROMPTS_TOML)
                .expect("prompts/prompts.toml is a compile-time asset and must parse");
        let mut env = Environment::new();
        for section in SECTIONS {
            let pair = catalog
                .get(section)
                .unwrap_or_else(|| panic!("prompts/prompts.toml is missing [{section}]"));
            if pair.system.is_empty() || pair.user.is_empty() {
                panic!("prompts/prompts.toml [{section}] needs system and user templates");
            }
            for (role, source) in [("system", &pair.system), ("user", &pair.user)] {
                env.add_template_owned(format!("{section}.{role}"), source.clone())
                    .unwrap_or_else(|e| {
                        panic!("prompt template {section}.{role} does not compile - {e}")
                    });
            }
        }
        let memory = catalog
            .get("project_memory")
            .unwrap_or_else(|| panic!("prompts/prompts.toml is missing [project_memory]"));
        if memory.brief.is_empty() {
            panic!("prompts/prompts.toml [project_memory] needs a brief template");
        }
        env.add_template_owned("project_memory.brief", memory.brief.clone())
            .unwrap_or_else(|e| {
                panic!("prompt template project_memory.brief does not compile - {e}")
            });
        let correction = catalog
            .get("correction")
            .unwrap_or_else(|| panic!("prompts/prompts.toml is missing [correction]"));
        if correction.user.is_empty() {
            panic!("prompts/prompts.toml [correction] needs a user template");
        }
        env.add_template_owned("correction.user", correction.user.clone())
            .unwrap_or_else(|e| panic!("prompt template correction.user does not compile - {e}"));
        let tool_rules = catalog
            .get("tool_rules")
            .unwrap_or_else(|| panic!("prompts/prompts.toml is missing [tool_rules]"));
        if tool_rules.system.is_empty() {
            panic!("prompts/prompts.toml [tool_rules] needs a system template");
        }
        env.add_template_owned("tool_rules.system", tool_rules.system.clone())
            .unwrap_or_else(|e| panic!("prompt template tool_rules.system does not compile - {e}"));
        let mcp = catalog
            .get("mcp")
            .unwrap_or_else(|| panic!("prompts/prompts.toml is missing [mcp]"));
        if mcp.system.is_empty() {
            panic!("prompts/prompts.toml [mcp] needs an instructions template in system");
        }
        env.add_template_owned("mcp.instructions", mcp.system.clone())
            .unwrap_or_else(|e| panic!("prompt template mcp.instructions does not compile - {e}"));
        env
    })
}

/// One bounded question for the decision plane, loaded from
/// `[decision.<name>]`.
///
/// Not a template and never rendered: a decision question is sent
/// verbatim, so there is no context to substitute and nothing that
/// would make two sends differ.
///
/// `version` lives in the same table as the wording it names, which is
/// the whole reason this type has the field at all. A threshold
/// calibrated against one phrasing is not evidence about another, so
/// wording that can change without its version changing turns a
/// published figure into a claim nobody can reproduce.
/// `when_true` and `when_false` describe the two outcomes of a boolean
/// question. A graded question has no two outcomes to describe - its
/// levels are the answer schema, and they live on the gate beside the
/// floor they are indexed against, so that a floor cannot name a level
/// that is not there. The pair is therefore optional, and absent
/// together: half an outcome pair describes nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct DecisionPrompt {
    pub version: String,
    pub instructions: String,
    #[serde(default)]
    pub when_true: Option<String>,
    #[serde(default)]
    pub when_false: Option<String>,
}

impl DecisionPrompt {
    /// The two outcomes of a boolean question.
    ///
    /// Panics like the rest of this module: asking a graded table for
    /// outcomes it does not have is a gate declared with the wrong
    /// [`crate::domain::decision::GateKind`], which is a build defect
    /// and not something a caller could act on.
    pub fn outcomes(&self, name: &str) -> (&str, &str) {
        match (&self.when_true, &self.when_false) {
            (Some(when_true), Some(when_false)) => (when_true, when_false),
            _ => panic!(
                "prompts/prompts.toml [decision.{name}] describes no outcomes, so it \
                 cannot answer a boolean question - give it when_true and when_false, \
                 or declare the gate as graded"
            ),
        }
    }
}

#[derive(serde::Deserialize)]
struct DecisionCatalog {
    #[serde(default)]
    decision: std::collections::BTreeMap<String, DecisionPrompt>,
}

/// The decision questions, keyed by the name in `[decision.<name>]`.
///
/// Panics like the rest of this module: the catalog is a compile-time
/// asset, so a missing question or an empty field is a build defect and
/// not a runtime condition any caller could act on.
fn decision_catalog() -> &'static std::collections::BTreeMap<String, DecisionPrompt> {
    static QUESTIONS: OnceLock<std::collections::BTreeMap<String, DecisionPrompt>> =
        OnceLock::new();
    QUESTIONS.get_or_init(|| {
        let catalog: DecisionCatalog = toml::from_str(PROMPTS_TOML)
            .expect("prompts/prompts.toml is a compile-time asset and must parse");
        for (name, question) in &catalog.decision {
            if question.when_true.is_some() != question.when_false.is_some() {
                panic!(
                    "prompts/prompts.toml [decision.{name}] describes one outcome and \
                     not the other - a boolean question needs both, a graded one needs \
                     neither"
                );
            }
            for (field, text) in [
                ("version", Some(&question.version)),
                ("instructions", Some(&question.instructions)),
                ("when_true", question.when_true.as_ref()),
                ("when_false", question.when_false.as_ref()),
            ] {
                let Some(text) = text else { continue };
                if text.trim().is_empty() {
                    panic!("prompts/prompts.toml [decision.{name}] needs a {field}");
                }
                // The bytes sent are the bytes the published figures were
                // measured on. A multi-line form would quietly change the
                // request behind every number in the evaluation.
                if text.contains('\n') {
                    panic!(
                        "prompts/prompts.toml [decision.{name}] {field} must be one line - \
                         use a single-quoted literal, not '''...'''"
                    );
                }
            }
        }
        catalog.decision
    })
}

/// One decision question by name, e.g. `measurable`.
pub fn decision_prompt(name: &str) -> &'static DecisionPrompt {
    decision_catalog()
        .get(name)
        .unwrap_or_else(|| panic!("prompts/prompts.toml is missing [decision.{name}]"))
}

/// Render one section's system and user templates with the same context.
pub(crate) fn render(section: &str, context: impl Serialize) -> RenderedPrompt {
    let value = minijinja::Value::from_serialize(&context);
    let rules = render_one("tool_rules.system", &minijinja::Value::from_serialize(()));
    RenderedPrompt {
        section: section.to_string(),
        system: format!(
            "{}\n\n{rules}",
            render_one(&format!("{section}.system"), &value)
        ),
        user: render_one(&format!("{section}.user"), &value),
    }
}

pub fn mcp_instructions() -> String {
    render_snippet("mcp.instructions", minijinja::context! {})
}

pub fn ask_prompt(task: &str) -> RenderedPrompt {
    render(
        "ask",
        minijinja::context! { task, instructions => mcp_instructions() },
    )
}

/// `spec diff`: one uncommitted change, for explaining in prose. The
/// diff travels in the user prompt rather than through a tool call, so
/// the model is summarizing the same bytes the developer would see.
pub fn diff_prompt(
    path: Option<&str>,
    diff: &str,
    truncated: bool,
    untracked: &[String],
) -> RenderedPrompt {
    render(
        "diff",
        minijinja::context! {
            path,
            diff,
            truncated,
            untracked,
            instructions => mcp_instructions(),
        },
    )
}

/// Render the correction snippet appended when a model reply fails
/// validation: the reason and the prior reply, so the next call can
/// learn from the mistake.
pub fn correction_user(reason: &str, reply: &str) -> String {
    render_snippet("correction.user", minijinja::context! { reason, reply })
}

/// Render a named snippet (not a call section) with the given context.
pub(crate) fn render_snippet(name: &str, context: impl Serialize) -> String {
    let value = minijinja::Value::from_serialize(&context);
    render_one(name, &value)
}

fn render_one(name: &str, context: &minijinja::Value) -> String {
    environment()
        .get_template(name)
        .unwrap_or_else(|_| panic!("no prompt template named {name}"))
        .render(context)
        .unwrap_or_else(|e| panic!("prompt template {name} failed to render - {e}"))
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_registers_a_system_and_a_user_template() {
        let env = environment();
        for section in SECTIONS {
            for role in ["system", "user"] {
                assert!(
                    env.get_template(&format!("{section}.{role}")).is_ok(),
                    "missing template {section}.{role}"
                );
            }
        }
        assert!(env.get_template("project_memory.brief").is_ok());
        assert!(env.get_template("correction.user").is_ok());
        assert!(env.get_template("tool_rules.system").is_ok());
        assert!(env.get_template("mcp.instructions").is_ok());
    }

    #[test]
    fn rendering_fills_the_context_into_both_prompts() {
        let prompt = render(
            "proposal",
            minijinja::context! { description => "sum numbers" },
        );
        assert_eq!(prompt.section, "proposal");
        assert!(prompt.system.contains("ONLY a JSON array"));
        assert!(
            prompt
                .user
                .contains("<description>\nsum numbers\n</description>")
        );
    }

    #[test]
    fn the_project_memory_snippet_names_the_stack() {
        let brief = render_snippet(
            "project_memory.brief",
            minijinja::context! {
                language => "Java",
                bdd_framework => "Cucumber-JVM",
                build_tool => "Maven",
                libraries => vec!["cucumber-java 7.20.1", "junit-jupiter 5.11.4"],
                layout => "src/main/java (production), features/",
            },
        );
        assert!(brief.starts_with("Project memory:"));
        assert!(brief.contains("Language: Java (Cucumber-JVM), build Maven"));
        assert!(brief.contains("cucumber-java 7.20.1"));
        assert!(brief.contains("src/main/java (production)"));
    }

    #[test]
    fn the_correction_snippet_carries_the_reason_and_prior_reply() {
        let text = correction_user("not a JSON array", "Sure, here you go!");
        assert!(text.contains("Your previous reply was invalid"));
        assert!(text.contains("Reason: not a JSON array"));
        assert!(text.contains("Sure, here you go!"));
    }

    #[test]
    fn the_measurable_question_is_loaded_from_the_catalog_with_its_version() {
        let question = decision_prompt("measurable");
        assert_eq!(question.version, "measurable/v2");
        assert!(question.instructions.contains("after \"then\""));
        let (when_true, when_false) = question.outcomes("measurable");
        assert!(when_true.contains("literal value"));
        assert!(when_false.contains("vague"));
    }

    /// A graded question has no two outcomes to describe, so it carries
    /// none - and asking it for them is a gate declared with the wrong
    /// kind, which says so rather than sending an empty criterion.
    #[test]
    fn a_graded_question_carries_no_outcomes_and_says_so_when_asked() {
        let question = decision_prompt("implementation_complete");
        assert_eq!(question.when_true, None);
        assert_eq!(question.when_false, None);
        assert!(question.instructions.contains("Grade `stub`"));
        let asked = std::panic::catch_unwind(|| question.outcomes("implementation_complete"));
        assert!(asked.is_err(), "a graded table cannot answer a boolean");
    }

    /// The bytes sent are the bytes the published evaluation measured.
    /// A `'''multi-line'''` edit would change every figure behind it
    /// without changing a single visible word, so the loader refuses
    /// one and this is the proof it still does.
    #[test]
    fn every_decision_question_is_one_line_per_field_and_nothing_is_blank() {
        let catalog = decision_catalog();
        assert!(!catalog.is_empty(), "the decision plane needs a question");
        for (name, question) in catalog {
            assert_eq!(
                question.when_true.is_some(),
                question.when_false.is_some(),
                "[decision.{name}] describes one outcome and not the other"
            );
            for (field, text) in [
                ("version", Some(&question.version)),
                ("instructions", Some(&question.instructions)),
                ("when_true", question.when_true.as_ref()),
                ("when_false", question.when_false.as_ref()),
            ] {
                let Some(text) = text else { continue };
                assert!(
                    !text.trim().is_empty(),
                    "[decision.{name}] {field} is blank"
                );
                assert!(
                    !text.contains('\n'),
                    "[decision.{name}] {field} spans lines"
                );
            }
        }
    }

    /// A version names a wording. The two live in one table so an edit
    /// cannot move one without the other being on screen, and the shape
    /// is asserted so `measurable` and `measurable/v2` stay tellable
    /// apart in a judgment record.
    #[test]
    fn a_question_version_names_the_question_and_a_revision() {
        for (name, question) in decision_catalog() {
            let (question_name, revision) = question
                .version
                .split_once('/')
                .unwrap_or_else(|| panic!("[decision.{name}] version is not <name>/v<n>"));
            assert_eq!(question_name, name, "the version names another question");
            assert!(
                revision.starts_with('v') && revision[1..].parse::<u32>().is_ok(),
                "[decision.{name}] revision {revision:?} is not v<n>"
            );
        }
    }
}

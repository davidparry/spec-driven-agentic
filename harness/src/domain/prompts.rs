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
pub const SECTIONS: [&str; 8] = [
    "proposal",
    "rewording",
    "polish",
    "implementation",
    "advice",
    "next_step",
    "ask",
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
}

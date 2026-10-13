//! Per-command tool profiles: which built-in tools a model call may use.
//! Pure. Resolution is the defaults merged with the config's list for the
//! command, plus attachments, minus removals - or, for one run, whatever
//! `--tools` pinned instead.

use std::collections::BTreeMap;

use crate::domain::tools::{ToolDefinition, canonical_name, find};

/// Every model call the harness makes, as a profile key. The kebab-case
/// name is what `.spec/config.toml` and `--for` use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Caller {
    SpecDraft,
    SpecReword,
    ScenarioGenerate,
    StepsGenerate,
    UnittestGenerate,
    ImplementAdvice,
    Implement,
    Refactor,
    Status,
    Ask,
    Diff,
}

impl Caller {
    pub const ALL: [Caller; 11] = [
        Caller::SpecDraft,
        Caller::SpecReword,
        Caller::ScenarioGenerate,
        Caller::StepsGenerate,
        Caller::UnittestGenerate,
        Caller::ImplementAdvice,
        Caller::Implement,
        Caller::Refactor,
        Caller::Status,
        Caller::Ask,
        Caller::Diff,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Caller::SpecDraft => "spec-draft",
            Caller::SpecReword => "spec-reword",
            Caller::ScenarioGenerate => "scenario-generate",
            Caller::StepsGenerate => "steps-generate",
            Caller::UnittestGenerate => "unittest-generate",
            Caller::ImplementAdvice => "implement-advice",
            Caller::Implement => "implement",
            Caller::Refactor => "refactor",
            Caller::Status => "status",
            Caller::Ask => "ask",
            Caller::Diff => "diff",
        }
    }

    /// The harness command whose LLM call offers this profile.
    pub fn cli_command(self) -> &'static str {
        match self {
            Caller::SpecDraft => "spec draft",
            Caller::SpecReword => "spec reword",
            Caller::ScenarioGenerate => "spec scenario generate",
            Caller::StepsGenerate => "spec steps generate",
            Caller::UnittestGenerate => "spec unittest generate",
            Caller::ImplementAdvice => "spec implement (preflight advice)",
            Caller::Implement => "spec implement",
            Caller::Refactor => "spec refactor",
            Caller::Status => "spec status",
            Caller::Ask => "spec ask",
            Caller::Diff => "spec diff",
        }
    }

    pub fn section(self) -> &'static str {
        match self {
            Caller::SpecDraft => "proposal",
            Caller::SpecReword => "rewording",
            Caller::ScenarioGenerate => "scenario",
            Caller::StepsGenerate | Caller::UnittestGenerate => "polish",
            Caller::ImplementAdvice => "advice",
            Caller::Implement => "implementation",
            Caller::Refactor => "refactor",
            Caller::Status => "next_step",
            Caller::Ask => "ask",
            Caller::Diff => "diff",
        }
    }

    pub fn parse(key: &str) -> Option<Caller> {
        Self::ALL.into_iter().find(|caller| caller.key() == key)
    }
}

/// Built-in tools this caller may use by default.
pub fn default_profile(caller: Caller) -> &'static [&'static str] {
    match caller {
        Caller::SpecDraft => &[
            "list_requirements",
            "get_requirement",
            "validate_spec",
            "refine_requirement",
        ],
        Caller::SpecReword => &["get_requirement", "validate_spec", "refine_requirement"],
        Caller::ScenarioGenerate => &["get_requirement", "feature_read", "step_definitions_find"],
        Caller::StepsGenerate => &[
            "project_inspect",
            "feature_list",
            "feature_read",
            "step_definitions_find",
        ],
        Caller::UnittestGenerate => &[
            "project_inspect",
            "get_requirement",
            "feature_read",
            "step_definitions_find",
        ],
        Caller::ImplementAdvice => &["get_tdd_state", "validate_spec", "feature_list"],
        Caller::Implement => &[
            "get_requirement",
            "feature_read",
            "step_definitions_find",
            "get_tdd_state",
            "run_tests",
            "command_run",
        ],
        // No run_tests and no command_run: the refactor loop runs the
        // suite itself, once per round, and judges the result. A model
        // that could run the tests could also report on a run the loop
        // never saw.
        Caller::Refactor => &[
            "get_requirement",
            "feature_read",
            "step_definitions_find",
            "project_inspect",
            "get_tdd_state",
        ],
        Caller::Status => &[
            "project_root",
            "list_requirements",
            "get_requirement",
            "get_tdd_state",
            "validate_spec",
        ],
        Caller::Ask => &[
            "project_root",
            "list_requirements",
            "get_requirement",
            "validate_spec",
            "refine_requirement",
            "get_tdd_state",
            "project_inspect",
            "feature_list",
            "feature_read",
            "step_definitions_find",
        ],
        // The diff itself is already in the prompt; these are for
        // reading around it. A requirements diff names ids and feature
        // files, and a summary that can look those up describes what
        // the change means rather than which lines moved.
        Caller::Diff => &[
            "git_diff",
            "project_root",
            "list_requirements",
            "get_requirement",
            "feature_read",
        ],
    }
}

/// Tools that mutate the project (write a file, mark implemented).
const MUTATING: [&str; 12] = [
    "scenario_add",
    "scenario_update",
    "scenario_delete",
    "feature_create",
    "requirement_reword",
    "requirement_mark_implemented",
    "requirement_create",
    "requirement_update",
    "requirement_delete",
    "requirement_ask",
    "requirement_decide",
    "step_definition_create",
    // unit_test_create is also mutating; listed separately for the
    // "command_run is the only default exception" rule.
];

const ALSO_MUTATING: [&str; 1] = ["unit_test_create"];

#[derive(Debug, Clone, Default)]
pub struct ProfileOverrides {
    /// `[tools.profiles]`: merged with the caller's defaults. A config
    /// that spells out a command's tools is a copy of the defaults to
    /// read and extend, so repeating a default adds nothing and leaving
    /// one out drops nothing.
    pub profiles: BTreeMap<String, Vec<String>>,
    /// `--tools a,b` for one run: the whole list, defaults and config
    /// set aside. Attachments and removals still apply.
    pub pinned: BTreeMap<String, Vec<String>>,
    /// `[tools.enabled]`.
    pub attached: BTreeMap<String, Vec<String>>,
    /// `[tools.disabled]`. Beats an attachment of the same name.
    pub removed: BTreeMap<String, Vec<String>>,
}

pub struct Resolved {
    pub tools: Vec<ToolDefinition>,
    pub unknown: Vec<String>,
}

/// The names a caller's profile holds before the catalog is consulted:
/// `pinned[caller]` when set, else `default_profile(caller)` with every
/// `profiles[caller]` name not already there appended in config order.
pub fn profile_names(caller: Caller, overrides: &ProfileOverrides) -> Vec<String> {
    let key = caller.key();
    if let Some(pinned) = overrides.pinned.get(key) {
        return pinned.clone();
    }
    let mut names: Vec<String> = default_profile(caller)
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    if let Some(configured) = overrides.profiles.get(key) {
        for name in configured {
            if !names_one_tool(&names, name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// Whether `name` is already in the list under any spelling of the
/// same tool (`validate_spec` / `builtin:validate_spec`,
/// `playwright:browser_navigate` / `playwright__browser_navigate`).
fn names_one_tool(names: &[String], name: &str) -> bool {
    let wanted = canonical_name(name);
    names
        .iter()
        .any(|existing| canonical_name(existing) == wanted)
}

/// resolved = profile_names(caller) + attached[caller] - removed[caller]
///
/// Names are deduplicated again after the catalog lookup, by the tool
/// they resolved to: `validate_spec` and `builtin:validate_spec` are
/// one tool and are offered once.
pub fn resolve(
    caller: Caller,
    overrides: &ProfileOverrides,
    catalog: &[ToolDefinition],
) -> Resolved {
    let key = caller.key();
    let mut names = profile_names(caller, overrides);
    if let Some(attached) = overrides.attached.get(key) {
        for name in attached {
            if !names_one_tool(&names, name) {
                names.push(name.clone());
            }
        }
    }
    if let Some(removed) = overrides.removed.get(key) {
        names.retain(|name| !removed.iter().any(|drop| drop == name));
    }

    let mut tools = Vec::new();
    let mut unknown = Vec::new();
    let mut seen = Vec::new();
    for name in names {
        if seen.iter().any(|existing| existing == &name) {
            continue;
        }
        seen.push(name.clone());
        match find(catalog, &name) {
            Ok(definition) => {
                if !tools
                    .iter()
                    .any(|t: &ToolDefinition| t.name == definition.name)
                {
                    tools.push(definition.clone());
                }
            }
            Err(_) => unknown.push(name),
        }
    }
    // Preserve catalog order.
    tools.sort_by_key(|tool| {
        catalog
            .iter()
            .position(|c| c.name == tool.name)
            .unwrap_or(usize::MAX)
    });
    Resolved { tools, unknown }
}

pub fn default_profile_is_non_mutating(caller: Caller) -> bool {
    default_profile(caller).iter().all(|name| {
        *name == "command_run" || (!MUTATING.contains(name) && !ALSO_MUTATING.contains(name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolDefinition, ToolOrigin};

    fn catalog(names: &[&str]) -> Vec<ToolDefinition> {
        names
            .iter()
            .map(|name| ToolDefinition {
                name: (*name).into(),
                description: String::new(),
                schema: serde_json::json!({"type": "object"}),
                origin: ToolOrigin::Builtin,
            })
            .collect()
    }

    fn all_builtin_names() -> Vec<&'static str> {
        vec![
            "list_requirements",
            "get_requirement",
            "validate_spec",
            "refine_requirement",
            "run_tests",
            "get_tdd_state",
            "start_refactor",
            "project_root",
            "project_inspect",
            "feature_list",
            "feature_read",
            "feature_create",
            "scenario_add",
            "scenario_update",
            "scenario_delete",
            "command_run",
            "requirement_reword",
            "requirement_mark_implemented",
            "requirement_create",
            "requirement_update",
            "requirement_delete",
            "requirement_ask",
            "requirement_decide",
            "step_definitions_find",
            "step_definition_create",
            "unit_test_create",
            "git_diff",
        ]
    }

    #[test]
    fn caller_keys_round_trip() {
        for caller in Caller::ALL {
            assert_eq!(Caller::parse(caller.key()), Some(caller));
        }
        assert_eq!(Caller::parse("nonsense"), None);
    }

    /// The section is what `AgentConfig` loads. A caller pointed at a
    /// template the catalog does not have panics on the first real run,
    /// which is too late: the mapping is data this crate owns.
    #[test]
    fn every_caller_section_is_a_prompt_the_catalog_can_render() {
        for caller in Caller::ALL {
            let section = caller.section();
            assert!(crate::domain::prompts::SECTIONS.contains(&section));
        }
    }

    #[test]
    fn every_caller_names_the_cli_command_that_loads_its_tools() {
        let mut seen = std::collections::BTreeSet::new();
        for caller in Caller::ALL {
            let command = caller.cli_command();
            assert!(command.starts_with("spec "), "{}: {command}", caller.key());
            assert!(seen.insert(command), "duplicate cli_command {command}");
        }
    }

    #[test]
    fn every_default_profile_is_non_empty_and_names_only_real_tools() {
        let known = all_builtin_names();
        for caller in Caller::ALL {
            let profile = default_profile(caller);
            assert!(!profile.is_empty(), "{}", caller.key());
            for name in profile {
                assert!(known.contains(name), "{} unknown in {}", name, caller.key());
            }
            assert!(default_profile_is_non_mutating(caller), "{}", caller.key());
        }
    }

    #[test]
    fn command_run_appears_only_for_implement() {
        for caller in Caller::ALL {
            let has = default_profile(caller).contains(&"command_run");
            assert_eq!(has, caller == Caller::Implement, "{}", caller.key());
        }
    }

    /// `--tools` pins the list for one run: defaults and config set
    /// aside, attachments and removals still applied.
    #[test]
    fn resolve_pinned_attach_remove_and_unknown() {
        let catalog = catalog(&all_builtin_names());
        let mut overrides = ProfileOverrides::default();
        overrides.pinned.insert(
            "status".into(),
            vec!["get_tdd_state".into(), "validate_spec".into()],
        );
        overrides
            .profiles
            .insert("status".into(), vec!["feature_list".into()]);
        overrides
            .attached
            .insert("status".into(), vec!["playwright__browser_navigate".into()]);
        overrides
            .removed
            .insert("status".into(), vec!["validate_spec".into()]);
        let resolved = resolve(Caller::Status, &overrides, &catalog);
        let names: Vec<_> = resolved.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["get_tdd_state"]);
        assert_eq!(resolved.unknown, vec!["playwright__browser_navigate"]);
    }

    /// A `[tools.profiles]` list is read on top of the defaults, not
    /// instead of them: the config is where a command's tools are
    /// spelled out to be seen and added to, and a list that named one
    /// tool used to take the other four away.
    #[test]
    fn a_profiles_list_adds_to_the_defaults_without_repeating_them() {
        let catalog = catalog(&all_builtin_names());
        let mut overrides = ProfileOverrides::default();
        let mut listed: Vec<String> = default_profile(Caller::Status)
            .iter()
            .map(|n| n.to_string())
            .collect();
        listed.push("git_diff".into());
        listed.push("builtin:get_tdd_state".into());
        overrides.profiles.insert("status".into(), listed);
        // Before any catalog is consulted the qualified spelling of a
        // default is already recognised as that default, so what
        // `spec config` reports is the set the model is offered.
        let mut before_catalog: Vec<&str> = default_profile(Caller::Status).to_vec();
        before_catalog.push("git_diff");
        assert_eq!(profile_names(Caller::Status, &overrides), before_catalog);
        let resolved = resolve(Caller::Status, &overrides, &catalog);
        let names: Vec<_> = resolved.tools.iter().map(|t| t.name.as_str()).collect();
        let mut expected: Vec<&str> = default_profile(Caller::Status).to_vec();
        expected.push("git_diff");
        expected.sort_by_key(|n| catalog.iter().position(|c| c.name == *n));
        assert_eq!(names, expected);
        assert!(resolved.unknown.is_empty());

        // A list shorter than the defaults drops none of them.
        let mut short = ProfileOverrides::default();
        short
            .profiles
            .insert("status".into(), vec!["get_tdd_state".into()]);
        let resolved = resolve(Caller::Status, &short, &catalog);
        assert_eq!(resolved.tools.len(), default_profile(Caller::Status).len());
    }

    #[test]
    fn removal_beats_attachment() {
        let catalog = catalog(&all_builtin_names());
        let mut overrides = ProfileOverrides::default();
        overrides
            .attached
            .insert("implement".into(), vec!["command_run".into()]);
        overrides
            .removed
            .insert("implement".into(), vec!["command_run".into()]);
        let resolved = resolve(Caller::Implement, &overrides, &catalog);
        assert!(!resolved.tools.iter().any(|t| t.name == "command_run"));
    }

    #[test]
    fn a_profiles_list_with_qualified_names_picks_builtin_over_mcp() {
        let mut catalog = catalog(&["validate_spec", "get_tdd_state"]);
        catalog.push(ToolDefinition {
            name: crate::domain::tools::namespaced("self", "validate_spec"),
            description: String::new(),
            schema: serde_json::json!({"type": "object"}),
            origin: ToolOrigin::Server("self".into()),
        });
        let mut overrides = ProfileOverrides::default();
        overrides.profiles.insert(
            "status".into(),
            vec![
                "builtin:validate_spec".into(),
                "self:validate_spec".into(),
                "get_tdd_state".into(),
            ],
        );
        let resolved = resolve(Caller::Status, &overrides, &catalog);
        let names: Vec<_> = resolved.tools.iter().map(|t| t.name.as_str()).collect();
        // The qualified built-in is the same tool as the default's bare
        // name and is offered once; the MCP namesake is a second tool.
        assert_eq!(
            names,
            vec!["validate_spec", "get_tdd_state", "self__validate_spec"]
        );
        // The three status defaults this small catalog lacks are the
        // only unknowns - the qualified names all resolved.
        assert_eq!(
            resolved.unknown,
            vec!["project_root", "list_requirements", "get_requirement"]
        );
    }
}

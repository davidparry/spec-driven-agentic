//! Per-command tool profiles: which built-in tools a model call may use.
//! Pure. Resolution is replace-or-default, plus attachments, minus removals.

use std::collections::BTreeMap;

use crate::domain::tools::{ToolDefinition, find};

/// Every model call the harness makes, as a profile key. The kebab-case
/// name is what `.spec.toml` and `--for` use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Caller {
    SpecDraft,
    SpecReword,
    ScenarioGenerate,
    StepsGenerate,
    UnittestGenerate,
    ImplementAdvice,
    Implement,
    Status,
    Ask,
}

impl Caller {
    pub const ALL: [Caller; 9] = [
        Caller::SpecDraft,
        Caller::SpecReword,
        Caller::ScenarioGenerate,
        Caller::StepsGenerate,
        Caller::UnittestGenerate,
        Caller::ImplementAdvice,
        Caller::Implement,
        Caller::Status,
        Caller::Ask,
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
            Caller::Status => "status",
            Caller::Ask => "ask",
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
            Caller::Status => "spec status",
            Caller::Ask => "spec ask",
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
            Caller::Status => "next_step",
            Caller::Ask => "ask",
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
        Caller::ImplementAdvice => &[
            "get_tdd_state",
            "validate_spec",
            "feature_list",
            "changes_show",
            "changes_validate",
        ],
        Caller::Implement => &[
            "get_requirement",
            "feature_read",
            "step_definitions_find",
            "get_tdd_state",
            "run_tests",
            "command_run",
            "changes_show",
        ],
        Caller::Status => &[
            "project_root",
            "list_requirements",
            "get_requirement",
            "get_tdd_state",
            "validate_spec",
            "changes_show",
            "changes_validate",
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
            "changes_show",
            "changes_validate",
        ],
    }
}

/// Tools that mutate the project (stage, commit, mark implemented).
const MUTATING: [&str; 9] = [
    "scenario_add",
    "scenario_update",
    "scenario_delete",
    "feature_create",
    "changes_commit",
    "changes_discard",
    "requirement_reword",
    "requirement_mark_implemented",
    "step_definition_create",
    // unit_test_create is also mutating; listed separately for the
    // "command_run is the only default exception" rule.
];

const ALSO_MUTATING: [&str; 1] = ["unit_test_create"];

#[derive(Debug, Clone, Default)]
pub struct ProfileOverrides {
    pub replace: BTreeMap<String, Vec<String>>,
    pub attached: BTreeMap<String, Vec<String>>,
    pub removed: BTreeMap<String, Vec<String>>,
}

pub struct Resolved {
    pub tools: Vec<ToolDefinition>,
    pub unknown: Vec<String>,
}

/// resolved = (replace[caller] or default_profile(caller)) + attached[caller] - removed[caller]
pub fn resolve(
    caller: Caller,
    overrides: &ProfileOverrides,
    catalog: &[ToolDefinition],
) -> Resolved {
    let key = caller.key();
    let mut names: Vec<String> = overrides.replace.get(key).cloned().unwrap_or_else(|| {
        default_profile(caller)
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    });
    if let Some(attached) = overrides.attached.get(key) {
        for name in attached {
            if !names.iter().any(|existing| existing == name) {
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
            "changes_show",
            "changes_validate",
            "changes_commit",
            "changes_discard",
            "command_run",
            "requirement_reword",
            "requirement_mark_implemented",
            "step_definitions_find",
            "step_definition_create",
            "unit_test_create",
        ]
    }

    #[test]
    fn caller_keys_round_trip() {
        for caller in Caller::ALL {
            assert_eq!(Caller::parse(caller.key()), Some(caller));
        }
        assert_eq!(Caller::parse("nonsense"), None);
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

    #[test]
    fn resolve_replace_attach_remove_and_unknown() {
        let catalog = catalog(&all_builtin_names());
        let mut overrides = ProfileOverrides::default();
        overrides.replace.insert(
            "status".into(),
            vec!["get_tdd_state".into(), "changes_show".into()],
        );
        overrides
            .attached
            .insert("status".into(), vec!["playwright__browser_navigate".into()]);
        overrides
            .removed
            .insert("status".into(), vec!["changes_show".into()]);
        let resolved = resolve(Caller::Status, &overrides, &catalog);
        let names: Vec<_> = resolved.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["get_tdd_state"]);
        assert_eq!(resolved.unknown, vec!["playwright__browser_navigate"]);
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
        overrides.replace.insert(
            "status".into(),
            vec![
                "builtin:validate_spec".into(),
                "self:validate_spec".into(),
                "get_tdd_state".into(),
            ],
        );
        let resolved = resolve(Caller::Status, &overrides, &catalog);
        let names: Vec<_> = resolved.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["validate_spec", "get_tdd_state", "self__validate_spec"]
        );
        assert!(resolved.unknown.is_empty());
    }
}

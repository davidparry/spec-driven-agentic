//! Per-command tool catalog: list, show, attach, detach, refresh.

use crate::domain::mcp_registry::RegistryLoad;
use crate::domain::tool_profile::{Caller, ProfileOverrides, Resolved, default_profile, resolve};
use crate::domain::tools::{ToolDefinition, ToolOrigin, find, first_sentence};
use crate::ports::{McpRegistrySource, ToolDiscovery, ToolError, ToolStore};

pub struct ToolList {
    pub path: Option<String>,
    pub problems: Vec<String>,
    pub tools: Vec<ToolDefinition>,
    pub undiscovered: Vec<String>,
}

pub struct ProfileView {
    pub caller: String,
    pub tools: Vec<String>,
    pub unknown: Vec<String>,
}

pub struct ToolService<S, D, R>
where
    S: ToolStore,
    D: ToolDiscovery,
    R: McpRegistrySource,
{
    store: S,
    discovery: D,
    registry: R,
    builtins: Vec<ToolDefinition>,
}

impl<S, D, R> ToolService<S, D, R>
where
    S: ToolStore,
    D: ToolDiscovery,
    R: McpRegistrySource,
{
    pub fn new(store: S, discovery: D, registry: R, builtins: Vec<ToolDefinition>) -> Self {
        Self {
            store,
            discovery,
            registry,
            builtins,
        }
    }

    pub fn registry(&self) -> RegistryLoad {
        self.registry.load()
    }

    pub fn catalog(&self, refresh: bool, offline: bool) -> ToolList {
        let load = self.registry.load();
        let mut tools = self.builtins.clone();
        let mut undiscovered = Vec::new();
        let mut problems = load.problems.clone();
        if !offline {
            for server in &load.servers {
                let result = if refresh {
                    self.discovery.discover_fresh(server)
                } else {
                    self.discovery.discover(server)
                };
                match result {
                    Ok(found) => tools.extend(found),
                    Err(error) => {
                        problems.push(format!("{}: {}", server.name, error.0));
                        undiscovered.push(server.name.clone());
                    }
                }
            }
        } else {
            for server in &load.servers {
                undiscovered.push(server.name.clone());
            }
        }
        ToolList {
            path: load.path,
            problems,
            tools,
            undiscovered,
        }
    }

    pub fn list_for(
        &self,
        caller: Caller,
        refresh: bool,
        offline: bool,
    ) -> (Resolved, Vec<String>) {
        let list = self.catalog(refresh, offline);
        let resolved = resolve(caller, &self.store.overrides(), &list.tools);
        (resolved, list.problems)
    }

    pub fn profiles(&self, offline: bool) -> (Vec<ProfileView>, Vec<String>) {
        let list = self.catalog(false, offline);
        let overrides = self.store.overrides();
        let views = Caller::ALL
            .into_iter()
            .map(|caller| {
                let resolved = resolve(caller, &overrides, &list.tools);
                ProfileView {
                    caller: caller.key().to_string(),
                    tools: resolved.tools.iter().map(|t| t.name.clone()).collect(),
                    unknown: resolved.unknown,
                }
            })
            .collect();
        (views, list.problems)
    }

    pub fn show(&self, name: &str) -> Result<ToolDefinition, ToolError> {
        let list = self.catalog(false, false);
        find(&list.tools, name).cloned().map_err(ToolError)
    }

    pub fn enable(&self, name: &str, caller: Caller) -> Result<(), ToolError> {
        let list = self.catalog(false, false);
        let definition = find(&list.tools, name).map_err(ToolError)?;
        self.store.attach(caller.key(), &definition.name)
    }

    pub fn disable(&self, name: &str, caller: Caller) -> Result<(), ToolError> {
        self.store.detach(caller.key(), name)
    }

    pub fn summary(tool: &ToolDefinition) -> String {
        format!("{} — {}", tool.name, first_sentence(&tool.description))
    }

    pub fn origin_label(origin: &ToolOrigin) -> String {
        match origin {
            ToolOrigin::Builtin => "builtin".into(),
            ToolOrigin::Server(name) => format!("server {name}"),
        }
    }

    pub fn default_names(caller: Caller) -> &'static [&'static str] {
        default_profile(caller)
    }

    pub fn overrides(&self) -> ProfileOverrides {
        self.store.overrides()
    }
}

/// Parse `--for <caller>`. Missing or unknown names are errors that list
/// the valid keys so the human can fix the flag without guessing.
pub fn parse_caller(raw: Option<&str>) -> Result<Caller, ToolError> {
    let names: Vec<&str> = Caller::ALL.iter().map(|c| c.key()).collect();
    let Some(raw) = raw else {
        return Err(ToolError(format!(
            "bdd tools enable/disable requires --for <caller>. Valid callers: {}",
            names.join(", ")
        )));
    };
    Caller::parse(raw).ok_or_else(|| {
        ToolError(format!(
            "unknown caller {raw:?} - pick one of: {}",
            names.join(", ")
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mcp_registry::{RegistryLoad, ServerSpec};
    use crate::domain::tools::{ToolDefinition, ToolOrigin};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    struct MemoryStore(RefCell<ProfileOverrides>);
    impl ToolStore for MemoryStore {
        fn overrides(&self) -> ProfileOverrides {
            self.0.borrow().clone()
        }
        fn attach(&self, caller: &str, tool: &str) -> Result<(), ToolError> {
            self.0
                .borrow_mut()
                .attached
                .entry(caller.into())
                .or_default()
                .push(tool.into());
            Ok(())
        }
        fn detach(&self, caller: &str, tool: &str) -> Result<(), ToolError> {
            self.0
                .borrow_mut()
                .removed
                .entry(caller.into())
                .or_default()
                .push(tool.into());
            Ok(())
        }
    }

    struct FakeDiscovery {
        calls: RefCell<usize>,
        fail: bool,
    }
    impl ToolDiscovery for FakeDiscovery {
        fn discover(&self, server: &ServerSpec) -> Result<Vec<ToolDefinition>, ToolError> {
            *self.calls.borrow_mut() += 1;
            if self.fail {
                return Err(ToolError(format!("{} down", server.name)));
            }
            Ok(vec![ToolDefinition {
                name: format!("{}__extra", server.name),
                description: "external".into(),
                schema: serde_json::json!({"type": "object"}),
                origin: ToolOrigin::Server(server.name.clone()),
            }])
        }
    }

    struct FakeRegistry(RegistryLoad);
    impl McpRegistrySource for FakeRegistry {
        fn load(&self) -> RegistryLoad {
            self.0.clone()
        }
    }

    fn builtin() -> Vec<ToolDefinition> {
        default_profile(Caller::Status)
            .iter()
            .map(|name| ToolDefinition {
                name: (*name).into(),
                description: format!("{name}."),
                schema: serde_json::json!({"type": "object"}),
                origin: ToolOrigin::Builtin,
            })
            .collect()
    }

    fn service(fail: bool) -> ToolService<MemoryStore, FakeDiscovery, FakeRegistry> {
        ToolService::new(
            MemoryStore(RefCell::new(ProfileOverrides::default())),
            FakeDiscovery {
                calls: RefCell::new(0),
                fail,
            },
            FakeRegistry(RegistryLoad {
                path: Some("mcp.json".into()),
                servers: vec![ServerSpec {
                    name: "self".into(),
                    program: "bdd".into(),
                    args: vec![],
                    env: vec![],
                }],
                problems: vec![],
            }),
            builtin(),
        )
    }

    #[test]
    fn offline_list_skips_discovery() {
        let service = service(false);
        let list = service.catalog(false, true);
        assert!(
            list.tools
                .iter()
                .all(|t| matches!(t.origin, ToolOrigin::Builtin))
        );
        assert_eq!(*service.discovery.calls.borrow(), 0);
        assert_eq!(list.undiscovered, vec!["self"]);
    }

    #[test]
    fn discovery_failure_is_inline_and_non_fatal() {
        let service = service(true);
        let list = service.catalog(false, false);
        assert!(!list.problems.is_empty());
        assert!(list.tools.iter().any(|t| t.name == "get_tdd_state"));
    }

    #[test]
    fn enable_for_an_unknown_caller_is_the_caller_parse() {
        assert!(Caller::parse("nonsense").is_none());
        let error = parse_caller(Some("nonsense")).unwrap_err();
        assert!(error.0.contains("unknown caller"), "{}", error.0);
        let missing = parse_caller(None).unwrap_err();
        assert!(missing.0.contains("requires --for"), "{}", missing.0);
        assert!(missing.0.contains("spec-draft"), "{}", missing.0);
    }

    #[test]
    fn enable_persists_attachment() {
        let service = service(false);
        service.enable("get_tdd_state", Caller::Ask).unwrap();
        assert!(
            service
                .overrides()
                .attached
                .get("ask")
                .unwrap()
                .contains(&"get_tdd_state".to_string())
        );
    }

    #[test]
    fn enable_unknown_tool_is_refused() {
        let service = service(false);
        let error = service.enable("nope", Caller::Status).unwrap_err();
        assert!(error.0.contains("unknown"), "{}", error.0);
    }

    #[test]
    fn show_returns_the_definition() {
        let service = service(false);
        let shown = service.show("validate_spec").unwrap();
        assert_eq!(shown.name, "validate_spec");
        assert_eq!(
            ToolService::<MemoryStore, FakeDiscovery, FakeRegistry>::origin_label(&shown.origin),
            "builtin"
        );
    }

    #[test]
    fn profiles_name_every_caller() {
        let service = service(true);
        let (views, _) = service.profiles(true);
        assert_eq!(views.len(), Caller::ALL.len());
        assert!(
            views
                .iter()
                .any(|v| v.caller == "status" && !v.tools.is_empty())
        );
    }

    #[test]
    fn replace_override_is_visible() {
        let service = service(true);
        service
            .store
            .0
            .borrow_mut()
            .replace
            .insert("status".into(), vec!["get_tdd_state".into()]);
        let (resolved, _) = service.list_for(Caller::Status, false, true);
        let names: Vec<_> = resolved.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["get_tdd_state"]);
        let _unused: BTreeMap<String, Vec<String>> = BTreeMap::new();
    }
}

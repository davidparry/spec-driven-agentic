//! Reads the first existing mcp.json candidate and parses it. Never
//! fails: an absent or broken file is reported as problems.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::mcp_registry::{RegistryLoad, config_candidates, parse_registry};
use crate::ports::McpRegistrySource;

pub struct FsMcpRegistry {
    root: PathBuf,
    configured: Option<String>,
}

impl FsMcpRegistry {
    pub fn new(root: PathBuf, configured: Option<String>) -> Self {
        Self { root, configured }
    }
}

impl McpRegistrySource for FsMcpRegistry {
    fn load(&self) -> RegistryLoad {
        let env_path = std::env::var("SPEC_MCP_CONFIG").ok();
        let home = std::env::var("HOME")
            .ok()
            .or_else(|| std::env::var("USERPROFILE").ok())
            .map(PathBuf::from);
        let candidates = config_candidates(
            &self.root,
            self.configured.as_deref(),
            env_path.as_deref(),
            home.as_deref(),
        );
        let Some(path) = candidates.into_iter().find(|path| path.exists()) else {
            return RegistryLoad::default();
        };
        load_file(&path, &self.root)
    }
}

fn load_file(path: &Path, root: &Path) -> RegistryLoad {
    let display = path.display().to_string();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return RegistryLoad {
                path: Some(display.clone()),
                servers: Vec::new(),
                problems: vec![format!("{display} is unreadable: {error}")],
            };
        }
    };
    let root = root.to_string_lossy();
    let mut load = parse_registry(&text, &root, &|name| std::env::var(name).ok());
    if load.problems.iter().any(|p| p.starts_with("invalid JSON")) {
        load.problems = load
            .problems
            .into_iter()
            .map(|problem| format!("{display}: {problem}"))
            .collect();
    }
    load.path = Some(display);
    load
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let load = FsMcpRegistry::new(dir.path().to_path_buf(), None).load();
        assert!(load.path.is_none());
        assert!(load.servers.is_empty());
        assert!(load.problems.is_empty());
    }

    #[test]
    fn the_root_mcp_json_is_used_when_present() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("mcp.json"),
            r#"{"mcpServers":{"self":{"command":"spec","args":["mcp","serve"]}}}"#,
        )
        .unwrap();
        let load = FsMcpRegistry::new(dir.path().to_path_buf(), None).load();
        assert!(load.path.unwrap().ends_with("mcp.json"));
        assert_eq!(load.servers.len(), 1);
        assert_eq!(load.servers[0].program, "spec");
    }

    #[test]
    fn invalid_json_is_a_problem_and_built_ins_still_work() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mcp.json"), "{").unwrap();
        let load = FsMcpRegistry::new(dir.path().to_path_buf(), None).load();
        assert!(!load.problems.is_empty());
        assert!(load.servers.is_empty());
        assert!(load.path.is_some());
    }

    #[test]
    fn configured_path_wins_when_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom.json");
        fs::write(&custom, r#"{"mcpServers":{"only":{"command":"echo"}}}"#).unwrap();
        fs::write(
            dir.path().join("mcp.json"),
            r#"{"mcpServers":{"ignored":{"command":"false"}}}"#,
        )
        .unwrap();
        let load = FsMcpRegistry::new(
            dir.path().to_path_buf(),
            Some(custom.to_string_lossy().into()),
        )
        .load();
        assert_eq!(load.servers[0].name, "only");
    }

    #[test]
    fn an_unreadable_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        fs::write(&path, "{}").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o000);
            fs::set_permissions(&path, perms).unwrap();
        }
        let load = load_file(&path, dir.path());
        #[cfg(unix)]
        {
            let mut restore = fs::metadata(&path).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            restore.set_mode(0o644);
            let _ = fs::set_permissions(&path, restore);
            if load.problems.is_empty() {
                // Some environments (root) can still read mode 000.
                return;
            }
            assert!(load.problems[0].contains("unreadable"));
        }
        let _ = load;
    }
}

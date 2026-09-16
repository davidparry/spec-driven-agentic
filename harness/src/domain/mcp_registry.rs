//! mcp.json registry: path candidates, `${}` expansion, and parsing.
//! Pure — the adapter checks `.exists()` and reads the file.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistryLoad {
    pub path: Option<String>,
    pub servers: Vec<ServerSpec>,
    pub problems: Vec<String>,
}

pub fn config_candidates(
    root: &Path,
    configured: Option<&str>,
    env_path: Option<&str>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(configured) = configured {
        paths.push(PathBuf::from(configured));
    }
    if let Some(env_path) = env_path {
        paths.push(PathBuf::from(env_path));
    }
    paths.push(root.join("mcp.json"));
    paths.push(root.join(".mcp.json"));
    paths.push(root.join("config/mcp.json"));
    paths.push(root.join(".cursor/mcp.json"));
    paths.push(root.join(".vscode/mcp.json"));
    if let Some(home) = home {
        paths.push(home.join(".bdd/mcp.json"));
    }
    paths
}

pub fn expand(
    text: &str,
    root: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> (String, Vec<String>) {
    let mut problems = Vec::new();
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if rest.starts_with("${workspaceFolder}")
            || rest.starts_with("${workspaceRoot}")
            || rest.starts_with("${cwd}")
        {
            let token = if rest.starts_with("${workspaceFolder}") {
                "${workspaceFolder}"
            } else if rest.starts_with("${workspaceRoot}") {
                "${workspaceRoot}"
            } else {
                "${cwd}"
            };
            out.push_str(root);
            rest = &rest[token.len()..];
            continue;
        }
        if let Some(inner) = rest.strip_prefix("${env:")
            && let Some(end) = inner.find('}')
        {
            let var = &inner[..end];
            match env(var) {
                Some(value) => out.push_str(&value),
                None => {
                    problems.push(format!("environment variable {var} is unset"));
                }
            }
            rest = &inner[end + 1..];
            continue;
        }
        if rest.starts_with('$') {
            let name: String = rest[1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                out.push('$');
                rest = &rest[1..];
                continue;
            }
            match env(&name) {
                Some(value) => out.push_str(&value),
                None => problems.push(format!("environment variable {name} is unset")),
            }
            rest = &rest[1 + name.len()..];
            continue;
        }
        out.push('$');
        rest = &rest[1..];
    }
    out.push_str(rest);
    (out, problems)
}

pub fn parse_registry(
    json: &str,
    root: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> RegistryLoad {
    let mut load = RegistryLoad::default();
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            load.problems.push(format!("invalid JSON: {error}"));
            return load;
        }
    };
    let servers = value
        .get("mcpServers")
        .or_else(|| value.get("servers"))
        .and_then(|v| v.as_object());
    let Some(servers) = servers else {
        load.problems
            .push("neither mcpServers nor servers is present".into());
        return load;
    };
    let mut used_names = Vec::new();
    for (raw_name, entry) in servers {
        if entry.get("disabled").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let transport = entry
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("stdio");
        if entry.get("url").is_some() || transport != "stdio" {
            load.problems.push(format!(
                "{raw_name}: {transport} transport is not supported yet — only stdio"
            ));
            continue;
        }
        let Some(command) = entry.get("command").and_then(|v| v.as_str()) else {
            load.problems
                .push(format!("{raw_name}: only stdio servers are supported"));
            continue;
        };
        let (program, mut problems) = expand(command, root, env);
        load.problems.append(&mut problems);
        let args = entry
            .get("args")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(|item| {
                        let (expanded, mut extra) = expand(item, root, env);
                        load.problems.append(&mut extra);
                        expanded
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut env_pairs: Vec<(String, String)> = entry
            .get("env")
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .map(|(k, v)| {
                        let raw = v.as_str().unwrap_or("");
                        let (expanded, mut extra) = expand(raw, root, env);
                        load.problems.append(&mut extra);
                        (k.clone(), expanded)
                    })
                    .collect()
            })
            .unwrap_or_default();
        env_pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut name = crate::domain::tools::sanitize_segment(raw_name);
        if name.is_empty() {
            name = "server".into();
        }
        let mut suffix = 2u32;
        let base = name.clone();
        while used_names.iter().any(|used| used == &name) {
            name = format!("{base}_{suffix}");
            load.problems
                .push(format!("duplicate server name {base} renamed to {name}"));
            suffix += 1;
        }
        used_names.push(name.clone());
        load.servers.push(ServerSpec {
            name,
            program,
            args,
            env: env_pairs,
        });
    }
    load
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn candidates_are_ordered_and_configured_wins_first() {
        let root = Path::new("/proj");
        let home = Path::new("/home/dev");
        let paths = config_candidates(root, Some("/cfg.json"), Some("/env.json"), Some(home));
        assert_eq!(paths[0], PathBuf::from("/cfg.json"));
        assert_eq!(paths[1], PathBuf::from("/env.json"));
        assert_eq!(paths[2], root.join("mcp.json"));
        assert_eq!(paths.last().unwrap(), &home.join(".bdd/mcp.json"));
    }

    fn env(name: &str) -> Option<String> {
        match name {
            "FOO" => Some("bar".into()),
            _ => None,
        }
    }

    #[test]
    fn expand_workspace_and_env_forms() {
        let (text, problems) = expand(
            "${workspaceFolder}/bin $FOO ${env:FOO} ${env:MISSING}",
            "/root",
            &env,
        );
        assert_eq!(text, "/root/bin bar bar ");
        assert!(problems.iter().any(|p| p.contains("MISSING")));
    }

    #[test]
    fn parse_table_covers_the_shapes() {
        let json = r#"{
            "servers": {
                "play wright": { "command": "${workspaceFolder}/bin", "args": ["${env:FOO}"] },
                "play-wright": { "command": "npx" },
                "skip": { "disabled": true, "command": "x" },
                "http": { "url": "http://localhost", "type": "http" },
                "nope": {}
            }
        }"#;
        let load = parse_registry(json, "/root", &env);
        assert!(load.servers.iter().any(|s| s.program == "/root/bin"));
        assert!(
            load.problems
                .iter()
                .any(|p| p.contains("only stdio") || p.contains("not supported"))
        );
        assert!(
            load.problems
                .iter()
                .any(|p| p.contains("only stdio servers") || p.contains("nope"))
        );
    }

    #[test]
    fn invalid_json_and_missing_keys_are_problems() {
        let load = parse_registry("{", "/root", &env);
        assert!(!load.problems.is_empty());
        assert!(load.servers.is_empty());
        let load = parse_registry("{}", "/root", &env);
        assert!(load.problems.iter().any(|p| p.contains("neither")));
    }
}

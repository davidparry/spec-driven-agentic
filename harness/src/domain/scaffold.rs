//! Greenfield scaffolding: the per-language file set `bdd init` creates —
//! a build file, a Cucumber runner, an empty requirements spec, and the
//! harness configuration. Pure text; writing is the adapter's job.

use crate::domain::config_report::{
    DEFAULT_LLM_CACHE_TTL_SECONDS, DEFAULT_LLM_ENDPOINT, DEFAULT_LLM_RETRY,
    DEFAULT_LLM_TIMEOUT_SECONDS, DEFAULT_TOOLS_CACHE_TTL_SECONDS,
    DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS, DEFAULT_TOOLS_CONFIRM,
    DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS, DEFAULT_TOOLS_MAX_ROUNDS,
};
use crate::domain::language::Language;
use crate::domain::tool_profile::{Caller, default_profile};
use crate::domain::tools::BUILTIN_ORIGIN;
use crate::domain::{CONFIG_FILE, RECOMMENDED_MODEL};

/// One file the scaffold wants on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldFile {
    pub path: String,
    pub content: String,
}

/// The scaffold for one language: shared workflow files plus the
/// ecosystem's build file and Cucumber runner.
pub fn scaffold(language: Language, project_name: &str) -> Vec<ScaffoldFile> {
    let mut files = vec![
        ScaffoldFile {
            path: "requirements/requirements.json".into(),
            content: format!(
                "{{\n  \"project\": \"{project_name}\",\n  \"requirements\": []\n}}\n"
            ),
        },
        ScaffoldFile {
            path: CONFIG_FILE.into(),
            content: default_config_toml(),
        },
        ScaffoldFile {
            path: ".gitignore".into(),
            content: "# Cached LLM responses; safe to delete at any time.\n.bdd-cache/\n\
                      # Diagnostic logs written by the bdd harness; safe to delete.\n.bdd-log/\n"
                .into(),
        },
    ];
    files.extend(match language {
        Language::Java => java_files(project_name),
        Language::JavaScript => javascript_files(project_name),
        Language::TypeScript => typescript_files(project_name),
        Language::DotNet => dotnet_files(project_name),
        Language::Rust => rust_files(project_name),
    });
    files
}

/// `.bdd.toml` with every key the harness reads. Scalar knobs stay
/// commented at their defaults. `[tools.profiles]` is written live:
/// each LLM-backed command and the tools that call offers the model.
pub fn default_config_toml() -> String {
    format!(
        "\
# bdd harness configuration.
#
# Keys left commented use the defaults shown. Uncomment to override.
# [tools.profiles] is the tools each command offers the model.

[llm]
# Persisted by `bdd model use`. Flag `--model` wins for one run.
# model = \"{model}\"
endpoint = \"{endpoint}\"
# Generation timeout; large prompts on local models can need more.
# timeout_seconds = {timeout}
# Identical requests reuse the cached response in .bdd-cache/
# for this many seconds; 0 disables the cache.
# cache_ttl_seconds = {llm_cache}
# How many times to try a model call when the reply fails
# validation; each retry includes the invalid reply. `--retry` wins.
# retry = {retry}

[tools]
# Tool-call rounds Agent::ask may take before it must return a reply.
# `--max-rounds` wins for one run.
# max_rounds = {max_rounds}
# Names that must be confirmed by the human before the call runs.
# confirm = [{confirm}]
# How long connecting to an mcp.json server may take.
# discovery_timeout_seconds = {discovery}
# How long one tool invocation may take.
# call_timeout_seconds = {call}
# How long discovered mcp.json tool lists stay cached under .bdd-cache/tools/.
# cache_ttl_seconds = {tools_cache}
# Optional path to an mcp.json (otherwise the usual candidates are tried).
# mcp_config = \"mcp.json\"

# Tools offered to the model on each LLM-backed command. These are the
# built-in defaults, written so you can see and edit them. A listed
# command's array replaces that command's set. Name a tool as:
#   validate_spec                 — built-in (exact catalog name)
#   {builtin}:validate_spec       — the same built-in, pinned when an
#                                   mcp.json tool shares the short name
#   playwright:browser_navigate   — mcp.json server \"playwright\"
#   playwright__browser_navigate  — same MCP tool (catalog name)
#
# Add an MCP server in mcp.json, then `bdd tools refresh`, then put
# `server:tool` (or `server__tool`) on the command that should use it.

{profiles}\
# Extra tools attached on top of the default or profiles list.
# [tools.enabled]
# implement = [\"playwright:browser_navigate\"]
#
# Tools removed from the default or profiles list.
# [tools.disabled]
# implement = [\"command_run\"]
",
        model = RECOMMENDED_MODEL,
        endpoint = DEFAULT_LLM_ENDPOINT,
        timeout = DEFAULT_LLM_TIMEOUT_SECONDS,
        llm_cache = DEFAULT_LLM_CACHE_TTL_SECONDS,
        retry = DEFAULT_LLM_RETRY,
        max_rounds = DEFAULT_TOOLS_MAX_ROUNDS,
        confirm = toml_quoted_list(DEFAULT_TOOLS_CONFIRM),
        discovery = DEFAULT_TOOLS_DISCOVERY_TIMEOUT_SECONDS,
        call = DEFAULT_TOOLS_CALL_TIMEOUT_SECONDS,
        tools_cache = DEFAULT_TOOLS_CACHE_TTL_SECONDS,
        builtin = BUILTIN_ORIGIN,
        profiles = default_profiles_toml(),
    )
}

fn default_profiles_toml() -> String {
    let mut out = String::from("[tools.profiles]\n");
    for caller in Caller::ALL {
        out.push_str(&format!(
            "# {}\n{} = [{}]\n",
            caller.cli_command(),
            caller.key(),
            toml_quoted_list(default_profile(caller))
        ));
    }
    out
}

fn toml_quoted_list(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn java_files(project_name: &str) -> Vec<ScaffoldFile> {
    let artifact = slug(project_name);
    vec![
        ScaffoldFile {
            path: "pom.xml".into(),
            content: format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>{artifact}</artifactId>
  <version>0.1.0-SNAPSHOT</version>
  <properties>
    <maven.compiler.release>21</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencies>
    <dependency>
      <groupId>io.cucumber</groupId>
      <artifactId>cucumber-java</artifactId>
      <version>7.20.1</version>
      <scope>test</scope>
    </dependency>
    <dependency>
      <groupId>io.cucumber</groupId>
      <artifactId>cucumber-junit-platform-engine</artifactId>
      <version>7.20.1</version>
      <scope>test</scope>
    </dependency>
    <dependency>
      <groupId>org.junit.jupiter</groupId>
      <artifactId>junit-jupiter</artifactId>
      <version>5.11.4</version>
      <scope>test</scope>
    </dependency>
    <dependency>
      <groupId>org.junit.platform</groupId>
      <artifactId>junit-platform-suite</artifactId>
      <version>1.11.4</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
</project>
"#
            ),
        },
        ScaffoldFile {
            path: "src/test/java/RunCucumberTest.java".into(),
            // No glue configuration: Cucumber then scans the classpath
            // root, which includes the default package where the
            // generated steps live. Pinning glue to a named package
            // (e.g. "steps") is a trap - Java forbids a named package
            // from referencing the default-package production class,
            // so generated steps there could never compile against it.
            content: r#"import org.junit.platform.suite.api.SelectDirectories;
import org.junit.platform.suite.api.Suite;

@Suite
@SelectDirectories("features")
public class RunCucumberTest {
}
"#
            .into(),
        },
        ScaffoldFile {
            path: "features/.gitkeep".into(),
            content: String::new(),
        },
    ]
}

fn javascript_files(project_name: &str) -> Vec<ScaffoldFile> {
    vec![
        ScaffoldFile {
            path: "package.json".into(),
            content: format!(
                r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "private": true,
  "scripts": {{
    "test": "cucumber-js"
  }},
  "devDependencies": {{
    "@cucumber/cucumber": "^11.0.0"
  }}
}}
"#,
                name = slug(project_name)
            ),
        },
        ScaffoldFile {
            path: "cucumber.js".into(),
            content: "module.exports = { default: { paths: ['features/**/*.feature'] } };\n".into(),
        },
        ScaffoldFile {
            path: "features/step_definitions/.gitkeep".into(),
            content: String::new(),
        },
    ]
}

fn typescript_files(project_name: &str) -> Vec<ScaffoldFile> {
    let mut files = vec![
        ScaffoldFile {
            path: "package.json".into(),
            content: format!(
                r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "private": true,
  "scripts": {{
    "test": "cucumber-js"
  }},
  "devDependencies": {{
    "@cucumber/cucumber": "^11.0.0",
    "ts-node": "^10.9.2",
    "typescript": "^5.6.0"
  }}
}}
"#,
                name = slug(project_name)
            ),
        },
        ScaffoldFile {
            path: "tsconfig.json".into(),
            content: r#"{
  "compilerOptions": {
    "module": "commonjs",
    "target": "es2022",
    "strict": true,
    "esModuleInterop": true
  }
}
"#
            .into(),
        },
        ScaffoldFile {
            path: "cucumber.js".into(),
            content: "module.exports = { default: { requireModule: ['ts-node/register'], \
                      require: ['features/step_definitions/**/*.ts'], \
                      paths: ['features/**/*.feature'] } };\n"
                .into(),
        },
    ];
    files.push(ScaffoldFile {
        path: "features/step_definitions/.gitkeep".into(),
        content: String::new(),
    });
    files
}

fn dotnet_files(project_name: &str) -> Vec<ScaffoldFile> {
    let name = pascal(project_name);
    vec![
        ScaffoldFile {
            path: format!("{name}.Tests.csproj"),
            content: r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <Nullable>enable</Nullable>
    <IsPackable>false</IsPackable>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Reqnroll.xUnit" Version="2.2.1" />
    <PackageReference Include="xunit" Version="2.9.2" />
    <PackageReference Include="xunit.runner.visualstudio" Version="2.8.2" />
    <PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.12.0" />
  </ItemGroup>
</Project>
"#
            .into(),
        },
        ScaffoldFile {
            path: "features/.gitkeep".into(),
            content: String::new(),
        },
    ]
}

fn rust_files(project_name: &str) -> Vec<ScaffoldFile> {
    vec![
        ScaffoldFile {
            path: "Cargo.toml".into(),
            content: format!(
                r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[[test]]
name = "cucumber"
harness = false

[dev-dependencies]
cucumber = "0.23"
futures = "0.3"
"#,
                name = slug(project_name)
            ),
        },
        ScaffoldFile {
            path: "src/lib.rs".into(),
            content: "// Production code lives here.\n".into(),
        },
        ScaffoldFile {
            path: "tests/cucumber.rs".into(),
            content: r#"use cucumber::World as _;

#[derive(Debug, Default, cucumber::World)]
struct World;

fn main() {
    futures::executor::block_on(World::run("features"));
}
"#
            .into(),
        },
        ScaffoldFile {
            path: "features/.gitkeep".into(),
            content: String::new(),
        },
    ]
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A lowercase, dash-separated identifier from free text; also used for
/// feature file names derived from requirement titles.
pub fn slug(text: &str) -> String {
    let name = words(text).join("-");
    if name.is_empty() {
        "project".into()
    } else {
        name
    }
}

fn pascal(text: &str) -> String {
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
        "Project".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_scaffolds_the_shared_workflow_files() {
        for language in Language::ALL {
            let files = scaffold(language, "String Calculator");
            let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
            assert!(
                paths.contains(&"requirements/requirements.json"),
                "{language:?}: {paths:?}"
            );
            assert!(paths.contains(&CONFIG_FILE), "{language:?}: {paths:?}");
            let spec = &files[0].content;
            assert!(spec.contains("\"project\": \"String Calculator\""));
            assert!(spec.contains("\"requirements\": []"));
            let toml = files.iter().find(|f| f.path == CONFIG_FILE).unwrap();
            assert!(
                toml.content.contains(crate::domain::RECOMMENDED_MODEL),
                "{language:?}: recommended Ollama model missing from scaffold"
            );
            assert!(
                toml.content.contains("cache_ttl_seconds"),
                "{language:?}: response-cache knob missing from scaffold"
            );
            assert!(
                toml.content
                    .contains(&format!("retry = {DEFAULT_LLM_RETRY}")),
                "{language:?}: retry knob missing from scaffold"
            );
            assert!(
                toml.content.contains("[tools.profiles]"),
                "{language:?}: per-command tool mapping missing from scaffold"
            );
            assert!(
                toml.content.contains("\nspec-draft = ["),
                "{language:?}: default profiles must be written live, not commented"
            );
            assert!(
                toml.content
                    .contains(&format!("{BUILTIN_ORIGIN}:validate_spec")),
                "{language:?}: builtin qualifier missing from scaffold"
            );
            for caller in Caller::ALL {
                assert!(
                    toml.content.contains(caller.key()),
                    "{language:?}: caller {} missing from scaffold",
                    caller.key()
                );
                assert!(
                    toml.content.contains(caller.cli_command()),
                    "{language:?}: harness command {} missing from scaffold",
                    caller.cli_command()
                );
            }
            let table = toml
                .content
                .parse::<toml::Table>()
                .expect("the scaffold .bdd.toml must be valid TOML");
            let profiles = table
                .get("tools")
                .and_then(|v| v.get("profiles"))
                .and_then(|v| v.as_table())
                .expect("live [tools.profiles] table");
            for caller in Caller::ALL {
                let names: Vec<&str> = profiles
                    .get(caller.key())
                    .and_then(|v| v.as_array())
                    .unwrap_or_else(|| panic!("missing profile {}", caller.key()))
                    .iter()
                    .map(|item| item.as_str().expect("tool name"))
                    .collect();
                assert_eq!(
                    names,
                    default_profile(caller),
                    "{language:?}: {} tools",
                    caller.key()
                );
            }
            let gitignore = files.iter().find(|f| f.path == ".gitignore").unwrap();
            assert!(
                gitignore.content.contains(".bdd-cache/"),
                "{language:?}: the response cache must stay out of version control"
            );
            assert!(
                gitignore.content.contains(".bdd-log/"),
                "{language:?}: the diagnostic logs must stay out of version control"
            );
        }
    }

    #[test]
    fn the_java_scaffold_has_a_maven_build_and_junit_platform_runner() {
        let files = scaffold(Language::Java, "String Calculator");
        let pom = files.iter().find(|f| f.path == "pom.xml").unwrap();
        assert!(
            pom.content
                .contains("<artifactId>string-calculator</artifactId>")
        );
        assert!(pom.content.contains("cucumber-junit-platform-engine"));
        let runner = files
            .iter()
            .find(|f| f.path == "src/test/java/RunCucumberTest.java")
            .unwrap();
        assert!(runner.content.contains("@SelectDirectories(\"features\")"));
        // Glue must stay unpinned: with no glue configuration Cucumber
        // scans the classpath root, so default-package steps are found.
        // A "steps"-package pin made the greenfield loop unwinnable -
        // named packages cannot reference the default-package production
        // class.
        assert!(!runner.content.contains("GLUE_PROPERTY_NAME"));
    }

    #[test]
    fn the_javascript_scaffold_wires_cucumber_js() {
        let files = scaffold(Language::JavaScript, "String Calculator");
        let package = files.iter().find(|f| f.path == "package.json").unwrap();
        assert!(package.content.contains("\"@cucumber/cucumber\""));
        assert!(package.content.contains("\"name\": \"string-calculator\""));
        assert!(files.iter().any(|f| f.path == "cucumber.js"));
    }

    #[test]
    fn the_typescript_scaffold_adds_tsconfig_and_ts_node() {
        let files = scaffold(Language::TypeScript, "String Calculator");
        assert!(files.iter().any(|f| f.path == "tsconfig.json"));
        let config = files.iter().find(|f| f.path == "cucumber.js").unwrap();
        assert!(config.content.contains("ts-node/register"));
    }

    #[test]
    fn the_dotnet_scaffold_is_a_reqnroll_test_project() {
        let files = scaffold(Language::DotNet, "String Calculator");
        let csproj = files
            .iter()
            .find(|f| f.path == "StringCalculator.Tests.csproj")
            .unwrap();
        assert!(csproj.content.contains("Reqnroll.xUnit"));
    }

    #[test]
    fn the_rust_scaffold_wires_cucumber_rs_with_a_harness_free_test() {
        let files = scaffold(Language::Rust, "String Calculator");
        let cargo = files.iter().find(|f| f.path == "Cargo.toml").unwrap();
        assert!(cargo.content.contains("name = \"string-calculator\""));
        assert!(cargo.content.contains("harness = false"));
        let harness = files
            .iter()
            .find(|f| f.path == "tests/cucumber.rs")
            .unwrap();
        assert!(harness.content.contains("World::run(\"features\")"));
    }

    #[test]
    fn empty_project_names_fall_back_to_generic_identifiers() {
        assert_eq!(slug("!!!"), "project");
        assert_eq!(pascal("!!!"), "Project");
    }
}

//! Greenfield scaffolding: the per-language file set `bdd init` creates —
//! a build file, a Cucumber runner, an empty requirements spec, and the
//! CLI configuration. Pure text; writing is the adapter's job.

use crate::domain::RECOMMENDED_MODEL;
use crate::domain::language::Language;

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
            path: ".bdd-mcp.toml".into(),
            content: format!(
                "[llm]\n# model = \"{RECOMMENDED_MODEL}\"\n\
                 endpoint = \"http://localhost:11434\"\n\
                 # Generation timeout; large prompts on local models can need more.\n\
                 # timeout_seconds = 300\n\
                 # Identical requests reuse the cached response in .bdd-cache/\n\
                 # for this many seconds; 0 disables the cache.\n\
                 # cache_ttl_seconds = 600\n\
                 # How many times to try a model call when the reply fails\n\
                 # validation; each retry includes the invalid reply.\n\
                 # retry = 3\n\n\
                 # Per-command tool profiles for model calls. Built-in defaults\n\
                 # apply until you attach or disable a tool.\n\
                 # [tools]\n\
                 # max_rounds = 12\n\
                 # confirm = [\"command_run\"]\n"
            ),
        },
        ScaffoldFile {
            path: ".gitignore".into(),
            content: "# Cached LLM responses; safe to delete at any time.\n.bdd-cache/\n\
                      # Diagnostic logs written by the bdd CLI; safe to delete.\n.bdd-log/\n"
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
            assert!(paths.contains(&".bdd-mcp.toml"), "{language:?}: {paths:?}");
            let spec = &files[0].content;
            assert!(spec.contains("\"project\": \"String Calculator\""));
            assert!(spec.contains("\"requirements\": []"));
            let toml = files.iter().find(|f| f.path == ".bdd-mcp.toml").unwrap();
            assert!(
                toml.content.contains(crate::domain::RECOMMENDED_MODEL),
                "{language:?}: recommended Ollama model missing from scaffold"
            );
            assert!(
                toml.content.contains("cache_ttl_seconds"),
                "{language:?}: response-cache knob missing from scaffold"
            );
            assert!(
                toml.content.contains("retry = 3"),
                "{language:?}: retry knob missing from scaffold"
            );
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

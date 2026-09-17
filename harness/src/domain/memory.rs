//! Project memory: the recorded language, libraries, and layout of the
//! target project. Scanning is pure — file bytes and the directory outline
//! arrive through the caller — so this module stays IO-free.

use serde::{Deserialize, Serialize};

use crate::domain::language::Language;
use crate::domain::layout::{LayoutInput, ResolvedLayout, resolve_layout};
use crate::domain::prompts::render_snippet;

/// Schema version written into `.spec-memory.json`.
pub const MEMORY_VERSION: u32 = 1;

/// How many outline entries survive into stored memory.
pub const MAX_OUTLINE_ENTRIES: usize = 40;

/// Paths deeper than this (slash count) are dropped from the outline.
pub const MAX_OUTLINE_DEPTH: usize = 4;

/// How many libraries the prompt brief lists.
const BRIEF_LIBRARY_LIMIT: usize = 20;

/// One dependency recorded from a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// The project's source layout: the one answer the harness uses for
/// every path it writes, the directory it runs tests in, and the layout
/// it briefs the model with. Resolved by
/// [`crate::domain::layout::resolve_layout`].
///
/// Every field is optional with a serde default so a `.spec-memory.json`
/// written before the layout was recorded still loads.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProjectStructure {
    /// The directory whose build file the test runner is pointed at,
    /// `None` when that is the project root.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "moduleRoot"
    )]
    pub module_root: Option<String>,
    /// The production source root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production: Option<String>,
    /// The test source root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<String>,
    /// The step-definition file generated steps are added to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "stepDefinitions"
    )]
    pub step_definitions: Option<String>,
    /// The package or namespace existing tests declare, where the
    /// ecosystem has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<String>,
}

impl ProjectStructure {
    /// Whether a resolver has answered this, as opposed to a
    /// `.spec-memory.json` written before layouts were recorded. The
    /// resolver always names a step-definition path, so that is the
    /// field that distinguishes the two.
    pub fn is_resolved(&self) -> bool {
        self.step_definitions.is_some()
    }
}

/// Durable project identity stored in `.spec-memory.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub version: u32,
    #[serde(default)]
    pub language: String,
    #[serde(default, rename = "bddFramework")]
    pub bdd_framework: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "buildTool")]
    pub build_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub structure: ProjectStructure,
    #[serde(default, rename = "refreshedAt")]
    pub refreshed_at: String,
}

impl ProjectMemory {
    /// True when nothing useful has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.language.is_empty()
    }

    /// Compact prompt section, or empty when there is nothing to brief.
    pub fn brief(&self) -> String {
        memory_brief(self)
    }
}

/// Manifest file contents the scanner already read.
#[derive(Debug, Default, Clone)]
pub struct Manifests {
    pub pom_xml: Option<String>,
    pub build_gradle: Option<String>,
    pub build_gradle_kts: Option<String>,
    pub package_json: Option<String>,
    pub cargo_toml: Option<String>,
    pub csproj: Vec<String>,
}

/// Everything the scanner needs, supplied by the application layer.
pub struct ScanInput<'a> {
    pub languages: &'a [Language],
    pub chosen: Option<Language>,
    pub manifests: &'a Manifests,
    pub tree: &'a [String],
    /// Feature files the spec's requirements name, which decide the
    /// module root when a tree holds several buildable modules.
    pub spec_features: &'a [String],
    pub now: &'a str,
}

/// A scan: the memory to persist, plus the module roots discovery could
/// not choose between. The candidates are deliberately not persisted —
/// they are a question for this run, answered once and then recorded as
/// a resolved `module_root`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryScan {
    pub memory: ProjectMemory,
    pub module_candidates: Vec<String>,
}

/// Build memory from a scan. `chosen` wins over marker detection so a
/// greenfield language pick survives a polyglot tree.
pub fn scan_memory(input: &ScanInput<'_>) -> MemoryScan {
    let language = input.chosen.or_else(|| input.languages.first().copied());
    let Some(language) = language else {
        return MemoryScan::default();
    };
    let libraries = collect_libraries(input.manifests);
    let ResolvedLayout {
        structure,
        candidates,
    } = infer_structure(input, language);
    MemoryScan {
        memory: ProjectMemory {
            version: MEMORY_VERSION,
            language: language.display().to_string(),
            bdd_framework: language.bdd_framework().to_string(),
            build_tool: infer_build_tool(language, input.manifests),
            libraries,
            structure,
            refreshed_at: input.now.to_string(),
        },
        module_candidates: candidates,
    }
}

/// Keep a previously chosen language when a later scan would pick another
/// ecosystem from extra marker files.
pub fn apply_chosen(mut scanned: ProjectMemory, chosen: Option<Language>) -> ProjectMemory {
    if let Some(language) = chosen {
        scanned.language = language.display().to_string();
        scanned.bdd_framework = language.bdd_framework().to_string();
    }
    scanned
}

fn infer_build_tool(language: Language, manifests: &Manifests) -> Option<String> {
    match language {
        Language::Java if manifests.pom_xml.is_some() => Some("Maven".into()),
        Language::Java
            if manifests.build_gradle.is_some() || manifests.build_gradle_kts.is_some() =>
        {
            Some("Gradle".into())
        }
        Language::JavaScript | Language::TypeScript if manifests.package_json.is_some() => {
            Some("npm".into())
        }
        Language::DotNet if !manifests.csproj.is_empty() => Some("dotnet".into()),
        Language::Rust if manifests.cargo_toml.is_some() => Some("Cargo".into()),
        _ => None,
    }
}

fn collect_libraries(manifests: &Manifests) -> Vec<Library> {
    let mut libraries = Vec::new();
    if let Some(text) = &manifests.pom_xml {
        libraries.extend(parse_pom_xml(text));
    }
    if let Some(text) = &manifests.build_gradle {
        libraries.extend(parse_gradle(text));
    }
    if let Some(text) = &manifests.build_gradle_kts {
        libraries.extend(parse_gradle(text));
    }
    if let Some(text) = &manifests.package_json {
        libraries.extend(parse_package_json(text));
    }
    if let Some(text) = &manifests.cargo_toml {
        libraries.extend(parse_cargo_toml(text));
    }
    for text in &manifests.csproj {
        libraries.extend(parse_csproj(text));
    }
    libraries
}

/// The layout, from the one resolver, with the tree outline attached for
/// the prompt brief.
fn infer_structure(input: &ScanInput<'_>, language: Language) -> ResolvedLayout {
    let mut resolved = resolve_layout(&LayoutInput {
        language,
        build_tool: infer_build_tool(language, input.manifests).as_deref(),
        tree: input.tree,
        spec_features: input.spec_features,
    });
    resolved.structure.outline = capped_outline(input.tree);
    resolved
}

/// The tree outline, capped for a prompt: shallow entries only, and a
/// bounded count, so a large repository cannot crowd out the question.
pub(crate) fn capped_outline(tree: &[String]) -> Vec<String> {
    tree.iter()
        .filter(|path| path_depth(path) <= MAX_OUTLINE_DEPTH)
        .take(MAX_OUTLINE_ENTRIES)
        .cloned()
        .collect()
}

fn path_depth(path: &str) -> usize {
    path.trim_end_matches('/').matches('/').count()
}

/// Render the shared `[project_memory]` snippet, or empty when unusable.
pub fn memory_brief(memory: &ProjectMemory) -> String {
    if memory.is_empty() {
        return String::new();
    }
    let libraries: Vec<String> = memory
        .libraries
        .iter()
        .take(BRIEF_LIBRARY_LIMIT)
        .map(format_library)
        .collect();
    render_snippet(
        "project_memory.brief",
        minijinja::context! {
            language => memory.language.as_str(),
            bdd_framework => memory.bdd_framework.as_str(),
            build_tool => memory.build_tool.clone().unwrap_or_default(),
            libraries,
            layout => layout_line(memory),
        },
    )
}

fn format_library(library: &Library) -> String {
    match &library.version {
        Some(version) if !version.is_empty() => format!("{} {version}", library.name),
        _ => library.name.clone(),
    }
}

fn layout_line(memory: &ProjectMemory) -> String {
    let structure = &memory.structure;
    let mut parts = Vec::new();
    if let Some(path) = &structure.module_root {
        parts.push(format!("{path}/ (module the build runs in)"));
    }
    if let Some(path) = &structure.production {
        parts.push(format!("{path} (production)"));
    }
    if let Some(path) = &structure.tests {
        parts.push(format!("{path} (tests)"));
    }
    if let Some(path) = &structure.features {
        parts.push(format!("{path}/"));
    }
    if let Some(path) = &structure.step_definitions {
        parts.push(format!("{path} (step definitions)"));
    }
    if let Some(package) = &structure.package {
        parts.push(format!("package {package}"));
    }
    if let Some(path) = &structure.spec {
        parts.push(path.clone());
    }
    parts.join(", ")
}

/// Prepend the memory brief to a system prompt, or return the system
/// prompt unchanged when the brief is empty.
pub fn prepend_brief(brief: &str, system: &str) -> String {
    let brief = brief.trim();
    if brief.is_empty() {
        system.to_string()
    } else {
        format!("{brief}\n\n{system}")
    }
}

pub fn parse_pom_xml(text: &str) -> Vec<Library> {
    let block = regex::Regex::new(r"(?s)<dependency>(.*?)</dependency>")
        .expect("dependency regex compiles");
    let artifact = regex::Regex::new(r"<artifactId>\s*([^<]+?)\s*</artifactId>")
        .expect("artifactId regex compiles");
    let version =
        regex::Regex::new(r"<version>\s*([^<]+?)\s*</version>").expect("version regex compiles");
    let scope = regex::Regex::new(r"<scope>\s*([^<]+?)\s*</scope>").expect("scope regex compiles");
    block
        .captures_iter(text)
        .filter_map(|caps| {
            let body = caps.get(1)?.as_str();
            let name = artifact.captures(body)?.get(1)?.as_str().trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(Library {
                name,
                version: version
                    .captures(body)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|v| !v.is_empty()),
                scope: scope
                    .captures(body)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|v| !v.is_empty()),
            })
        })
        .collect()
}

pub fn parse_gradle(text: &str) -> Vec<Library> {
    let coord = regex::Regex::new(
        r#"(?x)
        (?:testImplementation|implementation|api|compileOnly|runtimeOnly|testCompileOnly)
        \s*[\(\s]+['"]([^'"]+)['"]"#,
    )
    .expect("gradle regex compiles");
    coord
        .captures_iter(text)
        .filter_map(|caps| {
            let raw = caps.get(1)?.as_str();
            let mut parts = raw.split(':');
            let _group = parts.next()?;
            let name = parts.next()?.to_string();
            let version = parts.next().map(String::from);
            if name.is_empty() {
                return None;
            }
            Some(Library {
                name,
                version,
                scope: None,
            })
        })
        .collect()
}

pub fn parse_package_json(text: &str) -> Vec<Library> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut libraries = Vec::new();
    for (key, scope) in [("dependencies", None), ("devDependencies", Some("dev"))] {
        let Some(map) = value.get(key).and_then(|v| v.as_object()) else {
            continue;
        };
        for (name, version) in map {
            libraries.push(Library {
                name: name.clone(),
                version: version.as_str().map(String::from),
                scope: scope.map(String::from),
            });
        }
    }
    libraries
}

pub fn parse_cargo_toml(text: &str) -> Vec<Library> {
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let mut libraries = Vec::new();
    for (section, scope) in [("dependencies", None), ("dev-dependencies", Some("dev"))] {
        let Some(deps) = table.get(section).and_then(|v| v.as_table()) else {
            continue;
        };
        for (name, value) in deps {
            let version = match value {
                toml::Value::String(version) => Some(version.clone()),
                toml::Value::Table(inner) => inner
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                _ => None,
            };
            libraries.push(Library {
                name: name.clone(),
                version,
                scope: scope.map(String::from),
            });
        }
    }
    libraries
}

pub fn parse_csproj(text: &str) -> Vec<Library> {
    let include_first =
        regex::Regex::new(r#"<PackageReference\s+Include="([^"]+)"\s+Version="([^"]+)""#)
            .expect("csproj include-first regex compiles");
    let version_first =
        regex::Regex::new(r#"<PackageReference\s+Version="([^"]+)"\s+Include="([^"]+)""#)
            .expect("csproj version-first regex compiles");
    let mut libraries: Vec<Library> = include_first
        .captures_iter(text)
        .filter_map(|caps| {
            Some(Library {
                name: caps.get(1)?.as_str().to_string(),
                version: Some(caps.get(2)?.as_str().to_string()),
                scope: None,
            })
        })
        .collect();
    for caps in version_first.captures_iter(text) {
        let (Some(version), Some(name)) = (caps.get(1), caps.get(2)) else {
            continue;
        };
        libraries.push(Library {
            name: name.as_str().to_string(),
            version: Some(version.as_str().to_string()),
            scope: None,
        });
    }
    libraries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java_scan(manifests: Manifests, tree: &[&str]) -> ProjectMemory {
        let languages = [Language::Java];
        let tree: Vec<String> = tree.iter().map(|s| (*s).to_string()).collect();
        scan_memory(&ScanInput {
            languages: &languages,
            chosen: None,
            manifests: &manifests,
            tree: &tree,
            spec_features: &[],
            now: "2026-08-21T01:00:00Z",
        })
        .memory
    }

    #[test]
    fn an_empty_scan_with_no_language_is_empty() {
        let memory = scan_memory(&ScanInput {
            languages: &[],
            chosen: None,
            manifests: &Manifests::default(),
            tree: &[],
            spec_features: &[],
            now: "2026-08-21T01:00:00Z",
        })
        .memory;
        assert!(memory.is_empty());
        assert!(memory.brief().is_empty());
    }

    #[test]
    fn chosen_language_wins_over_detected_markers() {
        let languages = [Language::TypeScript, Language::Java];
        let memory = scan_memory(&ScanInput {
            languages: &languages,
            chosen: Some(Language::Java),
            manifests: &Manifests::default(),
            tree: &[],
            spec_features: &[],
            now: "2026-08-21T01:00:00Z",
        })
        .memory;
        assert_eq!(memory.language, "Java");
        assert_eq!(memory.bdd_framework, "Cucumber-JVM");
    }

    #[test]
    fn apply_chosen_rewrites_language_and_framework_only() {
        let scanned = java_scan(
            Manifests {
                pom_xml: Some(
                    "<dependency><artifactId>cucumber-java</artifactId>\
                     <version>7.20.1</version></dependency>"
                        .into(),
                ),
                ..Default::default()
            },
            &["pom.xml"],
        );
        let merged = apply_chosen(scanned.clone(), Some(Language::Rust));
        assert_eq!(merged.language, "Rust");
        assert_eq!(merged.bdd_framework, "cucumber-rs");
        assert_eq!(merged.libraries, scanned.libraries);
    }

    #[test]
    fn pom_xml_dependencies_become_libraries_and_maven() {
        let pom = r#"
            <dependency>
              <groupId>io.cucumber</groupId>
              <artifactId>cucumber-java</artifactId>
              <version>7.20.1</version>
              <scope>test</scope>
            </dependency>
            <dependency>
              <artifactId>junit-jupiter</artifactId>
              <version>5.11.4</version>
              <scope>test</scope>
            </dependency>
        "#;
        let memory = java_scan(
            Manifests {
                pom_xml: Some(pom.into()),
                ..Default::default()
            },
            &["pom.xml", "src/main/java/", "src/test/java/", "features/"],
        );
        assert_eq!(memory.build_tool.as_deref(), Some("Maven"));
        assert_eq!(memory.libraries[0].name, "cucumber-java");
        assert_eq!(memory.libraries[0].version.as_deref(), Some("7.20.1"));
        assert_eq!(memory.libraries[0].scope.as_deref(), Some("test"));
        assert_eq!(
            memory.structure.production.as_deref(),
            Some("src/main/java")
        );
        assert_eq!(memory.structure.features.as_deref(), Some("features"));
        let brief = memory.brief();
        assert!(brief.contains("Language: Java (Cucumber-JVM), build Maven"));
        assert!(brief.contains("cucumber-java 7.20.1"));
        assert!(brief.contains("src/main/java (production)"));
    }

    #[test]
    fn gradle_coordinates_are_parsed() {
        let text = r#"
            implementation 'com.google.guava:guava:33.0.0-jre'
            testImplementation("io.cucumber:cucumber-java:7.20.1")
        "#;
        let libs = parse_gradle(text);
        assert_eq!(libs[0].name, "guava");
        assert_eq!(libs[0].version.as_deref(), Some("33.0.0-jre"));
        assert_eq!(libs[1].name, "cucumber-java");
    }

    #[test]
    fn package_json_splits_deps_and_dev_deps() {
        let text = r#"{
            "dependencies": { "left-pad": "1.3.0" },
            "devDependencies": { "@cucumber/cucumber": "^11.0.0" }
        }"#;
        let libs = parse_package_json(text);
        assert!(
            libs.iter()
                .any(|l| l.name == "left-pad" && l.scope.is_none())
        );
        assert!(libs.iter().any(|l| {
            l.name == "@cucumber/cucumber"
                && l.version.as_deref() == Some("^11.0.0")
                && l.scope.as_deref() == Some("dev")
        }));
    }

    #[test]
    fn cargo_toml_reads_plain_and_table_deps() {
        let text = r#"
[dependencies]
serde = "1.0"

[dev-dependencies]
cucumber = { version = "0.23", features = ["libtest"] }
"#;
        let libs = parse_cargo_toml(text);
        assert!(
            libs.iter()
                .any(|l| l.name == "serde" && l.version.as_deref() == Some("1.0"))
        );
        assert!(libs.iter().any(|l| {
            l.name == "cucumber"
                && l.version.as_deref() == Some("0.23")
                && l.scope.as_deref() == Some("dev")
        }));
    }

    #[test]
    fn csproj_package_references_are_parsed() {
        let text = r#"
            <PackageReference Include="Reqnroll.xUnit" Version="2.2.1" />
            <PackageReference Version="2.9.2" Include="xunit" />
        "#;
        let libs = parse_csproj(text);
        assert_eq!(libs[0].name, "Reqnroll.xUnit");
        assert_eq!(libs[0].version.as_deref(), Some("2.2.1"));
        assert_eq!(libs[1].name, "xunit");
        assert_eq!(libs[1].version.as_deref(), Some("2.9.2"));
    }

    #[test]
    fn outline_drops_deep_paths_and_caps_length() {
        let mut tree = vec!["src/".into(), "src/main/".into()];
        for i in 0..50 {
            tree.push(format!("src/main/java/pkg/f{i}.java"));
        }
        let outline = capped_outline(&tree);
        assert!(outline.len() <= MAX_OUTLINE_ENTRIES);
        assert!(outline.iter().all(|p| path_depth(p) <= MAX_OUTLINE_DEPTH));
        assert!(outline.contains(&"src/".into()));
    }

    #[test]
    fn prepend_brief_skips_empty_and_joins_otherwise() {
        assert_eq!(prepend_brief("  ", "You implement"), "You implement");
        assert_eq!(
            prepend_brief("Project memory:\n- Language: Java", "You implement"),
            "Project memory:\n- Language: Java\n\nYou implement"
        );
    }

    #[test]
    fn unreadable_manifests_yield_no_libraries() {
        assert!(parse_package_json("not json").is_empty());
        assert!(parse_cargo_toml("[[[").is_empty());
        assert!(parse_pom_xml("<project/>").is_empty());
    }

    fn scan(language: Language, manifests: Manifests, tree: &[&str]) -> ProjectMemory {
        let languages = [language];
        let tree: Vec<String> = tree.iter().map(|s| (*s).to_string()).collect();
        scan_memory(&ScanInput {
            languages: &languages,
            chosen: None,
            manifests: &manifests,
            tree: &tree,
            spec_features: &[],
            now: "2026-08-21T01:00:00Z",
        })
        .memory
    }

    #[test]
    fn gradle_is_the_java_build_tool_without_a_pom() {
        let memory = scan(
            Language::Java,
            Manifests {
                build_gradle: Some("implementation 'com.google.guava:guava:33.0'".into()),
                ..Default::default()
            },
            &["build.gradle", "src/main/java/App.java"],
        );
        assert_eq!(memory.build_tool.as_deref(), Some("Gradle"));
        assert_eq!(memory.libraries[0].name, "guava");
    }

    #[test]
    fn gradle_kotlin_dsl_coordinates_are_collected() {
        let memory = scan(
            Language::Java,
            Manifests {
                build_gradle_kts: Some(
                    "testImplementation(\"io.cucumber:cucumber-java:7.20.1\")".into(),
                ),
                ..Default::default()
            },
            &["build.gradle.kts"],
        );
        assert_eq!(memory.build_tool.as_deref(), Some("Gradle"));
        assert_eq!(memory.libraries[0].name, "cucumber-java");
    }

    #[test]
    fn npm_is_the_build_tool_for_javascript_and_typescript() {
        let manifests = Manifests {
            package_json: Some(r#"{"dependencies":{"left-pad":"1.3.0"}}"#.into()),
            ..Default::default()
        };
        for language in [Language::JavaScript, Language::TypeScript] {
            let memory = scan(language, manifests.clone(), &["package.json", "src/app.js"]);
            assert_eq!(memory.build_tool.as_deref(), Some("npm"));
            assert_eq!(memory.structure.production.as_deref(), Some("src"));
        }
    }

    #[test]
    fn cargo_and_dotnet_scans_fill_build_tool_and_layout() {
        let rust = scan(
            Language::Rust,
            Manifests {
                cargo_toml: Some("[dependencies]\nserde = \"1.0\"\n".into()),
                ..Default::default()
            },
            &["Cargo.toml", "src/lib.rs", "tests/it.rs"],
        );
        assert_eq!(rust.build_tool.as_deref(), Some("Cargo"));
        assert_eq!(rust.structure.production.as_deref(), Some("src"));
        assert_eq!(rust.structure.tests.as_deref(), Some("tests"));

        // .NET used to come back with no layout at all. Its sources sit
        // beside the project file, so a flat tree has no production
        // subdirectory - but the tests and step definitions now resolve.
        let dotnet = scan(
            Language::DotNet,
            Manifests {
                csproj: vec![r#"<PackageReference Include="Reqnroll" Version="2.2.1" />"#.into()],
                ..Default::default()
            },
            &["App.csproj", "Program.cs", "Tests/CalculatorSteps.cs"],
        );
        assert_eq!(dotnet.build_tool.as_deref(), Some("dotnet"));
        assert_eq!(dotnet.structure.production.as_deref(), None);
        assert_eq!(dotnet.structure.tests.as_deref(), Some("Tests"));
        assert_eq!(
            dotnet.structure.step_definitions.as_deref(),
            Some("Tests/CalculatorSteps.cs")
        );
        assert_eq!(dotnet.libraries[0].name, "Reqnroll");
    }

    #[test]
    fn empty_artifact_names_are_dropped() {
        assert!(parse_pom_xml("<dependency><artifactId>  </artifactId></dependency>").is_empty());
        assert!(parse_gradle("implementation 'com.google::33.0'").is_empty());
    }

    #[test]
    fn package_json_skips_a_non_object_dependency_map() {
        let libs =
            parse_package_json(r#"{"dependencies":"nope","devDependencies":{"left-pad":"1.0"}}"#);
        assert_eq!(libs.len(), 1);
        assert_eq!(libs[0].name, "left-pad");
        assert_eq!(libs[0].scope.as_deref(), Some("dev"));
    }

    #[test]
    fn cargo_toml_skips_missing_sections_and_records_non_string_deps_without_a_version() {
        let libs = parse_cargo_toml("[dependencies]\nfoo = 1\nserde = \"1.0\"\n");
        assert!(
            libs.iter()
                .any(|l| l.name == "serde" && l.version.as_deref() == Some("1.0"))
        );
        assert!(
            libs.iter().any(|l| l.name == "foo" && l.version.is_none()),
            "an integer cargo value is stored without a version: {libs:?}"
        );
        assert!(libs.iter().all(|l| l.scope.is_none()));
    }

    #[test]
    fn a_library_without_a_version_is_named_plainly() {
        let libs = parse_pom_xml("<dependency><artifactId>junit</artifactId></dependency>");
        assert_eq!(libs[0].name, "junit");
        assert!(libs[0].version.is_none());
        assert_eq!(format_library(&libs[0]), "junit");
    }

    #[test]
    fn a_project_with_no_sources_yet_is_briefed_with_the_convention() {
        let memory = java_scan(Manifests::default(), &["README.md", "docs/guide.md"]);
        let brief = memory.brief();
        // Nothing is on disk to observe, so the layout is where the
        // ecosystem puts things - which is still what the model needs,
        // because that is where generated files will land.
        assert!(brief.contains("src/main/java (production)"), "{brief}");
        assert!(brief.contains("src/test/java (tests)"), "{brief}");
        // The full outline stays in memory for the layout question.
        assert!(memory.structure.outline.contains(&"README.md".to_string()));
    }

    #[test]
    fn a_spec_json_anywhere_in_the_tree_is_recorded() {
        let memory = java_scan(Manifests::default(), &["nested/deep/requirements.json"]);
        assert_eq!(
            memory.structure.spec.as_deref(),
            Some("nested/deep/requirements.json")
        );
    }
}

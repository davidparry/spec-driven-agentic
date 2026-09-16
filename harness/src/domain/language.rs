//! Supported target ecosystems and how a project reveals which ones it
//! uses. Detection is pure: marker-file questions are answered through
//! the [`ProjectFiles`] port.

use crate::ports::ProjectFiles;

/// A target ecosystem the harness can author for and (runtime permitting)
/// execute tests in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Java,
    JavaScript,
    TypeScript,
    DotNet,
    Rust,
}

/// The toolchain a language needs before tests can run. Authoring and
/// validation never require it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Jdk,
    Node,
    DotNetSdk,
    CargoToolchain,
}

impl Language {
    pub const ALL: [Language; 5] = [
        Language::Java,
        Language::JavaScript,
        Language::TypeScript,
        Language::DotNet,
        Language::Rust,
    ];

    pub fn display(self) -> &'static str {
        match self {
            Language::Java => "Java",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::DotNet => ".NET",
            Language::Rust => "Rust",
        }
    }

    /// The Cucumber-family BDD framework for this ecosystem. For .NET
    /// that is Reqnroll, the maintained successor of SpecFlow.
    pub fn bdd_framework(self) -> &'static str {
        match self {
            Language::Java => "Cucumber-JVM",
            Language::JavaScript | Language::TypeScript => "Cucumber-JS",
            Language::DotNet => "Reqnroll",
            Language::Rust => "cucumber-rs",
        }
    }

    pub fn runtime(self) -> Runtime {
        match self {
            Language::Java => Runtime::Jdk,
            Language::JavaScript | Language::TypeScript => Runtime::Node,
            Language::DotNet => Runtime::DotNetSdk,
            Language::Rust => Runtime::CargoToolchain,
        }
    }

    /// Parse a human answer or stored display name into a language.
    pub fn parse(answer: &str) -> Option<Language> {
        match answer.trim().to_lowercase().as_str() {
            "java" => Some(Language::Java),
            "javascript" | "js" => Some(Language::JavaScript),
            "typescript" | "ts" => Some(Language::TypeScript),
            "dotnet" | ".net" | "csharp" | "c#" => Some(Language::DotNet),
            "rust" => Some(Language::Rust),
            _ => Language::ALL
                .iter()
                .copied()
                .find(|language| language.display().eq_ignore_ascii_case(answer.trim())),
        }
    }
}

impl Runtime {
    /// The command probed to decide whether the runtime is installed.
    pub fn command(self) -> &'static str {
        match self {
            Runtime::Jdk => "java",
            Runtime::Node => "node",
            Runtime::DotNetSdk => "dotnet",
            Runtime::CargoToolchain => "cargo",
        }
    }
}

/// Detects which supported ecosystems a project uses, from its marker
/// files. A project with `tsconfig.json` is TypeScript, not JavaScript
/// as well — one entry per ecosystem.
pub fn detect_languages(files: &dyn ProjectFiles) -> Vec<Language> {
    let mut languages = Vec::new();
    if files.exists("pom.xml") || files.exists("build.gradle") || files.exists("build.gradle.kts") {
        languages.push(Language::Java);
    }
    if files.exists("package.json") {
        languages.push(if files.exists("tsconfig.json") {
            Language::TypeScript
        } else {
            Language::JavaScript
        });
    }
    if files.any_with_extension("csproj") || files.any_with_extension("sln") {
        languages.push(Language::DotNet);
    }
    if files.exists("Cargo.toml") {
        languages.push(Language::Rust);
    }
    languages
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct FakeFiles {
        names: HashSet<&'static str>,
        extensions: HashSet<&'static str>,
    }

    impl ProjectFiles for FakeFiles {
        fn exists(&self, name: &str) -> bool {
            self.names.contains(name)
        }
        fn any_with_extension(&self, extension: &str) -> bool {
            self.extensions.contains(extension)
        }
    }

    fn with_names(names: &[&'static str]) -> FakeFiles {
        FakeFiles {
            names: names.iter().copied().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn maven_and_gradle_markers_mean_java() {
        for marker in ["pom.xml", "build.gradle", "build.gradle.kts"] {
            assert_eq!(
                detect_languages(&with_names(&[marker])),
                vec![Language::Java]
            );
        }
    }

    #[test]
    fn package_json_alone_means_javascript() {
        assert_eq!(
            detect_languages(&with_names(&["package.json"])),
            vec![Language::JavaScript]
        );
    }

    #[test]
    fn package_json_with_tsconfig_means_typescript_only() {
        assert_eq!(
            detect_languages(&with_names(&["package.json", "tsconfig.json"])),
            vec![Language::TypeScript]
        );
    }

    #[test]
    fn csproj_or_sln_means_dotnet() {
        for extension in ["csproj", "sln"] {
            let files = FakeFiles {
                extensions: [extension].into_iter().collect(),
                ..Default::default()
            };
            assert_eq!(detect_languages(&files), vec![Language::DotNet]);
        }
    }

    #[test]
    fn cargo_toml_means_rust() {
        assert_eq!(
            detect_languages(&with_names(&["Cargo.toml"])),
            vec![Language::Rust]
        );
    }

    #[test]
    fn a_polyglot_project_reports_every_ecosystem_in_stable_order() {
        let files = FakeFiles {
            names: ["pom.xml", "package.json", "tsconfig.json", "Cargo.toml"]
                .into_iter()
                .collect(),
            extensions: ["csproj"].into_iter().collect(),
        };
        assert_eq!(
            detect_languages(&files),
            vec![
                Language::Java,
                Language::TypeScript,
                Language::DotNet,
                Language::Rust
            ]
        );
    }

    #[test]
    fn an_empty_directory_detects_nothing() {
        assert!(detect_languages(&FakeFiles::default()).is_empty());
    }

    #[test]
    fn every_language_names_its_framework_and_runtime_command() {
        let expected = [
            (Language::Java, "Java", "Cucumber-JVM", "java"),
            (Language::JavaScript, "JavaScript", "Cucumber-JS", "node"),
            (Language::TypeScript, "TypeScript", "Cucumber-JS", "node"),
            (Language::DotNet, ".NET", "Reqnroll", "dotnet"),
            (Language::Rust, "Rust", "cucumber-rs", "cargo"),
        ];
        for (language, display, framework, command) in expected {
            assert_eq!(language.display(), display);
            assert_eq!(language.bdd_framework(), framework);
            assert_eq!(language.runtime().command(), command);
        }
        assert_eq!(Language::ALL.len(), 5);
    }

    #[test]
    fn parse_accepts_aliases_and_display_names() {
        assert_eq!(Language::parse("java"), Some(Language::Java));
        assert_eq!(Language::parse("TS"), Some(Language::TypeScript));
        assert_eq!(Language::parse("c#"), Some(Language::DotNet));
        assert_eq!(Language::parse(".NET"), Some(Language::DotNet));
        assert_eq!(Language::parse("Java"), Some(Language::Java));
        assert_eq!(Language::parse("cobol"), None);
    }
}

//! Project inspection: which supported ecosystems the project uses, and
//! whether each one's runtime is installed. Execution is gated on the
//! runtime being present; authoring and validation never are — and the
//! harness never installs anything.

use serde::Serialize;

use crate::domain::language::{Language, detect_languages};
use crate::domain::memory::ProjectStructure;
use crate::ports::{GitState, ProjectFiles, RuntimeProbe, Vcs};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LanguageReport {
    pub language: String,
    #[serde(rename = "bddFramework")]
    pub bdd_framework: String,
    pub runtime: String,
    #[serde(rename = "runtimePresent")]
    pub runtime_present: bool,
    #[serde(rename = "runtimeVersion", skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Where this project keeps its code, as the layout resolver settled
/// it. Reported so an agent writes to the paths the harness reads back
/// instead of guessing a conventional one — the mismatch that made
/// scenarios land outside the project.
///
/// A projection of [`ProjectStructure`] rather than the type itself:
/// this reply shape is frozen, and it should not shift because
/// `.spec/memory.json` gained a field.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_definitions: Option<String>,
}

impl From<&ProjectStructure> for LayoutReport {
    fn from(structure: &ProjectStructure) -> Self {
        Self {
            module_root: structure.module_root.clone(),
            production: structure.production.clone(),
            tests: structure.tests.clone(),
            features: structure.features.clone(),
            step_definitions: structure.step_definitions.clone(),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct InspectionReport {
    pub languages: Vec<LanguageReport>,
    /// Where version control stands. Part of the project's layout as
    /// much as its source roots are: it decides whether `spec
    /// greenfield` and `spec deliver` can offer a branch, and whether
    /// what they write is undoable at all.
    pub git: GitState,
    pub layout: LayoutReport,
    #[serde(rename = "nextStep")]
    pub next_step: String,
}

pub struct InspectService<P: ProjectFiles, R: RuntimeProbe, V: Vcs> {
    files: P,
    probe: R,
    vcs: V,
    /// Injected, not read: resolving the layout is filesystem work the
    /// composition roots already do, and a service that fetched it
    /// could not be tested without one.
    structure: ProjectStructure,
}

impl<P: ProjectFiles, R: RuntimeProbe, V: Vcs> InspectService<P, R, V> {
    pub fn new(files: P, probe: R, vcs: V, structure: ProjectStructure) -> Self {
        Self {
            files,
            probe,
            vcs,
            structure,
        }
    }

    pub fn inspect(&self) -> InspectionReport {
        self.inspect_with(InspectCopy::Cli)
    }

    /// Frozen MCP `nextStep` strings (`Call validate_spec...`).
    pub fn inspect_mcp(&self) -> InspectionReport {
        self.inspect_with(InspectCopy::Mcp)
    }

    fn inspect_with(&self, copy: InspectCopy) -> InspectionReport {
        let languages: Vec<LanguageReport> = detect_languages(&self.files)
            .into_iter()
            .map(|language| self.report_for(language))
            .collect();
        let git = self.vcs.state();
        let next_step = next_step(&languages, &git, copy);
        InspectionReport {
            languages,
            git,
            layout: LayoutReport::from(&self.structure),
            next_step,
        }
    }

    fn report_for(&self, language: Language) -> LanguageReport {
        let command = self.test_command(language);
        let version = self.probe.version(command);
        let present = version.is_some();
        let note = if !present {
            Some(format!(
                "runtime_missing: the {} runtime ({command}) is not installed - test \
                 execution is disabled until it is present; authoring and validation \
                 still work. The harness never installs runtimes.",
                language.display()
            ))
        } else if language == Language::Java && self.probe.version("java").is_none() {
            Some(
                "runtime_missing: a JDK (java) is not installed - Maven tests need \
                 both mvn and a JDK. The harness never installs runtimes."
                    .into(),
            )
        } else {
            None
        };
        let jdk_ok = language != Language::Java || self.probe.version("java").is_some();
        LanguageReport {
            language: language.display().to_string(),
            bdd_framework: language.bdd_framework().to_string(),
            runtime: command.to_string(),
            runtime_present: present && jdk_ok,
            runtime_version: version,
            note,
        }
    }

    fn test_command(&self, language: Language) -> &'static str {
        match language {
            Language::Java if self.files.exists("pom.xml") => "mvn",
            Language::Java => "gradle",
            other => other.runtime().command(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum InspectCopy {
    Cli,
    Mcp,
}

fn next_step(languages: &[LanguageReport], git: &GitState, copy: InspectCopy) -> String {
    if !git.repository {
        // Said first because it changes what every later step means:
        // outside a repository the harness's writes have no undo.
        return "This project is not a git repository, so nothing the harness writes \
                can be undone with git. Run git init first, or keep your own backup."
            .to_string();
    }
    if languages.is_empty() {
        let supported: Vec<String> = Language::ALL
            .iter()
            .map(|l| format!("{} ({})", l.display(), l.bdd_framework()))
            .collect();
        return format!(
            "No supported project detected. Supported ecosystems: {}.",
            supported.join(", ")
        );
    }
    if languages.iter().all(|l| l.runtime_present) {
        match copy {
            InspectCopy::Cli => "All detected runtimes are present. Run spec validate, then spec \
                 show for a pending requirement to start the loop."
                .to_string(),
            InspectCopy::Mcp => {
                "All detected runtimes are present. Call validate_spec, then get_requirement \
                 to start the loop."
                    .to_string()
            }
        }
    } else {
        "Some runtimes are missing - authoring and validation work now; install the \
         missing runtime yourself to enable test execution."
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// Most of these tests are about runtimes, not layout, so they get
    /// an unresolved one.
    fn inspect_service<V: Vcs>(
        files: FakeFiles,
        probe: FakeProbe,
        vcs: V,
    ) -> InspectService<FakeFiles, FakeProbe, V> {
        InspectService::new(files, probe, vcs, ProjectStructure::default())
    }

    #[derive(Default)]
    struct FakeFiles(HashSet<&'static str>, HashSet<&'static str>);

    impl ProjectFiles for FakeFiles {
        fn exists(&self, name: &str) -> bool {
            self.0.contains(name)
        }
        fn any_with_extension(&self, extension: &str) -> bool {
            self.1.contains(extension)
        }
    }

    #[derive(Default)]
    struct FakeProbe(HashMap<&'static str, &'static str>);

    /// A repository on a branch: the ordinary case, so the runtime
    /// assertions below are about runtimes.
    struct OnABranch;

    impl Vcs for OnABranch {
        fn state(&self) -> GitState {
            GitState {
                repository: true,
                branch: Some("main".into()),
                dirty: false,
            }
        }

        fn create_branch(&self, _: &str) -> Result<(), crate::ports::VcsError> {
            unimplemented!("inspect never creates a branch")
        }
    }

    /// No git at all.
    struct NoRepository;

    impl Vcs for NoRepository {
        fn state(&self) -> GitState {
            GitState::default()
        }

        fn create_branch(&self, _: &str) -> Result<(), crate::ports::VcsError> {
            unimplemented!("inspect never creates a branch")
        }
    }

    impl RuntimeProbe for FakeProbe {
        fn version(&self, command: &str) -> Option<String> {
            self.0.get(command).map(|v| v.to_string())
        }
    }

    #[test]
    fn a_java_project_with_a_jdk_reports_present_with_version() {
        let service = inspect_service(
            FakeFiles(["pom.xml"].into(), HashSet::new()),
            FakeProbe([("mvn", "Apache Maven 3.9.9"), ("java", "openjdk 21.0.2")].into()),
            OnABranch,
        );
        let report = service.inspect();
        assert_eq!(
            report.languages,
            vec![LanguageReport {
                language: "Java".into(),
                bdd_framework: "Cucumber-JVM".into(),
                runtime: "mvn".into(),
                runtime_present: true,
                runtime_version: Some("Apache Maven 3.9.9".into()),
                note: None,
            }]
        );
        assert!(
            report
                .next_step
                .starts_with("All detected runtimes are present.")
        );
        assert!(report.next_step.contains("spec validate"));
        assert!(
            service
                .inspect_mcp()
                .next_step
                .contains("Call validate_spec")
        );
    }

    #[test]
    fn a_java_project_with_maven_but_no_jdk_notes_the_missing_jdk() {
        let service = inspect_service(
            FakeFiles(["pom.xml"].into(), HashSet::new()),
            FakeProbe([("mvn", "Apache Maven 3.9.9")].into()),
            OnABranch,
        );
        let report = service.inspect();
        assert_eq!(report.languages.len(), 1);
        assert_eq!(report.languages[0].runtime, "mvn");
        assert!(!report.languages[0].runtime_present);
        assert_eq!(
            report.languages[0].note.as_deref(),
            Some(
                "runtime_missing: a JDK (java) is not installed - Maven tests need \
                 both mvn and a JDK. The harness never installs runtimes."
            )
        );
        assert!(report.next_step.starts_with("Some runtimes are missing"));
    }

    #[test]
    fn a_gradle_java_project_uses_the_gradle_runtime() {
        let service = inspect_service(
            FakeFiles(["build.gradle"].into(), HashSet::new()),
            FakeProbe([("gradle", "Gradle 8.14"), ("java", "21")].into()),
            OnABranch,
        );
        let report = service.inspect();
        assert_eq!(report.languages[0].runtime, "gradle");
        assert!(report.languages[0].runtime_present);
    }

    #[test]
    fn a_missing_runtime_disables_execution_but_not_authoring() {
        let service = inspect_service(
            FakeFiles(HashSet::new(), ["csproj"].into()),
            FakeProbe::default(),
            OnABranch,
        );
        let report = service.inspect();
        let dotnet = &report.languages[0];
        assert_eq!(dotnet.language, ".NET");
        assert_eq!(dotnet.bdd_framework, "Reqnroll");
        assert!(!dotnet.runtime_present);
        assert_eq!(dotnet.runtime_version, None);
        assert_eq!(
            dotnet.note.as_deref(),
            Some(
                "runtime_missing: the .NET runtime (dotnet) is not installed - test \
                 execution is disabled until it is present; authoring and validation \
                 still work. The harness never installs runtimes."
            )
        );
        assert!(report.next_step.starts_with("Some runtimes are missing"));
    }

    #[test]
    fn an_empty_directory_lists_every_supported_ecosystem() {
        let service = inspect_service(FakeFiles::default(), FakeProbe::default(), OnABranch);
        let report = service.inspect();
        assert!(report.languages.is_empty());
        assert_eq!(
            report.next_step,
            "No supported project detected. Supported ecosystems: Java (Cucumber-JVM), \
             JavaScript (Cucumber-JS), TypeScript (Cucumber-JS), .NET (Reqnroll), \
             Rust (cucumber-rs)."
        );
    }

    #[test]
    fn a_polyglot_project_reports_each_ecosystem_with_its_own_runtime_state() {
        let service = inspect_service(
            FakeFiles(
                ["package.json", "tsconfig.json", "Cargo.toml"].into(),
                HashSet::new(),
            ),
            FakeProbe([("cargo", "cargo 1.97.0")].into()),
            OnABranch,
        );
        let report = service.inspect();
        assert_eq!(report.languages.len(), 2);
        assert_eq!(report.languages[0].language, "TypeScript");
        assert!(!report.languages[0].runtime_present);
        assert_eq!(report.languages[1].language, "Rust");
        assert!(report.languages[1].runtime_present);
        assert!(report.next_step.starts_with("Some runtimes are missing"));
    }

    #[test]
    fn the_report_serializes_with_camel_case_field_names() {
        let service = inspect_service(
            FakeFiles(["package.json"].into(), HashSet::new()),
            FakeProbe([("node", "v22.1.0")].into()),
            OnABranch,
        );
        let json = serde_json::to_string(&service.inspect()).unwrap();
        assert!(json.contains("bddFramework"));
        assert!(json.contains("runtimePresent"));
        assert!(json.contains("runtimeVersion"));
        assert!(json.contains("nextStep"));
    }

    /// The report answers "where do I write?" so an agent does not have
    /// to guess a path for feature_create.
    #[test]
    fn the_report_names_the_roots_the_layout_resolved() {
        let service = InspectService::new(
            FakeFiles(["Cargo.toml"].into(), HashSet::new()),
            FakeProbe([("cargo", "cargo 1.97.0")].into()),
            OnABranch,
            ProjectStructure {
                production: Some("src".into()),
                tests: Some("tests".into()),
                features: Some("tests/features".into()),
                step_definitions: Some("tests/steps/generated.rs".into()),
                ..ProjectStructure::default()
            },
        );
        let report = service.inspect();
        assert_eq!(report.layout.features.as_deref(), Some("tests/features"));
        assert_eq!(report.layout.production.as_deref(), Some("src"));
        assert_eq!(
            report.layout.step_definitions.as_deref(),
            Some("tests/steps/generated.rs")
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("stepDefinitions"), "{json}");
        assert!(json.contains(r#""features":"tests/features""#), "{json}");
    }

    /// An unresolved layout reports nothing rather than inventing
    /// defaults: a path in this reply is one the harness will actually
    /// read back.
    #[test]
    fn an_unresolved_layout_reports_no_roots_at_all() {
        let service = inspect_service(FakeFiles::default(), FakeProbe::default(), OnABranch);
        let report = service.inspect();
        assert_eq!(report.layout, LayoutReport::default());
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains(r#""layout":{}"#), "{json}");
    }

    /// Outside a repository the harness's writes have no undo, and
    /// that outranks whichever runtime happens to be installed.
    #[test]
    fn a_project_outside_a_repository_is_told_so_before_anything_else() {
        let service = inspect_service(
            FakeFiles(["pom.xml"].into(), HashSet::new()),
            FakeProbe([("mvn", "3.9.9"), ("java", "21")].into()),
            NoRepository,
        );
        let report = service.inspect();
        assert!(!report.git.repository);
        assert_eq!(report.git.branch, None);
        assert!(
            report
                .next_step
                .starts_with("This project is not a git repository"),
            "{}",
            report.next_step
        );
        assert!(report.next_step.contains("git init"));
    }

    #[test]
    fn a_repository_is_reported_with_the_branch_it_is_on() {
        let service = inspect_service(
            FakeFiles(["pom.xml"].into(), HashSet::new()),
            FakeProbe([("mvn", "3.9.9"), ("java", "21")].into()),
            OnABranch,
        );
        let report = service.inspect();
        assert!(report.git.repository);
        assert_eq!(report.git.branch.as_deref(), Some("main"));
        assert!(report.next_step.contains("spec validate"));
    }
}

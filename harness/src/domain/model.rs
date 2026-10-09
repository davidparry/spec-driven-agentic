//! The requirement spec model — the same JSON shape the workshop's
//! `requirements/requirements.json` uses, so both servers read one spec.
//! The root document is a catalog: it holds requirements of its own and
//! may include child spec files, which may include further files — the
//! whole tree resolves into one [`SpecCatalog`].

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The file name of the root spec document. Every path in a
/// [`SpecCatalog`] is relative to the directory this file lives in.
pub const ROOT_SPEC_FILE: &str = "requirements.json";

/// One requirement of the spec: the unit the whole workflow revolves
/// around (draft -> validate -> refine -> scenario -> tests -> code).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub story: String,
    #[serde(rename = "acceptanceCriteria", default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(
        rename = "featureFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub feature_file: Option<String>,
    /// Where this requirement's production code lives, as the developer
    /// declared it or as a delivery recorded it. The mirror image of
    /// `feature_file`, plural because one requirement's code is not
    /// always one file - a port and its adapter, a type and its
    /// registration.
    ///
    /// Empty is the honest default: nothing has been declared and
    /// nothing has been written yet, and the conventional path is
    /// inferred instead. It stays out of the JSON when empty so an
    /// untouched spec is byte-for-byte what it was.
    #[serde(
        rename = "productionFiles",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub production_files: Vec<String>,
}

impl Requirement {
    pub fn is_pending(&self) -> bool {
        self.status.eq_ignore_ascii_case("pending")
    }
}

/// One spec document: the source of truth the loop starts from. The
/// root document is always `requirements/requirements.json`; to break a
/// large spec up, it may include child documents of the same shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Child spec files merged into this one, each path relative to the
    /// directory of the file declaring it. Includes nest: a child may
    /// include further files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
}

/// One spec document of the catalog and where it lives, relative to the
/// root document's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecFile {
    pub path: String,
    pub spec: Spec,
}

/// The whole spec tree: the root catalog first, then every included
/// file depth-first in listed order. Each document remembers its path
/// so mutations write back to the file a requirement actually lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecCatalog {
    files: Vec<SpecFile>,
}

impl SpecCatalog {
    /// A catalog holding one root document — the shape every spec had
    /// before includes existed, and what in-memory fakes provide.
    pub fn single_root(spec: Spec) -> Self {
        Self {
            files: vec![SpecFile {
                path: ROOT_SPEC_FILE.into(),
                spec,
            }],
        }
    }

    pub fn root(&self) -> &SpecFile {
        &self.files[0]
    }

    pub fn files(&self) -> &[SpecFile] {
        &self.files
    }

    /// The catalog flattened into one logical document: the root's
    /// project and description, and every file's requirements in
    /// catalog order (a file's own requirements before its includes').
    pub fn merged(&self) -> Spec {
        let root = &self.root().spec;
        Spec {
            project: root.project.clone(),
            description: root.description.clone(),
            includes: Vec::new(),
            requirements: self
                .files
                .iter()
                .flat_map(|file| file.spec.requirements.iter().cloned())
                .collect(),
        }
    }

    /// The path of the file declaring the requirement with `id`.
    pub fn source_of(&self, id: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.spec.requirements.iter().any(|r| r.id == id))
            .map(|file| file.path.as_str())
    }

    pub fn file(&self, path: &str) -> Option<&SpecFile> {
        self.files.iter().find(|file| file.path == path)
    }

    /// The file declaring `path` as one of its includes, or `None` for
    /// the root — the one document nothing includes.
    pub fn parent_of(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| {
                file.spec.includes.iter().any(|include| {
                    resolve_include(parent_dir(&file.path), include).as_deref() == Some(path)
                })
            })
            .map(|file| file.path.as_str())
    }

    pub fn file_mut(&mut self, path: &str) -> Option<&mut Spec> {
        self.files
            .iter_mut()
            .find(|file| file.path == path)
            .map(|file| &mut file.spec)
    }
}

/// One spec document as a file holds it: pretty-printed, and ending in
/// a newline.
///
/// Every writer of a spec file renders through here. The newline is the
/// point: a document that stops at `}` leaves `\ No newline at end of
/// file` in the student's diff on every reword, and reads differently
/// from the Gherkin and Java the same run generates, which do end in one.
pub fn render(spec: &Spec) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(spec).map(|json| json + "\n")
}

/// Reads one spec file's raw JSON by catalog-relative path, returning
/// the content plus the label error messages should call the file, or
/// an already formatted `spec: ...` error.
pub type SpecRead<'a> = dyn FnMut(&str) -> Result<(String, String), String> + 'a;

/// Walk the include tree from the root document, depth-first in listed
/// order. Paths handed to `read` are relative to the root document's
/// directory; includes resolve relative to the file declaring them and
/// must stay inside the root document's directory.
pub fn resolve_catalog(root: &str, read: &mut SpecRead<'_>) -> Result<SpecCatalog, String> {
    let mut files = Vec::new();
    let mut visited = HashSet::new();
    walk(root, read, &mut visited, &mut files)?;
    Ok(SpecCatalog { files })
}

fn walk(
    path: &str,
    read: &mut SpecRead<'_>,
    visited: &mut HashSet<String>,
    files: &mut Vec<SpecFile>,
) -> Result<(), String> {
    if !visited.insert(path.to_string()) {
        return Err(format!(
            "spec: {path} is included more than once - include every spec file exactly once"
        ));
    }
    let (content, label) = read(path)?;
    let spec: Spec = serde_json::from_str(&content)
        .map_err(|e| format!("spec: {label} is not readable JSON - {e}"))?;
    let includes = spec.includes.clone();
    files.push(SpecFile {
        path: path.to_string(),
        spec,
    });
    let dir = match path.rfind('/') {
        Some(cut) => &path[..cut],
        None => "",
    };
    for include in includes {
        let child = resolve_include(dir, &include).ok_or_else(|| {
            format!("spec: include \"{include}\" in {path} escapes the spec directory")
        })?;
        walk(&child, read, visited, files)?;
    }
    Ok(())
}

/// The directory part of a catalog-relative path, empty for the root.
pub(crate) fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(cut) => &path[..cut],
        None => "",
    }
}

/// Resolve `include` against the including file's directory, lexically:
/// `.` stays put, `..` pops. `None` when the path escapes above the
/// root document's directory.
pub(crate) fn resolve_include(dir: &str, include: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for part in include.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// The outcome of a single test-suite execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRunSummary {
    pub tests: u32,
    pub failures: u32,
    pub errors: u32,
    pub skipped: u32,
    #[serde(rename = "failureDetails")]
    pub failure_details: Vec<String>,
}

/// Whether a run's counts say the build failed before the suite could
/// start. Nothing executed, so they report on the compiler rather than
/// on the tests — a different fact from RED, where tests ran and failed,
/// even though both block a refactor.
///
/// Free so the reply DTOs carrying the same counts can answer it without
/// restating the rule.
pub fn build_broken(tests: u32, errors: u32) -> bool {
    tests == 0 && errors > 0
}

impl TestRunSummary {
    pub fn passed(&self) -> bool {
        self.tests > 0 && self.failures == 0 && self.errors == 0
    }

    /// A successful build that produced no reports — not GREEN (no tests
    /// ran) and not RED (nothing failed). Callers must not flip the
    /// phase to RED for this outcome.
    pub fn no_tests(&self) -> bool {
        self.tests == 0 && self.failures == 0 && self.errors == 0
    }

    /// See [`build_broken`].
    pub fn build_broken(&self) -> bool {
        build_broken(self.tests, self.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_status_is_case_insensitive() {
        let mut r = requirement("REQ-001");
        r.status = "Pending".into();
        assert!(r.is_pending());
        r.status = "implemented".into();
        assert!(!r.is_pending());
    }

    #[test]
    fn a_run_with_no_tests_has_not_passed() {
        assert!(!TestRunSummary::default().passed());
        assert!(TestRunSummary::default().no_tests());
    }

    /// A build error and a failing test are both red bars, and only one
    /// of them is evidence about the code under test.
    #[test]
    fn a_build_that_never_ran_the_suite_is_told_apart_from_a_failing_one() {
        let broken = TestRunSummary {
            tests: 0,
            errors: 1,
            ..Default::default()
        };
        assert!(broken.build_broken());
        assert!(!broken.no_tests(), "an error is not a quiet build");

        let red = TestRunSummary {
            tests: 12,
            failures: 6,
            ..Default::default()
        };
        assert!(!red.build_broken());

        // A build that compiled and reported nothing is the third case
        // `no_tests` already names, and is not this one.
        assert!(!TestRunSummary::default().build_broken());
    }

    #[test]
    fn a_run_passes_only_when_tests_ran_and_nothing_failed() {
        let run = TestRunSummary {
            tests: 5,
            ..Default::default()
        };
        assert!(run.passed());
        let red = TestRunSummary {
            tests: 5,
            failures: 1,
            ..Default::default()
        };
        assert!(!red.passed());
        let error = TestRunSummary {
            tests: 5,
            errors: 1,
            ..Default::default()
        };
        assert!(!error.passed());
    }

    fn catalog_of(files: &[(&str, &str)]) -> Result<SpecCatalog, String> {
        let files: Vec<(String, String)> = files
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect();
        resolve_catalog(ROOT_SPEC_FILE, &mut |path| {
            files
                .iter()
                .find(|(p, _)| p == path)
                .map(|(p, c)| (c.clone(), p.clone()))
                .ok_or_else(|| format!("spec: {path} is not readable - the file does not exist"))
        })
    }

    #[test]
    fn a_root_without_includes_resolves_to_a_single_file_catalog() {
        let catalog = catalog_of(&[(
            "requirements.json",
            r#"{"project":"Kata","requirements":[{"id":"REQ-001"}]}"#,
        )])
        .unwrap();
        assert_eq!(catalog.files().len(), 1);
        assert_eq!(catalog.root().path, "requirements.json");
        assert_eq!(catalog.merged().project, "Kata");
        assert_eq!(catalog.source_of("REQ-001"), Some("requirements.json"));
    }

    #[test]
    fn includes_resolve_depth_first_with_own_requirements_before_included_ones() {
        let catalog = catalog_of(&[
            (
                "requirements.json",
                r#"{"project":"Kata","includes":["core/a.json","b.json"],
                    "requirements":[{"id":"REQ-001"}]}"#,
            ),
            (
                "core/a.json",
                r#"{"includes":["deep.json"],"requirements":[{"id":"REQ-002"}]}"#,
            ),
            ("core/deep.json", r#"{"requirements":[{"id":"REQ-003"}]}"#),
            ("b.json", r#"{"requirements":[{"id":"REQ-004"}]}"#),
        ])
        .unwrap();
        let paths: Vec<&str> = catalog.files().iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "requirements.json",
                "core/a.json",
                "core/deep.json",
                "b.json"
            ]
        );
        let merged = catalog.merged();
        let ids: Vec<&str> = merged.requirements.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["REQ-001", "REQ-002", "REQ-003", "REQ-004"]);
        assert_eq!(catalog.source_of("REQ-003"), Some("core/deep.json"));
        assert_eq!(catalog.source_of("REQ-009"), None);
    }

    #[test]
    fn nested_includes_resolve_relative_to_the_file_declaring_them() {
        let catalog = catalog_of(&[
            (
                "requirements.json",
                r#"{"includes":["core/a.json"],"requirements":[]}"#,
            ),
            (
                "core/a.json",
                r#"{"includes":["../b.json"],"requirements":[]}"#,
            ),
            ("b.json", r#"{"requirements":[{"id":"REQ-001"}]}"#),
        ])
        .unwrap();
        assert_eq!(catalog.source_of("REQ-001"), Some("b.json"));
    }

    #[test]
    fn an_include_cycle_is_an_error() {
        let error = catalog_of(&[
            (
                "requirements.json",
                r#"{"includes":["a.json"],"requirements":[]}"#,
            ),
            (
                "a.json",
                r#"{"includes":["requirements.json"],"requirements":[]}"#,
            ),
        ])
        .unwrap_err();
        assert_eq!(
            error,
            "spec: requirements.json is included more than once - include every \
             spec file exactly once"
        );
    }

    #[test]
    fn a_file_included_twice_is_an_error() {
        let error = catalog_of(&[
            (
                "requirements.json",
                r#"{"includes":["a.json","a.json"],"requirements":[]}"#,
            ),
            ("a.json", r#"{"requirements":[]}"#),
        ])
        .unwrap_err();
        assert!(error.starts_with("spec: a.json is included more than once"));
    }

    #[test]
    fn a_missing_include_propagates_the_reader_error() {
        let error = catalog_of(&[(
            "requirements.json",
            r#"{"includes":["missing.json"],"requirements":[]}"#,
        )])
        .unwrap_err();
        assert_eq!(
            error,
            "spec: missing.json is not readable - the file does not exist"
        );
    }

    #[test]
    fn a_broken_included_file_reports_not_readable_json() {
        let error = catalog_of(&[
            (
                "requirements.json",
                r#"{"includes":["a.json"],"requirements":[]}"#,
            ),
            ("a.json", "{ nope"),
        ])
        .unwrap_err();
        assert!(error.starts_with("spec: a.json is not readable JSON -"));
    }

    #[test]
    fn an_include_escaping_the_spec_directory_is_an_error() {
        let error = catalog_of(&[(
            "requirements.json",
            r#"{"includes":["../outside.json"],"requirements":[]}"#,
        )])
        .unwrap_err();
        assert_eq!(
            error,
            "spec: include \"../outside.json\" in requirements.json escapes the \
             spec directory"
        );
    }

    #[test]
    fn a_catalog_document_updates_through_file_mut() {
        let mut catalog = SpecCatalog::single_root(Spec {
            project: "Kata".into(),
            ..Spec::default()
        });
        catalog
            .file_mut(ROOT_SPEC_FILE)
            .unwrap()
            .requirements
            .push(requirement("REQ-001"));
        assert_eq!(
            catalog
                .file(ROOT_SPEC_FILE)
                .unwrap()
                .spec
                .requirements
                .len(),
            1
        );
        assert!(catalog.file_mut("missing.json").is_none());
        assert!(catalog.file("missing.json").is_none());
    }

    #[test]
    fn includes_round_trip_and_stay_out_of_the_json_when_empty() {
        let spec: Spec =
            serde_json::from_str(r#"{"project":"Kata","includes":["a.json"]}"#).unwrap();
        assert_eq!(spec.includes, vec!["a.json"]);
        assert!(serde_json::to_string(&spec).unwrap().contains("includes"));
        let bare: Spec = serde_json::from_str(r#"{"project":"Kata"}"#).unwrap();
        assert!(!serde_json::to_string(&bare).unwrap().contains("includes"));
    }

    #[test]
    fn spec_round_trips_through_the_workshop_json_field_names() {
        let json = r#"{
            "project": "Kata",
            "requirements": [{
                "id": "REQ-001",
                "title": "T",
                "status": "pending",
                "story": "As a user, I want X so that Y.",
                "acceptanceCriteria": ["Given a, when b, then c"],
                "featureFile": "features/x.feature"
            }]
        }"#;
        let spec: Spec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.requirements[0].acceptance_criteria.len(), 1);
        assert_eq!(
            spec.requirements[0].feature_file.as_deref(),
            Some("features/x.feature")
        );
        let out = serde_json::to_string(&spec).unwrap();
        assert!(out.contains("acceptanceCriteria"));
        assert!(out.contains("featureFile"));

        // What a writer puts on disk: the same field names, pretty
        // printed, ending in a newline. Without the 0a every reword
        // leaves "\ No newline at end of file" in the student's diff,
        // and the spec reads differently from the Gherkin and Java the
        // same run generates.
        let written = render(&spec).unwrap();
        assert_eq!(written.as_bytes().last(), Some(&b'\n'), "{written:?}");
        assert!(!written.ends_with("\n\n"), "exactly one: {written:?}");
        assert!(written.contains("\n  \"requirements\": ["), "{written}");
        assert_eq!(
            serde_json::from_str::<Spec>(&written).unwrap(),
            spec,
            "the written bytes still parse back to the same spec"
        );
    }

    /// The field is new, and every spec on disk predates it. Reading one
    /// has to be unremarkable, and writing it back has to leave the file
    /// as it was - a spec that gains a `"productionFiles": []` line it
    /// never asked for is a diff in every student's working tree.
    #[test]
    fn production_files_are_absent_from_a_spec_that_declares_none() {
        let json = r#"{"project": "Kata", "requirements": [{"id": "REQ-001"}]}"#;
        let spec: Spec = serde_json::from_str(json).unwrap();
        assert!(spec.requirements[0].production_files.is_empty());
        assert!(
            !render(&spec).unwrap().contains("productionFiles"),
            "an untouched spec gains nothing"
        );
    }

    /// Plural from the start: one requirement's code is not always one
    /// file, and a list that round-trips is the whole point of storing
    /// it rather than inferring it every time.
    #[test]
    fn declared_production_files_round_trip_under_the_workshop_field_name() {
        let json = r#"{
            "project": "Kata",
            "requirements": [{
                "id": "REQ-001",
                "productionFiles": ["src/main/java/Calc.java", "src/main/java/Parser.java"]
            }]
        }"#;
        let spec: Spec = serde_json::from_str(json).unwrap();
        assert_eq!(
            spec.requirements[0].production_files,
            ["src/main/java/Calc.java", "src/main/java/Parser.java"]
        );
        let written = render(&spec).unwrap();
        assert!(written.contains("productionFiles"), "{written}");
        assert_eq!(serde_json::from_str::<Spec>(&written).unwrap(), spec);
    }

    pub fn requirement(id: &str) -> Requirement {
        Requirement {
            id: id.into(),
            title: "A title".into(),
            status: "pending".into(),
            story: "As a user, I want things so that value.".into(),
            acceptance_criteria: vec!["Given a, when b, then 3".into()],
            feature_file: Some("features/x.feature".into()),
            ..Default::default()
        }
    }
}

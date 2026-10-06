//! Workspace layout shared by every composition root (`main.rs`,
//! [`crate::mcp`], and [`crate::greenfield`]), so the spec location and
//! the kata layout are defined exactly once.

use std::fs;
use std::path::{Path, PathBuf};

use crate::application::spec_service::ProjectLayout;
use crate::domain::SPEC_DIR;
use crate::domain::language::{Language, detect_languages};
use crate::domain::memory::ProjectStructure;
use crate::domain::steps::source_extension;

/// Where the requirements spec lives, relative to the project root.
pub const SPEC_PATH: &str = "requirements/requirements.json";

/// The nearest enclosing spec project, walking up from `start`.
///
/// Nearest wins, so running inside `harness/src` works on the harness's
/// own spec rather than the repository's. A directory is a project when
/// it holds the spec catalog, or failing that the [`SPEC_DIR`] home —
/// the catalog is the stronger signal, but a project configured and not
/// yet drafted into is still a project.
///
/// `None` when `start` is itself the match, so the common case keeps the
/// relative `.` every path in a reply is already built from.
pub fn discover_project_root(start: &Path) -> Option<PathBuf> {
    let is_project = |dir: &Path| dir.join(SPEC_PATH).is_file() || dir.join(SPEC_DIR).is_dir();
    let found = start.ancestors().find(|dir| is_project(dir))?;
    (found != start).then(|| found.to_path_buf())
}

/// Where the command is being run, as a `/`-separated path relative to
/// the project root — empty at the root itself.
///
/// `None` when the working directory is outside the project, or when
/// either path will not canonicalize, so a caller that cannot tell where
/// it is falls back rather than guessing.
pub fn working_dir_in(root: &Path) -> Option<String> {
    let root = fs::canonicalize(root).ok()?;
    let cwd = fs::canonicalize(std::env::current_dir().ok()?).ok()?;
    let relative = cwd.strip_prefix(&root).ok()?;
    Some(
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Directories that never contain authored sources.
const SKIPPED_DIRS: [&str; 7] = [
    "target",
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".git",
    SPEC_DIR,
];

/// The workshop kata layout the frozen `get_requirement` tool reports,
/// byte-identical to the Java server.
pub fn workshop_layout() -> ProjectLayout {
    ProjectLayout {
        step_definitions:
            "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorSteps.java".into(),
        test_location: "kata/src/test/java/com/davidparry/workshop/kata/StringCalculatorTest.java"
            .into(),
        production_location:
            "kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java".into(),
    }
}

/// The project's resolved layout: the recorded one when
/// `.spec/memory.json` holds it, else a fresh scan that is then cached.
/// Mirrors [`primary_language`]'s precedence, so the language and the
/// layout are answered the same way and `spec inspect` refreshes both.
pub fn project_layout(root: &Path) -> ProjectStructure {
    use crate::adapters::fs_memory::{FsMemoryStore, FsProjectInventory};
    use crate::adapters::fs_project::FsProjectFiles;
    use crate::application::memory_service::MemoryService;

    let memory = MemoryService::new(
        FsMemoryStore::new(root.to_path_buf()),
        FsProjectInventory::new(root.to_path_buf()),
        FsProjectFiles::new(root.to_path_buf()),
        crate::adapters::git_cli::GitCli::new(root.to_path_buf()),
    );
    if let Ok(stored) = memory.load()
        && stored.structure.is_resolved()
    {
        return stored.structure;
    }
    memory
        .scan(None)
        .map(|scan| scan.memory.structure)
        .unwrap_or_default()
}

/// Detect the project's source layout for `spec show`: the three concrete
/// files, looked up inside the module the resolved layout names. When the
/// scan finds no usable set the workshop kata paths stand in. MCP
/// `get_requirement` keeps [`workshop_layout`] regardless.
pub fn detect_project_layout(root: &Path) -> ProjectLayout {
    let workshop = workshop_layout();
    if root.join(&workshop.production_location).is_file() {
        return workshop;
    }
    scan_layout(root, &project_layout(root)).unwrap_or(workshop)
}

/// The scan is restricted to the module the build compiles: a source
/// file in a sibling module (or an orphan outside every module) is not
/// what `spec show` should name. It is also restricted to the project's
/// own language — scanning for `.java` under a Rust root finds nothing
/// and silently hands back the kata's Java paths.
fn scan_layout(root: &Path, structure: &ProjectStructure) -> Option<ProjectLayout> {
    // An extracted kata carries no manifest, so an undetectable language
    // still means Java — the shape every workshop starts from.
    let language = primary_language(root).unwrap_or(Language::Java);
    let module = match &structure.module_root {
        Some(relative) => root.join(relative),
        None => root.to_path_buf(),
    };
    let mut sources = Vec::new();
    collect_files(&module, root, source_extension(language), &mut sources);
    // Directory order is whatever the filesystem hands back, so sort
    // before choosing: the same project must always show the same paths.
    sources.sort();
    let step_definitions = structure
        .step_definitions
        .clone()
        .filter(|path| sources.contains(path))
        .or_else(|| best(&sources, |path| steps_rank(language, path)));
    let test_location = best(&sources, |path| test_rank(language, path));
    let production_location = best(&sources, |path| match &structure.production {
        Some(root) => production_rank(language, path)
            .filter(|_| path.starts_with(&format!("{root}/")))
            .or_else(|| path.starts_with(&format!("{root}/")).then_some(LAST)),
        None => production_rank(language, path),
    });
    match (step_definitions, test_location, production_location) {
        (Some(step_definitions), Some(test_location), Some(production_location)) => {
            Some(ProjectLayout {
                step_definitions,
                test_location,
                production_location,
            })
        }
        _ => None,
    }
}

/// Rank beyond which a candidate is only a last resort.
const LAST: u8 = u8::MAX;

/// The lowest-ranked path, ties broken by the sorted order of `paths`.
fn best(paths: &[String], rank: impl Fn(&str) -> Option<u8>) -> Option<String> {
    paths
        .iter()
        .filter_map(|path| rank(path).map(|rank| (rank, path)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, path)| path.clone())
}

/// The file that wires Gherkin steps to code, by each language's own
/// naming convention. The cucumber *runner* is not a steps file.
fn steps_rank(language: Language, path: &str) -> Option<u8> {
    let name = file_name(path);
    if name.contains("RunCucumber") {
        return None;
    }
    match language {
        Language::Java | Language::DotNet => name.contains("Steps").then_some(0),
        Language::JavaScript | Language::TypeScript => {
            (name.contains(".steps.") || path.contains("step_definitions/")).then_some(0)
        }
        Language::Rust => (path.contains("tests/") && name.contains("steps")).then_some(0),
    }
}

/// Where a generated test belongs. Rust unit tests live beside the code
/// in `#[cfg(test)] mod tests`, so the file to name is the integration
/// test the generator writes — `tests/<requirement>_test.rs` — falling
/// back to whatever else the tests directory already holds.
fn test_rank(language: Language, path: &str) -> Option<u8> {
    let name = file_name(path);
    if name.contains("RunCucumber") {
        return None;
    }
    match language {
        Language::Java | Language::DotNet => name
            .ends_with("Test.java")
            .then_some(0)
            .or_else(|| name.contains("Test").then_some(1)),
        Language::JavaScript | Language::TypeScript => {
            (name.contains(".test.") || name.contains(".spec.")).then_some(0)
        }
        Language::Rust => {
            if !path.contains("tests/") {
                return None;
            }
            name.ends_with("_test.rs").then_some(0).or_else(|| {
                (steps_rank(language, path).is_none() && name != "cucumber.rs").then_some(1)
            })
        }
    }
}

/// Production code is whatever the language builds from. The crate or
/// module root is preferred, since that is the file a reader opens first.
fn production_rank(language: Language, path: &str) -> Option<u8> {
    let name = file_name(path);
    match language {
        Language::Java => path.contains("src/main/").then_some(0),
        Language::DotNet => (!path.contains("Test")).then_some(0),
        Language::JavaScript | Language::TypeScript => {
            (path.contains("src/") && test_rank(language, path).is_none()).then_some(0)
        }
        Language::Rust => {
            if !path.contains("src/") {
                return None;
            }
            match name {
                "lib.rs" => Some(0),
                "main.rs" => Some(1),
                _ => Some(2),
            }
        }
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn collect_files(dir: &Path, root: &Path, extension: &str, into: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let suffix = format!(".{extension}");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                collect_files(&path, root, extension, into);
            }
        } else if name.ends_with(&suffix) {
            into.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

/// Feature discovery is restricted to the features directory the layout
/// resolved, so `spec feature list` does not pick up Cucumber features
/// belonging to the harness or a sibling module. A project whose features
/// directory does not exist yet still walks the whole tree.
pub fn feature_search_root(root: &Path) -> PathBuf {
    feature_root_in(root, &project_layout(root))
}

fn feature_root_in(root: &Path, structure: &ProjectStructure) -> PathBuf {
    structure
        .features
        .as_deref()
        .map(|features| root.join(features))
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| root.to_path_buf())
}

/// Detect the project's primary language. Memory (a greenfield choice)
/// wins over marker detection so a polyglot tree keeps the chosen stack.
/// Failure names `spec inspect` so both composition roots can surface it.
pub fn primary_language(root: &Path) -> Result<Language, String> {
    use crate::adapters::fs_memory::{FsMemoryStore, FsProjectInventory};
    use crate::adapters::fs_project::FsProjectFiles;
    use crate::application::memory_service::MemoryService;

    let files = FsProjectFiles::new(root.to_path_buf());
    let memory = MemoryService::new(
        FsMemoryStore::new(root.to_path_buf()),
        FsProjectInventory::new(root.to_path_buf()),
        FsProjectFiles::new(root.to_path_buf()),
        crate::adapters::git_cli::GitCli::new(root.to_path_buf()),
    );
    if let Ok(memory) = memory.load()
        && let Some(language) = Language::parse(&memory.language)
    {
        return Ok(language);
    }
    detect_languages(&files).first().copied().ok_or_else(|| {
        "No supported project detected (pom.xml, build.gradle, package.json, \
         *.csproj, Cargo.toml). Run spec inspect."
            .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workshop_layout_names_the_kata_files() {
        let layout = workshop_layout();
        assert!(
            layout
                .step_definitions
                .ends_with("StringCalculatorSteps.java")
        );
        assert!(layout.test_location.ends_with("StringCalculatorTest.java"));
        assert!(
            layout
                .production_location
                .ends_with("StringCalculator.java")
        );
    }

    #[test]
    fn detect_prefers_workshop_paths_when_the_kata_files_exist() {
        let dir = tempfile::tempdir().unwrap();
        let production = dir
            .path()
            .join("kata/src/main/java/com/davidparry/workshop/kata/StringCalculator.java");
        fs::create_dir_all(production.parent().unwrap()).unwrap();
        fs::write(&production, "class StringCalculator {}").unwrap();
        let layout = detect_project_layout(dir.path());
        assert_eq!(
            layout.production_location,
            workshop_layout().production_location
        );
    }

    #[test]
    fn detect_scans_an_extracted_kata_without_the_kata_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |rel: &str| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "class X {}").unwrap();
        };
        write("src/main/java/com/example/StringCalculator.java");
        write("src/test/java/com/example/StringCalculatorTest.java");
        write("src/test/java/com/example/StringCalculatorSteps.java");
        let layout = detect_project_layout(root);
        assert_eq!(
            layout.production_location,
            "src/main/java/com/example/StringCalculator.java"
        );
        assert_eq!(
            layout.test_location,
            "src/test/java/com/example/StringCalculatorTest.java"
        );
        assert_eq!(
            layout.step_definitions,
            "src/test/java/com/example/StringCalculatorSteps.java"
        );
    }

    #[test]
    fn a_rust_crate_is_named_by_its_own_paths_not_the_kata() {
        // The bug this closes: the scan only ever looked for `.java`, so
        // every Rust project fell through to the workshop kata and
        // `spec show` sent the reader to a file that was not there.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |rel: &str, body: &str| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
        };
        write("Cargo.toml", "[package]\nname = \"demo\"\n");
        write("src/lib.rs", "pub fn add() {}");
        write("src/adapters/mod.rs", "");
        write("tests/demo_test.rs", "#[test] fn t() {}");
        write("tests/steps/generated.rs", "");
        let layout = detect_project_layout(root);
        assert_eq!(layout.production_location, "src/lib.rs");
        assert_eq!(layout.test_location, "tests/demo_test.rs");
        assert_eq!(layout.step_definitions, "tests/steps/generated.rs");
    }

    #[test]
    fn a_partial_java_tree_is_not_a_layout() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Foo.java"), "class Foo {}").unwrap();
        assert!(scan_layout(dir.path(), &ProjectStructure::default()).is_none());
    }

    #[test]
    fn detect_falls_back_to_the_workshop_layout_when_scan_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        fs::write(&file, "hello").unwrap();
        let layout = detect_project_layout(&file);
        assert_eq!(
            layout.production_location,
            workshop_layout().production_location
        );
    }

    #[test]
    fn primary_language_prefers_memory_then_markers() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        assert_eq!(primary_language(dir.path()).unwrap(), Language::Java);
        let memory = crate::adapters::spec_home::spec_file(dir.path(), crate::domain::MEMORY_FILE);
        fs::create_dir_all(memory.parent().unwrap()).unwrap();
        fs::write(
            &memory,
            r#"{"version":1,"language":"Rust","refreshedAt":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(primary_language(dir.path()).unwrap(), Language::Rust);
        let empty = tempfile::tempdir().unwrap();
        let error = primary_language(empty.path()).unwrap_err();
        assert!(error.contains("spec inspect"), "{error}");
    }

    /// The point of walking up: a command run inside the source tree
    /// works on the project that encloses it, not on the directory it
    /// happened to be typed in.
    #[test]
    fn the_nearest_enclosing_project_is_found_from_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("requirements")).unwrap();
        fs::write(root.join(SPEC_PATH), "{}").unwrap();
        let deep = root.join("src").join("domain");
        fs::create_dir_all(&deep).unwrap();

        assert_eq!(discover_project_root(&deep), Some(root.to_path_buf()));
        // Already at the root: nothing to report, so the caller keeps
        // the relative "." every reply path is built from.
        assert_eq!(discover_project_root(root), None);
    }

    /// A project configured but not yet drafted into is still a project.
    #[test]
    fn a_spec_home_marks_a_project_when_no_catalog_exists_yet() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(SPEC_DIR)).unwrap();
        let deep = root.join("src");
        fs::create_dir_all(&deep).unwrap();

        assert_eq!(discover_project_root(&deep), Some(root.to_path_buf()));
    }

    /// Nearest wins: running inside `harness/` works on the harness's
    /// own spec rather than the repository's.
    #[test]
    fn the_inner_project_wins_over_the_one_enclosing_it() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path();
        fs::create_dir_all(outer.join("requirements")).unwrap();
        fs::write(outer.join(SPEC_PATH), "{}").unwrap();
        let inner = outer.join("harness");
        fs::create_dir_all(inner.join("requirements")).unwrap();
        fs::write(inner.join(SPEC_PATH), "{}").unwrap();
        let deep = inner.join("src");
        fs::create_dir_all(&deep).unwrap();

        assert_eq!(discover_project_root(&deep), Some(inner));
    }

    #[test]
    fn a_directory_inside_no_project_discovers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(discover_project_root(dir.path()), None);
    }
}

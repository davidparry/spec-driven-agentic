//! Workspace layout shared by every composition root (`main.rs`,
//! [`crate::mcp`], and [`crate::greenfield`]), so the spec location and
//! the kata layout are defined exactly once.

use std::fs;
use std::path::{Path, PathBuf};

use crate::application::spec_service::ProjectLayout;
use crate::domain::SPEC_DIR;
use crate::domain::language::{Language, detect_languages};
use crate::domain::memory::ProjectStructure;

/// Where the requirements spec lives, relative to the project root.
pub const SPEC_PATH: &str = "requirements/requirements.json";

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

/// The scan is restricted to the module the build compiles: a Java file
/// in a sibling module (or an orphan outside every module) is not what
/// `spec show` should name.
fn scan_layout(root: &Path, structure: &ProjectStructure) -> Option<ProjectLayout> {
    let module = match &structure.module_root {
        Some(relative) => root.join(relative),
        None => root.to_path_buf(),
    };
    let mut java = Vec::new();
    collect_files(&module, root, "java", &mut java);
    let step_definitions = structure
        .step_definitions
        .clone()
        .filter(|path| java.contains(path))
        .or_else(|| {
            java.iter()
                .find(|path| path.rsplit('/').next().unwrap_or("").contains("Steps"))
                .cloned()
        });
    let test_location = java.iter().find(|path| is_unit_test(path)).cloned();
    let production_location = java
        .iter()
        .find(|path| match &structure.production {
            Some(root) => path.starts_with(&format!("{root}/")),
            None => path.contains("src/main/"),
        })
        .cloned();
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

fn is_unit_test(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with("Test.java") && !name.contains("RunCucumber")
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
}

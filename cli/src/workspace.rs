//! Workspace layout shared by every composition root (`main.rs`,
//! [`crate::mcp`], and [`crate::greenfield`]), so the spec location and
//! the kata layout are defined exactly once.

use std::fs;
use std::path::{Path, PathBuf};

use crate::application::spec_service::ProjectLayout;

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
    ".bdd-staged",
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

/// Detect the project's source layout. When the workshop `kata/` files
/// exist they win (this repo). Otherwise scan for steps, tests, and
/// production sources so an extracted kata without the `kata/` prefix
/// still names real files. MCP `get_requirement` keeps
/// [`workshop_layout`] regardless.
pub fn detect_project_layout(root: &Path) -> ProjectLayout {
    let workshop = workshop_layout();
    if root.join(&workshop.production_location).is_file() {
        return workshop;
    }
    scan_layout(root).unwrap_or(workshop)
}

fn scan_layout(root: &Path) -> Option<ProjectLayout> {
    let mut java = Vec::new();
    collect_files(root, root, "java", &mut java);
    let step_definitions = java
        .iter()
        .find(|path| path.rsplit('/').next().unwrap_or("").contains("Steps"))
        .cloned();
    let test_location = java.iter().find(|path| is_unit_test(path)).cloned();
    let production_location = java.iter().find(|path| path.contains("src/main/")).cloned();
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

/// When this workshop's kata feature directory exists, discovery is
/// restricted to it so `bdd feature list` does not pick up CLI/MCP
/// Cucumber features. Greenfield projects (no `kata/`) still walk the
/// whole tree.
pub fn feature_search_root(root: &Path) -> PathBuf {
    let kata = root.join("kata/src/test/resources/features");
    if kata.is_dir() {
        kata
    } else {
        root.to_path_buf()
    }
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
        assert!(scan_layout(dir.path()).is_none());
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
}

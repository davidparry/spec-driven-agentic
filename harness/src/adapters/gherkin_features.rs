//! Filesystem implementation of the [`FeatureCatalog`] port: discovers
//! `*.feature` files under the project root, reads them, and hands
//! parsing to the pure domain model.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::STAGED_DIR;
use crate::domain::feature::FeatureDoc;
use crate::ports::{FeatureCatalog, FeatureError};
use crate::workspace::feature_search_root;

/// Directories that never contain authored feature files.
const SKIPPED_DIRS: [&str; 6] = ["target", "node_modules", "bin", "obj", ".git", STAGED_DIR];

pub struct GherkinFeatureCatalog {
    root: PathBuf,
    search: PathBuf,
}

impl GherkinFeatureCatalog {
    pub fn new(root: PathBuf) -> Self {
        let search = feature_search_root(&root);
        Self { root, search }
    }

    fn feature_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        collect_features(&self.search, &mut paths);
        paths.sort();
        paths
    }

    fn parse(&self, absolute: &Path) -> Result<FeatureDoc, FeatureError> {
        let relative = absolute
            .strip_prefix(&self.root)
            .unwrap_or(absolute)
            .to_string_lossy()
            .into_owned();
        let content = fs::read_to_string(absolute)
            .map_err(|e| FeatureError(format!("{relative}: not readable - {e}")))?;
        crate::domain::feature::parse(&relative, &content).map_err(FeatureError)
    }
}

fn collect_features(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                collect_features(&path, into);
            }
        } else if name.ends_with(".feature") {
            into.push(path);
        }
    }
}

impl FeatureCatalog for GherkinFeatureCatalog {
    fn list(&self) -> Result<Vec<crate::domain::feature::FeatureSummary>, FeatureError> {
        self.feature_paths()
            .iter()
            .map(|path| self.parse(path).map(|doc| doc.summary()))
            .collect()
    }

    fn read(&self, path: &str) -> Result<FeatureDoc, FeatureError> {
        if !self.exists(path) {
            return Err(FeatureError(format!(
                "{path}: no such feature file. Call feature list to see valid paths."
            )));
        }
        self.parse(&self.root.join(path))
    }

    fn exists(&self, path: &str) -> bool {
        self.root.join(path).is_file()
    }
}

impl crate::ports::FeatureFiles for GherkinFeatureCatalog {
    fn exists(&self, path: &str) -> bool {
        FeatureCatalog::exists(self, path)
    }

    fn has_tag(&self, path: &str, tag: &str) -> bool {
        self.read(path)
            .map(|doc| doc.all_tags().iter().any(|t| t == tag))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURE: &str = "@kata\nFeature: String calculator\n\n  @REQ-001\n  Scenario: Empty string\n    Given a calculator\n    When add is called with \"\"\n    Then the result is 0\n";

    fn catalog_with_feature() -> (tempfile::TempDir, GherkinFeatureCatalog) {
        let dir = tempfile::tempdir().unwrap();
        let features = dir.path().join("features");
        fs::create_dir_all(&features).unwrap();
        fs::write(features.join("calc.feature"), FEATURE).unwrap();
        let catalog = GherkinFeatureCatalog::new(dir.path().to_path_buf());
        (dir, catalog)
    }

    #[test]
    fn list_discovers_and_summarizes_feature_files() {
        let (_dir, catalog) = catalog_with_feature();
        let summaries = catalog.list().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].path, "features/calc.feature");
        assert_eq!(summaries[0].name, "String calculator");
        assert_eq!(summaries[0].scenario_count, 1);
    }

    #[test]
    fn read_returns_the_full_parsed_document() {
        let (_dir, catalog) = catalog_with_feature();
        let doc = catalog.read("features/calc.feature").unwrap();
        assert_eq!(doc.tags, vec!["@kata"]);
        assert_eq!(doc.scenarios[0].name, "Empty string");
        assert_eq!(doc.scenarios[0].tags, vec!["@REQ-001"]);
        assert_eq!(
            doc.scenarios[0].steps,
            vec![
                "Given a calculator",
                "When add is called with \"\"",
                "Then the result is 0",
            ]
        );
    }

    #[test]
    fn read_of_a_missing_file_names_the_recovery_command() {
        let (_dir, catalog) = catalog_with_feature();
        let error = catalog.read("features/nope.feature").unwrap_err();
        assert_eq!(
            error.0,
            "features/nope.feature: no such feature file. \
             Call feature list to see valid paths."
        );
    }

    #[test]
    fn invalid_gherkin_is_a_structured_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("broken.feature"), "not gherkin at all").unwrap();
        let catalog = GherkinFeatureCatalog::new(dir.path().to_path_buf());
        let error = catalog.list().unwrap_err();
        assert!(
            error.0.starts_with("broken.feature: not valid Gherkin -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn skipped_directories_and_non_feature_files_are_not_listed() {
        let (dir, catalog) = catalog_with_feature();
        for skipped in ["target", "node_modules", ".hidden"] {
            let inside = dir.path().join(skipped);
            fs::create_dir_all(&inside).unwrap();
            fs::write(inside.join("x.feature"), FEATURE).unwrap();
        }
        fs::write(dir.path().join("README.md"), "not a feature").unwrap();
        assert_eq!(catalog.list().unwrap().len(), 1);
    }

    #[test]
    fn an_unreadable_root_lists_nothing() {
        let catalog = GherkinFeatureCatalog::new(PathBuf::from("/nonexistent/nowhere"));
        assert_eq!(catalog.list().unwrap(), vec![]);
    }

    #[test]
    fn a_kata_feature_root_hides_sibling_features() {
        let dir = tempfile::tempdir().unwrap();
        // An aggregator root plus the module the kata lives in: the
        // resolved layout names the module's features directory, so
        // Cucumber features belonging to other tooling stay out.
        fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        let module = dir.path().join("kata");
        fs::create_dir_all(module.join("src/main/java")).unwrap();
        fs::write(module.join("pom.xml"), "<project/>").unwrap();
        fs::write(
            module.join("src/main/java/StringCalculator.java"),
            "class StringCalculator {}",
        )
        .unwrap();
        let kata = module.join("src/test/resources/features");
        fs::create_dir_all(&kata).unwrap();
        fs::write(kata.join("calc.feature"), FEATURE).unwrap();
        let other = dir.path().join("harness/tests/features");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("harness.feature"), FEATURE).unwrap();
        let catalog = GherkinFeatureCatalog::new(dir.path().to_path_buf());
        let summaries = catalog.list().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].path,
            "kata/src/test/resources/features/calc.feature"
        );
        assert!(catalog.exists("kata/src/test/resources/features/calc.feature"));
        assert!(catalog.exists("harness/tests/features/harness.feature"));
    }
}

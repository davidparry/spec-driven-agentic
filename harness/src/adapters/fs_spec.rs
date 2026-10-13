//! Filesystem implementations of the spec and feature-file ports.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::model::{Spec, SpecCatalog, resolve_catalog};
use crate::ports::{FeatureFiles, SpecError, SpecRepository};

/// Reads the requirements spec fresh on every call — the spec the user
/// just saved is the spec that gets validated. The root document is a
/// catalog: its includes (and theirs, N levels deep) resolve relative
/// to the root file's directory into one [`SpecCatalog`].
pub struct FsSpecRepository {
    spec_file: PathBuf,
}

impl FsSpecRepository {
    pub fn new(spec_file: PathBuf) -> Self {
        Self { spec_file }
    }

    /// The root document's name inside the catalog — its file name, or
    /// the full path when there is none (so errors still name something).
    fn root_label(&self) -> String {
        self.spec_file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.spec_file.display().to_string())
    }

    fn full_path(&self, catalog_path: &str) -> PathBuf {
        if catalog_path == self.root_label() {
            self.spec_file.clone()
        } else {
            self.spec_file
                .parent()
                .unwrap_or(Path::new(""))
                .join(catalog_path)
        }
    }
}

impl SpecRepository for FsSpecRepository {
    fn load(&self) -> Result<Spec, SpecError> {
        self.load_catalog().map(|catalog| catalog.merged())
    }

    fn load_catalog(&self) -> Result<SpecCatalog, SpecError> {
        resolve_catalog(&self.root_label(), &mut |path| {
            fs::read_to_string(self.full_path(path))
                .map(|content| (content, path.to_string()))
                .map_err(|e| format!("spec: {path} is not readable - {e}"))
        })
        .map_err(SpecError)
    }

    fn read_raw(&self, path: &str) -> Result<String, SpecError> {
        fs::read_to_string(self.full_path(path))
            .map_err(|e| SpecError(format!("spec: {path} is not readable - {e}")))
    }
}

/// Answers feature-file questions relative to the workshop root.
pub struct FsFeatureFiles {
    root: PathBuf,
}

impl FsFeatureFiles {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl FeatureFiles for FsFeatureFiles {
    fn exists(&self, path: &str) -> bool {
        self.root.join(path).is_file()
    }

    fn has_tag(&self, path: &str, tag: &str) -> bool {
        fs::read_to_string(self.root.join(path))
            .map(|content| content.contains(tag))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn a_valid_spec_file_loads() {
        let root = temp_root();
        let spec_file = root.path().join("requirements.json");
        fs::write(
            &spec_file,
            r#"{"project":"Kata","requirements":[{"id":"REQ-001","title":"T",
                "status":"pending","story":"s","acceptanceCriteria":[]}]}"#,
        )
        .unwrap();
        let spec = FsSpecRepository::new(spec_file).load().unwrap();
        assert_eq!(spec.project, "Kata");
        assert_eq!(spec.requirements[0].id, "REQ-001");
    }

    #[test]
    fn a_missing_file_reports_not_readable() {
        let root = temp_root();
        let error = FsSpecRepository::new(root.path().join("requirements.json"))
            .load()
            .unwrap_err();
        assert!(
            error
                .0
                .starts_with("spec: requirements.json is not readable -")
        );
    }

    #[test]
    fn a_path_without_a_file_name_falls_back_to_the_full_path_in_the_error() {
        let error = FsSpecRepository::new(PathBuf::from("/"))
            .load()
            .unwrap_err();
        assert!(
            error.0.starts_with("spec: / is not readable -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn broken_json_reports_not_readable_json() {
        let root = temp_root();
        let spec_file = root.path().join("requirements.json");
        fs::write(&spec_file, "{ nope").unwrap();
        let error = FsSpecRepository::new(spec_file).load().unwrap_err();
        assert!(
            error
                .0
                .starts_with("spec: requirements.json is not readable JSON -")
        );
    }

    #[test]
    fn a_root_catalog_merges_its_included_files_depth_first() {
        let root = temp_root();
        let dir = root.path().join("requirements");
        fs::create_dir_all(dir.join("core")).unwrap();
        fs::write(
            dir.join("requirements.json"),
            r#"{"project":"Kata","includes":["core/math.json"],
                "requirements":[{"id":"REQ-001","title":"T","status":"pending",
                "story":"s","acceptanceCriteria":[]}]}"#,
        )
        .unwrap();
        fs::write(
            dir.join("core/math.json"),
            r#"{"includes":["extra.json"],
                "requirements":[{"id":"REQ-002","title":"T","status":"pending",
                "story":"s","acceptanceCriteria":[]}]}"#,
        )
        .unwrap();
        fs::write(
            dir.join("core/extra.json"),
            r#"{"requirements":[{"id":"REQ-003","title":"T","status":"pending",
                "story":"s","acceptanceCriteria":[]}]}"#,
        )
        .unwrap();
        let repository = FsSpecRepository::new(dir.join("requirements.json"));
        let spec = repository.load().unwrap();
        assert_eq!(spec.project, "Kata");
        let ids: Vec<&str> = spec.requirements.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["REQ-001", "REQ-002", "REQ-003"]);
        let catalog = repository.load_catalog().unwrap();
        assert_eq!(catalog.source_of("REQ-002"), Some("core/math.json"));
        assert_eq!(catalog.source_of("REQ-003"), Some("core/extra.json"));
        assert!(repository.read_raw("core/math.json").is_ok());
    }

    #[test]
    fn a_missing_included_file_names_the_child_in_the_error() {
        let root = temp_root();
        let spec_file = root.path().join("requirements.json");
        fs::write(
            &spec_file,
            r#"{"project":"Kata","includes":["missing.json"],"requirements":[]}"#,
        )
        .unwrap();
        let error = FsSpecRepository::new(spec_file).load().unwrap_err();
        assert!(
            error.0.starts_with("spec: missing.json is not readable -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn read_raw_of_a_missing_file_reports_not_readable() {
        let root = temp_root();
        let spec_file = root.path().join("requirements.json");
        fs::write(&spec_file, r#"{"project":"Kata","requirements":[]}"#).unwrap();
        let error = FsSpecRepository::new(spec_file)
            .read_raw("missing.json")
            .unwrap_err();
        assert!(error.0.starts_with("spec: missing.json is not readable -"));
    }

    #[test]
    fn feature_files_answer_existence_and_tags_relative_to_the_root() {
        let root = temp_root();
        let features_dir = root.path().join("features");
        fs::create_dir_all(&features_dir).unwrap();
        fs::write(features_dir.join("x.feature"), "@REQ-003\nScenario: s\n").unwrap();
        let files = FsFeatureFiles::new(root.path().to_path_buf());
        assert!(files.exists("features/x.feature"));
        assert!(!files.exists("features/missing.feature"));
        assert!(files.has_tag("features/x.feature", "@REQ-003"));
        assert!(!files.has_tag("features/x.feature", "@REQ-004"));
        assert!(!files.has_tag("features/missing.feature", "@REQ-003"));
    }
}

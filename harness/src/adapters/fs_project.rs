//! Filesystem implementation of the [`ProjectFiles`] port: answers
//! marker-file questions against the project root (top level only —
//! marker files live at the root of the project they mark).

use std::fs;
use std::path::PathBuf;

use crate::ports::ProjectFiles;

pub struct FsProjectFiles {
    root: PathBuf,
}

impl FsProjectFiles {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl ProjectFiles for FsProjectFiles {
    fn exists(&self, name: &str) -> bool {
        self.root.join(name).is_file()
    }

    fn any_with_extension(&self, extension: &str) -> bool {
        fs::read_dir(&self.root)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|entry| entry.path().extension().is_some_and(|e| e == extension))
            })
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_markers_are_found_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        let files = FsProjectFiles::new(dir.path().to_path_buf());
        assert!(files.exists("pom.xml"));
        assert!(!files.exists("package.json"));
    }

    #[test]
    fn extension_markers_are_found_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("App.csproj"), "<Project/>").unwrap();
        let files = FsProjectFiles::new(dir.path().to_path_buf());
        assert!(files.any_with_extension("csproj"));
        assert!(!files.any_with_extension("sln"));
    }

    #[test]
    fn a_missing_root_answers_no_to_everything() {
        let files = FsProjectFiles::new(PathBuf::from("/nonexistent/nowhere"));
        assert!(!files.exists("pom.xml"));
        assert!(!files.any_with_extension("csproj"));
    }
}

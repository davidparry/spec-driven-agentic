//! Filesystem implementation of the [`ScaffoldWriter`] port: creates
//! parent directories and writes the file only when it does not exist.

use std::fs;
use std::path::PathBuf;

use crate::ports::{ScaffoldError, ScaffoldWriter};

pub struct FsScaffoldWriter {
    root: PathBuf,
}

impl FsScaffoldWriter {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl ScaffoldWriter for FsScaffoldWriter {
    fn write_new(&self, path: &str, content: &str) -> Result<bool, ScaffoldError> {
        let absolute = self.root.join(path);
        if absolute.exists() {
            return Ok(false);
        }
        let parent = absolute
            .parent()
            .expect("a joined path always has a parent");
        fs::create_dir_all(parent)
            .map_err(|e| ScaffoldError(format!("{path}: cannot create directories - {e}")))?;
        fs::write(&absolute, content)
            .map_err(|e| ScaffoldError(format!("{path}: not writable - {e}")))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_is_created_with_its_directories() {
        let dir = tempfile::tempdir().unwrap();
        let writer = FsScaffoldWriter::new(dir.path().to_path_buf());
        assert!(writer.write_new("a/b/c.txt", "content").unwrap());
        assert_eq!(
            fs::read_to_string(dir.path().join("a/b/c.txt")).unwrap(),
            "content"
        );
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("kept.txt"), "original").unwrap();
        let writer = FsScaffoldWriter::new(dir.path().to_path_buf());
        assert!(!writer.write_new("kept.txt", "replacement").unwrap());
        assert_eq!(
            fs::read_to_string(dir.path().join("kept.txt")).unwrap(),
            "original"
        );
    }

    #[test]
    fn an_unwritable_location_is_a_structured_error() {
        let writer = FsScaffoldWriter::new(PathBuf::from("/dev/null"));
        let error = writer.write_new("x/y.txt", "content").unwrap_err();
        assert!(
            error.0.starts_with("x/y.txt: cannot create directories -"),
            "got: {}",
            error.0
        );
    }
}

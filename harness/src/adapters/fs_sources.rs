//! Filesystem implementation of the [`SourceFiles`] port: walks the
//! project tree (skipping build output and dependency directories) and
//! reads every source file with the requested extension.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ports::{SourceError, SourceFile, SourceFiles};

/// Directories that contain generated or third-party code, never
/// hand-written step definitions.
const SKIPPED_DIRS: [&str; 7] = [
    "target",
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".git",
    ".bdd-staged",
];

pub struct FsSourceFiles {
    root: PathBuf,
}

impl FsSourceFiles {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl SourceFiles for FsSourceFiles {
    fn sources(&self, extension: &str) -> Result<Vec<SourceFile>, SourceError> {
        let suffix = format!(".{extension}");
        let mut paths = Vec::new();
        collect_sources(&self.root, &suffix, &mut paths);
        paths.sort();
        paths
            .into_iter()
            .map(|absolute| {
                let path = absolute
                    .strip_prefix(&self.root)
                    .unwrap_or(&absolute)
                    .to_string_lossy()
                    .into_owned();
                let content = fs::read_to_string(&absolute)
                    .map_err(|e| SourceError(format!("{path}: not readable - {e}")))?;
                Ok(SourceFile { path, content })
            })
            .collect()
    }
}

fn collect_sources(dir: &Path, suffix: &str, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                collect_sources(&path, suffix, into);
            }
        } else if name.ends_with(suffix) {
            into.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_found_recursively_and_sorted() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/steps")).unwrap();
        fs::write(dir.path().join("src/steps/b.java"), "class B {}").unwrap();
        fs::write(dir.path().join("a.java"), "class A {}").unwrap();
        fs::write(dir.path().join("readme.md"), "not java").unwrap();
        let sources = FsSourceFiles::new(dir.path().to_path_buf())
            .sources("java")
            .unwrap();
        let paths: Vec<&str> = sources.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["a.java", "src/steps/b.java"]);
        assert_eq!(sources[0].content, "class A {}");
    }

    #[test]
    fn dependency_and_hidden_directories_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        for skipped in ["node_modules", "target", ".hidden"] {
            fs::create_dir_all(dir.path().join(skipped)).unwrap();
            fs::write(dir.path().join(skipped).join("x.js"), "ignored").unwrap();
        }
        fs::write(dir.path().join("kept.js"), "kept").unwrap();
        let sources = FsSourceFiles::new(dir.path().to_path_buf())
            .sources("js")
            .unwrap();
        let paths: Vec<&str> = sources.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["kept.js"]);
    }

    #[test]
    fn a_missing_root_yields_an_empty_list() {
        let catalog = FsSourceFiles::new(PathBuf::from("/does/not/exist"));
        assert_eq!(catalog.sources("java").unwrap(), vec![]);
    }

    #[test]
    fn an_unreadable_source_file_is_a_structured_error() {
        let dir = tempfile::tempdir().unwrap();
        // Invalid UTF-8 makes read_to_string fail without permission games.
        fs::write(dir.path().join("bad.java"), [0xff, 0xfe, 0x00]).unwrap();
        let error = FsSourceFiles::new(dir.path().to_path_buf())
            .sources("java")
            .unwrap_err();
        assert!(
            error.0.starts_with("bad.java: not readable -"),
            "got: {}",
            error.0
        );
    }
}

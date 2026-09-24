//! The `.spec/` directory: where configuration, state, cache, logs, and
//! staging live. Creating it also moves the layout those files used to
//! have in the project root, and only when the new path is still empty.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::{
    CACHE_DIR, CONFIG_FILE, HISTORY_FILE, LOG_DIR, MEMORY_FILE, SPEC_DIR, STAGED_DIR, STATE_FILE,
};

/// Names that used to sit in the project root, paired with the file or
/// directory they become inside [`.spec`](SPEC_DIR).
const LEGACY: &[(&str, &str)] = &[
    (".spec.toml", CONFIG_FILE),
    (".spec-state.json", STATE_FILE),
    (".spec-memory.json", MEMORY_FILE),
    (".spec-history", HISTORY_FILE),
    (".spec-cache", CACHE_DIR),
    (".spec-log", LOG_DIR),
    (".spec-staged", STAGED_DIR),
];

pub fn spec_home(root: &Path) -> PathBuf {
    root.join(SPEC_DIR)
}

pub fn spec_file(root: &Path, name: &str) -> PathBuf {
    spec_home(root).join(name)
}

/// Create parent directories for `path` when it has a non-empty parent.
pub fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Create `.spec/` and move each legacy artifact into it when the new
/// path does not already exist. An existing new file is left as it is;
/// the old one stays where it was.
pub fn ensure_spec_home(root: &Path) -> io::Result<()> {
    let home = spec_home(root);
    if home.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists as a file, not a directory", home.display()),
        ));
    }
    fs::create_dir_all(&home)?;
    for (old_name, new_name) in LEGACY {
        let from = root.join(old_name);
        let to = home.join(new_name);
        if !from.exists() || to.exists() {
            continue;
        }
        fs::rename(&from, &to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_files_move_and_an_existing_destination_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join(".spec.toml"), "from-old-config").unwrap();
        fs::write(root.join(".spec-state.json"), "from-old-state").unwrap();
        fs::create_dir_all(root.join(".spec-cache")).unwrap();
        fs::write(root.join(".spec-cache/keep.txt"), "cached").unwrap();

        let home = spec_home(root);
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(CONFIG_FILE), "already-new").unwrap();

        ensure_spec_home(root).unwrap();

        assert_eq!(
            fs::read_to_string(spec_file(root, CONFIG_FILE)).unwrap(),
            "already-new"
        );
        assert_eq!(
            fs::read_to_string(root.join(".spec.toml")).unwrap(),
            "from-old-config"
        );
        assert_eq!(
            fs::read_to_string(spec_file(root, STATE_FILE)).unwrap(),
            "from-old-state"
        );
        assert!(!root.join(".spec-state.json").exists());
        assert_eq!(
            fs::read_to_string(spec_file(root, CACHE_DIR).join("keep.txt")).unwrap(),
            "cached"
        );
        assert!(!root.join(".spec-cache").exists());
    }

    #[test]
    fn a_file_named_spec_is_not_replaced_by_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(SPEC_DIR), "not a directory").unwrap();
        let error = ensure_spec_home(dir.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
}

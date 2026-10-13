//! The `.spec/` directory: where configuration, state, cache, and logs
//! live. Creating it also moves the layout those files used to have in
//! the project root, and only when the new path is still empty.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::session::SessionId;
use crate::domain::{
    CACHE_DIR, CONFIG_FILE, HISTORY_FILE, LOG_DIR, MEMORY_FILE, SPEC_DIR, STATE_FILE,
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
];

pub fn spec_home(root: &Path) -> PathBuf {
    root.join(SPEC_DIR)
}

pub fn spec_file(root: &Path, name: &str) -> PathBuf {
    spec_home(root).join(name)
}

/// Where one session's diagnostics roll: `.spec/log/<guid>/`, holding
/// that session's `spec.log.YYYY-MM-DD` files and nobody else's, so a
/// run can be read back on its own. Without a session - a one-shot on a
/// project that has never opened one - the flat `.spec/log/` stands.
pub fn log_dir(root: &Path, session: Option<&SessionId>) -> PathBuf {
    let logs = spec_file(root, LOG_DIR);
    match session {
        Some(id) => logs.join(id.as_str()),
        None => logs,
    }
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

/// Distinguishes this process's concurrent writes from each other; the
/// process id distinguishes them from everybody else's.
static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Write `path` whole or not at all: the bytes go to a scratch file in
/// the same directory and are renamed over the target, so a reader gets
/// the old file or the new one and never a torn one. The parent is
/// created when missing.
pub fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    ensure_parent(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let scratch = path.with_file_name(format!(
        ".{name}.{}.{}.writing",
        std::process::id(),
        WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::write(&scratch, text)?;
    fs::rename(&scratch, path).inspect_err(|_| {
        let _ = fs::remove_file(&scratch);
    })
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
    fn an_atomic_write_lands_whole_and_leaves_no_scratch_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = spec_file(dir.path(), STATE_FILE);
        write_atomically(&target, "first").unwrap();
        write_atomically(&target, "second").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "second");
        let leftovers: Vec<_> = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".writing"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        // A target that cannot be renamed over: the scratch is removed.
        let blocked = dir.path().join("blocked");
        fs::create_dir_all(blocked.join("child")).unwrap();
        assert!(write_atomically(&blocked, "x").is_err());
        let scratch: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".writing"))
            .collect();
        assert!(scratch.is_empty(), "{scratch:?}");
    }

    #[test]
    fn a_session_gets_a_log_directory_of_its_own_and_none_keeps_the_flat_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let id = SessionId::parse("20bd0242-eae5-4edc-8b25-0c954a5c3b3f").unwrap();
        let logs = spec_file(root, LOG_DIR);

        assert_eq!(log_dir(root, Some(&id)), logs.join(id.as_str()));
        // Nothing has opened a session, so there is nothing to file the
        // run under and the directory logs have always used stands.
        assert_eq!(log_dir(root, None), logs);
        assert!(log_dir(root, Some(&id)).starts_with(spec_home(root)));
    }

    #[test]
    fn a_file_named_spec_is_not_replaced_by_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(SPEC_DIR), "not a directory").unwrap();
        let error = ensure_spec_home(dir.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
}

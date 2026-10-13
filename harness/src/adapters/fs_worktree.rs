//! Filesystem implementation of the [`WorkTree`] port. Writes land in
//! the project's own files, so the developer's editor and `git diff`
//! see an edit the moment the harness makes one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::adapters::spec_home::spec_home;
use crate::adapters::work_lock;
use crate::domain::SPEC_DIR;
use crate::ports::{Exclusive, FileChange, WorkTree, WriteError};

pub struct FsWorkTree {
    root: PathBuf,
}

impl FsWorkTree {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Where the new content is written before being renamed over the
    /// real file. In the same directory, so the rename never crosses a
    /// filesystem, and under a name no other writer can be using: two
    /// writers sharing one scratch file rename each other's
    /// half-written bytes into place.
    fn scratch(target: &Path) -> PathBuf {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        target.with_file_name(format!(
            ".{name}.{}.{}.writing",
            std::process::id(),
            WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    /// The project-relative path and the absolute file it names.
    ///
    /// Paths reach here from a model's reply as often as from a human,
    /// so this is the jail: [`confine`](crate::domain::paths::confine)
    /// refuses absolute, home and escaping paths textually, and
    /// [`inside_the_project`] then refuses what a symlink could still
    /// reach. `.spec/` is the harness's own bookkeeping and is not a
    /// place generated code belongs.
    fn resolve(&self, path: &str) -> Result<(String, PathBuf), WriteError> {
        let relative =
            crate::domain::paths::confine(path).map_err(|e| WriteError(format!("{path}: {e}")))?;
        if relative == SPEC_DIR || relative.starts_with(&format!("{SPEC_DIR}/")) {
            return Err(WriteError(format!(
                "{relative}: {SPEC_DIR}/ holds the harness's own state and is not writable here."
            )));
        }
        let target = self.root.join(&relative);
        inside_the_project(&self.root, &target)?;
        Ok((relative, target))
    }
}

/// Distinguishes this process's concurrent writes from each other; the
/// process id distinguishes them from everybody else's.
static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Whether `target` really lands under `root` once the filesystem has
/// had its say.
///
/// `confine` works on the text of a path and cannot see a symlink, so
/// `src/vendor/Foo.java` is innocent until `src/vendor` turns out to
/// point at `/etc`. The deepest ancestor that exists is the one worth
/// asking about: the target itself may not exist yet, and the
/// directories under it are about to be created.
fn inside_the_project(root: &Path, target: &Path) -> Result<(), WriteError> {
    let Ok(real_root) = root.canonicalize() else {
        // No root on disk yet, so nothing it could be escaped from.
        return Ok(());
    };
    let mut existing = target;
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return Ok(()),
        }
    }
    let real = existing
        .canonicalize()
        .map_err(|e| WriteError(format!("{}: not resolvable - {e}", existing.display())))?;
    if real.starts_with(&real_root) {
        return Ok(());
    }
    Err(WriteError(format!(
        "{}: resolves to {} which is outside the project root.",
        target.display(),
        real.display()
    )))
}

/// An IO failure in the words of the thing that was being done to the
/// file, with permission trouble named as such: a developer who ran
/// `spec` against a read-only checkout should read that, not `os error
/// 13`.
fn failed(path: &str, doing: &str, error: &io::Error) -> WriteError {
    let detail = match error.kind() {
        io::ErrorKind::PermissionDenied => format!("permission denied - {error}"),
        _ => error.to_string(),
    };
    WriteError(format!("{path}: {doing} - {detail}"))
}

impl WorkTree for FsWorkTree {
    fn claim(&self) -> Result<Box<dyn Exclusive>, WriteError> {
        work_lock::claim(&spec_home(&self.root)).map(|claim| Box::new(claim) as Box<dyn Exclusive>)
    }

    /// Write the file whole or not at all.
    ///
    /// `fs::write` truncates and then fills, so a crash or a reader in
    /// between leaves or sees a half-written source file. Renaming
    /// over it from the same directory is atomic: a reader gets the old
    /// bytes or the new ones, never a torn file.
    fn write(&self, path: &str, content: &str, summary: &str) -> Result<FileChange, WriteError> {
        // Claimed here as well as by the services, so a caller that
        // writes one file on its own is still safe. Claims nest.
        let _claim = self.claim()?;
        let (relative, target) = self.resolve(path)?;
        let action = if target.exists() { "modify" } else { "create" };

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| failed(&relative, "directory not creatable", &e))?;
        }
        let partial = Self::scratch(&target);
        fs::write(&partial, content).map_err(|e| failed(&relative, "not writable", &e))?;
        fs::rename(&partial, &target).map_err(|e| {
            let _ = fs::remove_file(&partial);
            failed(&relative, "not replaceable", &e)
        })?;

        Ok(FileChange {
            path: relative,
            action: action.to_string(),
            summary: summary.to_string(),
        })
    }

    fn read(&self, path: &str) -> Result<Option<String>, WriteError> {
        let (relative, target) = self.resolve(path)?;
        match fs::read_to_string(&target) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(failed(&relative, "not readable", &e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, FsWorkTree) {
        let dir = tempfile::tempdir().unwrap();
        let tree = FsWorkTree::new(dir.path().to_path_buf());
        (dir, tree)
    }

    #[test]
    fn a_write_lands_in_the_project_immediately() {
        let (dir, tree) = tree();
        let change = tree
            .write("features/x.feature", "Feature: X\n", "new feature")
            .unwrap();
        assert_eq!(change.action, "create");
        assert_eq!(change.path, "features/x.feature");
        assert_eq!(change.summary, "new feature");
        assert_eq!(
            fs::read_to_string(dir.path().join("features/x.feature")).unwrap(),
            "Feature: X\n"
        );
    }

    #[test]
    fn writing_over_an_existing_file_records_a_modify() {
        let (dir, tree) = tree();
        fs::write(dir.path().join("notes.txt"), "old").unwrap();
        let change = tree.write("notes.txt", "new", "edit").unwrap();
        assert_eq!(change.action, "modify");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "new"
        );
    }

    #[test]
    fn a_file_reads_back_and_a_missing_one_is_none() {
        let (_dir, tree) = tree();
        assert_eq!(tree.read("a.txt").unwrap(), None);
        tree.write("a.txt", "one", "s").unwrap();
        assert_eq!(tree.read("a.txt").unwrap().as_deref(), Some("one"));
    }

    /// The rename is from a scratch file of this writer's own: when
    /// every writer shared one name, two of them renamed each other's
    /// half-written bytes into place.
    #[test]
    fn the_file_is_renamed_into_place_and_no_scratch_is_left_behind() {
        let (dir, tree) = tree();
        tree.write("a.txt", "x", "s").unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("writing"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "scratch file left behind: {leftovers:?}"
        );

        let target = dir.path().join("a.txt");
        assert_ne!(
            FsWorkTree::scratch(&target),
            FsWorkTree::scratch(&target),
            "two writes would share one scratch file"
        );
    }

    #[test]
    fn absolute_and_escaping_paths_are_refused() {
        let (dir, tree) = tree();
        for path in [
            "/etc/passwd",
            "../outside.txt",
            r"C:\Windows\win.ini",
            "~/x",
        ] {
            let error = tree.write(path, "x", "s").unwrap_err();
            assert!(
                error.0.contains("absolute")
                    || error.0.contains("..")
                    || error.0.contains("home-directory"),
                "{path}: {}",
                error.0
            );
        }
        assert!(!dir.path().join("etc").exists());
        assert!(!dir.path().parent().unwrap().join("outside.txt").exists());
    }

    #[test]
    fn a_dotdot_that_stays_inside_the_root_is_normalized() {
        let (dir, tree) = tree();
        let change = tree.write("features/../notes.txt", "ok", "norm").unwrap();
        assert_eq!(change.path, "notes.txt");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "ok"
        );
    }

    /// `.spec/` is the harness's own state. A model that proposed a
    /// file update there would be editing the config or the memory the
    /// next run reads back.
    #[test]
    fn the_spec_home_is_not_writable_through_the_work_tree() {
        let (_dir, tree) = tree();
        for path in [".spec/config.toml", ".spec"] {
            let error = tree.write(path, "x", "s").unwrap_err();
            assert!(error.0.contains(".spec/ holds"), "{path}: {}", error.0);
        }
    }

    /// `confine` reads the text of a path and cannot see a symlink, so
    /// the containment check has to ask the filesystem.
    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_project_is_refused() {
        let outside = tempfile::tempdir().unwrap();
        let (dir, tree) = tree();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("vendor")).unwrap();

        let error = tree.write("vendor/Foo.java", "x", "s").unwrap_err();
        assert!(
            error.0.contains("outside the project root"),
            "got: {}",
            error.0
        );
        assert!(!outside.path().join("Foo.java").exists());
    }

    #[test]
    fn an_unwritable_project_is_a_structured_error() {
        let tree = FsWorkTree::new(PathBuf::from("/dev/null/nowhere"));
        let error = tree.write("a.txt", "x", "s").unwrap_err();
        assert!(error.0.contains("not creatable"), "got: {}", error.0);
    }

    /// A read-only checkout is a thing developers have; "permission
    /// denied" is what they need to read, not a bare OS error number.
    #[cfg(unix)]
    #[test]
    fn a_permission_failure_says_permission_denied() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, tree) = tree();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();

        let error = tree.write("locked/a.txt", "x", "s").unwrap_err();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(error.0.contains("permission denied"), "got: {}", error.0);
    }
}

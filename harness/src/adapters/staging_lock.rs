//! Cross-process mutual exclusion for the staging area.
//!
//! The MCP server serializes its own tool calls with a `tokio` mutex,
//! which is enough while one server process owns the area. Separate
//! `spec` invocations share nothing: six concurrent `scenario add`
//! calls against one feature file all read the same base document and
//! wrote over each other, three of the six edits vanished, and every
//! one of the six was told `"staged": true`. A seventh read the
//! manifest halfway through somebody else's write and died on
//! "staging manifest is not valid JSON".
//!
//! An advisory lock on `.spec-staged/.lock` closes the second hole and,
//! held for the whole read-modify-write cycle rather than just the
//! write, the first as well. The kernel drops the lock when the file
//! descriptor closes, which happens on a normal return, on an error
//! path, on a panic, and on a killed process - there is no stale lock
//! to clean up.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

use crate::ports::{StageError, Staging};

/// The lock file, inside the area it guards.
const LOCK_FILE: &str = ".lock";

/// One lock file, shared by everything in this process that names the
/// same staging directory.
///
/// The in-process half is not a nicety: an advisory lock belongs to the
/// open file, not to the process, so a second handle on the same lock
/// file blocks its own process exactly as hard as it blocks a stranger.
/// Claims nest - a service claims the area for its read-modify-write
/// and the store claims it again for the write inside - so the claim
/// has to be re-entrant, and only the outermost one touches the file.
struct DirLock {
    path: PathBuf,
    held: Mutex<Held>,
    released: Condvar,
}

#[derive(Default)]
struct Held {
    owner: Option<ThreadId>,
    depth: usize,
    /// The locked handle. Closing it is what releases the kernel's
    /// lock, so it lives exactly as long as the claim.
    handle: Option<File>,
}

impl DirLock {
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn claim(self: &Arc<Self>) -> Result<StagingClaim, StageError> {
        let me = std::thread::current().id();
        let mut held = self.held();
        loop {
            if held.depth == 0 {
                // Claimed in this process before the file lock is even
                // asked for, so no other thread here can queue up
                // behind a handle of its own and deadlock us.
                held.owner = Some(me);
                held.depth = 1;
                drop(held);
                match self.take_file() {
                    Ok(handle) => {
                        self.held().handle = Some(handle);
                        return Ok(StagingClaim(Arc::clone(self)));
                    }
                    Err(error) => {
                        self.give_back();
                        return Err(error);
                    }
                }
            }
            if held.owner == Some(me) {
                held.depth += 1;
                return Ok(StagingClaim(Arc::clone(self)));
            }
            held = self
                .released
                .wait(held)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Open and lock the file, waiting for whoever has it.
    ///
    /// `changes commit` deletes the whole staging directory, lock file
    /// included, so a handle can end up locking an inode that is no
    /// longer the lock file - and the next process to come along would
    /// create a fresh one and walk straight in. Re-checking which inode
    /// the path points to after the lock is taken is what makes that
    /// race lose: the loser drops its handle and queues on the file
    /// that actually exists.
    fn take_file(&self) -> Result<File, StageError> {
        loop {
            let handle = self.open()?;
            lock_exclusive(&handle)
                .map_err(|e| StageError(format!("staging area could not be locked - {e}")))?;
            if is_still_the_lock_file(&handle, &self.path) {
                return Ok(handle);
            }
        }
    }

    fn open(&self) -> Result<File, StageError> {
        let parent = self.path.parent().expect("the lock file has a directory");
        std::fs::create_dir_all(parent).map_err(|e| {
            StageError(format!(
                "{}: directory not creatable - {e}",
                parent.display()
            ))
        })?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| StageError(format!("staging area could not be locked - {e}")))
    }

    fn give_back(&self) {
        let mut held = self.held();
        held.depth -= 1;
        if held.depth == 0 {
            held.owner = None;
            // Closing the handle releases the kernel's lock.
            held.handle = None;
            drop(held);
            self.released.notify_one();
        }
    }
}

/// Exclusive use of one staging area. Dropping it gives the area back,
/// whether the cycle returned, failed, or panicked.
pub struct StagingClaim(Arc<DirLock>);

impl Drop for StagingClaim {
    fn drop(&mut self) {
        self.0.give_back();
    }
}

impl Staging for StagingClaim {}

static LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<DirLock>>>> = LazyLock::new(Mutex::default);

/// Claim `staged_dir` for one read-modify-write cycle, waiting for any
/// other `spec` process that has it.
pub fn claim(staged_dir: &Path) -> Result<StagingClaim, StageError> {
    let unclaimed = staged_dir.join(LOCK_FILE);
    // Absolute, so two stores that reach the same directory by
    // different relative paths share one claim instead of queuing
    // behind each other's handle forever.
    let path = std::path::absolute(&unclaimed).unwrap_or(unclaimed);
    let lock = {
        let mut locks = LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(locks.entry(path.clone()).or_insert_with(|| {
            Arc::new(DirLock {
                path,
                held: Mutex::default(),
                released: Condvar::new(),
            })
        }))
    };
    lock.claim()
}

#[cfg(unix)]
fn lock_exclusive(handle: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    loop {
        // SAFETY: the descriptor is owned by `handle` and stays open
        // for the length of the call.
        if unsafe { libc::flock(handle.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        // A signal can interrupt the wait; that is not a failure to lock.
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(unix)]
fn is_still_the_lock_file(handle: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (handle.metadata(), std::fs::metadata(path)) {
        (Ok(locked), Ok(named)) => locked.ino() == named.ino() && locked.dev() == named.dev(),
        // The path is gone: whoever cleared the area will make a new
        // one, and this handle is not it.
        _ => false,
    }
}

/// Windows has `LockFileEx`, but reaching it needs a dependency this
/// crate does not carry. The workshop and CI are Unix, so the claim
/// degrades to the in-process half there rather than failing outright
/// - which is exactly the protection that existed before.
#[cfg(not(unix))]
fn lock_exclusive(_handle: &File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn is_still_the_lock_file(_handle: &File, path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_claim_creates_the_lock_file_inside_the_area_it_guards() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join(".spec-staged");
        let held = claim(&staged).unwrap();
        assert!(staged.join(LOCK_FILE).is_file());
        drop(held);
    }

    #[test]
    fn a_second_claim_on_the_same_thread_nests_instead_of_deadlocking() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join(".spec-staged");
        let outer = claim(&staged).unwrap();
        let inner = claim(&staged).unwrap();
        drop(inner);
        // Still held by the outer claim: another thread has to wait.
        let waiting = std::thread::spawn({
            let staged = staged.clone();
            move || claim(&staged).map(|_| ())
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiting.is_finished(), "the nested drop released the area");
        drop(outer);
        waiting.join().unwrap().unwrap();
    }

    #[test]
    fn another_thread_waits_for_the_claim_and_then_gets_it() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join(".spec-staged");
        let held = claim(&staged).unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let waiting = std::thread::spawn({
            let (staged, order) = (staged.clone(), Arc::clone(&order));
            move || {
                let _claim = claim(&staged).unwrap();
                order.lock().unwrap().push("second");
            }
        });
        std::thread::sleep(Duration::from_millis(100));
        order.lock().unwrap().push("first");
        drop(held);
        waiting.join().unwrap();
        assert_eq!(*order.lock().unwrap(), vec!["first", "second"]);
    }

    /// The cross-process half, exercised without a second process: an
    /// advisory lock belongs to the open file rather than to the
    /// process, so two independent handles on one lock file exclude
    /// each other here exactly the way two `spec` invocations do.
    #[cfg(unix)]
    #[test]
    fn a_second_handle_on_the_lock_file_is_excluded_the_way_a_second_process_is() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".lock");
        let mine = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        lock_exclusive(&mine).unwrap();

        let entered = Arc::new(AtomicUsize::new(0));
        let theirs = std::thread::spawn({
            let (path, entered) = (path.clone(), Arc::clone(&entered));
            move || {
                let handle = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                lock_exclusive(&handle).unwrap();
                entered.fetch_add(1, Ordering::SeqCst);
            }
        });
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            entered.load(Ordering::SeqCst),
            0,
            "a second handle walked straight past a held advisory lock"
        );
        drop(mine);
        theirs.join().unwrap();
        assert_eq!(entered.load(Ordering::SeqCst), 1);
    }

    /// A claim taken while the area is being torn down must not hand
    /// two holders a lock each. `changes commit` deletes the directory,
    /// so the handle one of them is holding stops being the lock file.
    #[cfg(unix)]
    #[test]
    fn a_handle_on_a_deleted_lock_file_is_not_mistaken_for_the_live_one() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join(".spec-staged");
        std::fs::create_dir_all(&staged).unwrap();
        let path = staged.join(LOCK_FILE);
        std::fs::write(&path, "").unwrap();
        let orphan = File::open(&path).unwrap();
        std::fs::remove_dir_all(&staged).unwrap();
        assert!(!is_still_the_lock_file(&orphan, &path));

        // ...and a live one still is.
        let held = claim(&staged).unwrap();
        let live = File::open(&path).unwrap();
        assert!(is_still_the_lock_file(&live, &path));
        drop(held);
    }
}

//! Filesystem implementation of the [`ChangeStore`] port. Staged files
//! live under `.spec-staged/files/` mirroring the project layout, with a
//! `manifest.json` describing each change; `commit` copies them into the
//! working tree and clears the area.

use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::staging_lock;
use crate::domain::STAGED_DIR;
use crate::ports::{ChangeStore, StageError, StagedChange, Staging};

pub struct FsChangeStore {
    root: PathBuf,
}

impl FsChangeStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn staged_dir(&self) -> PathBuf {
        self.root.join(STAGED_DIR)
    }

    fn manifest_file(&self) -> PathBuf {
        self.staged_dir().join("manifest.json")
    }

    /// Where the next manifest is written before being renamed over the
    /// real one. In the same directory, so the rename never crosses a
    /// filesystem, and under a name no other writer can be using: two
    /// writers sharing one scratch file rename each other's
    /// half-written bytes into place and the tear comes straight back.
    fn scratch_manifest(&self) -> PathBuf {
        self.staged_dir().join(format!(
            "manifest.json.{}.{}.writing",
            std::process::id(),
            WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    fn staged_file(&self, path: &str) -> Result<PathBuf, StageError> {
        let relative =
            crate::domain::paths::confine(path).map_err(|e| StageError(e.to_string()))?;
        Ok(self.staged_dir().join("files").join(relative))
    }

    fn manifest(&self) -> Result<Vec<StagedChange>, StageError> {
        let file = self.manifest_file();
        if !file.is_file() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&file)
            .map_err(|e| StageError(format!("staging manifest is not readable - {e}")))?;
        serde_json::from_str(&text)
            .map_err(|e| StageError(format!("staging manifest is not valid JSON - {e}")))
    }

    /// Write the manifest whole or not at all.
    ///
    /// `fs::write` truncates and then fills, so a reader that arrives
    /// in between sees an empty or half-written file - which is how a
    /// concurrent `scenario add` died on "staging manifest is not
    /// valid JSON - EOF while parsing a value at line 1 column 0".
    /// Renaming over the manifest from the same directory is atomic:
    /// a reader sees the old file or the new one, never a torn one.
    fn write_manifest(&self, changes: &[StagedChange]) -> Result<(), StageError> {
        let text = serde_json::to_string_pretty(changes).expect("manifest is always serializable");
        let unwritable = |e| StageError(format!("staging manifest is not writable - {e}"));
        let target = self.manifest_file();
        let partial = self.scratch_manifest();
        fs::write(&partial, text).map_err(unwritable)?;
        fs::rename(&partial, &target).map_err(|e| {
            let _ = fs::remove_file(&partial);
            unwritable(e)
        })
    }

    fn clear(&self) -> Result<(), StageError> {
        fs::remove_dir_all(self.staged_dir())
            .map_err(|e| StageError(format!("staging area could not be cleared - {e}")))
    }
}

/// Distinguishes this process's concurrent manifest writes from each
/// other; the process id distinguishes them from everybody else's.
static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const SUMMARY_SEPARATOR: &str = "; ";

/// How many of a file's most recent edit summaries are spelled out in
/// full before the older ones are counted instead.
const KEPT_SUMMARIES: usize = 5;

/// Join the summaries of repeated edits to one file, oldest first, skipping
/// a summary already recorded. Long runs are capped so one file's review
/// line cannot grow without bound.
///
/// The cap states the running total outright. `changes_show` is the human
/// review checkpoint, so the one thing it must never do is understate what
/// is about to be committed, and a reviewer should not have to add a
/// prefix to a list to learn how many edits they are approving.
fn merge_summaries(prior: &str, next: &str) -> String {
    let (elided, spelled) = split_elided(prior);
    let mut parts: Vec<&str> = spelled
        .split(SUMMARY_SEPARATOR)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.contains(&next) {
        return prior.to_string();
    }
    parts.push(next);
    if elided == 0 && parts.len() <= KEPT_SUMMARIES {
        return parts.join(SUMMARY_SEPARATOR);
    }
    let cut = parts.len().saturating_sub(KEPT_SUMMARIES);
    // The prefix of `prior` was one `parts` entry standing for `elided`
    // edits, so the newly cut entries add to that count rather than
    // replacing it. Counting the prefix itself as a single dropped edit
    // is what used to peg the total at two however long the run got.
    let elided = elided + cut;
    let kept = parts[cut..].join(SUMMARY_SEPARATOR);
    let total = elided + parts.len() - cut;
    format!("({total} edits in all, {elided} not shown){SUMMARY_SEPARATOR}{kept}")
}

/// Read back the count [`merge_summaries`] wrote: how many edits are
/// elided, and the summaries still spelled out. `(0, summary)` for a
/// summary that was never capped.
fn split_elided(summary: &str) -> (usize, &str) {
    let Some((marker, spelled)) = summary.split_once(SUMMARY_SEPARATOR) else {
        return (0, summary);
    };
    marker
        .strip_prefix('(')
        .and_then(|marker| marker.strip_suffix(" not shown)"))
        .and_then(|marker| marker.rsplit_once(", "))
        .and_then(|(_, count)| count.parse().ok())
        .map_or((0, summary), |elided| (elided, spelled))
}

fn ensure_parent(path: &Path) -> Result<(), StageError> {
    let parent = path.parent().expect("staged paths always have a parent");
    fs::create_dir_all(parent).map_err(|e| {
        StageError(format!(
            "{}: directory not creatable - {e}",
            parent.display()
        ))
    })
}

impl ChangeStore for FsChangeStore {
    fn claim(&self) -> Result<Box<dyn Staging>, StageError> {
        staging_lock::claim(&self.staged_dir()).map(|claim| Box::new(claim) as Box<dyn Staging>)
    }

    fn stage(&self, path: &str, content: &str, summary: &str) -> Result<StagedChange, StageError> {
        // Claimed here as well as by the services, so a caller that
        // stages one file on its own is still safe. Claims nest.
        let _claim = self.claim()?;
        let path = crate::domain::paths::confine(path).map_err(|e| StageError(e.to_string()))?;
        let mut changes = self.manifest()?;
        let target = self.staged_file(&path)?;
        ensure_parent(&target)?;
        fs::write(&target, content)
            .map_err(|e| StageError(format!("{path}: staged file not writable - {e}")))?;
        let action = if self.root.join(&path).exists() {
            "modify"
        } else {
            "create"
        };
        // One file is one staged entry, but it can be the product of several
        // edits - two scenario_add calls on the same feature, say. The
        // content already holds both; carry both summaries too, or the
        // review surface names only the last and the reviewer undercounts
        // what they are about to approve.
        let summary = match changes.iter().find(|c| c.path == path) {
            Some(prior) => merge_summaries(&prior.summary, summary),
            None => summary.to_string(),
        };
        let change = StagedChange {
            path: path.clone(),
            action: action.to_string(),
            summary,
        };
        changes.retain(|c| c.path != path);
        changes.push(change.clone());
        self.write_manifest(&changes)?;
        Ok(change)
    }

    /// Unclaimed on purpose: the manifest is renamed into place whole,
    /// so a reader cannot see a torn one, and claiming would create
    /// the staging directory as a side effect of `changes show` on a
    /// project that has nothing staged.
    fn changes(&self) -> Result<Vec<StagedChange>, StageError> {
        self.manifest()
    }

    fn content(&self, path: &str) -> Result<Option<String>, StageError> {
        if !self.manifest()?.iter().any(|c| c.path == path) {
            return Ok(None);
        }
        fs::read_to_string(self.staged_file(path)?)
            .map(Some)
            .map_err(|e| StageError(format!("{path}: staged file not readable - {e}")))
    }

    fn commit(&self) -> Result<Vec<StagedChange>, StageError> {
        let _claim = self.claim()?;
        let changes = self.manifest()?;
        for change in &changes {
            let relative = crate::domain::paths::confine(&change.path)
                .map_err(|e| StageError(format!("{}: {e}", change.path)))?;
            let target = self.root.join(&relative);
            ensure_parent(&target)?;
            fs::copy(self.staged_file(&relative)?, &target).map_err(|e| {
                StageError(format!(
                    "{}: could not apply staged file - {e}",
                    change.path
                ))
            })?;
        }
        if !changes.is_empty() {
            self.clear()?;
        }
        Ok(changes)
    }

    fn discard(&self) -> Result<Vec<StagedChange>, StageError> {
        let _claim = self.claim()?;
        let changes = self.manifest()?;
        if !changes.is_empty() {
            self.clear()?;
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, FsChangeStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsChangeStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn two_edits_to_one_file_stay_one_change_that_names_both() {
        let (_dir, store) = store();
        store
            .stage(
                "features/calc.feature",
                "one",
                "add scenario \"A\" for REQ-003",
            )
            .unwrap();
        let change = store
            .stage(
                "features/calc.feature",
                "one and two",
                "add scenario \"B\" for REQ-003",
            )
            .unwrap();
        // One file is still one entry, and its content is the latest write...
        let changes = store.changes().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            store.content("features/calc.feature").unwrap().as_deref(),
            Some("one and two")
        );
        // ...but the review line names both edits, so a reviewer reading
        // changes_show does not undercount what they are approving.
        assert_eq!(
            change.summary,
            "add scenario \"A\" for REQ-003; add scenario \"B\" for REQ-003"
        );
    }

    #[test]
    fn re_staging_the_same_summary_does_not_repeat_it() {
        let (_dir, store) = store();
        store.stage("a.txt", "one", "reword REQ-007").unwrap();
        let change = store.stage("a.txt", "two", "reword REQ-007").unwrap();
        assert_eq!(change.summary, "reword REQ-007");
    }

    #[test]
    fn a_long_run_of_edits_keeps_the_newest_and_counts_the_rest() {
        let (_dir, store) = store();
        for i in 1..=5 {
            store.stage("a.txt", "x", &format!("edit {i}")).unwrap();
        }
        // Up to the cap every edit is named, so there is nothing to count.
        assert_eq!(
            store.changes().unwrap()[0].summary,
            "edit 1; edit 2; edit 3; edit 4; edit 5"
        );

        let change = store.stage("a.txt", "x", "edit 6").unwrap();
        assert_eq!(
            change.summary,
            "(6 edits in all, 1 not shown); edit 2; edit 3; edit 4; edit 5; edit 6"
        );
    }

    /// The count used to be recomputed from the capped summary, where the
    /// "(N earlier edit(s))" prefix read as a single dropped edit - so
    /// from the eighth edit on it stuck at two and the review line
    /// understated the batch for as long as the run went on. Six edits
    /// happened to come out right, which is why it looked like a
    /// phrasing question.
    #[test]
    fn the_total_keeps_climbing_however_long_the_run_gets() {
        let (_dir, store) = store();
        for total in 1..=20 {
            let change = store.stage("a.txt", "x", &format!("edit {total}")).unwrap();
            let (elided, spelled) = split_elided(&change.summary);
            let named = spelled.split(SUMMARY_SEPARATOR).count();
            assert_eq!(elided + named, total, "at {total}: {}", change.summary);
            assert_eq!(named, total.min(KEPT_SUMMARIES), "at {total}");
            if total > KEPT_SUMMARIES {
                assert!(
                    change
                        .summary
                        .starts_with(&format!("({total} edits in all, ")),
                    "at {total}: {}",
                    change.summary
                );
            }
        }
        // The newest edits are always the ones spelled out.
        assert!(
            store.changes().unwrap()[0]
                .summary
                .ends_with("edit 16; edit 17; edit 18; edit 19; edit 20")
        );
    }

    #[test]
    fn a_summary_that_was_never_capped_reads_back_whole() {
        assert_eq!(split_elided("edit 1; edit 2"), (0, "edit 1; edit 2"));
        assert_eq!(split_elided("reword REQ-007"), (0, "reword REQ-007"));
        assert_eq!(split_elided(""), (0, ""));
        // A parenthesised summary is not mistaken for the marker.
        assert_eq!(
            split_elided("(manual) fix; edit 2"),
            (0, "(manual) fix; edit 2")
        );
    }

    /// Re-staging a summary already on the line is still skipped once the
    /// cap is in force, and skipping it does not disturb the total.
    #[test]
    fn a_repeat_after_the_cap_neither_repeats_nor_miscounts() {
        let (_dir, store) = store();
        for i in 1..=6 {
            store.stage("a.txt", "x", &format!("edit {i}")).unwrap();
        }
        let capped = store.changes().unwrap()[0].summary.clone();
        let change = store.stage("a.txt", "y", "edit 6").unwrap();
        assert_eq!(change.summary, capped);
        // The content still advanced, which is the point of re-staging.
        assert_eq!(store.content("a.txt").unwrap().as_deref(), Some("y"));
    }

    #[test]
    fn an_empty_store_has_no_changes_and_no_content() {
        let (_dir, store) = store();
        assert_eq!(store.changes().unwrap(), vec![]);
        assert_eq!(store.content("a.txt").unwrap(), None);
        assert_eq!(store.commit().unwrap(), vec![]);
        assert_eq!(store.discard().unwrap(), vec![]);
    }

    #[test]
    fn staging_a_new_path_records_a_create_and_keeps_the_working_tree_untouched() {
        let (dir, store) = store();
        let change = store
            .stage("features/x.feature", "Feature: X\n", "new feature")
            .unwrap();
        assert_eq!(change.action, "create");
        assert_eq!(change.summary, "new feature");
        assert!(!dir.path().join("features/x.feature").exists());
        assert_eq!(
            store.content("features/x.feature").unwrap().as_deref(),
            Some("Feature: X\n")
        );
    }

    #[test]
    fn staging_an_existing_path_records_a_modify() {
        let (dir, store) = store();
        fs::write(dir.path().join("notes.txt"), "old").unwrap();
        let change = store.stage("notes.txt", "new", "edit").unwrap();
        assert_eq!(change.action, "modify");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "old"
        );
    }

    #[test]
    fn restaging_the_same_path_replaces_the_content_and_keeps_both_summaries() {
        let (_dir, store) = store();
        store.stage("a.txt", "one", "first").unwrap();
        store.stage("a.txt", "two", "second").unwrap();
        let changes = store.changes().unwrap();
        assert_eq!(changes.len(), 1);
        // The latest write is the staged content; the summary is the record
        // of how it got there.
        assert_eq!(store.content("a.txt").unwrap().as_deref(), Some("two"));
        assert_eq!(changes[0].summary, "first; second");
    }

    #[test]
    fn commit_applies_every_change_and_clears_the_area() {
        let (dir, store) = store();
        store
            .stage("features/x.feature", "Feature: X\n", "f")
            .unwrap();
        store
            .stage("requirements/requirements.json", "{}", "spec")
            .unwrap();
        let applied = store.commit().unwrap();
        assert_eq!(applied.len(), 2);
        assert_eq!(
            fs::read_to_string(dir.path().join("features/x.feature")).unwrap(),
            "Feature: X\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("requirements/requirements.json")).unwrap(),
            "{}"
        );
        assert_eq!(store.changes().unwrap(), vec![]);
        assert!(!dir.path().join(STAGED_DIR).exists());
    }

    #[test]
    fn discard_drops_everything_without_touching_the_working_tree() {
        let (dir, store) = store();
        store.stage("a.txt", "content", "s").unwrap();
        let dropped = store.discard().unwrap();
        assert_eq!(dropped.len(), 1);
        assert!(!dir.path().join("a.txt").exists());
        assert_eq!(store.changes().unwrap(), vec![]);
    }

    /// The manifest is replaced by a rename, so a reader sees the old
    /// bytes or the new ones. The scratch file it is renamed from must
    /// be this writer's alone: when every writer shared one name, two
    /// of them renamed each other's half-written bytes into place and
    /// the tear came straight back.
    #[test]
    fn the_manifest_is_renamed_into_place_from_a_scratch_file_of_its_own() {
        let (dir, store) = store();
        store.stage("a.txt", "x", "s").unwrap();
        let staged = dir.path().join(STAGED_DIR);
        let leftovers: Vec<_> = fs::read_dir(&staged)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("writing"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "scratch file left behind: {leftovers:?}"
        );

        let one = FsChangeStore::new(dir.path().to_path_buf());
        let two = FsChangeStore::new(dir.path().to_path_buf());
        assert_ne!(
            one.scratch_manifest(),
            two.scratch_manifest(),
            "two writers would share one scratch file"
        );
    }

    #[test]
    fn a_corrupt_manifest_is_a_structured_error() {
        let (dir, store) = store();
        fs::create_dir_all(dir.path().join(STAGED_DIR)).unwrap();
        fs::write(
            dir.path().join(STAGED_DIR).join("manifest.json"),
            "not json",
        )
        .unwrap();
        let error = store.changes().unwrap_err();
        assert!(
            error.0.starts_with("staging manifest is not valid JSON -"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn an_unwritable_staging_area_is_a_structured_error() {
        let store = FsChangeStore::new(PathBuf::from("/dev/null/nowhere"));
        let error = store.stage("a.txt", "x", "s").unwrap_err();
        assert!(error.0.contains("not creatable"), "got: {}", error.0);
    }

    #[test]
    fn a_staged_file_missing_from_disk_is_a_structured_error() {
        let (dir, store) = store();
        store.stage("a.txt", "x", "s").unwrap();
        fs::remove_file(dir.path().join(STAGED_DIR).join("files/a.txt")).unwrap();
        let read = store.content("a.txt").unwrap_err();
        assert!(
            read.0.contains("staged file not readable"),
            "got: {}",
            read.0
        );
        let commit = store.commit().unwrap_err();
        assert!(
            commit.0.contains("could not apply staged file"),
            "got: {}",
            commit.0
        );
    }

    #[test]
    fn absolute_and_escaping_paths_are_refused() {
        let (dir, store) = store();
        for path in [
            "/etc/passwd",
            "../outside.txt",
            r"C:\Windows\win.ini",
            "~/x",
        ] {
            let error = store.stage(path, "x", "s").unwrap_err();
            assert!(
                error.0.contains("absolute")
                    || error.0.contains("..")
                    || error.0.contains("home-directory"),
                "{path}: {}",
                error.0
            );
        }
        assert!(store.changes().unwrap().is_empty());
        assert!(!dir.path().join("etc").exists());
    }

    #[test]
    fn a_dotdot_that_stays_inside_the_root_is_normalized() {
        let (dir, store) = store();
        let change = store.stage("features/../notes.txt", "ok", "norm").unwrap();
        assert_eq!(change.path, "notes.txt");
        assert_eq!(store.content("notes.txt").unwrap().as_deref(), Some("ok"));
        store.commit().unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "ok"
        );
    }
}

//! Keeping the user's work across a crash.
//!
//! Milestone 0 made Tessera able to keep a user's work. It kept it only when
//! asked. A crash between two saves lost everything since the last one, and
//! the cross-cutting rule is that the application never loses a user's work —
//! so this is a milestone-0 obligation being finished, not a convenience.
//!
//! The copy is written to the configuration directory rather than beside the
//! document. A recovery file next to the original turns up in the user's
//! folders, gets opened by mistake, and gets committed to their version
//! control; and there may be no original yet, because the work most worth
//! recovering is the work never saved at all.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tessera_document::document::Document;

use crate::app::{Status, TesseraApp};
use crate::prefs::Preferences;

/// When the autosave copy was last written, and for which revision.
pub struct Recovery {
    pub copy_path: Option<PathBuf>,
    file_name: String,
    /// The document revision the copy on disk holds.
    ///
    /// Comparing revisions is what stops an idle application rewriting the
    /// same bytes every thirty seconds.
    pub last_saved_revision: u64,
    pub last_write: Instant,
    /// Whether the inability to autosave has already been reported.
    ///
    /// Reported once, not once per frame — but reported, because an
    /// application silently not protecting your work is the exact failure the
    /// no-silent-fallbacks rule exists for.
    pub announced_failure: bool,
    /// Held for as long as this process owns the copy.
    ///
    /// A second Tessera — the one a file manager starts when a `.tessera` is
    /// double-clicked while the first is open — reads the same directory on
    /// its way up, and a copy that is merely *present* looks exactly like a
    /// crash's leavings. What tells them apart is whether somebody still has
    /// it, and the operating system is the only witness both processes can
    /// ask: the lock is released by the kernel when the owner dies, however it
    /// dies, which is the one property a crash-recovery scheme cannot do
    /// without.
    ///
    /// On a sibling `.lock` file rather than the copy itself, because the copy
    /// is rewritten by renaming a `.tmp` over it, and Windows will not rename
    /// over a locked file.
    lock: Option<std::fs::File>,
}

impl Recovery {
    const FILE_NAME: &'static str = "recovery.tessera";

    pub fn new(revision: u64) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            copy_path: None,
            file_name: format!("recovery-{}-{time}-{serial}.tessera", std::process::id()),
            last_saved_revision: revision,
            last_write: Instant::now(),
            announced_failure: false,
            lock: None,
        }
    }

    /// The lock file that says a copy is still owned.
    fn lock_path(copy: &Path) -> PathBuf {
        let mut name = copy.file_name().unwrap_or_default().to_os_string();
        name.push(".lock");
        copy.with_file_name(name)
    }

    /// Whether some process still owns the copy at `copy`.
    ///
    /// Answered by trying to take the lock: if it cannot be taken, somebody
    /// has it. A lock file with nobody holding it is what a crash leaves.
    fn is_owned(copy: &Path) -> bool {
        let lock = Self::lock_path(copy);
        if !lock.exists() {
            return false;
        }
        match std::fs::File::open(&lock) {
            Ok(file) => !matches!(file.try_lock(), Ok(())),
            Err(_) => false,
        }
    }

    /// Where the copy lives, if the platform will name a config directory.
    pub fn path() -> Option<PathBuf> {
        Preferences::directory().map(|dir| dir.join(Self::FILE_NAME))
    }

    /// Whether a copy is owed: the document has moved on, and enough time has
    /// passed.
    ///
    /// The interval is passed in rather than fixed here, because it is a
    /// preference — and a preference that only this module could see would be a
    /// switch in the interface that did nothing.
    pub fn due(&self, revision: u64, now: Instant, every: Duration) -> bool {
        revision != self.last_saved_revision && now.duration_since(self.last_write) >= every
    }

    /// A recovery file left behind by a previous run, if there is one.
    pub fn pending() -> Option<PathBuf> {
        Self::path().filter(|p| p.exists())
    }

    /// Remove the copy: the work it held is now safe somewhere else.
    pub fn discard() {
        if let Some(path) = Self::path() {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn discard_copy(&mut self) {
        if let Some(path) = self.copy_path.take() {
            let _ = std::fs::remove_file(&path);
            // Release before removing: Windows will not delete a locked file.
            self.lock = None;
            let _ = std::fs::remove_file(Self::lock_path(&path));
        }
    }

    pub fn save_if_due(
        &mut self,
        document: &Document,
        directory: &Path,
        now: Instant,
        every: Duration,
    ) -> Result<(), String> {
        if !self.due(document.revision(), now, every) {
            return Ok(());
        }
        let path = self
            .copy_path
            .clone()
            .unwrap_or_else(|| directory.join(&self.file_name));
        self.last_write = now;
        write_copy(document, &path)?;
        if self.lock.is_none() {
            self.lock = Some(take_lock(&Self::lock_path(&path))?);
        }
        self.copy_path = Some(path);
        self.last_saved_revision = document.revision();
        self.announced_failure = false;
        Ok(())
    }
}

impl Default for Recovery {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Write the recovery copy, creating its directory if it is not there yet.
///
/// The directory is the point. `write_atomic` writes a `.tmp` sibling and
/// renames it, which fails with "the system cannot find the path specified"
/// when the folder does not exist — and on a machine that had never saved a
/// preference, it never did, because `Preferences::save_to` was the only
/// thing creating it. So autosave failed on every fresh install, once every
/// thirty seconds, and said so in the status bar.
///
/// Found by using the application.
pub fn write_copy(document: &Document, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    tessera_document::format::save(document, path).map_err(|e| e.to_string())
}

/// Create the lock file and take the exclusive lock on it.
fn take_lock(path: &Path) -> Result<std::fs::File, String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("could not create {}: {e}", path.display()))?;
    file.try_lock()
        .map_err(|e| format!("could not lock {}: {e}", path.display()))?;
    Ok(file)
}

// --- testable core -----------------------------------------------------

/// Take up the recovered document at `path`.
///
/// It comes back **unsaved and untitled**, deliberately. It is not the user's
/// file — it is a copy Tessera made — and handing it back with the original's
/// path attached would let the next `Save` overwrite that original with
/// whatever the crash happened to catch.
pub fn recover_from_path(state: &mut TesseraApp, path: &Path) {
    match tessera_document::format::load(path) {
        Ok(document) => {
            let revision = document.revision();
            state.add_document(document, None);
            let recovery = &mut state.active_mut().recovery;
            recovery.copy_path = Some(path.to_path_buf());
            recovery.last_saved_revision = revision;
            // Ours now, and said so: the lock stops the next instance up
            // from taking it too. A lock that cannot be taken is not fatal —
            // the work is recovered either way — but it is not silent.
            match take_lock(&Recovery::lock_path(path)) {
                Ok(lock) => recovery.lock = Some(lock),
                Err(error) => {
                    state.status = Some(Status::error(format!(
                        "Recovered work, but could not claim its copy: {error}"
                    )));
                }
            }
            state.active_mut().current_path = None;
            // Unsaved, because it is: the user has nowhere on disk that holds
            // this yet. It also keeps the title's asterisk honest.
            state.active_mut().dirty = true;
            state.status = Some(Status::info(
                "Recovered unsaved work from a session that did not close. \
                 Save it somewhere before quitting.",
            ));
        }
        Err(error) => {
            state.status = Some(Status::error(format!(
                "Found work from a session that did not close, but could not \
                 read it: {error}"
            )));
        }
    }
}

/// Offer whatever a previous run left behind, if anything.
///
/// The file is **not** removed here. Recovering it does not make it safe —
/// the user has still saved nothing — so it stays until either a manual save
/// makes it redundant or the next autosave replaces it.
pub fn offer_pending(state: &mut TesseraApp) {
    if let Some(directory) = Preferences::directory() {
        recover_directory(state, &directory);
    }
}

pub fn recover_directory(state: &mut TesseraApp, directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            path.is_file()
                && (name == Recovery::FILE_NAME
                    || (name.starts_with("recovery-") && name.ends_with(".tessera")))
        })
        // Still being written by a Tessera that is running: not ours to take.
        .filter(|path| !Recovery::is_owned(path))
        .collect();
    paths.sort();
    for path in paths {
        recover_from_path(state, &path);
    }
}

#[cfg(test)]
mod tests {
    /// The interval these tests reason about, so a change to the default does
    /// not silently change what they are asserting.
    const EVERY: Duration = Duration::from_secs(30);

    use super::*;

    fn at(offset: Duration) -> (Recovery, Instant) {
        let now = Instant::now();
        (
            Recovery {
                last_saved_revision: 7,
                last_write: now,
                announced_failure: false,
                ..Recovery::default()
            },
            now + offset,
        )
    }

    #[test]
    fn nothing_is_due_when_the_document_has_not_changed() {
        let (r, later) = at(Duration::from_secs(600));
        assert!(
            !r.due(7, later, EVERY),
            "an idle application must not rewrite the same bytes forever"
        );
    }

    #[test]
    fn nothing_is_due_before_the_interval_has_passed() {
        let (r, soon) = at(Duration::from_secs(1));
        assert!(!r.due(8, soon, EVERY));
    }

    #[test]
    fn a_changed_document_is_due_once_the_interval_has_passed() {
        let (r, later) = at(EVERY + Duration::from_millis(1));
        assert!(r.due(8, later, EVERY));
    }

    #[test]
    fn the_default_interval_is_not_so_long_that_a_crash_costs_real_work() {
        assert!(Preferences::default().recovery_interval() <= Duration::from_secs(60));
    }

    #[test]
    fn the_interval_really_comes_from_the_preference() {
        // **The defect this fixes.** The interval was a constant here and the
        // preference beside it did nothing — a switch in the interface that a
        // person could move and watch have no effect.
        let quick = Preferences {
            recovery_seconds: 5,
            ..Preferences::default()
        };
        let slow = Preferences {
            recovery_seconds: 600,
            ..Preferences::default()
        };
        assert!(quick.recovery_interval() < slow.recovery_interval());

        let (r, soon) = at(Duration::from_secs(10));
        assert!(r.due(8, soon, quick.recovery_interval()), "the quick one");
        assert!(
            !r.due(8, soon, slow.recovery_interval()),
            "and the slow one"
        );
    }

    #[test]
    fn a_stray_interval_is_held_to_something_sensible() {
        let wild = Preferences {
            recovery_seconds: 99_999,
            ..Preferences::default()
        };
        assert_eq!(
            wild.recovery_interval(),
            Duration::from_secs(crate::prefs::RECOVERY_MOST as u64)
        );
    }

    #[test]
    fn writing_the_copy_creates_the_directory_it_needs() {
        // The bug this exists for: write_atomic renames a .tmp sibling into
        // place, which cannot work if the folder is not there. Nothing else
        // creates it on a fresh install.
        let dir = std::env::temp_dir().join("tessera-recovery-missing-dir/deeper");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("tessera-recovery-missing-dir"));
        let path = dir.join("recovery.tessera");
        assert!(!dir.exists(), "the directory must start absent");

        write_copy(&Document::new(), &path).expect("it should create the directory");
        assert!(path.exists(), "and then the file");

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("tessera-recovery-missing-dir"));
    }

    #[test]
    fn recovered_work_comes_back_unsaved_and_untitled() {
        // Handing it back with the original's path attached would let the
        // next Save overwrite that original with whatever the crash caught.
        let mut path = std::env::temp_dir();
        path.push("tessera-recovery-test.tessera");
        let _ = std::fs::remove_file(&path);

        let mut source = TesseraApp::headless();
        crate::command::apply(
            &mut source,
            crate::command::Command::AddRectangle(tessera_geometry::DocRect {
                x: 5.0,
                y: 5.0,
                width: 50.0,
                height: 20.0,
            }),
        );
        tessera_document::format::save(source.active().document(), &path).expect("save");

        let mut app = TesseraApp::headless();
        recover_from_path(&mut app, &path);

        assert_eq!(
            app.active().document().frames.len(),
            1,
            "the work came back"
        );
        assert!(
            app.active().current_path.is_none(),
            "it is not the user's file"
        );
        assert!(app.active().dirty, "and it is not saved anywhere yet");
        assert!(app.status.as_ref().is_some_and(|s| !s.is_error));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unreadable_recovery_file_is_reported_not_swallowed() {
        let mut path = std::env::temp_dir();
        path.push("tessera-recovery-damaged.tessera");
        std::fs::write(&path, b"not a tessera archive").unwrap();

        let mut app = TesseraApp::headless();
        recover_from_path(&mut app, &path);

        let status = app.status.as_ref().expect("a failure must be reported");
        assert!(status.is_error, "and reported as an error");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_copy_does_not_sit_beside_the_users_document() {
        // It belongs with the application's own files. See the module note.
        let path = Recovery::path().expect("this platform reports a config directory");
        assert!(path.is_absolute());
        assert_eq!(
            path.file_name().map(|n| n.to_string_lossy().to_string()),
            Some(Recovery::FILE_NAME.to_string())
        );
    }

    #[test]
    fn a_copy_a_running_instance_still_owns_is_not_taken_up_by_another() {
        // **The defect this fixes.** Recovery copies became one per document
        // and the application began opening the files it is launched with.
        // Double-clicking a second `.tessera` in a file manager starts a
        // second process, and that process swept up every copy the first one
        // was still writing and offered them as a crash's leavings — two
        // windows editing the same "recovered" work, and both autosaving to
        // the same file.
        let dir =
            std::env::temp_dir().join(format!("tessera-recovery-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let document = Document::new();
        let mut owner = Recovery {
            last_saved_revision: u64::MAX,
            ..Recovery::default()
        };
        owner
            .save_if_due(&document, &dir, Instant::now(), Duration::ZERO)
            .expect("the owner writes its copy");
        assert!(owner.copy_path.is_some());

        let mut second = TesseraApp::headless();
        recover_directory(&mut second, &dir);
        assert!(
            !second.active().dirty,
            "a copy its owner is still writing is not a crash's leavings"
        );

        drop(owner);
        let mut later = TesseraApp::headless();
        recover_directory(&mut later, &dir);
        assert!(
            later.active().dirty,
            "once the owner is gone, the copy is offered"
        );

        // And having taken it up, `later` owns it: a third instance started
        // in the meantime must not take it as well.
        let mut third = TesseraApp::headless();
        recover_directory(&mut third, &dir);
        assert!(
            !third.active().dirty,
            "recovering a copy makes the recoverer its owner"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

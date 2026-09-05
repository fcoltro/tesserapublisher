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
}

impl Recovery {
    /// Long enough not to intrude, short enough that a crash costs seconds.
    pub const INTERVAL: Duration = Duration::from_secs(30);

    const FILE_NAME: &'static str = "recovery.tessera";

    pub fn new(revision: u64) -> Self {
        Self {
            last_saved_revision: revision,
            last_write: Instant::now(),
            announced_failure: false,
        }
    }

    /// Where the copy lives, if the platform will name a config directory.
    pub fn path() -> Option<PathBuf> {
        Preferences::directory().map(|dir| dir.join(Self::FILE_NAME))
    }

    /// Whether a copy is owed: the document has moved on, and enough time has
    /// passed.
    pub fn due(&self, revision: u64, now: Instant) -> bool {
        revision != self.last_saved_revision
            && now.duration_since(self.last_write) >= Self::INTERVAL
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
            state.replace_document(document);
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
    if let Some(path) = Recovery::pending() {
        recover_from_path(state, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset: Duration) -> (Recovery, Instant) {
        let now = Instant::now();
        (
            Recovery {
                last_saved_revision: 7,
                last_write: now,
                announced_failure: false,
            },
            now + offset,
        )
    }

    #[test]
    fn nothing_is_due_when_the_document_has_not_changed() {
        let (r, later) = at(Duration::from_secs(600));
        assert!(
            !r.due(7, later),
            "an idle application must not rewrite the same bytes forever"
        );
    }

    #[test]
    fn nothing_is_due_before_the_interval_has_passed() {
        let (r, soon) = at(Duration::from_secs(1));
        assert!(!r.due(8, soon));
    }

    #[test]
    fn a_changed_document_is_due_once_the_interval_has_passed() {
        let (r, later) = at(Recovery::INTERVAL + Duration::from_millis(1));
        assert!(r.due(8, later));
    }

    #[test]
    fn the_interval_is_not_so_long_that_a_crash_costs_real_work() {
        assert!(Recovery::INTERVAL <= Duration::from_secs(60));
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
}

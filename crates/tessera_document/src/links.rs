//! Assets on disk, and what the document knows about them.
//!
//! **Linked, never embedded.** A layout that swallows its images is a layout
//! nobody can re-supply artwork for, and a package that cannot collect its
//! links is not a package. The document stores a path and what it last saw
//! there; the pixels stay on disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What the document last knew about a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub path: PathBuf,
    /// The size the artwork wants to be, in points.
    ///
    /// Read from the file when it was placed. Kept here so a document opens
    /// and lays out without touching the disk — a missing image must not stop
    /// a page from being drawn, and the size is what the layout depends on.
    pub natural: (f64, f64),
    /// When the file was last modified, as seconds since the epoch.
    ///
    /// Compared against the file to tell **modified** from **fine**, which is
    /// the distinction the previous codebase never drew and the reason
    /// somebody could send a printer last week's photograph.
    #[serde(default)]
    pub modified: Option<u64>,
}

/// What a link is doing right now.
///
/// Not stored: it is a fact about the disk, and the disk changes without the
/// document being told. Recomputed when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The file is there and has not changed since it was placed.
    Fine,
    /// The file is there and has changed.
    Modified,
    /// The file is not there.
    Missing,
}

impl Link {
    pub fn new(path: impl Into<PathBuf>, natural: (f64, f64)) -> Self {
        Self {
            path: path.into(),
            natural,
            modified: None,
        }
    }

    /// What the disk says about this link now.
    pub fn status(&self) -> Status {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return Status::Missing;
        };
        let Some(placed) = self.modified else {
            // Placed before modification times were recorded, or by something
            // that could not read one. Present is the most that can be said.
            return Status::Fine;
        };
        match modified_seconds(&meta) {
            Some(now) if now != placed => Status::Modified,
            _ => Status::Fine,
        }
    }
}

/// A file's modification time, in seconds since the epoch.
pub fn modified_seconds(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("tessera-link-{name}"));
        std::fs::write(&path, contents).expect("write");
        path
    }

    #[test]
    fn a_file_that_is_not_there_is_missing() {
        let link = Link::new(
            std::env::temp_dir().join("tessera-nothing-here.png"),
            (10.0, 10.0),
        );
        assert_eq!(link.status(), Status::Missing);
    }

    #[test]
    fn a_file_that_is_there_and_unchanged_is_fine() {
        let path = a_file("fine", "pixels");
        let mut link = Link::new(&path, (10.0, 10.0));
        link.modified = std::fs::metadata(&path)
            .ok()
            .as_ref()
            .and_then(modified_seconds);

        assert_eq!(link.status(), Status::Fine);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_has_changed_since_it_was_placed_is_modified() {
        // The distinction the previous codebase never drew, and the reason
        // somebody could send a printer last week's photograph.
        let path = a_file("modified", "pixels");
        let mut link = Link::new(&path, (10.0, 10.0));
        link.modified = Some(0);

        assert_eq!(link.status(), Status::Modified);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_link_placed_before_times_were_recorded_is_not_called_modified() {
        // Present is the most that can be said about it, and crying wolf on
        // every old document would teach people to ignore the warning.
        let path = a_file("untimed", "pixels");
        let link = Link::new(&path, (10.0, 10.0));
        assert_eq!(link.modified, None);
        assert_eq!(link.status(), Status::Fine);
        let _ = std::fs::remove_file(&path);
    }
}

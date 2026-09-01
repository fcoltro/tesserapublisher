//! Writing a file without destroying the previous one.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not replace {path}: {source}")]
    Rename {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The sibling file a pending write goes to before being renamed into place.
fn temp_path_for(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Write to a sibling temporary file, then rename it over the target.
///
/// A rename within a directory is atomic on every platform Tessera targets, so
/// an interrupted save leaves the previous file untouched rather than a
/// half-written one. **A failed save must never destroy the user's work**, and
/// that is what the tests below pin.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let temp = temp_path_for(path);

    std::fs::write(&temp, bytes).map_err(|source| IoError::Write {
        path: temp.clone(),
        source,
    })?;

    std::fs::rename(&temp, path).map_err(|source| {
        // Best effort: do not leave litter behind after a failed rename.
        let _ = std::fs::remove_file(&temp);
        IoError::Rename {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("tessera_atomic").join(name);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn writes_a_new_file() {
        let path = case_dir("new").join("a.bin");
        let _ = std::fs::remove_file(&path);

        write_atomic(&path, b"hello").expect("write");

        assert_eq!(std::fs::read(&path).expect("read"), b"hello");
    }

    #[test]
    fn overwrites_an_existing_file() {
        let path = case_dir("overwrite").join("b.bin");
        std::fs::write(&path, b"old").expect("seed");

        write_atomic(&path, b"new").expect("write");

        assert_eq!(std::fs::read(&path).expect("read"), b"new");
    }

    #[test]
    fn a_failed_write_leaves_the_original_intact() {
        let dir = case_dir("keep");
        let path = dir.join("c.bin");
        std::fs::write(&path, b"original").expect("seed");

        // A directory sitting where the temporary file must go cannot be
        // written to, so the write fails before the rename can happen.
        let blocked = temp_path_for(&path);
        let _ = std::fs::remove_file(&blocked);
        std::fs::create_dir_all(&blocked).expect("block the temp path");

        let result = write_atomic(&path, b"replacement");

        assert!(result.is_err(), "the write should have failed");
        assert_eq!(
            std::fs::read(&path).expect("read"),
            b"original",
            "a failed save must not destroy the previous file"
        );

        std::fs::remove_dir(&blocked).ok();
    }

    #[test]
    fn a_failed_write_reports_the_path_it_could_not_write() {
        let dir = case_dir("reports");
        let path = dir.join("d.bin");
        std::fs::write(&path, b"original").expect("seed");
        let blocked = temp_path_for(&path);
        std::fs::create_dir_all(&blocked).expect("block");

        let message = write_atomic(&path, b"x")
            .expect_err("must fail")
            .to_string();

        assert!(
            message.contains("d.bin"),
            "the error must name the file: {message}"
        );

        std::fs::remove_dir(&blocked).ok();
    }
}

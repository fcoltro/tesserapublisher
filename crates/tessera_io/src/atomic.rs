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
///
/// The bytes are flushed to the disk before the rename, not just to the
/// operating system's cache. Without that the rename can reach the disk
/// first, and a power cut in the seconds between leaves the document's name
/// pointing at an empty file — the previous save gone, the new one never
/// written. A rename is only atomic over what has already landed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let temp = temp_path_for(path);

    let written = std::fs::File::create(&temp)
        .and_then(|mut file| {
            std::io::Write::write_all(&mut file, bytes)?;
            file.sync_all()
        })
        .map_err(|source| IoError::Write {
            path: temp.clone(),
            source,
        });
    if let Err(error) = written {
        // A half-written sibling is litter, and one that fills the disk keeps
        // every later save from succeeding too.
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    rename_patiently(&temp, path).map_err(|source| {
        // Best effort: do not leave litter behind after a failed rename.
        let _ = std::fs::remove_file(&temp);
        IoError::Rename {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// How many times the final rename is tried before its failure is reported.
const RENAME_ATTEMPTS: u32 = 6;

/// Rename `from` over `to`, waiting out a moment when something else holds `to`.
///
/// Windows refuses to replace a file another process has open without
/// sharing its deletion — a virus scan of the last save, the search indexer,
/// OneDrive uploading it — and each of those holds it for milliseconds. Failing
/// the save on the first refusal told somebody their document could not be
/// written because the machine glanced at it. Five retries, doubling from ten
/// milliseconds, wait a third of a second at most; a refusal that outlasts that
/// is a real one and is reported as such.
fn rename_patiently(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 1;
    let mut wait = std::time::Duration::from_millis(10);
    loop {
        match std::fs::rename(from, to) {
            Err(error) if attempt < RENAME_ATTEMPTS && held_by_another(&error) => {
                std::thread::sleep(wait);
                attempt += 1;
                wait *= 2;
            }
            result => return result,
        }
    }
}

/// Whether a rename failed because another process has the file open: access
/// denied, or Windows' sharing and lock violations (32 and 33).
fn held_by_another(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(32 | 33))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_save_waits_out_another_process_holding_the_file() {
        use std::os::windows::fs::OpenOptionsExt;

        let path = case_dir("held").join("e.bin");
        std::fs::write(&path, b"original").expect("seed");
        // Opened sharing nothing, as a scanner might: the rename is refused
        // for as long as this handle lives.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .expect("hold");
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(40));
            drop(held);
        });

        write_atomic(&path, b"saved").expect("the save waits and then lands");
        release.join().expect("the holder lets go");
        assert_eq!(std::fs::read(&path).expect("read"), b"saved");
    }

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

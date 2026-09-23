//! What the disk last said about a file, without asking it every frame.
//!
//! Asking is not free. Layout asks after every placed picture on every edit,
//! the inspector asks every frame a picture frame is selected, the image cache
//! asks on every lookup — and on a network share each ask is a round trip, and
//! a share that has dropped holds the asking thread for seconds. That thread
//! was the one drawing the window.
//!
//! So an answer is kept, and one older than [`FRESH_FOR`] is asked again **on a
//! thread of its own** while the last answer stands. The first time a file is
//! asked about there is no last answer, so that ask is made where it is needed;
//! after that, the interface never waits on the disk. [`look_now`] is for the
//! places a person has asked for the truth this moment — "Check again".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long an answer is taken as true before it is asked again.
pub const FRESH_FOR: Duration = Duration::from_secs(2);

/// What the disk said about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// Nothing is there, or it cannot be read.
    Missing,
    /// A file is there, last modified at `modified` seconds since the epoch,
    /// when the file system says.
    Present { modified: Option<u64> },
}

struct Answer {
    seen: Seen,
    at: Instant,
    asking: bool,
}

static ANSWERS: Mutex<Option<HashMap<PathBuf, Answer>>> = Mutex::new(None);

/// Ask the disk, now, on this thread.
fn ask(path: &Path) -> Seen {
    match std::fs::metadata(path) {
        Ok(meta) => Seen::Present {
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        },
        Err(_) => Seen::Missing,
    }
}

fn remember(path: &Path, seen: Seen) {
    let mut answers = ANSWERS.lock().unwrap_or_else(PoisonError::into_inner);
    answers.get_or_insert_with(HashMap::new).insert(
        path.to_path_buf(),
        Answer {
            seen,
            at: Instant::now(),
            asking: false,
        },
    );
}

/// What the disk last said about `path`, asking again in the background when
/// that was more than [`FRESH_FOR`] ago.
pub fn seen(path: &Path) -> Seen {
    {
        let mut answers = ANSWERS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(answer) = answers.get_or_insert_with(HashMap::new).get_mut(path) {
            if answer.at.elapsed() > FRESH_FOR && !answer.asking {
                answer.asking = true;
                let owned = path.to_path_buf();
                let spawned = std::thread::Builder::new()
                    .name("tessera-seen".into())
                    .spawn(move || remember(&owned, ask(&owned)));
                if spawned.is_err() {
                    // No thread to ask on: the answer stands, and the next
                    // look tries again.
                    answer.asking = false;
                }
            }
            return answer.seen;
        }
    }
    look_now(path)
}

/// Ask the disk about `path` now, and keep the answer.
///
/// For a person asking for the truth this moment — relinking, "Check again" —
/// and for anything that has just written the file itself.
pub fn look_now(path: &Path) -> Seen {
    let seen = ask(path);
    remember(path, seen);
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_file(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("tessera-seen-{name}-{}", std::process::id()));
        std::fs::write(&path, b"pixels").expect("write");
        path
    }

    #[test]
    fn a_file_that_is_there_is_present_and_one_that_is_not_is_missing() {
        let path = a_file("present");
        assert!(matches!(seen(&path), Seen::Present { modified: Some(_) }));
        let nothing = std::env::temp_dir().join("tessera-seen-nothing-here");
        assert_eq!(seen(&nothing), Seen::Missing);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_answer_stands_until_it_is_stale_and_look_now_replaces_it() {
        // The whole point: the second ask does not touch the disk, so the
        // file going away is not seen until the answer is refreshed.
        let path = a_file("stands");
        assert!(matches!(seen(&path), Seen::Present { .. }));
        std::fs::remove_file(&path).expect("remove");
        assert!(
            matches!(seen(&path), Seen::Present { .. }),
            "a fresh answer is not asked again"
        );
        assert_eq!(look_now(&path), Seen::Missing);
        assert_eq!(seen(&path), Seen::Missing, "and look_now's answer is kept");
    }
}

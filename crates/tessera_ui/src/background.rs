//! Work done off the interface's thread: an export.
//!
//! A long document at 300 ppi takes a while, and an export on the
//! interface's thread froze the window for all of it. So the document is
//! resolved where it lives — which is quick, and needs the shaper and the
//! caches the interface holds — and what that made is handed to a thread
//! that renders, converts and writes, advancing a [`Progress`] the status
//! line reads as a bar with a Cancel beside it.
//!
//! **One at a time.** A second export asked for while one runs is refused
//! with a word rather than queued: two exports racing to the same folder
//! is a question nobody wants answered. The document can be worked on
//! meanwhile; the export is of the document as it was when it started.

use std::path::PathBuf;
use std::sync::Arc;

use tessera_pdf::Progress;

use crate::app::{Status, TesseraApp};

/// What a finished job says, and the files it made that are to be opened.
pub struct Finished {
    pub said: String,
    /// Opened with their application when there is one, shown in the file
    /// manager when there are several; empty for neither.
    pub open: Vec<PathBuf>,
}

/// A job under way.
pub struct Job {
    /// What the status line calls it: "Exporting PDF".
    pub what: String,
    pub progress: Arc<Progress>,
    handle: std::thread::JoinHandle<Result<Finished, String>>,
}

/// What an export says when it stopped because it was asked to.
pub const CANCELLED: &str = "Export cancelled";

impl TesseraApp {
    /// Start `work` on a thread of its own. Refused, with a word on the
    /// status line, while another job runs.
    pub fn start_job(
        &mut self,
        what: impl Into<String>,
        work: impl FnOnce(&Progress) -> Result<Finished, String> + Send + 'static,
    ) -> bool {
        if self.job.is_some() {
            self.status = Some(Status::error(
                "An export is already running; wait for it, or cancel it.",
            ));
            return false;
        }
        let progress = Arc::new(Progress::new());
        let shared = Arc::clone(&progress);
        let handle = std::thread::spawn(move || work(&shared));
        self.job = Some(Job {
            what: what.into(),
            progress,
            handle,
        });
        true
    }

    /// Take a finished job's word onto the status line, and open what it
    /// made. Nothing while it runs, and nothing without one.
    pub fn settle_job(&mut self) {
        if !self.job.as_ref().is_some_and(|j| j.handle.is_finished()) {
            return;
        }
        let Some(job) = self.job.take() else { return };
        let outcome = job
            .handle
            .join()
            .unwrap_or_else(|_| Err("The export stopped unexpectedly.".to_string()));
        self.status = Some(match outcome {
            Ok(finished) => {
                match finished.open.as_slice() {
                    [one] => crate::view::links::open_with_its_application(one),
                    [first, ..] => crate::view::links::reveal_in_file_manager(first),
                    [] => {}
                }
                Status::info(finished.said)
            }
            Err(why) if why == CANCELLED => Status::info(why),
            Err(why) => Status::error(why),
        });
    }

    /// Wait for the job to finish and settle it: for tests, and for a
    /// caller that has to have the file before it goes on.
    pub fn finish_job(&mut self) {
        if let Some(job) = self.job.as_ref() {
            while !job.handle.is_finished() {
                std::thread::yield_now();
            }
        }
        self.settle_job();
    }
}

/// The word for an export that failed, or [`CANCELLED`] for one that was
/// stopped.
pub fn failed(error: crate::file_ops::ExportError) -> String {
    match error {
        crate::file_ops::ExportError::Pdf(tessera_pdf::PdfError::Cancelled) => {
            CANCELLED.to_string()
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_off_the_thread_and_reports_when_settled() {
        let mut state = TesseraApp::headless();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        assert!(state.start_job("Exporting", move |progress| {
            wait.recv().ok();
            progress.cancel();
            Ok(Finished {
                said: "Done".into(),
                open: Vec::new(),
            })
        }));
        // Still running: a second is refused, and settling takes nothing.
        assert!(!state.start_job("Again", |_| Err("never".into())));
        assert!(state.status.as_ref().is_some_and(|s| s.is_error));
        state.settle_job();
        assert!(state.job.is_some(), "settled before it finished");

        go.send(()).unwrap();
        state.finish_job();
        assert!(state.job.is_none());
        assert_eq!(state.status.as_ref().unwrap().message, "Done");
    }

    #[test]
    fn a_cancelled_export_says_so_without_calling_it_an_error() {
        let mut state = TesseraApp::headless();
        state.start_job("Exporting", |_| {
            Err(failed(crate::file_ops::ExportError::Pdf(
                tessera_pdf::PdfError::Cancelled,
            )))
        });
        state.finish_job();
        let status = state.status.as_ref().unwrap();
        assert_eq!(status.message, CANCELLED);
        assert!(!status.is_error);
    }
}

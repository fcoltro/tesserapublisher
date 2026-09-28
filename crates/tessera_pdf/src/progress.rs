//! How far an export has got, for an export run off the interface's thread.
//!
//! A counter of steps — a picture prepared, a page written, a page
//! rendered — and how many there are to take, both only ever growing, so
//! another thread can read a fraction at any moment without a lock. And a
//! flag to stop at the next page, which is as soon as an export can stop
//! without leaving a file half-written: nothing is written until the whole
//! of it has been made.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    cancelled: AtomicBool,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// More steps to take. Said before any of them is taken, so the fraction
    /// does not run backwards more than it must.
    pub(crate) fn plan(&self, steps: usize) {
        self.total.fetch_add(steps, Ordering::Relaxed);
    }

    /// One step taken.
    pub(crate) fn step(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// How much is done, from 0 to 1; 0 before anything is planned.
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0)
    }

    /// Ask the export to stop at the next page.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// `Err(Cancelled)` once cancelling has been asked for: what a loop
    /// checks between one page and the next.
    pub(crate) fn go_on(&self) -> Result<(), crate::PdfError> {
        if self.is_cancelled() {
            Err(crate::PdfError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fraction_is_what_is_done_of_what_is_planned() {
        let progress = Progress::new();
        assert_eq!(progress.fraction(), 0.0, "nothing planned is nothing done");
        progress.plan(4);
        progress.step();
        assert_eq!(progress.fraction(), 0.25);
        for _ in 0..9 {
            progress.step();
        }
        assert_eq!(progress.fraction(), 1.0, "never past the whole");
        assert!(progress.go_on().is_ok());
        progress.cancel();
        assert!(matches!(progress.go_on(), Err(crate::PdfError::Cancelled)));
    }
}

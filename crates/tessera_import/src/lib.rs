//! Reading other applications' documents.
//!
//! Two formats, and one rule for both: **what cannot be carried is dropped
//! out loud.** An importer that silently approximates leaves a person
//! trusting a page that is not the one they made. So every import returns the
//! document *and* a list of what it could not bring — a table, a feature, an
//! object kind — for the status line to say.
//!
//! - [`idml`]: InDesign Markup Language, the package InDesign exports for
//!   interchange. A whole document: pages, parents, frames, threads, styles,
//!   colours, stories with footnotes and page-number markers.
//! - [`docx`]: a Word document, as text. One story with its paragraph styles,
//!   to place into a frame.

pub mod docx;
pub mod idml;
mod xml;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("could not read {0}")]
    Read(PathBuf),
    #[error("{0} is not a {1} package: {2}")]
    NotAPackage(PathBuf, &'static str, String),
    #[error("the package is missing {0}")]
    Missing(String),
    #[error("could not parse {entry}: {message}")]
    Parse { entry: String, message: String },
}

/// What an import could not carry, one line each, in the order met.
///
/// Said rather than swallowed: the person reading the page needs to know
/// that the table on page four is not there because Tessera cannot hold it
/// yet, not because their file was empty there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dropped(pub Vec<String>);

impl Dropped {
    fn note(&mut self, what: impl Into<String>) {
        let what = what.into();
        if !self.0.contains(&what) {
            self.0.push(what);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

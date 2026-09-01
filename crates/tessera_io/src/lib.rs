//! Filesystem primitives and image decoding.
//!
//! Deliberately ignorant of the document model, so that
//! `tessera_document`'s file format can call down into it without a
//! dependency cycle. Deciding *whether* a link is stale, and collecting links
//! and fonts into a package, are operations on a document and live above this
//! crate.

pub mod atomic;

pub use atomic::{IoError, write_atomic};

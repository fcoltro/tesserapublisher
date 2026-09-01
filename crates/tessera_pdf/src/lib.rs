//! PDF generation.
//!
//! **This crate depends on the document, and never on `tessera_render`.**
//! Vello is a screen rasterizer; a PDF is a vector program. An application
//! that generates its export from its screen scene ends up with "the export
//! doesn't match the screen" defects that cannot be fixed, because the two
//! pipelines have diverged by construction. Here both consume the same
//! `ResolvedDocument` — the same resolved geometry, and the same shaped glyph
//! runs — so they agree by construction instead.
//!
//! Milestone 0 targets a valid, readable PDF with embedded subsetted fonts and
//! RGB colour. PDF/X-1a and PDF/X-4, CMYK conversion and print marks arrive in
//! milestone 6.

mod writer;

pub use writer::{PdfError, export};

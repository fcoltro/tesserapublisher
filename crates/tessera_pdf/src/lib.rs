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
//! Milestone 0 targeted a valid, readable PDF with embedded subsetted fonts and
//! RGB colour. Milestone 6 adds what a commercial job needs: CMYK converted
//! through the press’s own profile, the marks a guillotine and a press
//! operator read, and the PDF/X claims a printer’s preflight will believe — which
//! is exactly why they are refused rather than written when they cannot be
//! honoured.

mod images;
mod ink;
mod marks;
mod options;
mod shadow;
mod writer;

pub use ink::Ink;
pub use options::{ExportOptions, MARK_LENGTH, Marks, Standard};
pub use writer::{PdfError, export, export_with};

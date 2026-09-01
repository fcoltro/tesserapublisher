//! Fonts, shaping, the story model, and the editable text buffer.
//!
//! This crate has no dependency on egui, on wgpu, or on the document tree.
//! That isolation is the point: cursor movement across grapheme clusters,
//! selection, and IME composition can be tested headless in hundreds of fast
//! tests, instead of only being exercisable by clicking in a running window.
//!
//! [`shape::PositionedGlyph`] is the shared currency of decision D3 — both
//! `tessera_render` and `tessera_pdf` consume exactly this type, which is what
//! guarantees a PDF export matches what was on screen.

pub mod edit;
pub mod shape;
pub mod story;

pub use edit::{EditBuffer, TextCursor};
pub use shape::{FontData, PositionedGlyph, ShapedLine, ShapedText, Shaper};
pub use story::{Story, TextStyle};

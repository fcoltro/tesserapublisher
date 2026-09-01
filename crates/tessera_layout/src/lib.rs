//! Everything that computes *where things go* without drawing them.
//!
//! Separated from `tessera_document` because it is pure computation over the
//! model, and separated from `tessera_render` because a headless PDF export
//! needs the same answers a screen redraw does.

pub mod resolve;

pub use resolve::{ResolvedDocument, ResolvedItem, ResolvedKind, StoryMap, resolve};

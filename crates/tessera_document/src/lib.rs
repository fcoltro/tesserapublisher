//! The document model, undo/redo, and the `.tessera` file format.
//!
//! The model is an arena of plain structs rather than an ECS (decision D1).
//! A spread holds dozens of frames, not tens of thousands, so ECS iteration
//! buys nothing and charges three times over: awkward serialization, undo that
//! needs a whole-World snapshot, and every read behind a lock. Plain structs
//! serialize with serde for free — which is why the file format could land in
//! milestone 0 at all.

pub mod document;
pub mod format;
pub mod history;
pub mod ids;
pub mod nodes;

pub use document::Document;
pub use history::History;
pub use ids::{FrameId, LayerId, PageId, SpreadId, StoryId};
pub use nodes::{Frame, FrameKind, Layer, Page, Spread, Stroke};

//! Typed arena keys.
//!
//! `slotmap`'s `new_key_type!` gives each a distinct type, so a `FrameId`
//! cannot be passed where a `LayerId` belongs, and a stale key from a deleted
//! node returns `None` rather than silently aliasing a new one.

slotmap::new_key_type! {
    pub struct FrameId;
    pub struct LayerId;
    pub struct PageId;
    pub struct SpreadId;
    pub struct StoryId;
}

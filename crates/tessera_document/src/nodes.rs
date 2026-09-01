//! The nodes held in the document's arenas.

use serde::{Deserialize, Serialize};
use tessera_color::Color;
use tessera_geometry::DocRect;

use crate::ids::{FrameId, LayerId, PageId, StoryId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}

/// The kinds of frame milestone 0 supports.
///
/// Additive by construction: groups, images and paths become new variants
/// without disturbing documents already written to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrameKind {
    Rectangle,
    Ellipse,
    /// Text frames reference a story rather than owning it. A threaded story
    /// flows through several frames but exists once — which is what makes
    /// milestone 4's threading natural rather than bolted on.
    Text {
        story: StoryId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub bounds: DocRect,
    pub kind: FrameKind,
    pub fill: Color,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Back to front. The last entry paints on top.
    pub frames: Vec<FrameId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub bounds: DocRect,
    pub layers: Vec<LayerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    pub pages: Vec<PageId>,
}

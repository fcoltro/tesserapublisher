//! The nodes held in the document's arenas.

use serde::{Deserialize, Serialize};
use tessera_color::Color;
use tessera_geometry::{DocPoint, DocRect, Transform};

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
    /// An arbitrary path, in **frame-local** coordinates: `(0, 0)` is the
    /// frame's top-left. Storing it locally rather than in document space is
    /// what makes moving a path frame work without rewriting its geometry.
    ///
    /// One variant covers both the line tool and the pen tool; a line is
    /// simply a two-point path.
    Path(kurbo::BezPath),
    /// A group of frames, treated as one object.
    ///
    /// Children are held here and **removed from the layer's own list**, so
    /// there is exactly one place that owns a frame's position in the paint
    /// order. Anything else drifts: a child listed in both would paint twice
    /// and hit-test inconsistently.
    Group(Vec<FrameId>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// The frame's box **in its own coordinate space**, before `transform`.
    pub bounds: DocRect,
    /// That space, mapped onto the document.
    ///
    /// Geometric bounds plus an item transform is InDesign's model, and the
    /// reason shear, flipping and correctly scaling a rotated group can be
    /// expressed at all. A rectangle plus one rotation angle — which this
    /// replaced — cannot represent any of them, because scaling a rotated
    /// object non-uniformly *is* a shear and an axis-aligned box has nowhere
    /// to put it.
    ///
    /// `serde(default)` reads as the identity, so a frame that never moved
    /// costs nothing on disk. Documents written before transforms existed are
    /// brought forward by the format's migration chain, which turns their
    /// `rotation` into a rotation about the frame's own centre.
    #[serde(default)]
    pub transform: Transform,
    pub kind: FrameKind,
    pub fill: Color,
    pub stroke: Option<Stroke>,
}

impl Frame {
    /// Where `bounds` really sits, with the placement applied.
    ///
    /// The four corners in document space, clockwise from the top left. Any
    /// question about where a frame *is* on the page goes through here rather
    /// than reading `bounds` directly, which answers only where it is in its
    /// own space.
    pub fn corners(&self) -> [DocPoint; 4] {
        let b = self.bounds;
        [
            DocPoint { x: b.x, y: b.y },
            DocPoint {
                x: b.x + b.width,
                y: b.y,
            },
            DocPoint {
                x: b.x + b.width,
                y: b.y + b.height,
            },
            DocPoint {
                x: b.x,
                y: b.y + b.height,
            },
        ]
        .map(|p| self.transform.apply(p))
    }

    /// The frame's centre, in document space.
    pub fn centre(&self) -> DocPoint {
        self.transform.apply(self.bounds.center())
    }

    /// A document-space point in the frame's own space.
    pub fn to_local(&self, point: DocPoint) -> DocPoint {
        self.transform.inverse().apply(point)
    }

    /// The angle to show in an inspector, in degrees.
    pub fn rotation_degrees(&self) -> f64 {
        self.transform.rotation_degrees()
    }
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

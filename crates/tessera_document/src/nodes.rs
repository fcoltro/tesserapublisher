//! The nodes held in the document's arenas.

use serde::{Deserialize, Serialize};
use tessera_color::Color;
use tessera_geometry::{DocPoint, DocRect, Transform};

use crate::ids::{FrameId, LayerId, PageId, StoryId};

/// Where a stroke sits relative to the edge it follows.
///
/// InDesign's three, and the one part of a stroke that changes *geometry*
/// rather than appearance — which is why it belongs in the model rather than
/// at the point of painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeAlign {
    #[default]
    Center,
    Inside,
    Outside,
}

/// The shape drawn at the end of an open stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    /// Extends half the stroke width past the end. PDF calls this projecting.
    Square,
}

/// How two segments of a stroke are joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// PostScript's default, and PDF's, and every drawing tool's.
fn default_miter_limit() -> f64 {
    4.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    /// Every field below carries `serde(default)`, so a document written
    /// before the stroke grew them loads as a plain centred stroke with butt
    /// caps and miter joins — which is exactly what it drew before.
    #[serde(default)]
    pub align: StrokeAlign,
    #[serde(default)]
    pub cap: LineCap,
    #[serde(default)]
    pub join: LineJoin,
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f64,
    /// Dash and gap lengths in points, alternating. Empty means solid.
    #[serde(default)]
    pub dashes: Vec<f64>,
    #[serde(default)]
    pub dash_offset: f64,
}

impl Stroke {
    /// A plain centred stroke, as everything drew before the model grew.
    pub fn new(color: Color, width: f64) -> Self {
        Self {
            color,
            width,
            align: StrokeAlign::default(),
            cap: LineCap::default(),
            join: LineJoin::default(),
            miter_limit: default_miter_limit(),
            dashes: Vec::new(),
            dash_offset: 0.0,
        }
    }

    /// How far the stroke's centreline sits from the shape's own edge.
    ///
    /// Negative is inward. A renderer moves the geometry by this and then
    /// strokes it centred, which is what makes an inside stroke land wholly
    /// inside the shape instead of straddling its edge.
    pub fn offset(&self) -> f64 {
        match self.align {
            StrokeAlign::Center => 0.0,
            StrokeAlign::Inside => -self.width / 2.0,
            StrokeAlign::Outside => self.width / 2.0,
        }
    }

    /// Whether this stroke is dashed rather than solid.
    pub fn is_dashed(&self) -> bool {
        self.dashes.iter().any(|d| *d > 0.0)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke() -> Stroke {
        Stroke::new(Color::BLACK, 4.0)
    }

    #[test]
    fn a_new_stroke_is_the_one_everything_drew_before() {
        // Every field added to Stroke defaults to what a plain centred
        // hairline already did, which is what lets an old document load
        // without a migration that rewrites anything.
        let s = stroke();
        assert_eq!(s.align, StrokeAlign::Center);
        assert_eq!(s.cap, LineCap::Butt);
        assert_eq!(s.join, LineJoin::Miter);
        assert_eq!(s.miter_limit, 4.0);
        assert!(!s.is_dashed());
        assert_eq!(s.offset(), 0.0, "a centred stroke moves no geometry");
    }

    #[test]
    fn alignment_moves_the_centreline_by_half_the_width() {
        // The whole of what alignment means: an inside stroke sits wholly
        // inside the shape, so its centreline runs half a width in.
        let mut s = stroke();

        s.align = StrokeAlign::Inside;
        assert_eq!(s.offset(), -2.0);

        s.align = StrokeAlign::Outside;
        assert_eq!(s.offset(), 2.0);
    }

    #[test]
    fn a_wider_stroke_is_offset_further() {
        let mut s = Stroke::new(Color::BLACK, 10.0);
        s.align = StrokeAlign::Inside;
        assert_eq!(s.offset(), -5.0);
    }

    #[test]
    fn a_pattern_of_zeroes_is_not_a_dash_pattern() {
        // An all-zero pattern would ask the renderer for infinitely many
        // zero-length dashes. It is solid, and says so.
        let mut s = stroke();
        s.dashes = vec![0.0, 0.0];
        assert!(!s.is_dashed());

        s.dashes = vec![6.0, 3.0];
        assert!(s.is_dashed());
    }

    #[test]
    fn a_stroke_round_trips_through_json_with_everything_it_carries() {
        let mut s = stroke();
        s.align = StrokeAlign::Outside;
        s.cap = LineCap::Round;
        s.join = LineJoin::Bevel;
        s.miter_limit = 10.0;
        s.dashes = vec![6.0, 3.0];
        s.dash_offset = 1.5;

        let json = serde_json::to_string(&s).expect("ser");
        let back: Stroke = serde_json::from_str(&json).expect("de");
        assert_eq!(back, s);
    }

    #[test]
    fn a_stroke_written_before_the_model_grew_loads_as_a_plain_one() {
        // What makes format 3 documents open without rewriting anything.
        let old = r#"{"color":{"Rgb":{"r":0.0,"g":0.0,"b":0.0,"a":1.0}},"width":2.0}"#;
        let s: Stroke = serde_json::from_str(old).expect("de");
        assert_eq!(s.width, 2.0);
        assert_eq!(s.align, StrokeAlign::Center);
        assert_eq!(
            s.miter_limit, 4.0,
            "the miter limit must default to PostScript's, not to zero"
        );
        assert!(!s.is_dashed());
    }
}

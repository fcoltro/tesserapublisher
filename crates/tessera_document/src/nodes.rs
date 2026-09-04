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

/// Distances **inward** from a page's edge to its type area.
///
/// Inside and outside rather than left and right, because that is what a
/// margin means in a bound document: the inside margin is the one against the
/// spine, and on a left-hand page it falls on the right. Storing left and
/// right would put the margins on the wrong side of every alternate page — an
/// error that first shows up at the printer.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Margins {
    pub top: f64,
    pub bottom: f64,
    /// Toward the spine on a facing-page spread; the left edge otherwise.
    pub inside: f64,
    /// Away from the spine; the right edge otherwise.
    pub outside: f64,
}

impl Margins {
    pub fn uniform(all: f64) -> Self {
        Self {
            top: all,
            bottom: all,
            inside: all,
            outside: all,
        }
    }
}

/// Distances **outward** from a page's edge.
///
/// Bleed and slug both grow away from the page, so these are left and right:
/// there is no spine to be inside of.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Insets {
    pub fn uniform(all: f64) -> Self {
        Self {
            top: all,
            bottom: all,
            left: all,
            right: all,
        }
    }

    /// Whether this inset moves anything at all.
    pub fn is_zero(self) -> bool {
        self.top == 0.0 && self.bottom == 0.0 && self.left == 0.0 && self.right == 0.0
    }
}

/// Which side of a spread a page sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSide {
    /// A verso: its spine is on the right.
    Left,
    /// A recto: its spine is on the left.
    Right,
    /// Not part of a facing-page spread, so it has no spine.
    Single,
}

/// A named page size, in points.
///
/// The model stores a width and a height and nothing else — a preset is only
/// a way of naming a pair a person recognises. Kept here rather than in the
/// interface because "A4 is 210 by 297 millimetres" is knowledge about
/// documents, and a test can hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePreset {
    A3,
    A4,
    A5,
    Letter,
    Legal,
    Tabloid,
}

impl PagePreset {
    pub const ALL: [PagePreset; 6] = [
        PagePreset::A3,
        PagePreset::A4,
        PagePreset::A5,
        PagePreset::Letter,
        PagePreset::Legal,
        PagePreset::Tabloid,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PagePreset::A3 => "A3",
            PagePreset::A4 => "A4",
            PagePreset::A5 => "A5",
            PagePreset::Letter => "Letter",
            PagePreset::Legal => "Legal",
            PagePreset::Tabloid => "Tabloid",
        }
    }

    /// Portrait width and height, in points.
    pub fn size(self) -> (f64, f64) {
        // The ISO sizes are defined in millimetres and the US ones in inches,
        // so each is written in its own unit and converted here rather than
        // carrying a rounded point value that matches neither.
        const MM: f64 = 72.0 / 25.4;
        const IN: f64 = 72.0;
        match self {
            PagePreset::A3 => (297.0 * MM, 420.0 * MM),
            PagePreset::A4 => (210.0 * MM, 297.0 * MM),
            PagePreset::A5 => (148.0 * MM, 210.0 * MM),
            PagePreset::Letter => (8.5 * IN, 11.0 * IN),
            PagePreset::Legal => (8.5 * IN, 14.0 * IN),
            PagePreset::Tabloid => (11.0 * IN, 17.0 * IN),
        }
    }

    /// The preset matching a size, in either orientation.
    ///
    /// Within a twentieth of a point, because a size that arrived through
    /// millimetres and back will not be bit-identical and should still be
    /// recognised as the paper it is.
    pub fn matching(width: f64, height: f64) -> Option<PagePreset> {
        const TOLERANCE: f64 = 0.05;
        PagePreset::ALL.into_iter().find(|preset| {
            let (w, h) = preset.size();
            let same = (w - width).abs() < TOLERANCE && (h - height).abs() < TOLERANCE;
            let turned = (h - width).abs() < TOLERANCE && (w - height).abs() < TOLERANCE;
            same || turned
        })
    }
}

/// Which way round a page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    /// A square page is portrait: it has to be one of them, and portrait is
    /// what a new document is.
    pub fn of(width: f64, height: f64) -> Self {
        if width > height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        }
    }

    /// `(width, height)` turned to this orientation.
    pub fn apply(self, width: f64, height: f64) -> (f64, f64) {
        let (long, short) = if width > height {
            (width, height)
        } else {
            (height, width)
        };
        match self {
            Orientation::Portrait => (short, long),
            Orientation::Landscape => (long, short),
        }
    }
}

/// The document's page setup.
///
/// Page **size** is deliberately absent: it already lives in [`Page::bounds`],
/// and holding it in two places would mean deciding, forever, which one is
/// right when they disagree.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DocumentSetup {
    pub margins: Margins,
    pub bleed: Insets,
    pub slug: Insets,
    pub facing_pages: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub bounds: DocRect,
    pub layers: Vec<LayerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// A ruler guide, in spread coordinates.
///
/// On the spread rather than on a page. InDesign has both, and the difference
/// only bites once pages within a spread can move independently — which is
/// milestone 3's concern. One kind now beats two kinds guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub axis: Axis,
    /// Where it sits along the axis it cuts across: an `x` for a vertical
    /// guide, a `y` for a horizontal one.
    pub position: f64,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    pub pages: Vec<PageId>,
    /// `serde(default)` so a document written before guides existed loads
    /// with none, which is the truth about it.
    #[serde(default)]
    pub guides: Vec<Guide>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_two_hundred_and_ten_by_two_hundred_and_ninety_seven_millimetres() {
        let (w, h) = PagePreset::A4.size();
        assert!((w - 210.0 * 72.0 / 25.4).abs() < 1e-9);
        assert!((h - 297.0 * 72.0 / 25.4).abs() < 1e-9);
    }

    #[test]
    fn letter_is_eight_and_a_half_by_eleven_inches() {
        assert_eq!(PagePreset::Letter.size(), (612.0, 792.0));
    }

    #[test]
    fn every_preset_is_taller_than_it_is_wide() {
        // `size` is defined as the portrait pair; `Orientation::apply` turns
        // it. A preset entered the other way round would silently make the
        // landscape button do nothing.
        for preset in PagePreset::ALL {
            let (w, h) = preset.size();
            assert!(h > w, "{} is stored landscape", preset.name());
        }
    }

    #[test]
    fn a_preset_is_recognised_in_either_orientation() {
        let (w, h) = PagePreset::A4.size();
        assert_eq!(PagePreset::matching(w, h), Some(PagePreset::A4));
        assert_eq!(PagePreset::matching(h, w), Some(PagePreset::A4), "turned");
    }

    #[test]
    fn a_size_that_is_no_preset_matches_nothing() {
        assert_eq!(PagePreset::matching(123.0, 456.0), None);
    }

    #[test]
    fn a_preset_survives_a_trip_through_millimetres() {
        // Which is what the document setup panel does to it, so a size that
        // came back a hair off must still be recognised as the paper it is.
        let (w, h) = PagePreset::A4.size();
        let mm = |v: f64| (v / (72.0 / 25.4) * 100.0).round() / 100.0 * (72.0 / 25.4);
        assert_eq!(PagePreset::matching(mm(w), mm(h)), Some(PagePreset::A4));
    }

    #[test]
    fn turning_a_page_landscape_swaps_its_sides() {
        assert_eq!(Orientation::Landscape.apply(210.0, 297.0), (297.0, 210.0));
        assert_eq!(Orientation::Portrait.apply(297.0, 210.0), (210.0, 297.0));
    }

    #[test]
    fn turning_a_page_to_the_orientation_it_already_has_changes_nothing() {
        assert_eq!(Orientation::Portrait.apply(210.0, 297.0), (210.0, 297.0));
    }

    #[test]
    fn a_square_page_is_portrait() {
        // It has to be one of them, and portrait is what a new document is.
        assert_eq!(Orientation::of(500.0, 500.0), Orientation::Portrait);
    }

    #[test]
    fn a_fresh_setup_has_no_margins_bleed_or_slug() {
        // A document that never had them has none. Inventing 10mm would be
        // fabricating a decision the user never made.
        let setup = DocumentSetup::default();
        assert_eq!(setup.margins, Margins::default());
        assert_eq!(setup.bleed, Insets::default());
        assert_eq!(setup.slug, Insets::default());
        assert!(!setup.facing_pages);
    }

    #[test]
    fn margins_are_uniform_when_every_edge_matches() {
        let m = Margins::uniform(36.0);
        assert_eq!(
            (m.top, m.bottom, m.inside, m.outside),
            (36.0, 36.0, 36.0, 36.0)
        );
    }

    #[test]
    fn insets_are_uniform_when_every_edge_matches() {
        let b = Insets::uniform(8.5);
        assert_eq!((b.top, b.bottom, b.left, b.right), (8.5, 8.5, 8.5, 8.5));
    }

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

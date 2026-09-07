//! The nodes held in the document's arenas.

use serde::{Deserialize, Serialize};
use tessera_color::Color;
use tessera_geometry::{DocPoint, DocRect, Transform};

use crate::ids::{FrameId, PageId, StoryId};

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
    /// A container showing artwork from a file.
    ///
    /// **Not a shape.** A rectangle has a fill; a graphic frame has contents,
    /// which sit inside it under a transform of their own and are clipped by
    /// it. Conflating the two is why the previous codebase could never move an
    /// image inside its frame.
    ///
    /// Empty until something is placed, and an empty one is a real thing: it
    /// is the box a designer draws to reserve room for a photograph that has
    /// not arrived.
    Graphic {
        #[serde(default)]
        placed: Option<crate::graphic::Placement>,
    },
    Text {
        story: StoryId,
        /// How this frame lays that story out: columns, inset, and where the
        /// text sits when it does not fill the frame.
        ///
        /// `serde(default)` is a single column with no inset, aligned to the
        /// top — which is what every text frame written before this did.
        #[serde(default)]
        layout: TextLayout,
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
    /// What the shape is filled with.
    ///
    /// A [`Paint`](crate::paint::Paint) rather than a `Color`, because a
    /// gradient is not a colour: a colour answers "what is your value" and a
    /// gradient has no single answer. Documents written before gradients existed
    /// carry a bare colour here and are brought forward by the format's
    /// migration chain, which wraps it as a solid paint.
    pub fill: crate::paint::Paint,
    pub stroke: Option<Stroke>,
    /// How text in other frames runs around this one.
    ///
    /// On the **obstacle**, not on the text: an object is given a wrap once
    /// and every frame near it obeys, which is what a person means by "wrap
    /// text around this picture". Putting it on the text frame would mean
    /// telling each of them about each object.
    #[serde(default)]
    pub wrap: TextWrap,
    /// How the whole object composites onto what is behind it.
    ///
    /// On the object, not on its fill. An object at half opacity is composited
    /// once as a whole — fill, stroke and artwork together — which is what a
    /// person means by "make this 50%"; a fill at half alpha leaves the stroke
    /// opaque and shows it through its own fill. Both are worth having, and
    /// they are different facts about different things.
    #[serde(default)]
    pub blend: crate::blending::Blending,
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

/// A stack of objects spanning the whole document.
///
/// **Document-wide, not per-page.** A layer used to belong to a page, which
/// made a frame's page and its layer the same fact — and that is what put every
/// frame drawn anywhere onto page one. InDesign's layer spans every page, so a
/// frame's page is derived from where it sits and cannot disagree with itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// A locked layer's frames cannot be selected or moved. Hiding a layer
    /// makes its frames unselectable too: a frame you cannot see but can still
    /// catch with a click is worse than one you can see.
    pub locked: bool,
    /// Back to front. The last entry paints on top.
    pub frames: Vec<FrameId>,
}

impl Layer {
    /// A new, empty, visible, unlocked layer.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            locked: false,
            frames: Vec::new(),
        }
    }
}

impl FrameKind {
    /// A text frame showing `story`, laid out as one column.
    pub fn text(story: StoryId) -> Self {
        Self::Text {
            story,
            layout: TextLayout::default(),
        }
    }
}

/// The rhythm every line locked to it sits on.
///
/// Measured from the top of the **page**, not the frame. That is the whole
/// point: two columns in different frames line up because they are both on the
/// page's grid, and a grid measured per frame would put each frame on a rhythm
/// of its own.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BaselineGrid {
    /// Where the first line of the grid sits, below the top of the page.
    pub start: f64,
    /// How far apart the rest are. Kept positive; a step of zero or less is a
    /// grid with every line in one place, which no caller could use.
    pub step: f64,
}

impl BaselineGrid {
    /// The first grid line at or below `y`, in the same space `y` is in.
    ///
    /// Down rather than to the nearest, which is what locking to a grid means:
    /// a line that does not fit its slot takes the next one, and text never
    /// rides up into the line above it.
    pub fn snap(&self, y: f64, origin: f64) -> f64 {
        let step = self.step.max(f64::EPSILON);
        let first = origin + self.start;
        let steps = ((y - first) / step).ceil().max(0.0);
        first + steps * step
    }
}

/// One of the document's named colours.
///
/// A swatch is a name and a value. Objects store the **name**, so editing the
/// value here changes every one of them at once — which is the whole reason a
/// swatch exists and the thing a copied colour cannot do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub colour: Color,
    /// Whether this names an ink of its own rather than a mix of the process
    /// colours.
    ///
    /// A spot separates onto its own plate at the printer, so it is a fact
    /// about the job and not only about the screen. Recorded here so preflight
    /// and the PDF's separation list can report it in milestone 6.
    #[serde(default)]
    pub spot: bool,
}

impl Swatch {
    pub fn new(name: impl Into<String>, colour: Color) -> Self {
        Self {
            name: name.into(),
            colour,
            spot: false,
        }
    }
}

/// How text runs around an object.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum TextWrap {
    /// Text runs straight over it, which is what every object did before this
    /// existed and what most objects should keep doing.
    #[default]
    None,
    /// Text keeps clear of the object's box, plus a standoff on each side.
    ///
    /// The box rather than the shape. Wrapping to a contour needs the outline
    /// and a way to intersect it with each line, which is a different piece of
    /// work; this is InDesign's "wrap around bounding box" and is what most
    /// wraps actually are.
    Bounds { standoff: Insets },
}

impl TextWrap {
    /// The standoff, or nothing when the object does not wrap.
    pub fn standoff(&self) -> Option<Insets> {
        match self {
            TextWrap::None => None,
            TextWrap::Bounds { standoff } => Some(*standoff),
        }
    }
}

/// Where text sits in a frame taller than the text needs.
///
/// InDesign calls this vertical justification. `Justify` is the one that does
/// real work: it spreads the lines to fill the frame, which is how facing
/// pages are made to align at the foot as well as the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VerticalJustify {
    #[default]
    Top,
    Centre,
    Bottom,
    Justify,
}

/// How a text frame lays its story out.
///
/// On the `Text` variant rather than on `Frame`, because none of it means
/// anything for a rectangle: a frame's kind is what decides whether a column
/// count is a fact about it or a nonsense.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    /// How many columns the text flows through. At least one.
    pub columns: u8,
    /// The space between them.
    pub gutter: f64,
    /// The margin inside the frame, before the text starts.
    pub inset: Insets,
    pub vertical: VerticalJustify,
    /// Whether this frame's lines sit on the document's baseline grid.
    ///
    /// Per frame rather than per document, because a caption or a pull quote
    /// is exactly the thing that should *not* be on the grid the body text is
    /// on.
    #[serde(default)]
    pub lock_to_grid: bool,
    /// The frame this one overflows into.
    ///
    /// A **forward** link only. The frame before is found by looking for
    /// whoever points here, which is a scan of the document — cheap at these
    /// sizes, and it means a chain has one description rather than two that
    /// can disagree. Two links pointing at each other is a class of bug this
    /// codebase has already paid for twice.
    #[serde(default)]
    pub next: Option<FrameId>,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            columns: 1,
            // A pica, the traditional gutter, and wide enough that two columns
            // of text read as two columns rather than as one with a crack in
            // it.
            gutter: 12.0,
            inset: Insets::default(),
            vertical: VerticalJustify::Top,
            lock_to_grid: false,
            next: None,
        }
    }
}

/// Divide an area into `columns` with `gutter` between them.
///
/// The one place that knows how columns divide, used by a text frame and by a
/// page's column guides alike. Two implementations would eventually disagree,
/// and a frame that did not line up with the guides it was drawn against would
/// be a very confusing thing to debug.
///
/// A gutter wide enough to swallow the area yields columns of **zero** width
/// rather than negative ones: text cannot be laid out in a negative measure,
/// and clamping here means nothing downstream has to check.
pub fn divide_into_columns(area: DocRect, columns: u8, gutter: f64) -> Vec<DocRect> {
    let count = columns.max(1);
    let gutters = gutter * f64::from(count - 1);
    let each = ((area.width - gutters) / f64::from(count)).max(0.0);

    (0..count)
        .map(|i| DocRect {
            x: area.x + f64::from(i) * (each + gutter),
            y: area.y,
            width: each,
            height: area.height,
        })
        .collect()
}

impl TextLayout {
    /// The columns, in the frame's own space.
    ///
    /// The inset comes off first and the gutters out of what is left, so a
    /// frame's columns always add up to its width however either is set. A
    /// gutter wide enough to swallow the frame yields columns of **zero**
    /// width rather than negative ones — text cannot be laid out in a negative
    /// measure, and clamping here means nothing downstream has to check.
    pub fn columns_of(&self, bounds: DocRect) -> Vec<DocRect> {
        let inner = DocRect {
            x: bounds.x + self.inset.left,
            y: bounds.y + self.inset.top,
            width: (bounds.width - self.inset.left - self.inset.right).max(0.0),
            height: (bounds.height - self.inset.top - self.inset.bottom).max(0.0),
        };

        divide_into_columns(inner, self.columns, self.gutter)
    }
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
    /// How many columns the page's guides divide the type area into.
    ///
    /// Guides only: they are furniture to lay out against and to snap to, and
    /// they do not make a text frame multi-column any more than a ruler guide
    /// does. A frame's own columns are on [`TextLayout`].
    #[serde(default)]
    pub columns: u8,
    /// The space between those guides.
    #[serde(default)]
    pub column_gutter: f64,

    /// The grid every locked line sits on, if the document has one.
    ///
    /// `None` rather than a zero step, and the difference is the point: a
    /// document without a grid has no grid, and a step of zero would be a
    /// grid whose lines are all in the same place.
    #[serde(default)]
    pub baseline_grid: Option<BaselineGrid>,
    pub margins: Margins,
    pub bleed: Insets,
    pub slug: Insets,
    pub facing_pages: bool,
}

/// One sheet, positioned in document space.
///
/// A page holds no objects. What is *on* a page is whatever lands on it, which
/// is why moving a frame across the fold needs no bookkeeping — see
/// [`Document::page_of_frame`](crate::document::Document::page_of_frame).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub bounds: DocRect,
    /// The parent page whose items appear on this one.
    ///
    /// A **reference**, never a copy. A master whose items were copied onto
    /// each page would not update those pages when it changed, which is the
    /// entire reason to have one. `serde(default)` reads as no parent, which
    /// is the truth about a document written before masters existed.
    #[serde(default)]
    pub master: Option<PageId>,
}

impl Page {
    /// A page at `bounds`, with no parent.
    pub fn at(bounds: DocRect) -> Self {
        Self {
            bounds,
            master: None,
        }
    }
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

    // --- columns ------------------------------------------------------------

    fn frame(w: f64, h: f64) -> DocRect {
        DocRect {
            x: 10.0,
            y: 20.0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn one_column_is_the_frame_itself() {
        let columns = TextLayout::default().columns_of(frame(200.0, 100.0));
        assert_eq!(columns, vec![frame(200.0, 100.0)]);
    }

    #[test]
    fn two_columns_share_the_width_with_a_gutter_between_them() {
        let layout = TextLayout {
            columns: 2,
            gutter: 20.0,
            ..TextLayout::default()
        };
        let columns = layout.columns_of(frame(220.0, 100.0));

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].width, 100.0);
        assert_eq!(columns[1].width, 100.0);
        assert_eq!(
            columns[1].x - (columns[0].x + columns[0].width),
            20.0,
            "the gutter sits between them"
        );
    }

    #[test]
    fn columns_always_add_up_to_the_frame() {
        // Whatever the count and whatever the gutter, the last column ends
        // where the frame does. A column that overhangs is text outside its
        // own frame.
        for count in 1..=6u8 {
            for gutter in [0.0, 6.0, 12.0, 31.7] {
                let layout = TextLayout {
                    columns: count,
                    gutter,
                    ..TextLayout::default()
                };
                let bounds = frame(300.0, 100.0);
                let columns = layout.columns_of(bounds);
                let last = columns.last().expect("at least one");
                assert!(
                    ((last.x + last.width) - (bounds.x + bounds.width)).abs() < 1e-9,
                    "{count} columns with a {gutter} gutter overhang"
                );
            }
        }
    }

    #[test]
    fn the_inset_comes_off_before_the_columns_are_divided() {
        let layout = TextLayout {
            columns: 2,
            gutter: 10.0,
            inset: Insets {
                top: 5.0,
                bottom: 5.0,
                left: 15.0,
                right: 15.0,
            },
            ..TextLayout::default()
        };
        let columns = layout.columns_of(frame(230.0, 100.0));

        assert_eq!(
            columns[0].x, 25.0,
            "ten for the frame, fifteen for the inset"
        );
        assert_eq!(columns[0].y, 25.0);
        assert_eq!(columns[0].height, 90.0, "top and bottom both come off");
        assert_eq!(columns[0].width, 95.0, "and the gutter out of what is left");
    }

    #[test]
    fn a_gutter_wide_enough_to_swallow_the_frame_yields_no_width_not_a_negative_one() {
        // Text cannot be laid out in a negative measure. Clamping here means
        // nothing downstream has to check for it.
        let layout = TextLayout {
            columns: 3,
            gutter: 500.0,
            ..TextLayout::default()
        };
        for column in layout.columns_of(frame(100.0, 100.0)) {
            assert_eq!(column.width, 0.0);
        }
    }

    #[test]
    fn an_inset_bigger_than_the_frame_yields_no_room_rather_than_a_hole() {
        let layout = TextLayout {
            inset: Insets {
                top: 90.0,
                bottom: 90.0,
                left: 200.0,
                right: 200.0,
            },
            ..TextLayout::default()
        };
        let columns = layout.columns_of(frame(100.0, 100.0));
        assert_eq!(columns[0].width, 0.0);
        assert_eq!(columns[0].height, 0.0);
    }

    #[test]
    fn a_column_count_of_zero_is_read_as_one() {
        // A frame with no columns could hold no text, which is not a layout
        // anybody means to ask for — and it would divide by zero on the way.
        let layout = TextLayout {
            columns: 0,
            ..TextLayout::default()
        };
        assert_eq!(layout.columns_of(frame(100.0, 50.0)).len(), 1);
    }

    #[test]
    fn a_new_text_frame_is_one_column_aligned_to_the_top() {
        let layout = TextLayout::default();
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.vertical, VerticalJustify::Top);
        assert_eq!(layout.inset, Insets::default());
        assert_eq!(layout.gutter, 12.0, "a pica, the traditional gutter");
    }
}

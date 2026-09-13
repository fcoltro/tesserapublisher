//! Icons, as geometry rather than assets.
//!
//! The shapes are [Lucide](https://lucide.dev) — drawn on a 24×24 grid with a
//! a light round-capped stroke — stored here as SVG path data, parsed by `kurbo`
//! (already a dependency), and painted through `egui::Painter`.
//!
//! No image files, no SVG renderer, no icon font. The icons stay crisp at any
//! DPI, re-tint with [`crate::theme`], and add nothing to the binary but a few
//! hundred bytes of text.
//!
//! Lucide is ISC-licensed; icons inherited from Feather are MIT. See
//! `ATTRIBUTION.md`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use kurbo::{BezPath, PathEl};

/// The grid Lucide draws on.
const GRID: f32 = 24.0;
/// Interface stroke width in grid units, lighter than Lucide's default 2.
const STROKE: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Sun,
    Moon,
    Select,
    DirectSelect,
    PictureFrame,
    Polygon,
    Scissors,
    Properties,
    Pages,
    Preflight,
    Swatches,
    Styles,
    Close,
    Rectangle,
    Ellipse,
    Line,
    Pen,
    Text,
    Hand,
    /// The pan tool mid-drag: fingers curled, as `grab` draws them.
    Grab,
    Rotate,
    Move,
    /// One icon for all eight resize handles, turned to point along the
    /// handle's own normal. See [`paint_rotated`].
    Scale,
    TextCursor,
    /// The type tool before anything is drawn: a frame waiting to be dragged.
    TextFrame,
    Crosshair,

    // The canvas toolbar's spatial verbs. Named for what they do here rather
    // than for Lucide's own name, which describes the divider's axis; each
    // one's source glyph is noted where its path data is.
    AlignLeft,
    AlignCentreH,
    AlignRight,
    AlignTop,
    AlignMiddleV,
    AlignBottom,
    DistributeH,
    DistributeV,
    FlipHorizontal,
    FlipVertical,
    RotateCw,
    RotateCcw,

    // Typography: the inspector's character and paragraph controls, and the
    // styles window.
    Bold,
    Italic,
    AlignJustify,
    TextAlignLeft,
    TextAlignCentre,
    TextAlignRight,
    TextAlignJustify,
    Palette,
    /// A paragraph mark, for the paragraph half of the text controls.
    Pilcrow,
    /// Aa, for the character half.
    CaseSensitive,
    /// Two letters at two sizes: the size control.
    TypeSize,
    Plus,
    Duplicate,
    PlaceImage,
    Trash,
    /// Navigation, and the disclosure a submenu shows.
    ChevronLeft,
    ChevronRight,
    /// The layers panel, and what a layer's two switches look like.
    Layers,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    /// Two fields that move together, and the same chain broken. Distinct from
    /// [`Icon::Lock`], which is about permission rather than about linkage:
    /// a locked layer cannot be touched, linked margins simply change as one.
    Link2,
    Unlink2,

    // The fill and stroke proxy, and the status bar's zoom.
    Swap,
    NoFill,
    Blend,
    ZoomIn,
    ZoomOut,
    ZoomFit,
}

impl Icon {
    /// SVG path data, in the 24×24 Lucide grid.
    ///
    /// Lucide's `<rect>` and `<circle>` primitives are written out as paths
    /// here so that everything goes through one parser.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
            Self::Sun => &[
                "M16 12 A4 4 0 1 1 8 12 A4 4 0 1 1 16 12 Z",
                "M12 2 V4 M12 20 V22 M2 12 H4 M20 12 H22 M4.93 4.93 L6.34 6.34 M17.66 17.66 L19.07 19.07 M4.93 19.07 L6.34 17.66 M17.66 6.34 L19.07 4.93",
            ],
            Self::Moon => &["M20.9 13 A9 9 0 0 1 11 3.1 A7 7 0 0 0 20.9 13 Z"],
            // Tessera: an arrow selecting an individual anchor.
            Self::DirectSelect => &[
                "M4 3 L4 18 L8 14 L11 21 L14 19 L11 13 L17 13 Z",
                "M18 3 H22 V7 H18 Z",
            ],
            // Publishing convention: an empty picture frame has diagonals.
            Self::PictureFrame => &["M3 3 H21 V21 H3 Z", "M3 3 L21 21", "M21 3 L3 21"],
            Self::Polygon => &["M12 2 L22 8 V16 L12 22 L2 16 V8 Z"],
            // Lucide: scissors.
            Self::Scissors => &[
                "M9 6 A3 3 0 1 1 3 6 A3 3 0 1 1 9 6 Z",
                "M9 18 A3 3 0 1 1 3 18 A3 3 0 1 1 9 18 Z",
                "M8.12 8.12 L20 20",
                "M14 10 L20 4",
                "M8.12 15.88 L12 12",
            ],
            // Sliders, page sheets, a checklist and a swatch grid identify panels.
            Self::Properties => &[
                "M3 6 H8 M8 3 V9 M8 6 H21",
                "M3 12 H16 M16 9 V15 M16 12 H21",
                "M3 18 H10 M10 15 V21 M10 18 H21",
            ],
            // lucide: file-text
            Self::Pages => &[
                "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z",
                "M14 2v4a2 2 0 0 0 2 2h4",
                "M16 13H8",
                "M16 17H8",
                "M10 9H8",
            ],
            // A type specimen sheet: the panel contains text and object styles.
            Self::Styles => &[
                "M4 3 H20 V21 H4 Z",
                "M8 14 L12 6 L16 14",
                "M10 11 H14",
                "M8 18 H16",
            ],
            Self::Close => &["M5 5 L19 19", "M19 5 L5 19"],
            // lucide: list-checks
            Self::Preflight => &[
                "m3 17 2 2 4-4",
                "m3 7 2 2 4-4",
                "M13 6h8",
                "M13 12h8",
                "M13 18h8",
            ],
            Self::Swatches => &[
                "M3 3 H9 V18 A3 3 0 0 1 3 18 Z",
                "M9 8 L14 3 L19 8 L9 18",
                "M13 15 H21 V21 H6",
            ],
            // lucide: mouse-pointer-2
            Self::Select => &[
                "M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z",
            ],
            // lucide: square — <rect width=18 height=18 x=3 y=3 rx=2>
            Self::Rectangle => &[
                "M5 3 h14 a2 2 0 0 1 2 2 v14 a2 2 0 0 1 -2 2 h-14 a2 2 0 0 1 -2 -2 v-14 a2 2 0 0 1 2 -2 z",
            ],
            // lucide: circle — <circle cx=12 cy=12 r=10>
            Self::Ellipse => &["M22 12 A10 10 0 1 1 2 12 A10 10 0 1 1 22 12 Z"],
            // lucide: image-plus — an image frame with a plus, which is
            // placing art rather than the artwork itself. The <circle> is
            // written as an arc pair, as everything else here is.
            Self::PlaceImage => &[
                "M16 5h6",
                "M19 2v6",
                "M21 11.5V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7.5",
                "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21",
                "M11 9 A2 2 0 1 1 7 9 A2 2 0 1 1 11 9 Z",
            ],
            // lucide: slash
            Self::Line => &["M22 2 2 22"],
            // lucide: blend — two overlapping circles, which is compositing
            // drawn rather than named.
            Self::Blend => &[
                "M16 9 A7 7 0 1 1 2 9 A7 7 0 1 1 16 9 Z",
                "M22 15 A7 7 0 1 1 8 15 A7 7 0 1 1 22 15 Z",
            ],
            // lucide: pen-tool
            Self::Pen => &[
                "M15.707 21.293a1 1 0 0 1-1.414 0l-1.586-1.586a1 1 0 0 1 0-1.414l5.586-5.586a1 1 0 0 1 1.414 0l1.586 1.586a1 1 0 0 1 0 1.414z",
                "m18 13-1.375-6.874a1 1 0 0 0-.746-.776L3.235 2.028a1 1 0 0 0-1.207 1.207L5.35 15.879a1 1 0 0 0 .776.746L13 18",
                "m2.3 2.3 7.286 7.286",
                "M13 11 A2 2 0 1 1 9 11 A2 2 0 1 1 13 11 Z",
            ],
            // lucide: type
            Self::Text => &[
                "M12 4v16",
                "M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2",
                "M9 20h6",
            ],
            // lucide: hand
            Self::Hand => &[
                "M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2",
                "M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2",
                "M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8",
                "M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15",
            ],
            // lucide: grab — the closed-up hand a pan drag shows.
            //
            // Lucide writes the palm as one `d` with an implicit repeated arc
            // command. It is split in two here rather than trusting every SVG
            // parser to carry the command across.
            Self::Grab => &[
                "M18 11.5V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v1.4",
                "M14 10V8a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2",
                "M10 9.9V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v5",
                "M6 14a2 2 0 0 0-2-2a2 2 0 0 0-2 2",
                "M18 11a2 2 0 1 1 4 0v3a8 8 0 0 1-8 8h-4a8 8 0 0 1-8-8",
                "M2 14a2 2 0 1 1 4 0",
            ],
            // lucide: rotate-cw
            Self::Rotate => &[
                "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8",
                "M21 3v5h-5",
            ],
            // lucide: move
            Self::Move => &[
                "M12 2v20",
                "M2 12h20",
                "m15 19-3 3-3-3",
                "m15 5-3-3-3 3",
                "m19 9 3 3-3 3",
                "m5 9-3 3 3 3",
            ],
            // lucide: move-horizontal. Drawn along +x and rotated to the
            // handle's outward normal, so a rotated frame gets a cursor that
            // actually points the way the edge will travel.
            Self::Scale => &["m18 8 4 4-4 4", "M2 12h20", "m6 8-4 4 4 4"],
            // lucide: text-cursor
            Self::TextCursor => &[
                "M17 22h-1a4 4 0 0 1-4-4V6a4 4 0 0 1 4-4h1",
                "M7 22h1a4 4 0 0 0 4-4v-1",
                "M7 2h1a4 4 0 0 1 4 4v1",
            ],
            // lucide: square-dashed-mouse-pointer
            Self::TextFrame => &[
                "M12.034 12.681a.498.498 0 0 1 .647-.647l9 3.5a.5.5 0 0 1-.033.943l-3.444 1.068a1 1 0 0 0-.66.66l-1.067 3.443a.5.5 0 0 1-.943.033z",
                "M5 3a2 2 0 0 0-2 2",
                "M19 3a2 2 0 0 1 2 2",
                "M5 21a2 2 0 0 1-2-2",
                "M9 3h1",
                "M9 21h2",
                "M14 3h1",
                "M3 9v1",
                "M21 9v2",
                "M3 14v1",
            ],
            // lucide: crosshair
            Self::Crosshair => &[
                "M22 12 A10 10 0 1 1 2 12 A10 10 0 1 1 22 12 Z",
                "M22 12h-4",
                "M6 12H2",
                "M12 6V2",
                "M12 18v4",
            ],

            // lucide: align-start-vertical
            Self::AlignLeft => &[
                "M8 14 h5 a2 2 0 0 1 2 2 v2 a2 2 0 0 1 -2 2 h-5 a2 2 0 0 1 -2 -2 v-2 a2 2 0 0 1 2 -2 z",
                "M8 4 h12 a2 2 0 0 1 2 2 v2 a2 2 0 0 1 -2 2 h-12 a2 2 0 0 1 -2 -2 v-2 a2 2 0 0 1 2 -2 z",
                "M2 2v20",
            ],
            // lucide: align-center-vertical
            Self::AlignCentreH => &[
                "M12 2v20",
                "M8 10H4a2 2 0 0 1-2-2V6c0-1.1.9-2 2-2h4",
                "M16 10h4a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-4",
                "M8 20H7a2 2 0 0 1-2-2v-2c0-1.1.9-2 2-2h1",
                "M16 14h1a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2h-1",
            ],
            // lucide: align-end-vertical
            Self::AlignRight => &[
                "M4 4 h12 a2 2 0 0 1 2 2 v2 a2 2 0 0 1 -2 2 h-12 a2 2 0 0 1 -2 -2 v-2 a2 2 0 0 1 2 -2 z",
                "M11 14 h5 a2 2 0 0 1 2 2 v2 a2 2 0 0 1 -2 2 h-5 a2 2 0 0 1 -2 -2 v-2 a2 2 0 0 1 2 -2 z",
                "M22 22V2",
            ],
            // lucide: align-start-horizontal
            Self::AlignTop => &[
                "M6 6 h2 a2 2 0 0 1 2 2 v12 a2 2 0 0 1 -2 2 h-2 a2 2 0 0 1 -2 -2 v-12 a2 2 0 0 1 2 -2 z",
                "M16 6 h2 a2 2 0 0 1 2 2 v5 a2 2 0 0 1 -2 2 h-2 a2 2 0 0 1 -2 -2 v-5 a2 2 0 0 1 2 -2 z",
                "M22 2H2",
            ],
            // lucide: align-center-horizontal
            Self::AlignMiddleV => &[
                "M2 12h20",
                "M10 16v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-4",
                "M10 8V4a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v4",
                "M20 16v1a2 2 0 0 1-2 2h-2a2 2 0 0 1-2-2v-1",
                "M14 8V7c0-1.1.9-2 2-2h2a2 2 0 0 1 2 2v1",
            ],
            // lucide: align-end-horizontal
            Self::AlignBottom => &[
                "M6 2 h2 a2 2 0 0 1 2 2 v12 a2 2 0 0 1 -2 2 h-2 a2 2 0 0 1 -2 -2 v-12 a2 2 0 0 1 2 -2 z",
                "M16 9 h2 a2 2 0 0 1 2 2 v5 a2 2 0 0 1 -2 2 h-2 a2 2 0 0 1 -2 -2 v-5 a2 2 0 0 1 2 -2 z",
                "M22 22H2",
            ],
            // lucide: align-horizontal-distribute-center
            Self::DistributeH => &[
                "M6 5 h2 a2 2 0 0 1 2 2 v10 a2 2 0 0 1 -2 2 h-2 a2 2 0 0 1 -2 -2 v-10 a2 2 0 0 1 2 -2 z",
                "M16 7 h2 a2 2 0 0 1 2 2 v6 a2 2 0 0 1 -2 2 h-2 a2 2 0 0 1 -2 -2 v-6 a2 2 0 0 1 2 -2 z",
                "M17 22v-5",
                "M17 7V2",
                "M7 22v-3",
                "M7 5V2",
            ],
            // lucide: align-vertical-distribute-center
            Self::DistributeV => &[
                "M22 17h-3",
                "M22 7h-5",
                "M5 17H2",
                "M7 7H2",
                "M7 14 h10 a2 2 0 0 1 2 2 v2 a2 2 0 0 1 -2 2 h-10 a2 2 0 0 1 -2 -2 v-2 a2 2 0 0 1 2 -2 z",
                "M9 4 h6 a2 2 0 0 1 2 2 v2 a2 2 0 0 1 -2 2 h-6 a2 2 0 0 1 -2 -2 v-2 a2 2 0 0 1 2 -2 z",
            ],
            // lucide: flip-horizontal
            Self::FlipHorizontal => &[
                "M8 3H5a2 2 0 0 0-2 2v14c0 1.1.9 2 2 2h3",
                "M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3",
                "M12 20v2",
                "M12 14v2",
                "M12 8v2",
                "M12 2v2",
            ],
            // lucide: flip-vertical
            Self::FlipVertical => &[
                "M21 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v3",
                "M21 16v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3",
                "M4 12H2",
                "M10 12H8",
                "M16 12h-2",
                "M22 12h-2",
            ],
            // lucide: rotate-cw
            Self::RotateCw => &[
                "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8",
                "M21 3v5h-5",
            ],
            // lucide: rotate-ccw
            Self::RotateCcw => &[
                "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8",
                "M3 3v5h5",
            ],
            // lucide: arrow-left-right
            // lucide: bold
            Self::Bold => {
                &["M6 12h9a4 4 0 0 1 0 8H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h7a4 4 0 0 1 0 8"]
            }
            // lucide: italic — three <line> elements, written as paths
            Self::Italic => &["M19 4 10 4", "M14 20 5 20", "M15 4 9 20"],
            // lucide: align-justify
            Self::AlignJustify => &["M3 5h18", "M3 12h18", "M3 19h18"],
            // The paragraph alignments. **Not the object ones**, which is what
            // was drawn here: `align-left` for an object is two bars pushed
            // against a rule, and for text it is ragged lines of type. They
            // mean different things and the panel was showing the wrong one.
            // lucide: align-left
            Self::TextAlignLeft => &["M15 12H3", "M17 18H3", "M21 6H3"],
            // lucide: align-center
            Self::TextAlignCentre => &["M17 12H7", "M19 18H5", "M21 6H3"],
            // lucide: align-right
            Self::TextAlignRight => &["M21 12H9", "M21 18H7", "M21 6H3"],
            // lucide: align-justify
            Self::TextAlignJustify => &["M3 12h18", "M3 18h18", "M3 6h18"],
            // lucide: palette — four <circle> dots written as arc pairs
            Self::Palette => &[
                "M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z",
                "M14 6.5 A0.5 0.5 0 1 1 13 6.5 A0.5 0.5 0 1 1 14 6.5 Z",
                "M18 10.5 A0.5 0.5 0 1 1 17 10.5 A0.5 0.5 0 1 1 18 10.5 Z",
                "M7 12.5 A0.5 0.5 0 1 1 6 12.5 A0.5 0.5 0 1 1 7 12.5 Z",
                "M9 7.5 A0.5 0.5 0 1 1 8 7.5 A0.5 0.5 0 1 1 9 7.5 Z",
            ],
            // lucide: pilcrow
            Self::Pilcrow => &["M13 4v16", "M17 4v16", "M19 4H9.5a4.5 4.5 0 0 0 0 9H13"],
            // lucide: case-sensitive
            Self::CaseSensitive => &[
                "m2 16 4.039-9.69a.5.5 0 0 1 .923 0L11 16",
                "M22 9v7",
                "M3.304 13h6.392",
                "M22 12.5 A3.5 3.5 0 1 1 15 12.5 A3.5 3.5 0 1 1 22 12.5 Z",
            ],
            // lucide: a-large-small
            Self::TypeSize => &[
                "m15 16 2.536-7.328a1.02 1.02 1 0 1 1.928 0L22 16",
                "M15.697 14h5.606",
                "m2 16 4.039-9.69a.5.5 0 0 1 .923 0L11 16",
                "M3.304 13h6.392",
            ],
            // lucide: plus
            Self::Plus => &["M5 12h14", "M12 5v14"],
            // lucide: copy — <rect width=14 height=14 x=8 y=8 rx=2> as a path
            Self::Duplicate => &[
                "M10 8 h10 a2 2 0 0 1 2 2 v10 a2 2 0 0 1 -2 2 h-10 a2 2 0 0 1 -2 -2 v-10 a2 2 0 0 1 2 -2 z",
                "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
            ],
            // lucide: chevron-left
            Self::ChevronLeft => &["m15 18-6-6 6-6"],
            // lucide: chevron-right
            Self::ChevronRight => &["m9 18 6-6-6-6"],
            // lucide: layers
            Self::Layers => &[
                "M12 3 L21 7.5 L12 12 L3 7.5 Z",
                "M3 12 L12 16.5 L21 12",
                "M3 16.5 L12 21 L21 16.5",
            ],
            // lucide: eye
            Self::Eye => &[
                "M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0",
                "M15 12 A3 3 0 1 1 9 12 A3 3 0 1 1 15 12 Z",
            ],
            // lucide: eye-off
            Self::EyeOff => &[
                "M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49",
                "M14.084 14.158a3 3 0 0 1-4.242-4.242",
                "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143",
                "m2 2 20 20",
            ],
            // lucide: lock — <rect width=18 height=11 x=3 y=11 rx=2> as a path
            Self::Lock => &[
                "M5 11 h14 a2 2 0 0 1 2 2 v7 a2 2 0 0 1 -2 2 h-14 a2 2 0 0 1 -2 -2 v-7 a2 2 0 0 1 2 -2 z",
                "M7 11V7a5 5 0 0 1 10 0v4",
            ],
            // lucide: lock-open
            Self::Unlock => &[
                "M5 11 h14 a2 2 0 0 1 2 2 v7 a2 2 0 0 1 -2 2 h-14 a2 2 0 0 1 -2 -2 v-7 a2 2 0 0 1 2 -2 z",
                "M7 11V7a5 5 0 0 1 9.9-1",
            ],
            // lucide: link-2
            Self::Link2 => &[
                "M9 17H7A5 5 0 0 1 7 7h2",
                "M15 7h2a5 5 0 1 1 0 10h-2",
                "M8 12 H16",
            ],
            // lucide: unlink-2 — link-2 without the bar joining the two rings.
            Self::Unlink2 => &["M9 17H7A5 5 0 0 1 7 7h2", "M15 7h2a5 5 0 1 1 0 10h-2"],
            // lucide: trash-2
            Self::Trash => &[
                "M10 11v6",
                "M14 11v6",
                "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6",
                "M3 6h18",
                "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
            ],
            Self::Swap => &["M8 3 4 7l4 4", "M4 7h16", "m16 21 4-4-4-4", "M20 17H4"],
            // lucide: ban — the universal "none", and what a swatch of no
            // colour has been in every drawing tool since MacPaint.
            Self::NoFill => &[
                "M22 12 A10 10 0 1 1 2 12 A10 10 0 1 1 22 12 Z",
                "M4.929 4.929 19.07 19.071",
            ],
            // lucide: zoom-in
            Self::ZoomIn => &[
                "M19 11 A8 8 0 1 1 3 11 A8 8 0 1 1 19 11 Z",
                "M21 21 L16.65 16.65",
                "M11 8 L11 14",
                "M8 11 L14 11",
            ],
            // lucide: zoom-out
            Self::ZoomOut => &[
                "M19 11 A8 8 0 1 1 3 11 A8 8 0 1 1 19 11 Z",
                "M21 21 L16.65 16.65",
                "M8 11 L14 11",
            ],
            // lucide: maximize
            Self::ZoomFit => &[
                "M8 3H5a2 2 0 0 0-2 2v3",
                "M21 8V5a2 2 0 0 0-2-2h-3",
                "M3 16v3a2 2 0 0 0 2 2h3",
                "M16 21h3a2 2 0 0 0 2-2v-3",
            ],
        }
    }

    /// The point in the 24-unit grid that must sit under the pointer.
    ///
    /// A cursor is not its bounding box: an arrow points from its tip, a
    /// crosshair from its centre, a text bar from the middle of its stem.
    /// Painting every icon centred would put the arrow's tip a dozen pixels
    /// down and to the right of what the click actually hits.
    pub fn hotspot(self) -> (f32, f32) {
        match self {
            // The arrow's tip, where `mouse-pointer-2` starts its outline.
            Self::Select => (4.3, 4.3),
            Self::DirectSelect => (4.0, 3.0),
            // The nib, not the barrel — and Lucide's `pen-tool` points up and
            // to the LEFT, where its outline turns the sharp corner at about
            // (2.3, 2.3). Reading the nib as the bottom-left corner put the
            // whole icon a full grid away from the point it draws from.
            Self::Pen => (2.3, 2.3),
            Self::Rectangle
            | Self::Ellipse
            | Self::Line
            | Self::Text
            | Self::Hand
            | Self::Grab
            | Self::Rotate
            | Self::Move
            | Self::Scale
            | Self::TextCursor
            | Self::TextFrame
            | Self::Crosshair
            // The toolbar's verbs are never cursors, so their hotspot is only
            // ever the centre. Listed rather than caught by a wildcard, so
            // that adding a cursor icon later still has to answer this.
            | Self::AlignLeft
            | Self::AlignCentreH
            | Self::AlignRight
            | Self::AlignTop
            | Self::AlignMiddleV
            | Self::AlignBottom
            | Self::DistributeH
            | Self::DistributeV
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::RotateCw
            | Self::RotateCcw
            | Self::Swap
            | Self::NoFill
            | Self::Blend
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomFit
            // Typography glyphs sit in buttons, never under the pointer.
            | Self::Bold
            | Self::Italic
            | Self::AlignJustify
            | Self::TextAlignLeft
            | Self::TextAlignCentre
            | Self::TextAlignRight
            | Self::TextAlignJustify
            | Self::Palette
            | Self::Pilcrow
            | Self::CaseSensitive
            | Self::TypeSize
            | Self::Plus
            | Self::Duplicate
            | Self::PlaceImage
            | Self::Trash
            | Self::ChevronLeft
            | Self::ChevronRight
            | Self::Layers
            | Self::Eye
            | Self::EyeOff
            | Self::Lock
            | Self::Unlock
            | Self::Link2
            | Self::Unlink2
            | Self::PictureFrame
            | Self::Polygon
            | Self::Scissors
            | Self::Properties
            | Self::Pages
            | Self::Preflight
            | Self::Swatches
            | Self::Styles
            | Self::Close
            | Self::Sun
            | Self::Moon => (12.0, 12.0),
        }
    }
}

/// Parsed path data, built on first use and shared thereafter.
///
/// The paths are static text and never change, so parsing them per paint was
/// pure waste — one allocation per icon per frame, and the tool strip alone
/// draws a dozen.
static GEOMETRY: OnceLock<HashMap<Icon, Vec<BezPath>>> = OnceLock::new();

/// Icons that were not in [`ALL`], parsed when first asked for.
///
/// The safety net for the bug this file already had: an icon missing from
/// `ALL` used to draw nothing at all, silently, and the tests iterate `ALL` so
/// they could not see it either. Parsing on demand costs one parse per icon
/// for the life of the process and turns an invisible button into a correct
/// one.
static STRAGGLERS: Mutex<Option<HashMap<Icon, &'static [BezPath]>>> = Mutex::new(None);

impl Icon {
    /// This icon's outlines, in the 24×24 Lucide grid.
    pub fn geometry(self) -> &'static [BezPath] {
        GEOMETRY
            .get_or_init(|| {
                ALL.into_iter()
                    .map(|icon| {
                        let parsed = icon
                            .paths()
                            .iter()
                            .map(|data| {
                                BezPath::from_svg(data).unwrap_or_else(|error| {
                                    // Path data is a compile-time constant in
                                    // this file, so a failure here is a typo
                                    // in the source rather than a runtime
                                    // condition. `every_icon_parses` catches
                                    // it first; naming the icon makes it
                                    // findable if it somehow does not.
                                    panic!("icon {icon:?} has malformed path data: {error}")
                                })
                            })
                            .collect();
                        (icon, parsed)
                    })
                    .collect()
            })
            .get(&self)
            .map(Vec::as_slice)
            .unwrap_or_else(|| self.parsed_late())
    }

    /// Geometry for an icon that was left out of [`ALL`].
    ///
    /// Parsed once and kept, so forgetting the list costs a first draw rather
    /// than the icon. `no_icon_is_missing_from_all` says it should never come
    /// to this.
    fn parsed_late(self) -> &'static [BezPath] {
        let mut cache = STRAGGLERS.lock().expect("the icon cache is not poisoned");
        let cache = cache.get_or_insert_with(HashMap::new);
        cache.entry(self).or_insert_with(|| {
            let parsed: Vec<BezPath> = self
                .paths()
                .iter()
                .filter_map(|data| BezPath::from_svg(data).ok())
                .collect();
            Box::leak(parsed.into_boxed_slice())
        })
    }
}

/// Paint `icon` to fill `rect`, stroked in `color`.
///
/// The icon is scaled uniformly from its 24-unit grid, so the stroke stays
/// proportional and the shape never distorts.
/// Give an icon-only control the name it is known by.
///
/// **The tooltip and the accessible name are one fact.** An icon says nothing
/// on its own: somebody who can see it gets the tooltip, and everybody else
/// gets whatever `WidgetInfo` carries — which, before this, was nothing at all,
/// so every tool, every panel tab and every glyph button announced itself as
/// "button" and stopped. That is worse than silence: it is a control a person
/// can reach, focus and press without ever learning what it does.
///
/// Set in two places they drift, and the one that drifts is always the one
/// nobody can see. So there is one function, and it is the only way an
/// icon-only control in Tessera gets a name.
pub fn named(response: egui::Response, name: impl Into<String>) -> egui::Response {
    let name = name.into();
    let enabled = response.enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    response.on_hover_text(name)
}

/// The same, for a control that is open or shut, on or off.
///
/// `selected` rather than `labeled`: a screen reader says "expanded" or
/// "collapsed" from it, and a disclosure whose state is not announced is one
/// somebody has to press to find out about — which is how you close the section
/// you were trying to open.
pub fn named_toggle(
    response: egui::Response,
    name: impl Into<String>,
    kind: egui::WidgetType,
    on: bool,
) -> egui::Response {
    let name = name.into();
    let enabled = response.enabled();
    response.widget_info(|| egui::WidgetInfo::selected(kind, enabled, on, &name));
    response.on_hover_text(name)
}

/// One click target for an icon and its title, with a shared size and weight.
/// Dock tabs can hide inactive titles and opt into dragging; style tabs keep
/// their titles and use ordinary clicks.
pub fn tab_button(
    ui: &mut egui::Ui,
    icon: Icon,
    title: &str,
    selected: bool,
    show_title: bool,
    sense: egui::Sense,
) -> egui::Response {
    use crate::theme::Theme;
    let label = ui.painter().layout_no_wrap(
        title.to_owned(),
        egui::FontId::proportional(Theme::TYPE_MD),
        Color32::PLACEHOLDER,
    );
    let width = Theme::row()
        + if show_title {
            label.size().x + Theme::space_1()
        } else {
            0.0
        };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, Theme::row()), sense);
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            Theme::RADIUS,
            if selected {
                Theme::selected_bg()
            } else {
                Theme::hover_bg()
            },
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            Theme::RADIUS,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
    let tint = if selected || response.hovered() {
        Theme::text_primary()
    } else {
        Theme::text_muted()
    };
    let icon_rect = Rect::from_min_size(rect.min, egui::Vec2::splat(Theme::row()));
    paint(ui.painter(), icon_rect, icon, tint);
    if show_title {
        ui.painter().galley(
            egui::pos2(
                rect.left() + Theme::row(),
                rect.center().y - label.size().y / 2.0,
            ),
            label,
            tint,
        );
    }
    named_toggle(response, title, egui::WidgetType::SelectableLabel, selected)
}

/// Name a control that already shows its name, but *paints* it.
///
/// **Painted text is visible and not readable.** `Painter::text` puts glyphs on
/// the screen and nothing in the widget tree, so a layer row showing "Artwork"
/// and a section header reading "Geometry" both reach a screen reader as
/// nameless. They need the name and emphatically not a tooltip — one repeating
/// a word already on screen an inch away is noise for everybody who can see it.
///
/// `on` says whether the control is a toggle, and if so which way it is set.
pub fn reads_as(
    response: egui::Response,
    name: impl Into<String>,
    kind: egui::WidgetType,
    on: Option<bool>,
) -> egui::Response {
    let name = name.into();
    let enabled = response.enabled();
    response.widget_info(|| match on {
        Some(on) => egui::WidgetInfo::selected(kind, enabled, on, &name),
        None => egui::WidgetInfo::labeled(kind, enabled, &name),
    });
    response
}

pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = crate::theme::Theme::ICON_SIZE
        .min(rect.width())
        .min(rect.height());
    let rect = Rect::from_center_size(rect.center(), egui::Vec2::splat(side));
    paint_rotated(painter, rect, icon, color, 0.0, 1.0);
}

/// Paint `icon` turned `degrees` clockwise about the centre of `rect`, with
/// its stroke multiplied by `weight`.
///
/// The rotation is what lets one `Scale` icon serve all eight resize handles
/// on a frame at any angle; the weight is what lets a cursor be painted twice,
/// a dark casing under a light stroke, so it reads on both the pasteboard and
/// a white page.
pub fn paint_rotated(
    painter: &Painter,
    rect: Rect,
    icon: Icon,
    color: Color32,
    degrees: f32,
    weight: f32,
) {
    let side = rect.width().min(rect.height());
    let scale = side / GRID;
    let origin = rect.center() - egui::vec2(side / 2.0, side / 2.0);
    let stroke = Stroke::new(STROKE * scale * weight, color);
    let (sin, cos) = degrees.to_radians().sin_cos();
    let pivot = rect.center();

    // Flatten in grid units, then scale — so the tolerance means the same
    // thing regardless of how large the icon is drawn.
    let tolerance = 0.1 / f64::from((scale * painter.ctx().pixels_per_point()).max(f32::EPSILON));

    for path in icon.geometry() {
        let mut run: Vec<Pos2> = Vec::new();
        let flush = |run: &mut Vec<Pos2>, closed: bool| {
            if run.len() > 1 {
                if closed {
                    painter.add(Shape::closed_line(std::mem::take(run), stroke));
                } else {
                    let first = run[0];
                    let last = *run.last().unwrap();
                    painter.add(Shape::line(std::mem::take(run), stroke));
                    // egui paths have butt caps; add the round caps the icon
                    // geometry was designed for, using the same coverage AA.
                    painter.circle_filled(first, stroke.width / 2.0, color);
                    painter.circle_filled(last, stroke.width / 2.0, color);
                }
            } else {
                run.clear();
            }
        };

        kurbo::flatten(path.iter(), tolerance, |el| {
            let at = |p: kurbo::Point| {
                let flat = origin + egui::vec2(p.x as f32 * scale, p.y as f32 * scale);
                let d = flat - pivot;
                pivot + egui::vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
            };
            match el {
                PathEl::MoveTo(p) => {
                    flush(&mut run, false);
                    run.push(at(p));
                }
                PathEl::LineTo(p) => run.push(at(p)),
                PathEl::ClosePath => {
                    if run.first() == run.last() {
                        run.pop();
                    }
                    flush(&mut run, true);
                }
                // `flatten` emits only MoveTo, LineTo and ClosePath.
                PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
            }
        });
        flush(&mut run, false);
    }
}

/// Every icon, for exhaustive tests and for building a palette.
/// Every icon, which is what the geometry cache is built from.
///
/// **An icon missing from this list draws nothing.** Seventeen were once, and
/// the tests could not see it either — they iterate this list, so an icon
/// absent from it was absent from them as well. `geometry` now parses a missing
/// icon rather than returning nothing, so the cost of forgetting is a slower
/// first draw instead of an invisible button; this list is the fast path, not
/// the only one.
pub const ALL: [Icon; 68] = [
    Icon::Sun,
    Icon::Moon,
    Icon::DirectSelect,
    Icon::PictureFrame,
    Icon::Polygon,
    Icon::Scissors,
    Icon::Properties,
    Icon::Pages,
    Icon::Preflight,
    Icon::Swatches,
    Icon::Styles,
    Icon::Close,
    Icon::PlaceImage,
    Icon::TextAlignLeft,
    Icon::TextAlignCentre,
    Icon::TextAlignRight,
    Icon::TextAlignJustify,
    Icon::Select,
    Icon::Rectangle,
    Icon::Ellipse,
    Icon::Line,
    Icon::Pen,
    Icon::Text,
    Icon::Hand,
    Icon::Grab,
    Icon::Rotate,
    Icon::Move,
    Icon::Scale,
    Icon::TextCursor,
    Icon::TextFrame,
    Icon::Crosshair,
    Icon::AlignLeft,
    Icon::AlignCentreH,
    Icon::AlignRight,
    Icon::AlignTop,
    Icon::AlignMiddleV,
    Icon::AlignBottom,
    Icon::DistributeH,
    Icon::DistributeV,
    Icon::FlipHorizontal,
    Icon::FlipVertical,
    Icon::RotateCw,
    Icon::RotateCcw,
    Icon::Bold,
    Icon::Italic,
    Icon::AlignJustify,
    Icon::Palette,
    Icon::Pilcrow,
    Icon::CaseSensitive,
    Icon::TypeSize,
    Icon::Plus,
    Icon::Duplicate,
    Icon::Trash,
    Icon::ChevronLeft,
    Icon::ChevronRight,
    Icon::Layers,
    Icon::Eye,
    Icon::EyeOff,
    Icon::Lock,
    Icon::Unlock,
    Icon::Link2,
    Icon::Unlink2,
    Icon::Swap,
    Icon::NoFill,
    Icon::Blend,
    Icon::ZoomIn,
    Icon::ZoomOut,
    Icon::ZoomFit,
];

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape as _;

    #[test]
    fn every_icon_parses() {
        // `paint` relies on this, so it must be checked rather than assumed.
        for icon in ALL {
            for data in icon.paths() {
                assert!(
                    BezPath::from_svg(data).is_ok(),
                    "{icon:?} has an unparseable path: {data}"
                );
            }
        }
    }

    #[test]
    fn the_cache_parses_every_icon_to_at_least_one_subpath() {
        for icon in ALL {
            let geometry = icon.geometry();
            assert!(
                !geometry.is_empty(),
                "{icon:?} produced no geometry — its path data is malformed"
            );
            for path in geometry {
                assert!(
                    path.elements().len() > 1,
                    "{icon:?} produced an empty subpath"
                );
            }
        }
    }

    #[test]
    fn the_same_icon_hands_back_the_same_allocation() {
        // Parsing on every paint is what this cache exists to stop, so the
        // test pins the pointer rather than the contents.
        let first = Icon::Select.geometry().as_ptr();
        let second = Icon::Select.geometry().as_ptr();
        assert_eq!(first, second);
    }

    #[test]
    fn every_icon_produces_real_geometry() {
        for icon in ALL {
            let segments: usize = icon
                .paths()
                .iter()
                .map(|d| BezPath::from_svg(d).expect("parses").segments().count())
                .sum();
            assert!(segments > 0, "{icon:?} draws nothing");
        }
    }

    #[test]
    fn the_panel_icons_are_the_lucide_glyphs_they_claim_to_be() {
        // Pinned by shape rather than by name, because the name is what was
        // wrong: `Pages` drew two offset sheets, which reads as "duplicate",
        // and `Preflight` drew one tick on a sheet rather than a checklist.
        // file-text has a folded corner and three lines of copy; list-checks
        // has two ticks and three rules and no enclosing box at all.
        assert_eq!(Icon::Pages.paths().len(), 5, "file-text has five subpaths");
        assert_eq!(
            Icon::Preflight.paths().len(),
            5,
            "list-checks has five subpaths"
        );
        assert!(
            Icon::Preflight
                .paths()
                .iter()
                .all(|d| !d.contains("H20 V21")),
            "list-checks is not drawn inside a sheet"
        );
    }

    #[test]
    fn a_link_and_a_broken_link_differ_by_the_bar_between_them() {
        // The whole of what distinguishes them, and the reason unlink-2 is
        // derived from link-2 rather than drawn separately: the two rings are
        // the same, and only the join says whether the fields move together.
        let linked = Icon::Link2.paths();
        let broken = Icon::Unlink2.paths();
        assert_eq!(linked.len(), broken.len() + 1);
        assert_eq!(&linked[..2], broken, "the rings are shared");
        assert!(linked[2].contains("M8 12"), "the bar is the difference");
    }

    #[test]
    fn every_icon_stays_inside_the_lucide_grid() {
        // A path outside 0..24 would be clipped or mis-scaled when painted.
        for icon in ALL {
            for data in icon.paths() {
                let b = BezPath::from_svg(data).expect("parses").bounding_box();
                assert!(
                    b.x0 >= -0.5 && b.y0 >= -0.5 && b.x1 <= 24.5 && b.y1 <= 24.5,
                    "{icon:?} escapes the 24x24 grid: {b:?}"
                );
            }
        }
    }

    /// The icons that point with a tip rather than with their middle.
    const POINTED: [Icon; 3] = [Icon::Select, Icon::DirectSelect, Icon::Pen];

    #[test]
    fn a_pointed_icon_has_its_hotspot_on_its_own_ink() {
        // The bug this pins: the pen's hotspot was read off the wrong corner —
        // inside the icon's bounding box, but nowhere near the nib — so the
        // cursor drew a whole grid away from the point it drew from.
        use kurbo::ParamCurveNearest as _;

        for icon in POINTED {
            let (hx, hy) = icon.hotspot();
            let at = kurbo::Point::new(f64::from(hx), f64::from(hy));
            // Collected first: `segments` borrows the path it walks, so
            // parsing inline would leave it dangling.
            let paths: Vec<BezPath> = icon
                .paths()
                .iter()
                .map(|d| BezPath::from_svg(d).expect("parses"))
                .collect();
            let nearest = paths
                .iter()
                .flat_map(|path| path.segments())
                .map(|seg| seg.nearest(at, 0.01).distance_sq)
                .fold(f64::MAX, f64::min);
            assert!(
                nearest.sqrt() < 1.5,
                "{icon:?} points from ({hx}, {hy}), which is {} units from any ink",
                nearest.sqrt()
            );
        }
    }

    #[test]
    fn every_other_icon_points_from_its_middle() {
        // A cursor that aims from somewhere other than its centre needs a
        // reason, and a test above proving it lands on the ink.
        for icon in ALL {
            if POINTED.contains(&icon) {
                continue;
            }
            assert_eq!(
                icon.hotspot(),
                (12.0, 12.0),
                "{icon:?} aims off-centre without being listed as pointed"
            );
        }
    }

    #[test]
    fn the_arc_based_icons_really_close_into_a_ring() {
        // The circle and the pen's nib are written as SVG arcs. If kurbo's arc
        // handling were wrong they would parse but draw an open sliver, so
        // check the ellipse spans the full grid in both axes.
        let b = BezPath::from_svg(Icon::Ellipse.paths()[0])
            .expect("parses")
            .bounding_box();
        assert!((b.width() - 20.0).abs() < 0.5, "width was {}", b.width());
        assert!((b.height() - 20.0).abs() < 0.5, "height was {}", b.height());
    }

    #[test]
    fn no_icon_is_missing_from_all() {
        // Seventeen were, and nothing noticed: `ALL` feeds the geometry cache
        // *and* every test in this file, so an icon absent from it drew nothing
        // and was absent from its own coverage. The tests were checking the
        // icons that worked.
        //
        // Rust cannot enumerate an enum's variants without a derive, so the
        // count is what is checked. Adding a variant and not adding it here
        // fails this rather than shipping an invisible button.
        assert_eq!(
            ALL.len(),
            68,
            "an icon was added to the enum without being added to ALL"
        );
    }

    #[test]
    fn the_icons_added_for_typography_and_pages_all_draw() {
        // The ones that were missing, named so the failure says which.
        for icon in [
            Icon::Bold,
            Icon::Italic,
            Icon::AlignJustify,
            Icon::Palette,
            Icon::Pilcrow,
            Icon::CaseSensitive,
            Icon::TypeSize,
            Icon::Plus,
            Icon::Duplicate,
            Icon::Trash,
            Icon::ChevronLeft,
            Icon::ChevronRight,
            Icon::Layers,
            Icon::Eye,
            Icon::EyeOff,
            Icon::Lock,
            Icon::Unlock,
        ] {
            assert!(
                !icon.geometry().is_empty(),
                "{icon:?} has no geometry, so it draws nothing"
            );
            assert!(ALL.contains(&icon), "{icon:?} is not in ALL");
        }
    }

    #[test]
    fn an_icon_left_out_of_all_still_draws() {
        // The safety net, exercised directly: geometry comes back even when the
        // cache built from `ALL` does not have it.
        assert!(!Icon::Bold.parsed_late().is_empty());
    }
}

//! Icons, as pictures drawn into the pixels they will cover.
//!
//! Most are Adobe's **Spectrum 2** workflow icons — the set InDesign and
//! Illustrator draw their own interface with — vendored as SVG under
//! `assets/icons/` (Apache-2.0: the licence is beside them, and see
//! `ATTRIBUTION.md`). Where Spectrum has no picture for a page-layout idea —
//! a line cap, a text wrap, a paragraph indent, the I-beam — Tessera draws its
//! own, as stroked path data in [`Icon::paths`], at Spectrum's line weight so
//! the two read as one set.
//!
//! Both are rasterised by resvg at exactly the device pixels they cover and
//! placed on whole pixels. That is what "pixel perfect" means here: an icon
//! is drawn at the size it is shown, never resampled from another, and its
//! edges land where its designer put them rather than straddling two pixels.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use egui::epaint::Mesh;
use egui::{Color32, Painter, Pos2, Rect, Stroke};

/// The grid Tessera's own drawings are made on, Lucide's.
const DRAWN_GRID: f32 = 24.0;
/// Their stroke, in that grid: 1.5 units of Spectrum's 20-unit grid, so a
/// drawn icon and a Spectrum one side by side have the same weight of line.
const DRAWN_STROKE: f32 = 1.5 * DRAWN_GRID / 20.0;

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
    /// The AI console.
    SquareTerminal,
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
    /// The eyedropper.
    Pipette,
    /// The Book panel.
    Book,
    /// The Glyphs panel: a letter that is only a glyph.
    Pi,

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
    Underline,
    Strikethrough,
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
    /// Tessera typography symbols, drawn on the same grid as the Lucide set.
    LineSpacing,
    LetterSpacing,
    BaselineShift,
    OpenType,
    Indent,
    ParagraphSpacing,
    List,
    TabStop,
    DropCap,
    // Stroke samples: show the resulting line end, join, or pattern.
    CapButt,
    CapRound,
    CapSquare,
    JoinMiter,
    JoinRound,
    JoinBevel,
    StrokeSolid,
    StrokeDashed,
    StrokeDotted,
    // Field glyphs: each stands beside a number and says what the number is.
    ScaleX,
    ScaleY,
    Shear,
    Angle,
    IndentLeft,
    IndentRight,
    SpaceBefore,
    SpaceAfter,
    CornerRadius,
    CornerTopLeft,
    CornerTopRight,
    CornerBottomLeft,
    CornerBottomRight,
    Opacity,
    Blur,
    PagePortrait,
    PageLandscape,
    Columns,
    Gutter,
    WrapNone,
    WrapBounds,
    WrapContour,
    WrapJump,
    Plus,
    Duplicate,
    PlaceImage,
    Trash,
    /// Navigation, and the disclosure a submenu shows.
    ChevronLeft,
    ChevronRight,
    /// A section's open-and-shut mark: Spectrum's own small chevron, drawn
    /// at ten pixels rather than a twenty-pixel one shrunk to half.
    Disclosure,
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
    /// Tessera's own drawing, as SVG path data stroked on the 24-unit grid —
    /// empty for an icon Spectrum draws (see [`Icon::spectrum`]).
    ///
    /// Only what Spectrum has no picture of is drawn here: the stroke's caps
    /// and joins, the text wraps, the paragraph indents and spaces, the
    /// pointer's I-beam and crosshair. `<rect>` and `<circle>` primitives are
    /// written out as paths so everything goes through one parser.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
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
            // lucide: crosshair
            Self::Crosshair => &[
                "M22 12 A10 10 0 1 1 2 12 A10 10 0 1 1 22 12 Z",
                "M22 12h-4",
                "M6 12H2",
                "M12 6V2",
                "M12 18v4",
            ],

            Self::LetterSpacing => &[
                "M5 13 L9 3 L13 13 M7 9 H11 M17 3 V13 M15 3 H19",
                "M3 19 H21 M6 16 L3 19 L6 22 M18 16 L21 19 L18 22",
            ],
            Self::Indent => &[
                "M3 4 H21 M10 9 H21 M10 14 H18 M3 20 H21",
                "M2 9 L5 12 L2 15 M2 12 H6",
            ],
            Self::ParagraphSpacing => &[
                "M9 3 H21 M9 7 H18 M9 17 H21 M9 21 H18",
                "M3 8 V16 M1 10 L3 8 L5 10 M1 14 L3 16 L5 14",
            ],
            Self::DropCap => {
                &["M2 18 L7 4 L12 18 M4 13 H10 M15 5 H22 M15 11 H22 M15 17 H22 M2 22 H22"]
            }
            // Tessera: outlined stroke samples. The cap ticks mark the path endpoint.
            Self::CapButt => &["M3 8 H16 V16 H3", "M16 3 V5 M16 19 V21"],
            Self::CapRound => &["M3 8 H16 A4 4 0 0 1 16 16 H3", "M16 3 V5 M16 19 V21"],
            Self::CapSquare => &["M3 8 H20 V16 H3", "M16 3 V5 M16 19 V21"],
            Self::JoinMiter => &["M4 20 V4 H20 V10 H10 V20 Z"],
            Self::JoinRound => &["M4 20 V10 A6 6 0 0 1 10 4 H20 V10 H10 V20 Z"],
            Self::JoinBevel => &["M4 20 V10 L10 4 H20 V10 H10 V20 Z"],
            Self::StrokeDashed => &["M3 12 H7 M10 12 H14 M17 12 H21"],
            // Tessera: field glyphs. A box with the arrow of the axis it scales
            // on; a leaning box for shear; two rays and an arc for an angle.
            Self::ScaleX => &[
                "M3 5 H21 V19 H3 Z",
                "M7 12 H17 M10 9 L7 12 L10 15 M14 9 L17 12 L14 15",
            ],
            Self::ScaleY => &[
                "M3 5 H21 V19 H3 Z",
                "M12 7 V17 M9 10 L12 7 L15 10 M9 14 L12 17 L15 14",
            ],
            Self::Angle => &["M4 20 H21 M4 20 L17 6", "M13 20 A9 9 0 0 0 10.1 13.4"],
            // Lines of text with the arrow on the side that moves; the first
            // and last lines stay put so the indent reads against them.
            Self::IndentLeft => &[
                "M3 4 H21 M11 9 H21 M11 14 H21 M3 19 H21",
                "M4 12 H8 M6 10 L8 12 L6 14",
            ],
            Self::IndentRight => &[
                "M3 4 H21 M3 9 H13 M3 14 H13 M3 19 H21",
                "M20 12 H16 M18 10 L16 12 L18 14",
            ],
            // Lines of text with an arrow arriving from the side the space is on.
            Self::SpaceBefore => &["M3 12 H21 M3 16 H18 M3 20 H21", "M12 3 V9 M9 6 L12 9 L15 6"],
            Self::SpaceAfter => &[
                "M3 4 H21 M3 8 H18 M3 12 H21",
                "M12 21 V15 M9 18 L12 15 L15 18",
            ],
            Self::Columns => &["M3 5 H21 V19 H3 Z", "M12 5 V19"],
            Self::Gutter => &["M3 5 H9 V19 H3 Z M15 5 H21 V19 H15 Z", "M10 12 H14"],
            // Lines of text and the object they meet: through it, stopping at
            // its box, stopping at its shape, or skipping the whole band.
            Self::WrapNone => &[
                "M3 5 H21 M3 10 H21 M3 14 H21 M3 19 H21",
                "M8 8 H16 V16 H8 Z",
            ],
            Self::WrapBounds => &[
                "M3 5 H21 M3 10 H6 M18 10 H21 M3 14 H6 M18 14 H21 M3 19 H21",
                "M8 8 H16 V16 H8 Z",
            ],
            Self::WrapContour => &[
                "M3 5 H21 M3 10 H6.5 M17.5 10 H21 M3 14 H6.5 M17.5 14 H21 M3 19 H21",
                "M12 8 A4 4 0 1 1 12 16 A4 4 0 1 1 12 8 Z",
            ],
            Self::WrapJump => &["M3 5 H21 M3 19 H21", "M8 8 H16 V16 H8 Z"],
            // Drawn by Spectrum; see [`Icon::spectrum`].
            Self::Sun
            | Self::Moon
            | Self::Select
            | Self::DirectSelect
            | Self::PictureFrame
            | Self::Polygon
            | Self::Scissors
            | Self::Properties
            | Self::Pages
            | Self::Preflight
            | Self::Swatches
            | Self::Styles
            | Self::Close
            | Self::SquareTerminal
            | Self::Rectangle
            | Self::Ellipse
            | Self::Line
            | Self::Pen
            | Self::Text
            | Self::Hand
            | Self::Grab
            | Self::Rotate
            | Self::Move
            | Self::TextFrame
            | Self::Pipette
            | Self::Book
            | Self::Pi
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
            | Self::Bold
            | Self::Italic
            | Self::Underline
            | Self::Strikethrough
            | Self::AlignJustify
            | Self::TextAlignLeft
            | Self::TextAlignCentre
            | Self::TextAlignRight
            | Self::TextAlignJustify
            | Self::Palette
            | Self::Pilcrow
            | Self::CaseSensitive
            | Self::TypeSize
            | Self::LineSpacing
            | Self::BaselineShift
            | Self::OpenType
            | Self::List
            | Self::TabStop
            | Self::StrokeSolid
            | Self::StrokeDotted
            | Self::Shear
            | Self::CornerRadius
            | Self::CornerTopLeft
            | Self::CornerTopRight
            | Self::CornerBottomLeft
            | Self::CornerBottomRight
            | Self::Opacity
            | Self::Blur
            | Self::PagePortrait
            | Self::PageLandscape
            | Self::Plus
            | Self::Duplicate
            | Self::PlaceImage
            | Self::Trash
            | Self::ChevronLeft
            | Self::ChevronRight
            | Self::Disclosure
            | Self::Layers
            | Self::Eye
            | Self::EyeOff
            | Self::Lock
            | Self::Unlock
            | Self::Link2
            | Self::Unlink2
            | Self::Swap
            | Self::NoFill
            | Self::Blend
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomFit => &[],
        }
    }

    /// The point in the icon's own grid ([`Icon::grid`]) that must sit
    /// under the pointer.
    ///
    /// A cursor is not its bounding box: an arrow points from its tip, a
    /// crosshair from its centre, a text bar from the middle of its stem.
    /// Painting every icon centred would put the arrow's tip a dozen pixels
    /// down and to the right of what the click actually hits.
    pub fn hotspot(self) -> (f32, f32) {
        match self {
            // The arrow's tip, at the top left of Spectrum's outline.
            Self::Select => (4.4, 2.4),
            // The small arrow's tip, beside the path whose point it picks.
            Self::DirectSelect => (10.3, 8.5),
            // The nib, at the bottom left of `VectorDraw`.
            Self::Pen => (1.9, 18.1),
            // The pipette's tip, at the bottom left of `Eyedropper`.
            Self::Pipette => (2.0, 18.0),
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
            | Self::Underline
            | Self::Strikethrough
            | Self::AlignJustify
            | Self::TextAlignLeft
            | Self::TextAlignCentre
            | Self::TextAlignRight
            | Self::TextAlignJustify
            | Self::Palette
            | Self::Pilcrow
            | Self::CaseSensitive
            | Self::TypeSize
            | Self::LineSpacing
            | Self::LetterSpacing
            | Self::BaselineShift
            | Self::OpenType
            | Self::Indent
            | Self::ParagraphSpacing
            | Self::List
            | Self::TabStop
            | Self::DropCap
            | Self::CapButt
            | Self::CapRound
            | Self::CapSquare
            | Self::JoinMiter
            | Self::JoinRound
            | Self::JoinBevel
            | Self::StrokeSolid
            | Self::StrokeDashed
            | Self::StrokeDotted
            | Self::ScaleX
            | Self::ScaleY
            | Self::Shear
            | Self::Angle
            | Self::IndentLeft
            | Self::IndentRight
            | Self::SpaceBefore
            | Self::SpaceAfter
            | Self::CornerRadius
            | Self::CornerTopLeft
            | Self::CornerTopRight
            | Self::CornerBottomLeft
            | Self::CornerBottomRight
            | Self::Opacity
            | Self::Blur
            | Self::PagePortrait
            | Self::PageLandscape
            | Self::Columns
            | Self::Gutter
            | Self::WrapNone
            | Self::WrapBounds
            | Self::WrapContour
            | Self::WrapJump
            | Self::Plus
            | Self::Duplicate
            | Self::PlaceImage
            | Self::Trash
            | Self::ChevronLeft
            | Self::ChevronRight
            | Self::Disclosure
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
            | Self::SquareTerminal
            | Self::Book
            | Self::Pi
            | Self::Swatches
            | Self::Styles
            | Self::Close
            | Self::Sun
            | Self::Moon => {
                let centre = self.grid() / 2.0;
                (centre, centre)
            }
        }
    }
}

impl Icon {
    /// Adobe's picture of this, when Spectrum 2 has one: the SVG as vendored,
    /// on its 20-unit grid.
    pub fn spectrum(self) -> Option<&'static str> {
        macro_rules! s2 {
            ($name:literal) => {
                Some(include_str!(concat!("../assets/icons/", $name, ".svg")))
            };
        }
        match self {
            Self::Sun => s2!("Lighten"),
            Self::Moon => s2!("Contrast"),
            Self::Select => s2!("Select"),
            Self::DirectSelect => s2!("DirectSelect"),
            Self::PictureFrame => s2!("Image"),
            Self::Polygon => s2!("Polygon6"),
            Self::Scissors => s2!("Cut"),
            Self::Properties => s2!("Properties"),
            Self::Pages => s2!("Files"),
            Self::Preflight => s2!("CheckmarkCircle"),
            Self::Swatches => s2!("ColorHarmony"),
            Self::Styles => s2!("TextParagraph"),
            Self::Close => s2!("Close"),
            Self::SquareTerminal => s2!("Prompt"),
            Self::Rectangle => s2!("Polygon4"),
            Self::Ellipse => s2!("Circle"),
            Self::Line => s2!("Line"),
            Self::Pen => s2!("VectorDraw"),
            Self::Text => s2!("Text"),
            Self::Hand => s2!("Hand"),
            Self::Grab => s2!("Hand"),
            Self::Rotate => s2!("RotateCW"),
            Self::Move => s2!("Move"),
            Self::TextFrame => s2!("Layout"),
            Self::Pipette => s2!("Eyedropper"),
            Self::Book => s2!("Bookmark"),
            Self::Pi => s2!("FontPicker"),
            Self::AlignLeft => s2!("AlignLeft"),
            Self::AlignCentreH => s2!("AlignCenter"),
            Self::AlignRight => s2!("AlignRight"),
            Self::AlignTop => s2!("AlignTop"),
            Self::AlignMiddleV => s2!("AlignMiddle"),
            Self::AlignBottom => s2!("AlignBottom"),
            Self::DistributeH => s2!("DistributeSpaceHorizontally"),
            Self::DistributeV => s2!("DistributeSpaceVertically"),
            Self::FlipHorizontal => s2!("FlipHorizontal"),
            Self::FlipVertical => s2!("FlipVertical"),
            Self::RotateCw => s2!("RotateCW"),
            Self::RotateCcw => s2!("RotateCCW"),
            Self::Bold => s2!("TextBold"),
            Self::Italic => s2!("TextItalic"),
            Self::Underline => s2!("TextUnderline"),
            Self::Strikethrough => s2!("TextStrikeThrough"),
            Self::AlignJustify => s2!("TextAlignJustify"),
            Self::TextAlignLeft => s2!("TextAlignLeft"),
            Self::TextAlignCentre => s2!("TextAlignCenter"),
            Self::TextAlignRight => s2!("TextAlignRight"),
            Self::TextAlignJustify => s2!("TextAlignJustify"),
            Self::Palette => s2!("Color"),
            Self::Pilcrow => s2!("TextParagraph"),
            Self::CaseSensitive => s2!("TextCapsSmall"),
            Self::TypeSize => s2!("TextSize"),
            Self::LineSpacing => s2!("LineHeight"),
            Self::BaselineShift => s2!("TextSuperscript"),
            Self::OpenType => s2!("TextVariableFontSettings"),
            Self::List => s2!("ListBulleted"),
            Self::TabStop => s2!("Ruler"),
            Self::StrokeSolid => s2!("StrokeSolid"),
            Self::StrokeDotted => s2!("StrokeDotted"),
            Self::Shear => s2!("TransformSkew"),
            Self::CornerRadius => s2!("CornerRadius"),
            Self::CornerTopLeft => s2!("CornerRadiusTopLeft"),
            Self::CornerTopRight => s2!("CornerRadiusTopRight"),
            Self::CornerBottomLeft => s2!("CornerRadiusBottomLeft"),
            Self::CornerBottomRight => s2!("CornerRadiusBottomRight"),
            Self::Opacity => s2!("ViewTransparency"),
            Self::Blur => s2!("Blur"),
            Self::PagePortrait => s2!("OrientationPortrait"),
            Self::PageLandscape => s2!("OrientationLandscape"),
            Self::Plus => s2!("Add"),
            Self::Duplicate => s2!("Duplicate"),
            Self::PlaceImage => s2!("ImageAdd"),
            Self::Trash => s2!("Delete"),
            Self::ChevronLeft => s2!("ChevronLeft"),
            Self::ChevronRight => s2!("ChevronRight"),
            Self::Disclosure => s2!("ChevronSize100"),
            Self::Layers => s2!("Layers"),
            Self::Eye => s2!("Visibility"),
            Self::EyeOff => s2!("VisibilityOff"),
            Self::Lock => s2!("Lock"),
            Self::Unlock => s2!("LockOpen"),
            Self::Link2 => s2!("Link"),
            Self::Unlink2 => s2!("UnLink"),
            Self::Swap => s2!("Switch"),
            Self::NoFill => s2!("Cancel"),
            Self::Blend => s2!("Effects"),
            Self::ZoomIn => s2!("ZoomIn"),
            Self::ZoomOut => s2!("ZoomOut"),
            Self::ZoomFit => s2!("ZoomFitToScreen"),
            // Page-layout ideas Spectrum has no picture of: drawn here.
            Self::Scale
            | Self::TextCursor
            | Self::Crosshair
            | Self::LetterSpacing
            | Self::Indent
            | Self::ParagraphSpacing
            | Self::DropCap
            | Self::CapButt
            | Self::CapRound
            | Self::CapSquare
            | Self::JoinMiter
            | Self::JoinRound
            | Self::JoinBevel
            | Self::StrokeDashed
            | Self::ScaleX
            | Self::ScaleY
            | Self::Angle
            | Self::IndentLeft
            | Self::IndentRight
            | Self::SpaceBefore
            | Self::SpaceAfter
            | Self::Columns
            | Self::Gutter
            | Self::WrapNone
            | Self::WrapBounds
            | Self::WrapContour
            | Self::WrapJump => None,
        }
    }

    /// The side of the grid this icon's picture is drawn on — Spectrum's
    /// workflow icons are 20, its small UI marks 10, Tessera's drawings 24 —
    /// which is also the one size it is exact at.
    pub fn grid(self) -> f32 {
        if self.spectrum().is_some() {
            self.tree().size().width().round()
        } else {
            DRAWN_GRID
        }
    }

    /// The picture as SVG text: Spectrum's own, or Tessera's paths stroked.
    fn svg(self) -> Cow<'static, str> {
        match self.spectrum() {
            Some(svg) => Cow::Borrowed(svg),
            None => {
                let mut svg = format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="#000" stroke-width="{DRAWN_STROKE}" stroke-linecap="round" stroke-linejoin="round">"##
                );
                for data in self.paths() {
                    svg.push_str(&format!(r#"<path d="{data}"/>"#));
                }
                svg.push_str("</g></svg>");
                Cow::Owned(svg)
            }
        }
    }

    /// The parsed picture, built on first use and shared thereafter.
    fn tree(self) -> Arc<usvg::Tree> {
        static TREES: Mutex<Option<HashMap<Icon, Arc<usvg::Tree>>>> = Mutex::new(None);
        let mut trees = TREES.lock().unwrap_or_else(PoisonError::into_inner);
        trees
            .get_or_insert_with(HashMap::new)
            .entry(self)
            .or_insert_with(|| {
                let tree = usvg::Tree::from_str(&self.svg(), &usvg::Options::default())
                    // The pictures are compile-time constants, so a failure is
                    // a typo in this file or a bad vendored SVG rather than a
                    // runtime condition; `every_icon_draws` catches it first.
                    .unwrap_or_else(|error| panic!("icon {self:?} does not parse: {error}"));
                Arc::new(tree)
            })
            .clone()
    }

    /// How much of each pixel the picture covers, row by row, drawn `side`
    /// pixels square and turned `degrees` clockwise about its middle.
    pub fn coverage(self, side: u32, degrees: f32) -> Vec<u8> {
        let tree = self.tree();
        let Some(mut pixmap) = tiny_skia::Pixmap::new(side, side) else {
            return Vec::new();
        };
        let scale = side as f32 / tree.size().width();
        let middle = side as f32 / 2.0;
        let transform =
            tiny_skia::Transform::from_scale(scale, scale).post_rotate_at(degrees, middle, middle);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        pixmap.pixels().iter().map(|p| p.alpha()).collect()
    }
}

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
                Theme::accent_soft()
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

/// Paint `icon` centred in `rect`, at the interface's icon size, in `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = crate::theme::Theme::ICON_SIZE
        .min(rect.width())
        .min(rect.height());
    let rect = Rect::from_center_size(rect.center(), egui::Vec2::splat(side));
    paint_rotated(painter, rect, icon, color, 0.0);
}

/// Paint `icon` to fill `rect`, turned `degrees` clockwise about its middle.
///
/// Drawn from a texture rasterised at exactly the pixels it covers and laid
/// on whole pixels, so nothing is resampled on the way to the screen. The
/// turn is rasterised too rather than applied to the texture: a section's
/// disclosure turned a quarter is as crisp as one that was not.
pub fn paint_rotated(painter: &Painter, rect: Rect, icon: Icon, color: Color32, degrees: f32) {
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let side = device_side(rect.width().min(rect.height()), ppp);
    let target = pixel_box(rect.center(), side, ppp);
    let texture = texture(ctx, icon, side, degrees);
    painter.image(
        texture,
        target,
        Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
        color,
    );
}

/// `icon` as a mesh with one square per pixel it covers, the square's colour
/// `color` scaled by that coverage — for a pass that draws triangles and no
/// textures, which is the pointer's inverting blend
/// ([`crate::view::invert_host`]). The same pixels [`paint_rotated`] would
/// show, so the pointer and the toolbar are one set.
pub fn coverage_mesh(
    rect: Rect,
    icon: Icon,
    color: Color32,
    degrees: f32,
    pixels_per_point: f32,
) -> Mesh {
    let ppp = pixels_per_point.max(f32::EPSILON);
    let side = device_side(rect.width().min(rect.height()), ppp);
    let target = pixel_box(rect.center(), side, ppp);
    let step = 1.0 / ppp;
    let mut mesh = Mesh::default();
    for (at, &covered) in icon.coverage(side, degrees).iter().enumerate() {
        if covered == 0 {
            continue;
        }
        let (x, y) = ((at as u32 % side) as f32, (at as u32 / side) as f32);
        let pixel = Rect::from_min_size(
            target.min + egui::vec2(x * step, y * step),
            egui::Vec2::splat(step),
        );
        mesh.add_colored_rect(pixel, color.gamma_multiply(f32::from(covered) / 255.0));
    }
    mesh
}

/// The whole number of device pixels `side` points covers, never none.
fn device_side(side: f32, pixels_per_point: f32) -> u32 {
    (side * pixels_per_point).round().max(1.0) as u32
}

/// A square `side` device pixels across, centred as near `centre` as whole
/// pixels allow. Off the pixel grid, every edge of the icon would be shared
/// between two pixels and read as grey.
fn pixel_box(centre: Pos2, side: u32, pixels_per_point: f32) -> Rect {
    let points = side as f32 / pixels_per_point;
    let min = centre - egui::Vec2::splat(points / 2.0);
    let min = egui::pos2(
        (min.x * pixels_per_point).round() / pixels_per_point,
        (min.y * pixels_per_point).round() / pixels_per_point,
    );
    Rect::from_min_size(min, egui::Vec2::splat(points))
}

/// Rasterised icons, kept per context: a texture belongs to the context
/// that made it, and the tests make many.
#[derive(Clone, Default)]
struct Textures(Arc<Mutex<HashMap<TextureKey, egui::TextureHandle>>>);

/// An icon, its side in device pixels, and its turn in whole degrees.
type TextureKey = (Icon, u32, i32);

/// The texture for `icon` at `side` device pixels and `degrees` of turn,
/// made on first use. The turn is kept to whole degrees, which a handle
/// following a rotated frame cannot tell from exact.
fn texture(ctx: &egui::Context, icon: Icon, side: u32, degrees: f32) -> egui::TextureId {
    let turn = (degrees.round() as i32).rem_euclid(360);
    let cache = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Textures>(egui::Id::new("tessera-icon-textures"))
            .clone()
    });
    let mut textures = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    textures
        .entry((icon, side, turn))
        .or_insert_with(|| {
            let rgba: Vec<u8> = icon
                .coverage(side, turn as f32)
                .into_iter()
                .flat_map(|a| [a, a, a, a])
                .collect();
            let image =
                egui::ColorImage::from_rgba_premultiplied([side as usize, side as usize], &rgba);
            ctx.load_texture(
                format!("icon-{icon:?}-{side}-{turn}"),
                image,
                egui::TextureOptions::LINEAR,
            )
        })
        .id()
}

/// Every icon, for the tests that must cover all of them.
///
/// **An icon missing from this list is missing from its own tests.** Seventeen
/// were once, and three more — `Book`, `Pipette`, `Pi` — until the move to
/// Spectrum, when a count taken by hand matched a list that was short.
pub const ALL: [Icon; 116] = [
    Icon::Disclosure,
    Icon::Book,
    Icon::Pipette,
    Icon::Pi,
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
    Icon::SquareTerminal,
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
    Icon::Underline,
    Icon::Strikethrough,
    Icon::AlignJustify,
    Icon::Palette,
    Icon::Pilcrow,
    Icon::CaseSensitive,
    Icon::TypeSize,
    Icon::LineSpacing,
    Icon::LetterSpacing,
    Icon::BaselineShift,
    Icon::OpenType,
    Icon::Indent,
    Icon::ParagraphSpacing,
    Icon::List,
    Icon::TabStop,
    Icon::DropCap,
    Icon::CapButt,
    Icon::CapRound,
    Icon::CapSquare,
    Icon::JoinMiter,
    Icon::JoinRound,
    Icon::JoinBevel,
    Icon::StrokeSolid,
    Icon::StrokeDashed,
    Icon::StrokeDotted,
    Icon::ScaleX,
    Icon::ScaleY,
    Icon::Shear,
    Icon::Angle,
    Icon::IndentLeft,
    Icon::IndentRight,
    Icon::SpaceBefore,
    Icon::SpaceAfter,
    Icon::CornerRadius,
    Icon::CornerTopLeft,
    Icon::CornerTopRight,
    Icon::CornerBottomLeft,
    Icon::CornerBottomRight,
    Icon::Opacity,
    Icon::Blur,
    Icon::PagePortrait,
    Icon::PageLandscape,
    Icon::Columns,
    Icon::Gutter,
    Icon::WrapNone,
    Icon::WrapBounds,
    Icon::WrapContour,
    Icon::WrapJump,
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
    use kurbo::BezPath;

    /// How much ink the picture has at `side` pixels: the sum of coverage.
    fn ink(icon: Icon, side: u32) -> u32 {
        icon.coverage(side, 0.0).iter().map(|&a| u32::from(a)).sum()
    }

    #[test]
    fn every_icon_draws() {
        // The picture parses and puts real ink on the pixels: a Spectrum SVG
        // whose fill did not resolve would parse and draw nothing.
        for icon in ALL {
            assert!(
                ink(icon, 20) > 255 * 8,
                "{icon:?} draws next to nothing at 20 pixels"
            );
        }
    }

    #[test]
    fn every_icon_has_exactly_one_source() {
        // Spectrum's picture or Tessera's paths, never both and never neither:
        // an icon with both would be carrying a drawing nothing shows.
        for icon in ALL {
            assert_ne!(
                icon.spectrum().is_some(),
                !icon.paths().is_empty(),
                "{icon:?} has {} sources",
                if icon.spectrum().is_some() { 2 } else { 0 }
            );
        }
    }

    #[test]
    fn every_drawn_path_parses_and_stays_on_its_grid() {
        use kurbo::Shape as _;
        for icon in ALL {
            for data in icon.paths() {
                let b = BezPath::from_svg(data)
                    .unwrap_or_else(|e| panic!("{icon:?} has an unparseable path: {e}"))
                    .bounding_box();
                assert!(
                    b.x0 >= -0.5 && b.y0 >= -0.5 && b.x1 <= 24.5 && b.y1 <= 24.5,
                    "{icon:?} escapes the 24x24 grid: {b:?}"
                );
            }
        }
    }

    #[test]
    fn the_spectrum_pictures_are_square_on_their_own_grid() {
        for icon in ALL {
            if icon.spectrum().is_some() {
                // Within a hair: Adobe's `AlignBottom` says 20.00001.
                let size = icon.tree().size();
                let want = if icon == Icon::Disclosure { 10.0 } else { 20.0 };
                assert!(
                    (size.width() - want).abs() < 0.01 && (size.height() - want).abs() < 0.01,
                    "{icon:?} is {size:?}"
                );
                assert_eq!(icon.grid(), want, "{icon:?}");
            }
        }
    }

    #[test]
    fn a_drawn_icon_weighs_what_a_spectrum_one_does() {
        // One set to the eye: a stroke sample beside a text-alignment picture
        // must not look bolder or fainter. Compared by ink per unit of line —
        // a line and a line, drawn by each.
        let drawn = ink(Icon::StrokeDashed, 40);
        let spectrum = ink(Icon::StrokeSolid, 40);
        let ratio = f64::from(drawn) / f64::from(spectrum);
        assert!(
            (0.4..=1.2).contains(&ratio),
            "a dashed line should carry a little less ink than a solid one, got {ratio}"
        );
    }

    #[test]
    fn turning_a_picture_moves_its_ink() {
        let upright = Icon::ChevronRight.coverage(20, 0.0);
        let turned = Icon::ChevronRight.coverage(20, 90.0);
        assert_ne!(upright, turned);
        let total = |c: &[u8]| c.iter().map(|&a| u32::from(a)).sum::<u32>();
        let (a, b) = (total(&upright), total(&turned));
        assert!(
            a.abs_diff(b) * 10 < a,
            "a quarter turn keeps the ink: {a} vs {b}"
        );
    }

    /// The icons that point with a tip rather than with their middle.
    const POINTED: [Icon; 4] = [Icon::Select, Icon::DirectSelect, Icon::Pen, Icon::Pipette];

    #[test]
    fn a_pointed_icon_has_its_hotspot_on_its_own_ink() {
        // The bug this pins, from the Lucide days: the pen's hotspot was read
        // off the wrong corner, so the cursor drew a whole grid away from the
        // point it drew from. Checked against the rendered pixels, which is
        // what the person sees, at four pixels to the unit.
        for icon in POINTED {
            let (hx, hy) = icon.hotspot();
            let per_unit = 4.0;
            let side = (icon.grid() * per_unit) as u32;
            let coverage = icon.coverage(side, 0.0);
            let near = (0..side * side).any(|at| {
                let (x, y) = ((at % side) as f32 + 0.5, (at / side) as f32 + 0.5);
                let d = ((x / per_unit - hx).powi(2) + (y / per_unit - hy).powi(2)).sqrt();
                d < 1.0 && coverage[at as usize] > 128
            });
            assert!(near, "{icon:?} points from ({hx}, {hy}), which has no ink");
        }
    }

    #[test]
    fn every_other_icon_points_from_its_middle() {
        for icon in ALL {
            if POINTED.contains(&icon) {
                continue;
            }
            let centre = icon.grid() / 2.0;
            assert_eq!(
                icon.hotspot(),
                (centre, centre),
                "{icon:?} aims off-centre without being listed as pointed"
            );
        }
    }

    #[test]
    fn a_link_and_a_broken_link_are_different_pictures() {
        assert_ne!(Icon::Link2.spectrum(), Icon::Unlink2.spectrum());
    }

    #[test]
    fn a_painted_icon_lands_on_whole_pixels() {
        // Off the grid, every edge is shared by two pixels and reads grey.
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            let rect = pixel_box(egui::pos2(10.3, 7.77), device_side(20.0, ppp), ppp);
            for v in [rect.min.x, rect.min.y, rect.max.x, rect.max.y] {
                let device = v * ppp;
                assert!(
                    (device - device.round()).abs() < 1e-3,
                    "{v} is not on a pixel at {ppp}"
                );
            }
        }
    }

    #[test]
    fn the_cursor_mesh_is_one_square_per_covered_pixel() {
        let mesh = coverage_mesh(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(20.0, 20.0)),
            Icon::Select,
            Color32::WHITE,
            0.0,
            1.0,
        );
        let covered = Icon::Select
            .coverage(20, 0.0)
            .iter()
            .filter(|&&a| a > 0)
            .count();
        assert_eq!(mesh.vertices.len(), covered * 4);
        assert_eq!(mesh.indices.len(), covered * 6);
    }

    #[test]
    fn no_icon_is_missing_from_all() {
        // Rust cannot enumerate an enum's variants without a derive, so the
        // count is what is checked, and it is the enum's own count.
        let unique: std::collections::HashSet<_> = ALL.iter().collect();
        assert_eq!(unique.len(), ALL.len(), "an icon is listed twice");
        assert_eq!(ALL.len(), 116);
    }
}

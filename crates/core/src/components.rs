use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// Document entity component holding document-level configuration
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub dpi: f32,
    /// Extra printed area beyond the trim, so a cut that drifts still lands on ink.
    pub bleed: f32,
    /// Working area outside the bleed for printer marks and notes.
    pub slug: f32,
    /// When true, pages pair across a spine after the first.
    pub facing_pages: bool,
    /// Vertical gap between spreads on the pasteboard.
    pub spread_gap: f32,
    /// Default margins and columns for new pages.
    pub guides: crate::layout::PageGuides,
    /// Vertical rhythm text frames can lock onto.
    pub baseline_grid: crate::layout::BaselineGrid,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            title: "Untitled Document".to_string(),
            width: 595.0,
            height: 842.0,
            dpi: 300.0,
            bleed: 3.0,
            slug: 0.0,
            facing_pages: true,
            spread_gap: 60.0,
            guides: crate::layout::PageGuides::default(),
            baseline_grid: crate::layout::BaselineGrid::default(),
        }
    }
}

impl Document {
    /// The spread arrangement implied by these document settings.
    pub fn spread_layout(&self) -> crate::layout::SpreadLayout {
        crate::layout::SpreadLayout {
            facing_pages: self.facing_pages,
            page_width: self.width,
            page_height: self.height,
            spread_gap: self.spread_gap,
        }
    }
}

/// Page entity component
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub page_number: u32,
    pub width: f32,
    pub height: f32,
    pub spread_index: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            page_number: 1,
            width: 800.0,
            height: 600.0,
            spread_index: 0,
        }
    }
}

/// Layer entity component for visual organization and z-stacking
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub z_index: i32,
    pub is_visible: bool,
    pub is_locked: bool,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            name: "Default Layer".to_string(),
            z_index: 0,
            is_visible: true,
            is_locked: false,
        }
    }
}

/// Content frame classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameType {
    Rectangle,
    Ellipse,
    Text,
    Image,
    /// A straight segment from the frame box's top-left to its bottom-right.
    Line,
    /// An arbitrary bezier outline, carried by a [`PathData`] component.
    Path,
}

/// Frame entity component
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub name: String,
    pub frame_type: FrameType,
}

/// Hierarchical relationship pointer component
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BelongsTo(pub Entity);

/// 2D Position component
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// 2D Size component
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            width: 100.0,
            height: 100.0,
        }
    }
}

/// 2D Transform component (Position, Rotation in radians, Scale)
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: Position,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Position::default(),
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}

/// Explicit Z-index component for entity depth ordering
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ZIndex(pub i32);

impl Default for ZIndex {
    fn default() -> Self {
        Self(0)
    }
}

/// Axis-Aligned Bounding Box (AABB) for spatial indexing and hit testing
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl BoundingBox {
    pub fn new(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
        Self {
            min_x: min_x.min(max_x),
            min_y: min_y.min(max_y),
            max_x: min_x.max(max_x),
            max_y: min_y.max(max_y),
        }
    }

    /// Computes a rectangular AABB from a Transform and Size.
    ///
    /// Kept for callers that have no frame type to hand. Prefer
    /// [`crate::geometry::frame_bounds`], which is exact for ellipses, lines and
    /// paths rather than assuming a rectangle.
    pub fn from_transform_and_size(transform: &Transform, size: &Size) -> Self {
        crate::geometry::frame_bounds(FrameType::Rectangle, transform, size, None)
    }

    /// Checks if a 2D point (e.g. mouse click) lies inside the bounding box
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.min_x && px <= self.max_x && py >= self.min_y && py <= self.max_y
    }

    /// Checks if this bounding box intersects another bounding box
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    pub fn width(&self) -> f32 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.max_y - self.min_y).max(0.0)
    }

    pub fn center(&self) -> (f32, f32) {
        ((self.min_x + self.max_x) / 2.0, (self.min_y + self.max_y) / 2.0)
    }
}

impl Default for BoundingBox {
    fn default() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 100.0,
            max_y: 100.0,
        }
    }
}

/// Visual styling component for rendering
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub fill_color: [f32; 4],
    pub stroke_color: Option<[f32; 4]>,
    pub stroke_width: f32,
    pub opacity: f32,
    /// Corner rounding in document units. Zero draws true square corners.
    pub corner_radius: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill_color: [0.22, 0.58, 0.98, 1.0], // #38bdf8
            stroke_color: Some([0.5, 0.55, 0.95, 1.0]),
            stroke_width: 1.5,
            opacity: 1.0,
            // Square by default. A rectangle drawn in a layout tool is a
            // rectangle; rounding is something the user asks for.
            corner_radius: 0.0,
        }
    }
}

/// Bezier outline for [`FrameType::Path`] frames, as an SVG path string.
///
/// Storing the outline as SVG keeps it serializable for save files and the IPC
/// bridge; `kurbo::BezPath::from_svg` turns it back into geometry on demand.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathData {
    pub svg: String,
}

impl Default for PathData {
    fn default() -> Self {
        Self {
            svg: "M 0 0 L 100 0 L 100 100 Z".to_string(),
        }
    }
}

/// Marks an entity as a master page: a template other pages inherit from.
///
/// Master pages live in their own hierarchy rather than the document's page
/// sequence, and their frames are stored in *page-local* coordinates so the
/// same master can be applied to any page wherever it sits on the pasteboard.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MasterPage {
    pub name: String,
    /// Short label shown on page thumbnails, e.g. "A".
    pub prefix: String,
    pub width: f32,
    pub height: f32,
}

impl Default for MasterPage {
    fn default() -> Self {
        Self {
            name: "A-Master".to_string(),
            prefix: "A".to_string(),
            width: 595.0,
            height: 842.0,
        }
    }
}

/// Links a document page to the master page it inherits from.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedMaster(pub Entity);

/// Marks a frame as a local override of an inherited master item.
///
/// Overriding breaks the link for that one item: it becomes an ordinary,
/// editable frame on the page, while the rest of the master still applies.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasterOverride {
    /// The master frame this was promoted from.
    pub source: Entity,
    /// The page the override belongs to.
    pub page: Entity,
}

/// Links a text frame to the frames before and after it in a story.
///
/// A story is a doubly-linked chain of text frames. Text that overflows one
/// frame continues in `next`, which is what lets an article run across
/// columns, pages and spreads.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextThread {
    pub previous: Option<Entity>,
    pub next: Option<Entity>,
}

impl TextThread {
    /// True when this frame starts a story.
    pub fn is_head(&self) -> bool {
        self.previous.is_none()
    }

    /// True when this frame ends a story.
    pub fn is_tail(&self) -> bool {
        self.next.is_none()
    }
}

/// Paragraph alignment for text frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlignment {
    /// Left for left-to-right text, right for right-to-left.
    #[default]
    Start,
    Center,
    /// Right for left-to-right text, left for right-to-left.
    End,
    Justify,
}

/// Text content and type settings for Text frames.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
    pub font_size: f32,
    /// Leading, as a multiple of font size.
    pub line_height: f32,
    pub align: TextAlignment,
    /// Preferred family name; `None` uses the system default.
    pub font_family: Option<String>,
    /// CSS-style numeric weight, where 400 is regular and 700 is bold.
    pub font_weight: f32,
    /// Lock every line onto the document's baseline grid.
    ///
    /// When set, `line_height` stops determining where lines land: leading
    /// opens up to a whole number of grid increments and the first baseline is
    /// pushed down onto a rung. Set per frame, because a pull quote usually
    /// wants to break the rhythm that body copy keeps.
    pub snap_to_baseline: bool,
}

impl Default for TextContent {
    fn default() -> Self {
        Self {
            text: "Double click to edit text".to_string(),
            font_size: 16.0,
            line_height: 1.4,
            align: TextAlignment::Start,
            font_family: None,
            font_weight: 400.0,
            snap_to_baseline: false,
        }
    }
}

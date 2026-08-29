use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// Document entity component holding document-level configuration
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub dpi: f32,
    pub bleed: f32,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            title: "Untitled Document".to_string(),
            width: 800.0,
            height: 600.0,
            dpi: 300.0,
            bleed: 3.0,
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

    /// Computes an AABB from a Transform and Size
    pub fn from_transform_and_size(transform: &Transform, size: &Size) -> Self {
        let w = size.width * transform.scale_x.abs();
        let h = size.height * transform.scale_y.abs();

        if transform.rotation == 0.0 {
            Self::new(
                transform.position.x,
                transform.position.y,
                transform.position.x + w,
                transform.position.y + h,
            )
        } else {
            // Compute rotated bounding box
            let cos_a = transform.rotation.cos().abs();
            let sin_a = transform.rotation.sin().abs();
            let bb_w = w * cos_a + h * sin_a;
            let bb_h = w * sin_a + h * cos_a;

            Self::new(
                transform.position.x,
                transform.position.y,
                transform.position.x + bb_w,
                transform.position.y + bb_h,
            )
        }
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
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill_color: [0.22, 0.58, 0.98, 1.0], // #38bdf8
            stroke_color: Some([0.5, 0.55, 0.95, 1.0]),
            stroke_width: 1.5,
            opacity: 1.0,
        }
    }
}

/// Text content component for Text frames
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
    pub font_size: f32,
    pub line_height: f32,
}

impl Default for TextContent {
    fn default() -> Self {
        Self {
            text: "Double click to edit text".to_string(),
            font_size: 16.0,
            line_height: 1.4,
        }
    }
}

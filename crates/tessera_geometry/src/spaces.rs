//! Point and rectangle types, one pair per coordinate space.

use serde::{Deserialize, Serialize};

/// A point in document space. Units are points (1/72 inch), matching PDF.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DocPoint {
    pub x: f64,
    pub y: f64,
}

impl DocPoint {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

/// A point in screen space, in physical pixels.
///
/// Deliberately a different type from [`DocPoint`], and deliberately `f32`:
/// screen coordinates come from egui as `f32`, and the type difference is
/// what stops a document coordinate being passed where a screen one belongs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
}

/// An axis-aligned rectangle in document space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DocRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DocRect {
    pub fn contains(&self, p: DocPoint) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    pub fn to_kurbo(self) -> kurbo::Rect {
        kurbo::Rect::new(self.x, self.y, self.x + self.width, self.y + self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_contains_a_point_inside_it() {
        let r = DocRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(r.contains(DocPoint { x: 50.0, y: 30.0 }));
    }

    #[test]
    fn a_rect_excludes_a_point_outside_it() {
        let r = DocRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(!r.contains(DocPoint { x: 5.0, y: 30.0 }));
        assert!(!r.contains(DocPoint { x: 50.0, y: 70.0 }));
    }

    #[test]
    fn a_rect_converts_to_kurbo_with_the_same_corners() {
        let r = DocRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let k = r.to_kurbo();
        assert_eq!((k.x0, k.y0, k.x1, k.y1), (10.0, 20.0, 40.0, 60.0));
    }
}

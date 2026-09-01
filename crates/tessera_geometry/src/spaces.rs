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

    /// Whether two rectangles overlap at all.
    ///
    /// Touching edges count as overlapping, so a marquee dragged exactly to an
    /// object's edge still catches it.
    pub fn intersects(&self, other: DocRect) -> bool {
        self.x <= other.x + other.width
            && other.x <= self.x + self.width
            && self.y <= other.y + other.height
            && other.y <= self.y + self.height
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

    #[test]
    fn overlapping_rects_intersect() {
        let a = DocRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let b = DocRect {
            x: 5.0,
            y: 5.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(a.intersects(b));
        assert!(b.intersects(a), "intersection is symmetric");
    }

    #[test]
    fn separated_rects_do_not_intersect() {
        let a = DocRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let b = DocRect {
            x: 20.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(!a.intersects(b));
    }

    #[test]
    fn a_contained_rect_intersects_its_container() {
        let outer = DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let inner = DocRect {
            x: 10.0,
            y: 10.0,
            width: 5.0,
            height: 5.0,
        };
        assert!(outer.intersects(inner));
    }

    #[test]
    fn touching_edges_count_as_intersecting() {
        let a = DocRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let b = DocRect {
            x: 10.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(
            a.intersects(b),
            "a marquee dragged to an edge must catch it"
        );
    }
}

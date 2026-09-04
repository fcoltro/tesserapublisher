//! The nine points a transform can be anchored to.

use serde::{Deserialize, Serialize};

use crate::spaces::{DocPoint, DocRect};
use crate::transform::Transform;

/// The point a transform holds still.
///
/// Scaling, rotating and flipping all need one, and which one it is changes
/// the result entirely. Choosing it is the user's decision, so it is a value
/// rather than an assumption buried in each operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopCentre,
    TopRight,
    MiddleLeft,
    #[default]
    Centre,
    MiddleRight,
    BottomLeft,
    BottomCentre,
    BottomRight,
}

impl Anchor {
    /// Reading order, which is the order a three-by-three proxy draws them in.
    pub const ALL: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::TopCentre,
        Anchor::TopRight,
        Anchor::MiddleLeft,
        Anchor::Centre,
        Anchor::MiddleRight,
        Anchor::BottomLeft,
        Anchor::BottomCentre,
        Anchor::BottomRight,
    ];

    /// Where this anchor sits in a given rectangle.
    pub fn in_rect(self, rect: DocRect) -> DocPoint {
        let (fx, fy) = self.fractions();
        DocPoint {
            x: rect.x + rect.width * fx,
            y: rect.y + rect.height * fy,
        }
    }

    /// How far along each axis this anchor sits, from 0 to 1.
    fn fractions(self) -> (f64, f64) {
        match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::TopCentre => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::MiddleLeft => (0.0, 0.5),
            Anchor::Centre => (0.5, 0.5),
            Anchor::MiddleRight => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::BottomCentre => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        }
    }

    pub fn scale(self, rect: DocRect, sx: f64, sy: f64) -> Transform {
        Transform::scale_about(sx, sy, self.in_rect(rect))
    }

    pub fn rotate(self, rect: DocRect, degrees: f64) -> Transform {
        Transform::rotate_about(degrees, self.in_rect(rect))
    }

    /// Lean this rectangle's contents sideways about the anchor.
    ///
    /// A horizontal shear: points move along `x` in proportion to their
    /// distance from the anchor in `y`. Positive leans the **top** to the
    /// right — the italic slant — which is the same sign
    /// [`crate::Decomposition`] reports, so a value written here reads back
    /// unchanged.
    pub fn shear(self, rect: DocRect, degrees: f64) -> Transform {
        if degrees == 0.0 {
            return Transform::IDENTITY;
        }
        let about = self.in_rect(rect);
        let m = degrees.to_radians().tan();
        // x' = x - m*(y - about.y), y' = y.
        Transform::from_affine(kurbo::Affine::new([1.0, 0.0, -m, 1.0, m * about.y, 0.0]))
    }

    /// A flip is a scale by minus one, which is why it belongs here rather
    /// than as an operation of its own.
    pub fn flip(self, rect: DocRect, horizontal: bool, vertical: bool) -> Transform {
        self.scale(
            rect,
            if horizontal { -1.0 } else { 1.0 },
            if vertical { -1.0 } else { 1.0 },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> DocRect {
        DocRect {
            x: 100.0,
            y: 200.0,
            width: 40.0,
            height: 60.0,
        }
    }

    fn close(a: DocPoint, b: DocPoint) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn each_anchor_lands_where_its_name_says() {
        let r = rect();
        assert!(close(
            Anchor::TopLeft.in_rect(r),
            DocPoint { x: 100.0, y: 200.0 }
        ));
        assert!(close(
            Anchor::TopCentre.in_rect(r),
            DocPoint { x: 120.0, y: 200.0 }
        ));
        assert!(close(
            Anchor::TopRight.in_rect(r),
            DocPoint { x: 140.0, y: 200.0 }
        ));
        assert!(close(
            Anchor::MiddleLeft.in_rect(r),
            DocPoint { x: 100.0, y: 230.0 }
        ));
        assert!(close(
            Anchor::Centre.in_rect(r),
            DocPoint { x: 120.0, y: 230.0 }
        ));
        assert!(close(
            Anchor::MiddleRight.in_rect(r),
            DocPoint { x: 140.0, y: 230.0 }
        ));
        assert!(close(
            Anchor::BottomLeft.in_rect(r),
            DocPoint { x: 100.0, y: 260.0 }
        ));
        assert!(close(
            Anchor::BottomCentre.in_rect(r),
            DocPoint { x: 120.0, y: 260.0 }
        ));
        assert!(close(
            Anchor::BottomRight.in_rect(r),
            DocPoint { x: 140.0, y: 260.0 }
        ));
    }

    #[test]
    fn the_anchor_is_the_one_point_a_scale_leaves_alone() {
        let r = rect();
        for anchor in Anchor::ALL {
            let fixed = anchor.in_rect(r);
            let moved = anchor.scale(r, 3.0, 0.5).apply(fixed);
            assert!(close(fixed, moved), "{anchor:?} moved its own anchor point");
        }
    }

    #[test]
    fn the_anchor_is_the_one_point_a_rotation_leaves_alone() {
        let r = rect();
        for anchor in Anchor::ALL {
            let fixed = anchor.in_rect(r);
            let moved = anchor.rotate(r, 37.0).apply(fixed);
            assert!(close(fixed, moved), "{anchor:?} moved its own anchor point");
        }
    }

    #[test]
    fn a_horizontal_flip_about_the_left_edge_sends_the_right_edge_out_past_it() {
        let r = rect();
        let flipped = Anchor::MiddleLeft.flip(r, true, false);
        let right_edge = DocPoint { x: 140.0, y: 230.0 };
        let landed = flipped.apply(right_edge);
        assert!(close(landed, DocPoint { x: 60.0, y: 230.0 }));
    }

    #[test]
    fn flipping_twice_is_doing_nothing() {
        let r = rect();
        let once = Anchor::Centre.flip(r, true, true);
        let there_and_back = once.then(once);
        let p = DocPoint { x: 123.0, y: 234.0 };
        assert!(close(there_and_back.apply(p), p));
    }

    #[test]
    fn a_shear_holds_its_anchor_still() {
        let r = rect();
        for anchor in Anchor::ALL {
            let fixed = anchor.in_rect(r);
            let moved = anchor.shear(r, 20.0).apply(fixed);
            assert!(close(fixed, moved), "{anchor:?} moved its own anchor point");
        }
    }

    #[test]
    fn a_shear_leans_the_top_of_a_box_to_the_right() {
        let r = rect();
        // About the bottom edge, so the bottom stays put and the top leans.
        let t = Anchor::BottomLeft.shear(r, 45.0);
        let top_left = DocPoint { x: r.x, y: r.y };
        let leaned = t.apply(top_left);
        assert!(
            (leaned.x - (top_left.x + r.height)).abs() < 1e-9,
            "45 degrees should lean the top right by the box's own height, got {leaned:?}"
        );
        assert!(
            (leaned.y - top_left.y).abs() < 1e-9,
            "and not move it vertically"
        );
    }

    #[test]
    fn a_shear_written_here_reads_back_from_the_decomposition() {
        // The two must agree on the sign, or the inspector's field would
        // report the negation of what it just set.
        let t = Anchor::Centre.shear(rect(), 15.0);
        assert!(
            (t.decompose().shear_degrees - 15.0).abs() < 1e-9,
            "read back {}",
            t.decompose().shear_degrees
        );
    }

    #[test]
    fn no_shear_is_the_identity() {
        assert!(Anchor::Centre.shear(rect(), 0.0).is_identity());
    }

    #[test]
    fn the_default_anchor_is_the_centre() {
        assert_eq!(Anchor::default(), Anchor::Centre);
    }
}

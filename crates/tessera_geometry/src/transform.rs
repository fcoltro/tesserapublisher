//! An object's placement: its own coordinate space mapped onto the document.
//!
//! A frame stores its box in its **own** space and a [`Transform`] that puts
//! that space on the page. This is InDesign's model — geometric bounds plus an
//! item transform — and it is the reason shear, flipping, and correctly
//! scaling a rotated group are expressible at all.
//!
//! The alternative, a rectangle plus one rotation angle, cannot express any of
//! them: scaling a rotated object non-uniformly *is* a shear, and an
//! axis-aligned box with an angle has nowhere to put it.
//!
//! Stored as its six coefficients rather than as `kurbo::Affine` so the
//! on-disk shape is explicit and stable, and so the document model does not
//! hand its file format to a dependency's serde implementation.

use serde::{Deserialize, Serialize};

use crate::spaces::DocPoint;

/// A 2D affine transform, in kurbo's coefficient order.
///
/// `[a, b, c, d, e, f]` maps a point as
///
/// ```text
/// x' = a*x + c*y + e
/// y' = b*x + d*y + f
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Transform {
    pub coefficients: [f64; 6],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        coefficients: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    pub fn from_affine(affine: kurbo::Affine) -> Self {
        Self {
            coefficients: affine.as_coeffs(),
        }
    }

    pub fn to_affine(self) -> kurbo::Affine {
        kurbo::Affine::new(self.coefficients)
    }

    /// Clockwise rotation by `degrees` about `about`.
    ///
    /// Clockwise because document y grows downward, which is also what the
    /// interface shows and what a positive angle has always meant here.
    pub fn rotate_about(degrees: f64, about: DocPoint) -> Self {
        Self::from_affine(
            kurbo::Affine::translate((about.x, about.y))
                * kurbo::Affine::rotate(degrees.to_radians())
                * kurbo::Affine::translate((-about.x, -about.y)),
        )
    }

    pub fn translate(dx: f64, dy: f64) -> Self {
        Self::from_affine(kurbo::Affine::translate((dx, dy)))
    }

    /// Scale by `sx`, `sy` about `about`.
    pub fn scale_about(sx: f64, sy: f64, about: DocPoint) -> Self {
        Self::from_affine(
            kurbo::Affine::translate((about.x, about.y))
                * kurbo::Affine::scale_non_uniform(sx, sy)
                * kurbo::Affine::translate((-about.x, -about.y)),
        )
    }

    pub fn apply(self, p: DocPoint) -> DocPoint {
        let [a, b, c, d, e, f] = self.coefficients;
        DocPoint {
            x: a * p.x + c * p.y + e,
            y: b * p.x + d * p.y + f,
        }
    }

    /// `self` first, then `outer`.
    ///
    /// Named for the order it reads in, because `outer * self` at the call
    /// site is the single easiest thing to get backwards here.
    pub fn then(self, outer: Self) -> Self {
        Self::from_affine(outer.to_affine() * self.to_affine())
    }

    /// The inverse, or the identity if this transform collapses space.
    ///
    /// A degenerate transform — everything scaled to zero on some axis — has
    /// no inverse. Returning the identity keeps hit testing and the gestures
    /// answering *something* rather than propagating NaN through the document.
    pub fn inverse(self) -> Self {
        if self.determinant().abs() < f64::EPSILON {
            return Self::IDENTITY;
        }
        Self::from_affine(self.to_affine().inverse())
    }

    pub fn determinant(self) -> f64 {
        let [a, b, c, d, _, _] = self.coefficients;
        a * d - b * c
    }

    pub fn is_identity(self) -> bool {
        self.coefficients == Self::IDENTITY.coefficients
    }

    /// The rotation this transform applies, in degrees, for display.
    ///
    /// Read off the image of the x-axis. Exact for a rotation, and still the
    /// right answer when a scale is composed with one. A sheared transform has
    /// no single angle; this reports the one an inspector should show, which
    /// is what every layout tool does with the same question.
    pub fn rotation_degrees(self) -> f64 {
        let [a, b, ..] = self.coefficients;
        if a == 0.0 && b == 0.0 {
            return 0.0;
        }
        b.atan2(a).to_degrees()
    }

    /// Whether this transform is a pure translation.
    ///
    /// Lets callers skip work that only matters for turned or scaled frames.
    pub fn is_axis_aligned(self) -> bool {
        let [a, b, c, d, _, _] = self.coefficients;
        b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn close_point(a: DocPoint, b: DocPoint) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    const P: DocPoint = DocPoint { x: 3.0, y: 7.0 };

    #[test]
    fn the_identity_moves_nothing() {
        assert!(close_point(Transform::IDENTITY.apply(P), P));
        assert!(Transform::default().is_identity());
    }

    #[test]
    fn rotating_about_a_point_leaves_that_point_fixed() {
        let about = DocPoint { x: 10.0, y: 20.0 };
        for degrees in [0.0, 30.0, 90.0, 180.0, -45.0] {
            let t = Transform::rotate_about(degrees, about);
            assert!(
                close_point(t.apply(about), about),
                "{degrees} degrees moved its own centre"
            );
        }
    }

    #[test]
    fn a_positive_angle_turns_clockwise_on_screen() {
        // Document y grows downward, so x must swing onto +y. This is the
        // convention the whole application already uses; a transform that
        // disagreed would silently mirror every rotated frame.
        let t = Transform::rotate_about(90.0, DocPoint::ZERO);
        let moved = t.apply(DocPoint { x: 1.0, y: 0.0 });
        assert!(close(moved.x, 0.0), "x was {}", moved.x);
        assert!(close(moved.y, 1.0), "y was {}", moved.y);
    }

    #[test]
    fn it_agrees_with_the_rotation_it_replaces() {
        // DocPoint::rotated_about is what frames used before transforms. The
        // two must be the same operation, or loading an old document would
        // move its contents.
        let about = DocPoint { x: 4.0, y: -9.0 };
        for degrees in [0.0, 17.0, 90.0, 200.0, -120.0] {
            let by_transform = Transform::rotate_about(degrees, about).apply(P);
            let by_point = P.rotated_about(about, degrees);
            assert!(
                close_point(by_transform, by_point),
                "{degrees} degrees: {by_transform:?} vs {by_point:?}"
            );
        }
    }

    #[test]
    fn an_inverse_undoes_its_transform() {
        let t = Transform::rotate_about(33.0, DocPoint { x: 5.0, y: 5.0 })
            .then(Transform::scale_about(2.0, 3.0, DocPoint::ZERO))
            .then(Transform::translate(10.0, -4.0));
        assert!(close_point(t.inverse().apply(t.apply(P)), P));
    }

    #[test]
    fn a_collapsed_transform_inverts_to_the_identity_rather_than_to_nan() {
        // Scaling an axis to nothing is reachable by dragging a handle past
        // itself. NaN spreading through the document would be far worse than
        // a gesture that briefly does nothing.
        let flat = Transform::scale_about(1.0, 0.0, DocPoint::ZERO);
        assert!(flat.inverse().is_identity());
        assert!(flat.inverse().apply(P).x.is_finite());
    }

    #[test]
    fn then_composes_in_the_order_it_reads() {
        // Rotate, then move. The other order puts the point somewhere else,
        // and getting it backwards is the easiest mistake available here.
        let rotate = Transform::rotate_about(90.0, DocPoint::ZERO);
        let move_right = Transform::translate(100.0, 0.0);

        let rotate_then_move = rotate.then(move_right).apply(DocPoint { x: 1.0, y: 0.0 });
        assert!(close_point(rotate_then_move, DocPoint { x: 100.0, y: 1.0 }));

        let move_then_rotate = move_right.then(rotate).apply(DocPoint { x: 1.0, y: 0.0 });
        assert!(close_point(move_then_rotate, DocPoint { x: 0.0, y: 101.0 }));
    }

    #[test]
    fn rotation_is_reported_back_for_the_inspector() {
        for degrees in [0.0, 30.0, 90.0, -45.0, 179.0] {
            let t = Transform::rotate_about(degrees, DocPoint { x: 2.0, y: 2.0 });
            assert!(
                close(t.rotation_degrees(), degrees),
                "{degrees} read back as {}",
                t.rotation_degrees()
            );
        }
    }

    #[test]
    fn a_uniform_scale_does_not_disturb_the_reported_rotation() {
        let t = Transform::rotate_about(40.0, DocPoint::ZERO).then(Transform::scale_about(
            3.0,
            3.0,
            DocPoint::ZERO,
        ));
        assert!(
            close(t.rotation_degrees(), 40.0),
            "{}",
            t.rotation_degrees()
        );
    }

    #[test]
    fn only_a_translation_counts_as_axis_aligned() {
        assert!(Transform::IDENTITY.is_axis_aligned());
        assert!(Transform::translate(3.0, 4.0).is_axis_aligned());
        assert!(Transform::scale_about(2.0, 5.0, DocPoint::ZERO).is_axis_aligned());
        assert!(!Transform::rotate_about(1.0, DocPoint::ZERO).is_axis_aligned());
    }

    #[test]
    fn it_survives_a_json_round_trip_as_six_bare_numbers() {
        // The on-disk shape is the document's, not a dependency's. A change
        // here is a format change and must be a deliberate one.
        let t = Transform::rotate_about(30.0, DocPoint { x: 1.0, y: 2.0 });
        let json = serde_json::to_string(&t).expect("ser");
        assert!(json.starts_with('['), "should be a bare array, got {json}");
        let back: Transform = serde_json::from_str(&json).expect("de");
        assert_eq!(back, t);
    }

    #[test]
    fn shear_is_expressible_at_all() {
        // The whole point. A rotated box scaled on one axis only is a shear,
        // which a rectangle-plus-angle could not represent -- so the group
        // transform code had to approximate it.
        let t = Transform::rotate_about(45.0, DocPoint::ZERO).then(Transform::scale_about(
            2.0,
            1.0,
            DocPoint::ZERO,
        ));
        let [a, b, c, d, _, _] = t.coefficients;
        // Not a similarity: the two axes are no longer perpendicular-and-equal.
        let dot = a * c + b * d;
        assert!(
            dot.abs() > 1e-9,
            "axes stayed orthogonal, so this is no shear"
        );
        assert!(t.determinant().abs() > 1e-9, "and it did not collapse");
    }
}

//! Regular polygons and stars, as paths.
//!
//! A polygon is not a new kind of frame. It is a [`FrameKind::Path`] like a pen
//! drawing or a line, made by arithmetic instead of by hand — which is what
//! makes every tool that already works on paths work on it: the direct-select
//! tool can drag its corners, the scissors can cut it, and it exports through
//! exactly one code path.
//!
//! [`FrameKind::Path`]: crate::nodes::FrameKind::Path

use kurbo::{BezPath, Point};
use tessera_geometry::DocRect;

/// The fewest sides a polygon can have.
///
/// Two points is a line and one is a dot; three is the first thing that
/// encloses an area.
pub const FEWEST_SIDES: u32 = 3;

/// The most.
///
/// Past this a polygon is a circle drawn expensively — an ellipse is the shape
/// somebody wants, and it is one object rather than two hundred points.
pub const MOST_SIDES: u32 = 200;

/// A regular polygon or star inscribed in `bounds`.
///
/// `inset` is how far the inner points of a star are pulled in, as a share of
/// the radius: zero is a plain polygon, and anything above it alternates
/// between the full radius and `1 - inset` of it.
///
/// The path is in **frame-local coordinates**, like every other path here:
/// `(0, 0)` is the frame's top-left, which is what lets the frame move without
/// its geometry being rewritten.
pub fn path(bounds: DocRect, sides: u32, inset: f64) -> BezPath {
    let sides = sides.clamp(FEWEST_SIDES, MOST_SIDES);
    let inset = inset.clamp(0.0, 0.95);

    // Inscribed in the box rather than in a circle, so dragging out a wide
    // frame gives a wide polygon. A polygon that stayed circular in a
    // rectangular frame would ignore half of the gesture that made it.
    let (rx, ry) = (bounds.width / 2.0, bounds.height / 2.0);
    let (cx, cy) = (rx, ry);

    // Starting at the top, which is where every drawing tool starts one: a
    // triangle with a point at the top is the triangle somebody drew, and one
    // rotated by a twelfth of a turn is a mistake they have to correct.
    let start = -std::f64::consts::FRAC_PI_2;

    let mut path = BezPath::new();
    let corners = if inset > 0.0 { sides * 2 } else { sides };
    let corner_step = std::f64::consts::TAU / f64::from(corners);

    for corner in 0..corners {
        let angle = start + f64::from(corner) * corner_step;
        // Odd corners of a star are the inner ones. A plain polygon has no odd
        // corners in this sense, because `corners` is `sides`.
        let pull = if inset > 0.0 && corner % 2 == 1 {
            1.0 - inset
        } else {
            1.0
        };
        let point = Point::new(cx + rx * pull * angle.cos(), cy + ry * pull * angle.sin());
        if corner == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{PathEl, Shape};

    fn box_of(w: f64, h: f64) -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        }
    }

    fn corners(path: &BezPath) -> usize {
        path.elements()
            .iter()
            .filter(|el| !matches!(el, PathEl::ClosePath))
            .count()
    }

    #[test]
    fn a_polygon_has_the_sides_it_was_asked_for() {
        for sides in [3, 5, 6, 12] {
            let made = path(box_of(100.0, 100.0), sides, 0.0);
            assert_eq!(corners(&made), sides as usize, "{sides} sides");
        }
    }

    #[test]
    fn a_star_has_twice_as_many_points_as_sides() {
        // One out, one in, all the way round.
        let made = path(box_of(100.0, 100.0), 5, 0.5);
        assert_eq!(corners(&made), 10);
    }

    #[test]
    fn a_polygon_starts_at_the_top() {
        // A triangle with a point at the top is the triangle somebody drew.
        // One rotated by a twelfth of a turn is a mistake they have to correct.
        let made = path(box_of(100.0, 100.0), 3, 0.0);
        let Some(PathEl::MoveTo(first)) = made.elements().first() else {
            panic!("no starting point");
        };
        assert!((first.x - 50.0).abs() < 0.01, "not centred: {first:?}");
        assert!(first.y < 0.01, "not at the top: {first:?}");
    }

    #[test]
    fn a_polygon_fills_the_box_it_was_drawn_in() {
        // Inscribed in the box, not in a circle. A polygon that stayed circular
        // in a rectangular frame would ignore half the gesture that made it.
        let made = path(box_of(200.0, 80.0), 4, 0.0);
        let around = made.bounding_box();
        assert!((around.width() - 200.0).abs() < 0.01, "{around:?}");
        assert!((around.height() - 80.0).abs() < 0.01, "{around:?}");
    }

    #[test]
    fn a_polygon_is_closed() {
        // An open polygon fills differently and strokes very differently: the
        // join at the start would be two ends instead.
        let made = path(box_of(100.0, 100.0), 6, 0.0);
        assert!(matches!(made.elements().last(), Some(PathEl::ClosePath)));
    }

    #[test]
    fn too_few_sides_is_clamped_rather_than_drawn() {
        // Two points is a line and one is a dot. Neither encloses an area, and
        // a "polygon" that is a dot cannot be selected again.
        let made = path(box_of(100.0, 100.0), 1, 0.0);
        assert_eq!(corners(&made), FEWEST_SIDES as usize);
    }

    #[test]
    fn too_many_sides_is_clamped_too() {
        // Past the limit a polygon is a circle drawn expensively, and an
        // ellipse is one object rather than two hundred points.
        let made = path(box_of(100.0, 100.0), 100_000, 0.0);
        assert_eq!(corners(&made), MOST_SIDES as usize);
    }

    #[test]
    fn a_full_inset_still_leaves_a_shape() {
        // Clamped below one, because an inset of exactly one puts every inner
        // point at the centre and the star becomes a fan of zero-width spikes.
        let made = path(box_of(100.0, 100.0), 5, 1.0);
        let around = made.bounding_box();
        assert!(around.width() > 1.0 && around.height() > 1.0);
    }
}

//! Geometry for frames, backed by `kurbo`.
//!
//! Every frame has an outline in its own local space (origin at its top-left,
//! before scale or rotation) and an affine that places it in the document. All
//! bounds and hit tests derive from those two things, so a rotated ellipse and
//! a rotated bezier path are handled by exactly the same code path.
//!
//! This module is pure math and holds no GPU or ECS types.

use kurbo::{Affine, BezPath, Ellipse, Line, ParamCurveNearest, Point, Rect, Shape};

use crate::components::{BoundingBox, FrameType, PathData, Size, Transform};

/// Tolerance for flattening curves when computing bounds and hit tests.
///
/// A quarter of a document unit is well below what a user can perceive at
/// typical zoom, and keeps path operations cheap on large documents.
const FLATTEN_TOLERANCE: f64 = 0.25;

/// The outline of a frame in local space, before transform.
///
/// Local space runs from `(0, 0)` to `(width, height)`; the transform applies
/// scale, rotation and translation on top.
pub fn local_outline(frame_type: FrameType, size: &Size, path: Option<&PathData>) -> BezPath {
    let width = size.width.max(0.0) as f64;
    let height = size.height.max(0.0) as f64;

    match frame_type {
        FrameType::Ellipse => {
            let (rx, ry) = (width / 2.0, height / 2.0);
            Ellipse::new((rx, ry), (rx, ry), 0.0).to_path(FLATTEN_TOLERANCE)
        }
        FrameType::Line => Line::new((0.0, 0.0), (width, height)).to_path(FLATTEN_TOLERANCE),
        FrameType::Path => path
            .and_then(|data| BezPath::from_svg(&data.svg).ok())
            .filter(|parsed| !parsed.is_empty())
            // An unparseable or empty path falls back to its frame box, so the
            // entity stays selectable rather than becoming a click-through ghost.
            .unwrap_or_else(|| Rect::new(0.0, 0.0, width, height).to_path(FLATTEN_TOLERANCE)),
        // Rectangle, Text and Image frames are all rectangular boxes.
        _ => Rect::new(0.0, 0.0, width, height).to_path(FLATTEN_TOLERANCE),
    }
}

/// The affine placing a frame's local outline into document space.
///
/// Rotation is about the frame's own centre, matching how the renderer draws it.
pub fn frame_affine(transform: &Transform, size: &Size) -> Affine {
    let scaled_width = (size.width * transform.scale_x) as f64;
    let scaled_height = (size.height * transform.scale_y) as f64;
    let (cx, cy) = (scaled_width / 2.0, scaled_height / 2.0);

    let placement = Affine::translate((
        transform.position.x as f64,
        transform.position.y as f64,
    ));
    let scale = Affine::scale_non_uniform(transform.scale_x as f64, transform.scale_y as f64);

    if transform.rotation == 0.0 {
        return placement * scale;
    }

    placement
        * Affine::translate((cx, cy))
        * Affine::rotate(transform.rotation as f64)
        * Affine::translate((-cx, -cy))
        * scale
}

/// The exact axis-aligned bounds of a frame in document space.
///
/// Unlike an approximation from width and height, this is tight for rotated and
/// curved shapes because it measures the transformed outline itself.
pub fn frame_bounds(
    frame_type: FrameType,
    transform: &Transform,
    size: &Size,
    path: Option<&PathData>,
) -> BoundingBox {
    let mut outline = local_outline(frame_type, size, path);
    outline.apply_affine(frame_affine(transform, size));
    let rect = outline.bounding_box();

    BoundingBox::new(
        rect.x0 as f32,
        rect.y0 as f32,
        rect.x1 as f32,
        rect.y1 as f32,
    )
}

/// Whether a document-space point falls inside a frame's outline.
///
/// The point is mapped back into local space rather than transforming the
/// outline forward, which keeps this allocation-free for the common case.
pub fn frame_contains_point(
    frame_type: FrameType,
    transform: &Transform,
    size: &Size,
    path: Option<&PathData>,
    px: f32,
    py: f32,
) -> bool {
    let affine = frame_affine(transform, size);
    let Some(inverse) = invertible(affine) else {
        // A zero scale collapses the shape to nothing, so nothing can hit it.
        return false;
    };

    let local = inverse * Point::new(px as f64, py as f64);
    let outline = local_outline(frame_type, size, path);

    // An open shape such as a line has no interior to be inside of, so it is
    // hit by proximity to the stroke instead.
    if is_open(frame_type) {
        return distance_to_outline(&outline, local) <= line_hit_tolerance(transform);
    }

    outline.contains(local)
}

/// Frame types drawn as open strokes rather than closed areas.
fn is_open(frame_type: FrameType) -> bool {
    matches!(frame_type, FrameType::Line)
}

/// How far from a line a click may land and still count, in local units.
fn line_hit_tolerance(transform: &Transform) -> f64 {
    // Scale the tolerance back out so the grab area stays constant on screen.
    let scale = (transform.scale_x.abs()).max(transform.scale_y.abs()).max(0.001) as f64;
    4.0 / scale
}

/// Shortest distance from a point to a path's outline.
fn distance_to_outline(outline: &BezPath, point: Point) -> f64 {
    outline
        .segments()
        .map(|segment| segment.nearest(point, FLATTEN_TOLERANCE).distance_sq)
        .fold(f64::INFINITY, f64::min)
        .sqrt()
}

/// Returns the inverse of an affine, or `None` when it is degenerate.
fn invertible(affine: Affine) -> Option<Affine> {
    let [a, b, c, d, _, _] = affine.as_coeffs();
    let determinant = a * d - b * c;
    if determinant.abs() < f64::EPSILON {
        return None;
    }
    Some(affine.inverse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Position;

    fn transform_at(x: f32, y: f32) -> Transform {
        Transform {
            position: Position { x, y },
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }

    fn size(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    #[test]
    fn rectangle_bounds_match_position_and_size() {
        let bounds = frame_bounds(
            FrameType::Rectangle,
            &transform_at(10.0, 20.0),
            &size(100.0, 50.0),
            None,
        );

        assert!((bounds.min_x - 10.0).abs() < 1e-3);
        assert!((bounds.min_y - 20.0).abs() < 1e-3);
        assert!((bounds.max_x - 110.0).abs() < 1e-3);
        assert!((bounds.max_y - 70.0).abs() < 1e-3);
    }

    #[test]
    fn rotating_a_square_keeps_its_centre() {
        // The old approximation grew the box from the top-left corner, which
        // moved the centre. Exact bounds must leave it where it was.
        let transform = Transform {
            rotation: std::f32::consts::FRAC_PI_4,
            ..transform_at(0.0, 0.0)
        };
        let upright = frame_bounds(FrameType::Rectangle, &transform_at(0.0, 0.0), &size(100.0, 100.0), None);
        let rotated = frame_bounds(FrameType::Rectangle, &transform, &size(100.0, 100.0), None);

        let (ux, uy) = upright.center();
        let (rx, ry) = rotated.center();
        assert!((ux - rx).abs() < 1e-2, "centre moved in x: {ux} vs {rx}");
        assert!((uy - ry).abs() < 1e-2, "centre moved in y: {uy} vs {ry}");
    }

    #[test]
    fn a_square_rotated_45_degrees_grows_by_root_two() {
        let transform = Transform {
            rotation: std::f32::consts::FRAC_PI_4,
            ..transform_at(0.0, 0.0)
        };
        let bounds = frame_bounds(FrameType::Rectangle, &transform, &size(100.0, 100.0), None);

        let expected = 100.0 * std::f32::consts::SQRT_2;
        assert!(
            (bounds.width() - expected).abs() < 0.5,
            "expected width ~{expected}, got {}",
            bounds.width()
        );
    }

    #[test]
    fn ellipse_hit_test_excludes_the_corners() {
        // The decisive case for precise hit testing: a corner of the bounding
        // box is inside the AABB but outside the ellipse itself.
        let transform = transform_at(0.0, 0.0);
        let dimensions = size(100.0, 100.0);

        assert!(frame_contains_point(
            FrameType::Ellipse, &transform, &dimensions, None, 50.0, 50.0
        ));
        assert!(!frame_contains_point(
            FrameType::Ellipse, &transform, &dimensions, None, 2.0, 2.0
        ));
    }

    #[test]
    fn rectangle_hit_test_respects_edges() {
        let transform = transform_at(10.0, 10.0);
        let dimensions = size(80.0, 40.0);

        assert!(frame_contains_point(FrameType::Rectangle, &transform, &dimensions, None, 50.0, 30.0));
        assert!(!frame_contains_point(FrameType::Rectangle, &transform, &dimensions, None, 95.0, 30.0));
    }

    #[test]
    fn rotated_rectangle_hit_test_follows_the_rotation() {
        // A tall thin box rotated 90 degrees becomes wide and flat; a point off
        // its short axis must miss, and one along the long axis must hit.
        let transform = Transform {
            rotation: std::f32::consts::FRAC_PI_2,
            ..transform_at(0.0, 0.0)
        };
        let dimensions = size(20.0, 100.0);

        assert!(frame_contains_point(FrameType::Rectangle, &transform, &dimensions, None, 20.0, 50.0));
        assert!(!frame_contains_point(FrameType::Rectangle, &transform, &dimensions, None, 10.0, 5.0));
    }

    #[test]
    fn line_frames_are_hit_by_proximity() {
        let transform = transform_at(0.0, 0.0);
        let dimensions = size(100.0, 100.0);

        // On the diagonal.
        assert!(frame_contains_point(FrameType::Line, &transform, &dimensions, None, 50.0, 50.0));
        // Far off it.
        assert!(!frame_contains_point(FrameType::Line, &transform, &dimensions, None, 90.0, 10.0));
    }

    #[test]
    fn path_frames_use_their_svg_outline() {
        // A triangle occupying the lower-left half of a 100x100 box.
        let path = PathData {
            svg: "M 0 0 L 0 100 L 100 100 Z".to_string(),
        };
        let transform = transform_at(0.0, 0.0);
        let dimensions = size(100.0, 100.0);

        assert!(frame_contains_point(
            FrameType::Path, &transform, &dimensions, Some(&path), 20.0, 80.0
        ));
        // The opposite corner is inside the box but outside the triangle.
        assert!(!frame_contains_point(
            FrameType::Path, &transform, &dimensions, Some(&path), 90.0, 10.0
        ));
    }

    #[test]
    fn an_unparseable_path_falls_back_to_its_box() {
        let path = PathData {
            svg: "not a path".to_string(),
        };
        let transform = transform_at(0.0, 0.0);
        let dimensions = size(100.0, 100.0);

        assert!(frame_contains_point(
            FrameType::Path, &transform, &dimensions, Some(&path), 50.0, 50.0
        ));
    }

    #[test]
    fn a_degenerate_scale_cannot_be_hit() {
        let transform = Transform {
            scale_x: 0.0,
            ..transform_at(0.0, 0.0)
        };
        assert!(!frame_contains_point(
            FrameType::Rectangle, &transform, &size(100.0, 100.0), None, 0.0, 50.0
        ));
    }

    #[test]
    fn scaling_expands_bounds() {
        let transform = Transform {
            scale_x: 2.0,
            scale_y: 3.0,
            ..transform_at(0.0, 0.0)
        };
        let bounds = frame_bounds(FrameType::Rectangle, &transform, &size(50.0, 20.0), None);

        assert!((bounds.width() - 100.0).abs() < 1e-3);
        assert!((bounds.height() - 60.0).abs() < 1e-3);
    }
}

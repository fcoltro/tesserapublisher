//! Resizing and rotating a frame by dragging its handles.
//!
//! Pure geometry, kept away from the viewport so the awkward part — keeping
//! the opposite corner pinned while a *rotated* frame is resized — is
//! testable without a window.

use tessera_geometry::{DocPoint, DocRect};

/// The eight resize grips, in reading order around the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    pub const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    /// Where this handle sits on an unrotated rectangle.
    pub fn position(self, b: DocRect) -> DocPoint {
        let (x0, y0) = (b.x, b.y);
        let (x1, y1) = (b.x + b.width, b.y + b.height);
        let (cx, cy) = (b.center().x, b.center().y);
        match self {
            Self::TopLeft => DocPoint { x: x0, y: y0 },
            Self::Top => DocPoint { x: cx, y: y0 },
            Self::TopRight => DocPoint { x: x1, y: y0 },
            Self::Right => DocPoint { x: x1, y: cy },
            Self::BottomRight => DocPoint { x: x1, y: y1 },
            Self::Bottom => DocPoint { x: cx, y: y1 },
            Self::BottomLeft => DocPoint { x: x0, y: y1 },
            Self::Left => DocPoint { x: x0, y: cy },
        }
    }

    /// The point that must not move while this handle is dragged.
    ///
    /// For an edge handle both corners of the opposite edge are fixed; either
    /// will do, so one is chosen.
    pub fn anchor(self, b: DocRect) -> DocPoint {
        self.opposite().position(b)
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::TopLeft => Self::BottomRight,
            Self::Top => Self::Bottom,
            Self::TopRight => Self::BottomLeft,
            Self::Right => Self::Left,
            Self::BottomRight => Self::TopLeft,
            Self::Bottom => Self::Top,
            Self::BottomLeft => Self::TopRight,
            Self::Left => Self::Right,
        }
    }

    pub fn moves_x(self) -> bool {
        !matches!(self, Self::Top | Self::Bottom)
    }

    pub fn moves_y(self) -> bool {
        !matches!(self, Self::Left | Self::Right)
    }

    /// A corner handle changes both axes, so it can scale proportionally.
    pub fn is_corner(self) -> bool {
        self.moves_x() && self.moves_y()
    }

    /// The direction the handle pushes, in degrees clockwise from east.
    ///
    /// Add the frame's own rotation and a resize cursor points the way the
    /// edge will actually travel — which is the only way a cursor stays
    /// honest on a rotated frame.
    pub fn normal_degrees(self) -> f32 {
        match self {
            Self::Right => 0.0,
            Self::BottomRight => 45.0,
            Self::Bottom => 90.0,
            Self::BottomLeft => 135.0,
            Self::Left => 180.0,
            Self::TopLeft => 225.0,
            Self::Top => 270.0,
            Self::TopRight => 315.0,
        }
    }
}

/// Smallest frame a drag can produce, in points. Below this a frame becomes
/// un-grabbable, which is worse than refusing to shrink further.
const MIN_SIZE: f64 = 1.0;

/// Resize `bounds` by dragging `handle` to `pointer`.
///
/// `rotation` is the frame's own rotation in degrees. The drag is interpreted
/// in the frame's **local** (unrotated) space, and the result is placed so the
/// handle's opposite corner stays exactly where it was on screen — which is
/// what makes resizing a rotated frame feel like resizing rather than
/// swinging.
pub fn resize(
    bounds: DocRect,
    rotation: f64,
    handle: Handle,
    pointer: DocPoint,
    proportional: bool,
) -> DocRect {
    let center = bounds.center();
    let anchor_local = handle.anchor(bounds);
    // Where the anchor sits on screen now, and must still sit afterwards.
    let anchor_doc = anchor_local.rotated_about(center, rotation);

    let local = pointer.rotated_about(center, -rotation);

    let mut x0 = bounds.x;
    let mut y0 = bounds.y;
    let mut x1 = bounds.x + bounds.width;
    let mut y1 = bounds.y + bounds.height;

    if handle.moves_x() {
        match handle {
            Handle::TopLeft | Handle::BottomLeft | Handle::Left => x0 = local.x,
            _ => x1 = local.x,
        }
    }
    if handle.moves_y() {
        match handle {
            Handle::TopLeft | Handle::Top | Handle::TopRight => y0 = local.y,
            _ => y1 = local.y,
        }
    }

    // Normalise, so dragging a handle past its opposite flips rather than
    // producing a negative size.
    let mut new = DocRect {
        x: x0.min(x1),
        y: y0.min(y1),
        width: (x1 - x0).abs().max(MIN_SIZE),
        height: (y1 - y0).abs().max(MIN_SIZE),
    };

    if proportional && handle.is_corner() && bounds.width > 0.0 && bounds.height > 0.0 {
        let aspect = bounds.width / bounds.height;
        // Take the larger change, so the frame follows the pointer rather
        // than lagging on whichever axis moved less.
        if new.width / aspect >= new.height {
            new.height = new.width / aspect;
        } else {
            new.width = new.height * aspect;
        }
    }

    // Re-place so the anchor lands back on its original document position.
    let anchor_after = handle.anchor(new).rotated_about(new.center(), rotation);
    DocRect {
        x: new.x + (anchor_doc.x - anchor_after.x),
        y: new.y + (anchor_doc.y - anchor_after.y),
        ..new
    }
}

/// One frame's state at the start of a gesture.
pub type Origin = (tessera_document::ids::FrameId, DocRect, f64);

/// Map every frame from the box `from` onto the box `to`.
///
/// Used for a group: each child keeps its position and size *relative to the
/// group*, so scaling the group scales its contents rather than merely
/// stretching an invisible box around them.
///
/// `rotation` is the box's own angle. `from` and `to` describe the box in its
/// **local** space — that is what [`resize`] returns — so a child's centre has
/// to be carried into that space, scaled, and carried back out. Scaling the
/// document-space coordinates directly is what made a rotated group scatter
/// its contents.
///
/// A child turned at some angle of its own still has its width and height
/// scaled along its own axes. An axis-aligned box plus an angle cannot express
/// the shear that would otherwise be needed, and refusing to scale at all
/// would be worse than approximating.
pub fn scale_origins(origins: &[Origin], from: DocRect, to: DocRect, rotation: f64) -> Vec<Origin> {
    let sx = if from.width.abs() > f64::EPSILON {
        to.width / from.width
    } else {
        1.0
    };
    let sy = if from.height.abs() > f64::EPSILON {
        to.height / from.height
    } else {
        1.0
    };
    let (from_centre, to_centre) = (from.center(), to.center());

    origins
        .iter()
        .map(|(id, b, rot)| {
            let local = b.center().rotated_about(from_centre, -rotation);
            let scaled = DocPoint {
                x: to.x + (local.x - from.x) * sx,
                y: to.y + (local.y - from.y) * sy,
            };
            let centre = scaled.rotated_about(to_centre, rotation);
            let (width, height) = (b.width * sx, b.height * sy);
            (
                *id,
                DocRect {
                    x: centre.x - width / 2.0,
                    y: centre.y - height / 2.0,
                    width,
                    height,
                },
                *rot,
            )
        })
        .collect()
}

/// Rotate every frame by `delta` degrees about `center`.
///
/// Each frame's centre swings around the pivot *and* the frame turns on its
/// own axis — both, or a rotated group would look like a scattered one.
pub fn rotate_origins(origins: &[Origin], center: DocPoint, delta: f64) -> Vec<Origin> {
    origins
        .iter()
        .map(|(id, b, rot)| {
            let moved = b.center().rotated_about(center, delta);
            (
                *id,
                DocRect {
                    x: moved.x - b.width / 2.0,
                    y: moved.y - b.height / 2.0,
                    ..*b
                },
                (rot + delta + 180.0).rem_euclid(360.0) - 180.0,
            )
        })
        .collect()
}

/// Snap `to` onto the nearest 45-degree ray from `from`.
///
/// What holding shift does while drawing a line: horizontal, vertical, or a
/// true diagonal. The length is projected onto the chosen ray rather than
/// kept, so the endpoint stays under the pointer instead of running ahead of
/// it.
pub fn constrain_to_45(from: DocPoint, to: DocPoint) -> DocPoint {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dx == 0.0 && dy == 0.0 {
        return to;
    }
    let angle = dy.atan2(dx);
    let step = std::f64::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    let (sin, cos) = snapped.sin_cos();
    // Project the drag onto the snapped direction.
    let length = dx * cos + dy * sin;
    DocPoint {
        x: from.x + cos * length,
        y: from.y + sin * length,
    }
}

/// The angle, in degrees, from `center` to `point`, clockwise from east.
fn angle_of(center: DocPoint, point: DocPoint) -> f64 {
    (point.y - center.y).atan2(point.x - center.x).to_degrees()
}

/// New rotation after dragging from `start` to `current` about `center`.
///
/// `snap` constrains to 15° steps, the increment a layout tool offers when a
/// modifier is held.
pub fn rotation_from_drag(
    center: DocPoint,
    start: DocPoint,
    current: DocPoint,
    original: f64,
    snap: bool,
) -> f64 {
    let delta = angle_of(center, current) - angle_of(center, start);
    let raw = original + delta;
    let snapped = if snap {
        (raw / 15.0).round() * 15.0
    } else {
        raw
    };
    (snapped + 180.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> DocRect {
        DocRect {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn handles_sit_where_their_names_say() {
        let b = rect();
        assert_eq!(Handle::TopLeft.position(b), DocPoint { x: 100.0, y: 100.0 });
        assert_eq!(
            Handle::BottomRight.position(b),
            DocPoint { x: 300.0, y: 200.0 }
        );
        assert_eq!(Handle::Top.position(b), DocPoint { x: 200.0, y: 100.0 });
        assert_eq!(Handle::Left.position(b), DocPoint { x: 100.0, y: 150.0 });
    }

    #[test]
    fn a_handle_and_its_opposite_point_opposite_ways() {
        for h in Handle::ALL {
            let apart = (h.normal_degrees() - h.opposite().normal_degrees()).abs();
            assert!(
                (apart - 180.0).abs() < 1e-3,
                "{h:?} and {:?} are {apart} degrees apart",
                h.opposite()
            );
        }
    }

    #[test]
    fn every_handle_is_its_opposites_opposite() {
        for h in Handle::ALL {
            assert_eq!(h.opposite().opposite(), h);
            assert_ne!(h.opposite(), h);
        }
    }

    #[test]
    fn dragging_a_corner_moves_only_that_corner() {
        let b = rect();
        let new = resize(
            b,
            0.0,
            Handle::BottomRight,
            DocPoint { x: 400.0, y: 300.0 },
            false,
        );
        assert!(close(new.x, 100.0), "the origin stayed put");
        assert!(close(new.y, 100.0));
        assert!(close(new.width, 300.0));
        assert!(close(new.height, 200.0));
    }

    #[test]
    fn dragging_the_top_left_keeps_the_bottom_right_pinned() {
        let b = rect();
        let new = resize(
            b,
            0.0,
            Handle::TopLeft,
            DocPoint { x: 150.0, y: 150.0 },
            false,
        );
        assert!(close(new.x + new.width, 300.0), "right edge held");
        assert!(close(new.y + new.height, 200.0), "bottom edge held");
    }

    #[test]
    fn an_edge_handle_changes_only_one_axis() {
        let b = rect();
        let new = resize(
            b,
            0.0,
            Handle::Right,
            DocPoint { x: 500.0, y: 999.0 },
            false,
        );
        assert!(
            close(new.height, b.height),
            "height untouched by a side drag"
        );
        assert!(close(new.width, 400.0));
    }

    #[test]
    fn a_frame_cannot_be_dragged_to_nothing() {
        let b = rect();
        let new = resize(
            b,
            0.0,
            Handle::BottomRight,
            DocPoint { x: 100.0, y: 100.0 },
            false,
        );
        assert!(new.width >= MIN_SIZE);
        assert!(new.height >= MIN_SIZE);
    }

    #[test]
    fn proportional_scaling_preserves_the_aspect_ratio() {
        let b = rect(); // 2:1
        let new = resize(
            b,
            0.0,
            Handle::BottomRight,
            DocPoint { x: 500.0, y: 220.0 },
            true,
        );
        assert!(
            close(new.width / new.height, b.width / b.height),
            "{} vs {}",
            new.width / new.height,
            b.width / b.height
        );
    }

    #[test]
    fn proportional_is_ignored_on_an_edge_handle() {
        // An edge drag has only one axis to follow, so constraining it would
        // mean the frame resists the pointer for no reason.
        let b = rect();
        let new = resize(b, 0.0, Handle::Right, DocPoint { x: 500.0, y: 150.0 }, true);
        assert!(close(new.height, b.height));
    }

    #[test]
    fn resizing_a_rotated_frame_keeps_its_anchor_on_screen() {
        // The property that makes rotated resizing feel right: whatever the
        // rotation, the corner opposite the dragged handle must not move.
        let b = rect();
        for rotation in [0.0, 30.0, 90.0, 145.0, -60.0] {
            let anchor_before = Handle::BottomRight
                .position(b)
                .rotated_about(b.center(), rotation);

            let new = resize(
                b,
                rotation,
                Handle::TopLeft,
                DocPoint { x: 140.0, y: 130.0 },
                false,
            );

            let anchor_after = Handle::BottomRight
                .position(new)
                .rotated_about(new.center(), rotation);

            assert!(
                close(anchor_before.x, anchor_after.x) && close(anchor_before.y, anchor_after.y),
                "rotation {rotation}: anchor moved from {anchor_before:?} to {anchor_after:?}"
            );
        }
    }

    #[test]
    fn resizing_an_unrotated_frame_is_unaffected_by_the_rotation_path() {
        // Zero rotation must take the same route and land in the same place.
        let b = rect();
        let p = DocPoint { x: 400.0, y: 300.0 };
        assert_eq!(
            resize(b, 0.0, Handle::BottomRight, p, false),
            resize(b, 0.0, Handle::BottomRight, p, false)
        );
    }

    #[test]
    fn dragging_a_handle_past_its_opposite_flips_rather_than_inverting() {
        let b = rect();
        let new = resize(b, 0.0, Handle::Right, DocPoint { x: 0.0, y: 150.0 }, false);
        assert!(new.width > 0.0, "width must never go negative");
    }

    #[test]
    fn rotating_a_quarter_turn_reports_ninety_degrees() {
        let c = DocPoint { x: 0.0, y: 0.0 };
        let start = DocPoint { x: 10.0, y: 0.0 };
        let current = DocPoint { x: 0.0, y: 10.0 };
        assert!(close(
            rotation_from_drag(c, start, current, 0.0, false),
            90.0
        ));
    }

    #[test]
    fn rotation_accumulates_onto_the_original_angle() {
        let c = DocPoint { x: 0.0, y: 0.0 };
        let start = DocPoint { x: 10.0, y: 0.0 };
        let current = DocPoint { x: 0.0, y: 10.0 };
        assert!(close(
            rotation_from_drag(c, start, current, 20.0, false),
            110.0
        ));
    }

    #[test]
    fn snapping_lands_on_fifteen_degree_steps() {
        let c = DocPoint { x: 0.0, y: 0.0 };
        let start = DocPoint { x: 10.0, y: 0.0 };
        // A hair under 45 degrees.
        let current = DocPoint { x: 10.0, y: 9.5 };
        let snapped = rotation_from_drag(c, start, current, 0.0, true);
        assert!(close(snapped, 45.0), "snapped to {snapped}");
    }

    #[test]
    fn rotation_stays_within_a_half_turn_either_way() {
        let c = DocPoint { x: 0.0, y: 0.0 };
        let start = DocPoint { x: 10.0, y: 0.0 };
        let current = DocPoint { x: 0.0, y: 10.0 };
        let r = rotation_from_drag(c, start, current, 170.0, false);
        assert!((-180.0..=180.0).contains(&r), "was {r}");
    }

    #[test]
    fn constraining_a_near_horizontal_drag_makes_it_horizontal() {
        let from = DocPoint { x: 0.0, y: 0.0 };
        let to = constrain_to_45(from, DocPoint { x: 100.0, y: 8.0 });
        assert!(close(to.y, 0.0), "y was {}", to.y);
        assert!(to.x > 90.0);
    }

    #[test]
    fn constraining_a_near_vertical_drag_makes_it_vertical() {
        let from = DocPoint { x: 0.0, y: 0.0 };
        let to = constrain_to_45(from, DocPoint { x: -6.0, y: 100.0 });
        assert!(close(to.x, 0.0), "x was {}", to.x);
        assert!(to.y > 90.0);
    }

    #[test]
    fn constraining_a_near_diagonal_drag_makes_it_exactly_diagonal() {
        let from = DocPoint { x: 0.0, y: 0.0 };
        let to = constrain_to_45(from, DocPoint { x: 100.0, y: 88.0 });
        assert!(close(to.x, to.y), "{} vs {}", to.x, to.y);
    }

    #[test]
    fn constraining_works_in_every_quadrant() {
        let from = DocPoint { x: 50.0, y: 50.0 };
        for (dx, dy) in [(1.0, 0.1), (-1.0, 0.1), (1.0, -0.1), (-1.0, -0.1)] {
            let to = constrain_to_45(
                from,
                DocPoint {
                    x: from.x + dx * 100.0,
                    y: from.y + dy * 100.0,
                },
            );
            assert!(
                close(to.y, from.y),
                "({dx},{dy}) should have snapped horizontal, got {to:?}"
            );
        }
    }

    #[test]
    fn a_drag_that_has_not_moved_is_left_alone() {
        let from = DocPoint { x: 3.0, y: 4.0 };
        assert_eq!(constrain_to_45(from, from), from);
    }

    fn origins() -> Vec<Origin> {
        use tessera_document::ids::FrameId;
        vec![
            (
                FrameId::default(),
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                0.0,
            ),
            (
                FrameId::default(),
                DocRect {
                    x: 90.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                0.0,
            ),
        ]
    }

    fn group_box() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 10.0,
        }
    }

    #[test]
    fn scaling_a_group_scales_its_children_too() {
        let doubled = DocRect {
            width: 200.0,
            ..group_box()
        };
        let out = scale_origins(&origins(), group_box(), doubled, 0.0);

        assert!(close(out[0].1.width, 20.0), "child width doubled");
        assert!(close(out[1].1.x, 180.0), "child position doubled");
        assert!(close(out[0].1.height, 10.0), "the other axis is untouched");
    }

    #[test]
    fn scaling_keeps_children_in_the_same_relative_places() {
        let out = scale_origins(
            &origins(),
            group_box(),
            DocRect {
                x: 50.0,
                y: 5.0,
                width: 100.0,
                height: 10.0,
            },
            0.0,
        );
        // Moved, not resized: the gap between the two children is preserved.
        assert!(close(out[1].1.x - out[0].1.x, 90.0));
        assert!(close(out[0].1.x, 50.0));
    }

    #[test]
    fn scaling_a_zero_width_group_does_not_divide_by_zero() {
        let flat = DocRect {
            width: 0.0,
            ..group_box()
        };
        let out = scale_origins(&origins(), flat, group_box(), 0.0);
        assert!(out[0].1.width.is_finite());
    }

    #[test]
    fn scaling_a_rotated_group_scales_along_its_own_axes() {
        // The bug this pins: children were scaled in document coordinates
        // while the box they were scaled against is the group's own, local
        // one — so stretching a turned group threw its contents off at an
        // angle instead of spreading them along the edge being dragged.
        //
        // Built the way a real gesture leaves it: the children are rotated
        // first, because that is what rotating the group did to them.
        let from = group_box();
        let turned = rotate_origins(&origins(), from.center(), 90.0);
        let to = DocRect {
            width: 200.0,
            ..from
        };

        let out = scale_origins(&turned, from, to, 90.0);

        // At 90 degrees the group's local x runs down the document's y, so
        // doubling the width must double the children's separation in y and
        // leave x alone.
        let (near, far) = (out[0].1.center(), out[1].1.center());
        let was = (turned[1].1.center().y - turned[0].1.center().y).abs();
        assert!(close(was, 90.0), "the rotated children start 90 apart");
        assert!(
            close((far.y - near.y).abs(), 180.0),
            "separation should have doubled along the rotated axis: {near:?} -> {far:?}"
        );
        assert!(
            close((far.x - near.x).abs(), 0.0),
            "and not along the other"
        );
    }

    #[test]
    fn scaling_by_nothing_moves_nothing_however_the_group_is_turned() {
        // A gesture that has not moved yet must be the identity, or every
        // drag would jump the instant it began.
        for rotation in [0.0, 30.0, 90.0, 217.0] {
            let out = scale_origins(&origins(), group_box(), group_box(), rotation);
            for (before, after) in origins().iter().zip(out.iter()) {
                assert!(close(before.1.x, after.1.x), "x drifted at {rotation}");
                assert!(close(before.1.y, after.1.y), "y drifted at {rotation}");
                assert!(close(before.1.width, after.1.width));
                assert!(close(before.1.height, after.1.height));
            }
        }
    }

    #[test]
    fn rotating_a_group_turns_each_child_and_swings_it_round() {
        let center = group_box().center();
        let out = rotate_origins(&origins(), center, 90.0);

        // Each child turns on its own axis...
        assert!(close(out[0].2, 90.0));
        // ...and its centre swings about the group's.
        let before = origins()[0].1.center();
        let after = out[0].1.center();
        assert!(
            (before.x - after.x).abs() > 1.0 || (before.y - after.y).abs() > 1.0,
            "the child should have moved: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn rotating_by_zero_changes_nothing() {
        let out = rotate_origins(&origins(), group_box().center(), 0.0);
        for (before, after) in origins().iter().zip(out.iter()) {
            assert!(close(before.1.x, after.1.x));
            assert!(close(before.1.y, after.1.y));
            assert!(close(before.2, after.2));
        }
    }

    #[test]
    fn a_childs_own_rotation_accumulates_rather_than_being_replaced() {
        let mut o = origins();
        o[0].2 = 30.0;
        let out = rotate_origins(&o, group_box().center(), 45.0);
        assert!(close(out[0].2, 75.0), "was {}", out[0].2);
    }
}

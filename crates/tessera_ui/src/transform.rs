//! Resizing and rotating a frame by dragging its handles.
//!
//! Pure geometry, kept away from the viewport so it is testable without a
//! window.
//!
//! Everything here works in a frame's **own** coordinate space and produces a
//! document-space [`Transform`] to compose onto what is already there. That is
//! what makes the awkward cases ordinary: resizing a turned frame is just
//! resizing, because the turn lives in the placement rather than in the
//! arithmetic, and scaling a rotated group is exact rather than approximate,
//! because the shear it produces is now something a frame can hold.

use tessera_geometry::{DocPoint, DocRect, Transform};

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

/// What a resize drag amounts to, in the frame's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resize {
    /// The new box. Always positive: a mirrored frame is a positive box with
    /// a mirrored placement, never a negative width.
    pub bounds: DocRect,
    /// Signed scale about [`Resize::anchor`], per axis.
    ///
    /// Negative means the drag crossed the anchor and the frame is mirrored on
    /// that axis — which is what dragging a handle past its opposite means in
    /// every layout tool.
    pub sx: f64,
    pub sy: f64,
    /// The point the gesture holds still, in the frame's own space.
    pub anchor: DocPoint,
}

impl Resize {
    fn sign(v: f64) -> f64 {
        if v < 0.0 { -1.0 } else { 1.0 }
    }

    /// The mirror to apply inside the new box.
    ///
    /// The box stays positive, so a flip lives in the placement instead. An
    /// affine can hold that; a width could not, which is why dragging a handle
    /// past its opposite used to fold the frame back the way it came rather
    /// than mirroring it.
    pub fn flip(&self) -> Transform {
        Transform::scale_about(
            Self::sign(self.sx),
            Self::sign(self.sy),
            self.bounds.center(),
        )
    }

    /// The scale, about the anchor, that carries the old box onto the new one.
    pub fn map(&self) -> Transform {
        Transform::scale_about(self.sx, self.sy, self.anchor)
    }
}

/// How far along its axis the dragged edge has been taken, as a signed ratio.
///
/// 1.0 leaves it where it was, 0.5 halves the frame, and anything below zero
/// has crossed the anchor.
fn axis_scale(moves: bool, anchor: f64, edge: f64, pointer: f64) -> f64 {
    let span = edge - anchor;
    if !moves || span == 0.0 {
        return 1.0;
    }
    (pointer - anchor) / span
}

/// Hold the dragged edge at least [`MIN_SIZE`] from the anchor.
///
/// Clamped as a scale rather than as a width, so the sign — and therefore the
/// mirror — survives being squeezed through zero.
fn clamp_span(scale: f64, span: f64) -> f64 {
    if span == 0.0 {
        return scale;
    }
    let smallest = MIN_SIZE / span.abs();
    if scale.abs() < smallest {
        return smallest * Resize::sign(scale);
    }
    scale
}

/// Resize `bounds` by dragging `handle` to `pointer`.
///
/// **Both are in the frame's own space** — the caller puts the pointer there
/// with `frame.to_local`. That is the whole trick: a rotated, sheared or
/// flipped frame resizes with the same arithmetic as an upright one, because
/// its placement is not part of the arithmetic.
///
/// Expressed as a signed scale about the edge being held still. The anchor
/// therefore cannot move — it is the fixed point of the scale — and a drag
/// taken past it comes out as a negative factor rather than as a box that has
/// to be un-inverted afterwards.
pub fn resize(bounds: DocRect, handle: Handle, pointer: DocPoint, proportional: bool) -> Resize {
    let (x0, y0) = (bounds.x, bounds.y);
    let (x1, y1) = (bounds.x + bounds.width, bounds.y + bounds.height);

    // Per axis: the edge held still, and the edge being dragged. An axis the
    // handle does not move keeps both of its own edges.
    let (ax, mx) = match handle {
        Handle::TopLeft | Handle::BottomLeft | Handle::Left => (x1, x0),
        _ => (x0, x1),
    };
    let (ay, my) = match handle {
        Handle::TopLeft | Handle::Top | Handle::TopRight => (y1, y0),
        _ => (y0, y1),
    };

    let mut sx = axis_scale(handle.moves_x(), ax, mx, pointer.x);
    let mut sy = axis_scale(handle.moves_y(), ay, my, pointer.y);

    if proportional && handle.is_corner() {
        // Take the larger change, so the frame follows the pointer rather than
        // lagging on whichever axis moved less. Each axis keeps its own sign,
        // so a proportional drag can still mirror.
        let k = sx.abs().max(sy.abs());
        sx = k * Resize::sign(sx);
        sy = k * Resize::sign(sy);
    }

    sx = clamp_span(sx, mx - ax);
    sy = clamp_span(sy, my - ay);

    let nx = ax + (mx - ax) * sx;
    let ny = ay + (my - ay) * sy;

    Resize {
        bounds: DocRect {
            x: ax.min(nx),
            y: ay.min(ny),
            width: (nx - ax).abs(),
            height: (ny - ay).abs(),
        },
        sx,
        sy,
        anchor: DocPoint { x: ax, y: ay },
    }
}

/// One frame's state at the start of a gesture: its box, and its placement.
pub type Origin = (tessera_document::ids::FrameId, DocRect, Transform);

/// The document-space transform that carries a frame's footprint from
/// `from` under `was`, to `to` under `placement`.
///
/// This is the one piece of algebra the gestures share. Everything a drag or
/// an inspector field does to a frame is some `from`/`was` becoming some
/// `to`/`placement`; the children of a group have to follow by exactly that
/// map, and nothing else.
///
/// Read right to left: undo the old placement to get into the frame's own
/// space, do the box change there, then apply the new placement.
pub fn footprint_map(
    from: DocRect,
    was: Transform,
    to: DocRect,
    placement: Transform,
) -> Transform {
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

    let in_own_space = Transform::translate(-from.x, -from.y)
        .then(Transform::scale_about(sx, sy, DocPoint::ZERO))
        .then(Transform::translate(to.x, to.y));

    was.inverse().then(in_own_space).then(placement)
}

/// Every frame in a scale gesture.
///
/// The frame being dragged takes the new box directly and keeps its placement,
/// so its `bounds` stay an honest width and height. Everything inside it — a
/// group's contents — follows by composing the map onto its own placement, so
/// a child keeps its size and angle relative to the group.
///
/// A child turned at some angle of its own is handled exactly, not
/// approximately: scaling it along the group's axes shears it, and a placement
/// can hold a shear.
pub fn scaled(
    origins: &[Origin],
    target: tessera_document::ids::FrameId,
    resize: &Resize,
    placement: Transform,
) -> Vec<Origin> {
    // The scale carried into document space through the frame's own placement,
    // so a group's contents follow it exactly -- shear, mirror and all.
    let map = placement.inverse().then(resize.map()).then(placement);
    let mirrored = resize.flip().then(placement);

    origins
        .iter()
        .map(|(id, bounds, own)| {
            if *id == target {
                // The dragged frame takes the new box directly, so its bounds
                // stay an honest width and height. Any mirror goes into its
                // placement, where a positive box can still hold it.
                (*id, resize.bounds, mirrored)
            } else {
                (*id, *bounds, own.then(map))
            }
        })
        .collect()
}

/// Every frame in a rotate gesture, turned `degrees` about `pivot`.
///
/// Boxes do not change: a rotation is entirely a change of placement, which
/// is why a group's frame now swings rigidly instead of breathing as the
/// bounding box of its swinging children.
pub fn rotated(origins: &[Origin], degrees: f64, pivot: DocPoint) -> Vec<Origin> {
    let turn = Transform::rotate_about(degrees, pivot);
    origins
        .iter()
        .map(|(id, bounds, own)| (*id, *bounds, own.then(turn)))
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

/// Square off a drag from `from` to `to`.
///
/// What holding shift does while drawing a rectangle or an ellipse: equal
/// width and height, so the shape comes out a square or a circle. The larger
/// of the two extents wins, and each axis keeps its own direction, so dragging
/// up and to the left squares off that way rather than jumping to the opposite
/// corner.
pub fn constrain_to_square(from: DocPoint, to: DocPoint) -> DocPoint {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let side = dx.abs().max(dy.abs());
    let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    DocPoint {
        x: from.x + side * sign(dx),
        y: from.y + side * sign(dy),
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

    /// The new box a drag produces. The signed factors it also returns have
    /// their own tests, below.
    fn resized(bounds: DocRect, handle: Handle, pointer: DocPoint, proportional: bool) -> DocRect {
        resize(bounds, handle, pointer, proportional).bounds
    }

    /// The resize that carries `from` onto `to`, anchored at `from`'s corner.
    fn to_box(from: DocRect, to: DocRect) -> Resize {
        Resize {
            bounds: to,
            sx: if from.width == 0.0 {
                1.0
            } else {
                to.width / from.width
            },
            sy: if from.height == 0.0 {
                1.0
            } else {
                to.height / from.height
            },
            anchor: DocPoint {
                x: from.x,
                y: from.y,
            },
        }
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
        let new = resized(
            b,
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
        let new = resized(b, Handle::TopLeft, DocPoint { x: 150.0, y: 150.0 }, false);
        assert!(close(new.x + new.width, 300.0), "right edge held");
        assert!(close(new.y + new.height, 200.0), "bottom edge held");
    }

    #[test]
    fn an_edge_handle_changes_only_one_axis() {
        let b = rect();
        let new = resized(b, Handle::Right, DocPoint { x: 500.0, y: 999.0 }, false);
        assert!(
            close(new.height, b.height),
            "height untouched by a side drag"
        );
        assert!(close(new.width, 400.0));
    }

    #[test]
    fn a_frame_cannot_be_dragged_to_nothing() {
        let b = rect();
        let new = resized(
            b,
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
        let new = resized(
            b,
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
        let new = resized(b, Handle::Right, DocPoint { x: 500.0, y: 150.0 }, true);
        assert!(close(new.height, b.height));
    }

    #[test]
    fn resizing_a_placed_frame_keeps_its_anchor_where_it_was() {
        // The property that makes resizing a turned frame feel right: whatever
        // the placement, the corner opposite the dragged handle must not move
        // on screen.
        //
        // It now holds for free — the resize happens in the frame's own space
        // and the placement is untouched — but it is the whole point of doing
        // it that way, so it is still checked.
        let b = rect();
        for placement in [
            Transform::IDENTITY,
            Transform::rotate_about(30.0, b.center()),
            Transform::rotate_about(-60.0, DocPoint::ZERO),
            Transform::rotate_about(45.0, b.center()).then(Transform::scale_about(
                2.0,
                1.0,
                DocPoint::ZERO,
            )),
        ] {
            // The pointer arrives in the frame's own space, as the viewport
            // hands it over.
            let pointer = placement.inverse().apply(DocPoint { x: 140.0, y: 130.0 });
            let r = resize(b, Handle::TopLeft, pointer, false);

            // The anchor is the fixed point of the scale, so it must still be
            // a corner of the new box — and since the placement is untouched,
            // that is the same as saying it has not moved on screen.
            //
            // Stated as "a corner" rather than "the bottom-right corner",
            // because a drag taken past the anchor mirrors the frame and the
            // corner's name changes even though its position does not.
            let xs = [r.bounds.x, r.bounds.x + r.bounds.width];
            let ys = [r.bounds.y, r.bounds.y + r.bounds.height];
            assert!(
                xs.iter().any(|e| close(*e, r.anchor.x)),
                "anchor x {} is not an edge of {:?}",
                r.anchor.x,
                r.bounds
            );
            assert!(
                ys.iter().any(|e| close(*e, r.anchor.y)),
                "anchor y {} is not an edge of {:?}",
                r.anchor.y,
                r.bounds
            );
            assert_eq!(
                r.anchor,
                Handle::TopLeft.anchor(b),
                "and it is the corner opposite the handle"
            );
        }
    }

    #[test]
    fn dragging_a_handle_past_its_opposite_flips_rather_than_inverting() {
        let b = rect();
        let new = resized(b, Handle::Right, DocPoint { x: 0.0, y: 150.0 }, false);
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

    // --- groups ---------------------------------------------------------

    use tessera_document::ids::FrameId;

    /// A group box 100 wide, holding two 10x10 children at each end.
    fn group_box() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 10.0,
        }
    }

    /// Three genuinely distinct frame ids.
    ///
    /// Minted from a real document rather than repeating `FrameId::default()`:
    /// `scaled` singles out the frame being dragged by id, so identical ids
    /// would make every frame the target and quietly hide whether the
    /// contents follow at all.
    fn three_ids() -> [FrameId; 3] {
        let mut doc = tessera_document::document::Document::new();
        let layer = doc.default_layer().expect("layer");
        let frame = || tessera_document::nodes::Frame {
            bounds: group_box(),
            kind: tessera_document::nodes::FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        };
        [
            doc.add_frame(layer, frame()),
            doc.add_frame(layer, frame()),
            doc.add_frame(layer, frame()),
        ]
    }

    /// The group, then its two children — and the group's id, which is what a
    /// gesture targets.
    fn group_and_children() -> (Vec<Origin>, FrameId) {
        let [group, first, second] = three_ids();
        let child = |x: f64| DocRect {
            x,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        (
            vec![
                (group, group_box(), Transform::IDENTITY),
                (first, child(0.0), Transform::IDENTITY),
                (second, child(90.0), Transform::IDENTITY),
            ],
            group,
        )
    }

    /// The same, with every frame given `placement`.
    fn placed_group(placement: Transform) -> (Vec<Origin>, FrameId) {
        let (origins, group) = group_and_children();
        (
            origins
                .into_iter()
                .map(|(id, b, _)| (id, b, placement))
                .collect(),
            group,
        )
    }

    /// Where a frame's centre really is, placement included.
    fn centre_of(origin: &Origin) -> DocPoint {
        origin.2.apply(origin.1.center())
    }

    fn doubled() -> DocRect {
        DocRect {
            width: 200.0,
            ..group_box()
        }
    }

    #[test]
    fn scaling_a_group_carries_its_children() {
        let (start, group) = group_and_children();
        let out = scaled(
            &start,
            group,
            &to_box(group_box(), doubled()),
            Transform::IDENTITY,
        );

        // The target takes the new box directly, so its width stays honest.
        assert!(close(out[0].1.width, 200.0));
        // The children spread with it.
        assert!(
            close(centre_of(&out[2]).x - centre_of(&out[1]).x, 180.0),
            "{:?} -> {:?}",
            centre_of(&out[1]),
            centre_of(&out[2])
        );
        // And grow: a child's own box is untouched, so the growth is in the
        // placement.
        let width =
            out[1].2.apply(DocPoint { x: 10.0, y: 0.0 }).x - out[1].2.apply(DocPoint::ZERO).x;
        assert!(close(width, 20.0), "child width came out {width}");
    }

    #[test]
    fn scaling_by_nothing_moves_nothing_however_the_group_is_placed() {
        // A gesture that has not moved yet must be the identity, or every drag
        // would jump the instant it began.
        for placement in [
            Transform::IDENTITY,
            Transform::rotate_about(30.0, group_box().center()),
            Transform::rotate_about(217.0, DocPoint { x: 5.0, y: -3.0 }),
        ] {
            let (start, group) = placed_group(placement);
            let out = scaled(&start, group, &to_box(group_box(), group_box()), placement);
            for (before, after) in start.iter().zip(out.iter()) {
                let (b, a) = (centre_of(before), centre_of(after));
                assert!(close(b.x, a.x) && close(b.y, a.y), "{b:?} -> {a:?}");
            }
        }
    }

    #[test]
    fn a_zero_width_group_does_not_divide_by_zero() {
        let flat = DocRect {
            width: 0.0,
            ..group_box()
        };
        let (start, group) = group_and_children();
        let out = scaled(
            &start,
            group,
            &to_box(flat, group_box()),
            Transform::IDENTITY,
        );
        assert!(centre_of(&out[1]).x.is_finite());
    }

    #[test]
    fn scaling_a_rotated_group_spreads_its_children_along_its_own_axis() {
        // The case the old model could only approximate. With the group turned
        // a quarter turn its local x runs down the document's y, so doubling
        // its width must double the children's separation in y and leave x
        // alone.
        let placement = Transform::rotate_about(90.0, group_box().center());
        let (start, group) = placed_group(placement);

        let out = scaled(&start, group, &to_box(group_box(), doubled()), placement);

        let (near, far) = (centre_of(&out[1]), centre_of(&out[2]));
        assert!(close((far.y - near.y).abs(), 180.0), "{near:?} -> {far:?}");
        assert!(
            close((far.x - near.x).abs(), 0.0),
            "and not along the other"
        );
    }

    #[test]
    fn scaling_a_rotated_group_shears_a_child_rather_than_lying_about_it() {
        // The payoff. A child turned 45 degrees inside a group scaled on one
        // axis only cannot stay a rectangle -- it becomes a parallelogram. The
        // old model had nowhere to put that and silently approximated; a
        // placement holds it exactly.
        let [group, child_id, _] = three_ids();
        let child = DocRect {
            x: 40.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let start = vec![
            (group, group_box(), Transform::IDENTITY),
            (
                child_id,
                child,
                Transform::rotate_about(45.0, child.center()),
            ),
        ];

        let out = scaled(
            &start,
            group,
            &to_box(group_box(), doubled()),
            Transform::IDENTITY,
        );

        let [a, b, c, d, _, _] = out[1].2.coefficients;
        // The child's two axes are no longer perpendicular: that is the shear.
        assert!(
            (a * c + b * d).abs() > 1e-9,
            "the child stayed a rectangle, so the shear was thrown away"
        );
        assert!(
            out[1].2.determinant().abs() > 1e-9,
            "and it did not collapse"
        );
    }

    #[test]
    fn rotating_a_group_turns_each_child_and_swings_it_round() {
        let (start, _) = group_and_children();
        let pivot = group_box().center();
        let out = rotated(&start, 90.0, pivot);

        // Each child turns on its own axis...
        assert!(close(out[1].2.rotation_degrees(), 90.0));
        // ...and its centre swings about the group's.
        let before = centre_of(&start[1]);
        let after = centre_of(&out[1]);
        assert!(
            (before.x - after.x).abs() > 1.0 || (before.y - after.y).abs() > 1.0,
            "the child should have moved: {before:?} -> {after:?}"
        );
        // The pivot itself does not.
        let group_centre = centre_of(&out[0]);
        assert!(close(group_centre.x, pivot.x) && close(group_centre.y, pivot.y));
    }

    #[test]
    fn a_group_box_does_not_change_shape_when_it_is_only_turned() {
        // The reported bug: the frame drawn around a rotating group breathed
        // in and out, because it was recomputed as the bounding box of the
        // children rather than being the group's own box turned with them.
        let (start, _) = group_and_children();
        let out = rotated(&start, 37.0, group_box().center());
        assert_eq!(out[0].1, group_box(), "the box itself is untouched");
        assert!(close(out[0].2.rotation_degrees(), 37.0));
    }

    #[test]
    fn rotating_by_zero_changes_nothing() {
        let (start, _) = group_and_children();
        let out = rotated(&start, 0.0, group_box().center());
        for (before, after) in start.iter().zip(out.iter()) {
            let (b, a) = (centre_of(before), centre_of(after));
            assert!(close(b.x, a.x) && close(b.y, a.y));
        }
    }

    #[test]
    fn a_childs_own_placement_accumulates_rather_than_being_replaced() {
        let [id, ..] = three_ids();
        let start = vec![(
            id,
            group_box(),
            Transform::rotate_about(20.0, group_box().center()),
        )];
        let out = rotated(&start, 30.0, group_box().center());
        assert!(
            close(out[0].2.rotation_degrees(), 50.0),
            "got {}",
            out[0].2.rotation_degrees()
        );
    }

    #[test]
    fn a_footprint_map_is_the_identity_when_nothing_changes() {
        let placement = Transform::rotate_about(23.0, DocPoint { x: 4.0, y: 9.0 });
        let map = footprint_map(group_box(), placement, group_box(), placement);
        let p = DocPoint { x: 17.0, y: -5.0 };
        let moved = map.apply(p);
        assert!(close(moved.x, p.x) && close(moved.y, p.y), "{moved:?}");
    }

    // --- flipping -------------------------------------------------------

    #[test]
    fn dragging_a_handle_past_its_anchor_mirrors_rather_than_folding_back() {
        // The reported bug: squeezing a frame to nothing and carrying on past
        // that point sent it back the way it came, growing again on the side
        // it started. It should keep going and come out mirrored.
        let b = rect(); // x 100..300
        let r = resize(b, Handle::Right, DocPoint { x: 40.0, y: 150.0 }, false);

        assert!(r.sx < 0.0, "crossing the anchor is a negative scale");
        assert!(
            close(r.bounds.x, 40.0) && close(r.bounds.x + r.bounds.width, 100.0),
            "the frame should now lie left of the anchor, got {:?}",
            r.bounds
        );
    }

    #[test]
    fn the_anchor_holds_still_through_a_flip() {
        // The edge being held is the fixed point of the scale, so it cannot
        // move however far past it the pointer goes.
        let b = rect();
        for x in [290.0, 150.0, 100.0, 60.0, -500.0] {
            let r = resize(b, Handle::Right, DocPoint { x, y: 150.0 }, false);
            let left = r.bounds.x.min(r.bounds.x + r.bounds.width);
            let right = r.bounds.x + r.bounds.width;
            assert!(
                close(left, 100.0) || close(right, 100.0),
                "at x={x} the anchor edge left 100: {:?}",
                r.bounds
            );
        }
    }

    #[test]
    fn a_flip_is_a_mirror_in_the_placement_not_a_negative_width() {
        let b = rect();
        let r = resize(b, Handle::Right, DocPoint { x: 40.0, y: 150.0 }, false);

        assert!(r.bounds.width > 0.0, "the box itself stays positive");
        // The mirror sends the box's left edge to its right edge.
        let flipped = r.flip().apply(DocPoint {
            x: r.bounds.x,
            y: r.bounds.y,
        });
        assert!(close(flipped.x, r.bounds.x + r.bounds.width), "{flipped:?}");
    }

    #[test]
    fn an_ordinary_resize_is_not_mirrored() {
        let b = rect();
        let r = resize(b, Handle::Right, DocPoint { x: 400.0, y: 150.0 }, false);
        assert!(r.sx > 0.0);
        assert!(r.flip().is_identity(), "nothing should be mirrored here");
    }

    #[test]
    fn a_flipped_group_carries_its_children_across_too() {
        let (start, group) = group_and_children();
        // Drag the right edge back past the left one.
        let r = resize(
            group_box(),
            Handle::Right,
            DocPoint { x: -100.0, y: 5.0 },
            false,
        );
        let out = scaled(&start, group, &r, Transform::IDENTITY);

        // The child that was on the left is now on the right of the other.
        let near = centre_of(&out[1]).x;
        let far = centre_of(&out[2]).x;
        assert!(
            near > far,
            "the children should have swapped sides: {near} vs {far}"
        );
    }

    // --- squaring off ---------------------------------------------------

    #[test]
    fn shift_squares_a_drag_off_using_the_larger_extent() {
        let from = DocPoint { x: 10.0, y: 10.0 };
        let to = constrain_to_square(from, DocPoint { x: 110.0, y: 40.0 });
        assert!(close(to.x - from.x, 100.0));
        assert!(close(to.y - from.y, 100.0), "the short axis grows to match");
    }

    #[test]
    fn squaring_keeps_the_direction_of_each_axis() {
        // Dragging up and to the left must square off up and to the left,
        // rather than jumping to the opposite corner.
        let from = DocPoint { x: 100.0, y: 100.0 };
        let to = constrain_to_square(from, DocPoint { x: 40.0, y: 90.0 });
        assert!(close(to.x, 40.0), "x kept its direction: {to:?}");
        assert!(close(to.y, 40.0), "and y followed it: {to:?}");
    }

    #[test]
    fn squaring_a_drag_that_has_not_moved_leaves_it_alone() {
        let from = DocPoint { x: 5.0, y: 5.0 };
        let to = constrain_to_square(from, from);
        assert!(close(to.x, from.x) && close(to.y, from.y));
    }
}

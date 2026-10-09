//! The rotate, scale and shear tools: a drag about the reference point.
//!
//! InDesign's three transform tools, which the instrument spec had refused
//! (D6) and the user asked for. Each works the same way: the reference point
//! is where the Properties proxy says, on the selection's box, until a click
//! with the tool puts it somewhere else; a drag then turns, scales or slants
//! everything selected about it, measured from where the drag began.
//!
//! Pure geometry, kept away from the viewport so it is testable without a
//! window. The fourth, Free Transform, needs none of this: it is the Select
//! tool's handles under a tool of their own.

use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect, Transform};

use crate::transform::Origin;

/// Which of the three a drag is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Rotate,
    Scale,
    Shear,
}

/// A reference point put somewhere by a click, and the selection it was put
/// down for. Another selection goes back to the proxy's point: a pivot left
/// in the corner of one object is not where anybody means to turn the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Pivot {
    pub at: DocPoint,
    pub selection: Vec<FrameId>,
}

/// Nearer than this to the reference point, a drag has no lever to work
/// with: a scale or a shear measured over a hair's breadth jumps wildly.
const LEVER: f64 = 0.5;

/// The smallest a scale may take anything to, as a factor. Below it a frame
/// collapses to a line nobody can take hold of again.
const SMALLEST: f64 = 0.01;

/// How far a drag from `start` to `current` turns about `pivot`, in degrees
/// clockwise, as [`Transform::rotate_about`] takes them. Shift holds it to
/// steps of 45°, as InDesign's rotate tool does.
pub fn rotation(pivot: DocPoint, start: DocPoint, current: DocPoint, constrain: bool) -> f64 {
    let angle = |p: DocPoint| (p.y - pivot.y).atan2(p.x - pivot.x).to_degrees();
    let raw = angle(current) - angle(start);
    let turned = if constrain {
        (raw / 45.0).round() * 45.0
    } else {
        raw
    };
    (turned + 180.0).rem_euclid(360.0) - 180.0
}

/// The factors a drag scales by, per axis: how much nearer to or further from
/// the reference point the pointer is than where it was pressed. An axis the
/// press was level with on that axis is left alone, having no lever. Shift
/// makes them equal, taking the larger, each keeping its sign.
pub fn scaling(pivot: DocPoint, start: DocPoint, current: DocPoint, constrain: bool) -> (f64, f64) {
    let factor = |from: f64, to: f64, about: f64| {
        let lever = from - about;
        if lever.abs() < LEVER {
            return None;
        }
        Some((to - about) / lever)
    };
    let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    let clamp = |v: f64| {
        if v.abs() < SMALLEST {
            SMALLEST * sign(v)
        } else {
            v
        }
    };
    let sx = factor(start.x, current.x, pivot.x);
    let sy = factor(start.y, current.y, pivot.y);
    let (sx, sy) = if constrain {
        let k = match (sx, sy) {
            (Some(x), Some(y)) => x.abs().max(y.abs()),
            (Some(x), None) => x.abs(),
            (None, Some(y)) => y.abs(),
            (None, None) => 1.0,
        };
        (k * sx.map_or(1.0, sign), k * sy.map_or(1.0, sign))
    } else {
        (sx.unwrap_or(1.0), sy.unwrap_or(1.0))
    };
    (clamp(sx), clamp(sy))
}

/// The slant a drag makes about the reference point: along whichever axis
/// the pointer has moved further, by the angle that carries the pressed
/// point to where the pointer is. Shift holds the angle to steps of 15°.
/// Never past 85°, where a slant becomes a line.
pub fn shearing(pivot: DocPoint, start: DocPoint, current: DocPoint, constrain: bool) -> Transform {
    let (dx, dy) = (current.x - start.x, current.y - start.y);
    let settle = |degrees: f64| {
        let d = if constrain {
            (degrees / 15.0).round() * 15.0
        } else {
            degrees
        };
        d.clamp(-85.0, 85.0)
    };
    if dx.abs() >= dy.abs() {
        // Along x, in proportion to the height above the reference point:
        // x' = x - tan(a) * (y - pivot.y).
        let rise = start.y - pivot.y;
        if rise.abs() < LEVER {
            return Transform::IDENTITY;
        }
        let degrees = settle((-dx / rise).atan().to_degrees());
        Transform::shear_about(degrees, pivot)
    } else {
        // Along y, in proportion to the distance across from it:
        // y' = y + tan(a) * (x - pivot.x).
        let run = start.x - pivot.x;
        if run.abs() < LEVER {
            return Transform::IDENTITY;
        }
        let degrees = settle((dy / run).atan().to_degrees());
        let n = degrees.to_radians().tan();
        Transform::from_affine(kurbo::Affine::new([1.0, n, 0.0, 1.0, 0.0, -n * pivot.x]))
    }
}

/// The document-space transform a drag of `kind` amounts to.
pub fn drag_transform(
    kind: Kind,
    pivot: DocPoint,
    start: DocPoint,
    current: DocPoint,
    constrain: bool,
) -> Transform {
    match kind {
        Kind::Rotate => Transform::rotate_about(rotation(pivot, start, current, constrain), pivot),
        Kind::Scale => {
            let (sx, sy) = scaling(pivot, start, current, constrain);
            Transform::scale_about(sx, sy, pivot)
        }
        Kind::Shear => shearing(pivot, start, current, constrain),
    }
}

/// Every frame in the gesture with `map` applied.
///
/// Each follows by composing the map onto its placement — exact for any
/// turn, slant or mirror, and a group's contents follow it rigidly. The one
/// exception is the selected frame itself when the map, seen from inside
/// the frame, is only a stretch along its own axes: then it takes a new box
/// and keeps its placement, as the Select tool's handles do, so a text frame
/// scaled wider is a wider frame with the same type in it rather than wider
/// type.
pub fn applied(origins: &[Origin], target: Option<FrameId>, map: Transform) -> Vec<Origin> {
    let boxed = target.and_then(|id| {
        let (_, bounds, placement) = origins.iter().find(|o| o.0 == id)?;
        let own = placement.then(map).then(placement.inverse());
        let [a, b, c, d, _, _] = own.to_affine().as_coeffs();
        let upright = b.abs() < 1e-9 && c.abs() < 1e-9 && a > 0.0 && d > 0.0;
        upright.then(|| (id, stretched(*bounds, own)))
    });
    origins
        .iter()
        .map(|(id, bounds, placement)| match boxed {
            Some((target, new)) if target == *id => (*id, new, *placement),
            _ => (*id, *bounds, placement.then(map)),
        })
        .collect()
}

/// A box carried by an upright stretch: its two corners, mapped.
fn stretched(bounds: DocRect, map: Transform) -> DocRect {
    let a = map.apply(DocPoint {
        x: bounds.x,
        y: bounds.y,
    });
    let b = map.apply(DocPoint {
        x: bounds.x + bounds.width,
        y: bounds.y + bounds.height,
    });
    DocRect {
        x: a.x.min(b.x),
        y: a.y.min(b.y),
        width: (b.x - a.x).abs(),
        height: (b.y - a.y).abs(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> DocPoint {
        DocPoint { x, y }
    }

    fn near(a: DocPoint, b: DocPoint) -> bool {
        (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
    }

    fn one_frame() -> (FrameId, Vec<Origin>) {
        use slotmap::KeyData;
        let id = FrameId::from(KeyData::from_ffi(1 << 32 | 1));
        let bounds = DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        (id, vec![(id, bounds, Transform::translate(100.0, 100.0))])
    }

    #[test]
    fn a_quarter_turn_drag_is_ninety_degrees_and_shift_holds_it_to_eighths() {
        let pivot = p(0.0, 0.0);
        assert!((rotation(pivot, p(10.0, 0.0), p(0.0, 10.0), false) - 90.0).abs() < 1e-9);
        assert!((rotation(pivot, p(10.0, 0.0), p(10.0, 7.0), true) - 45.0).abs() < 1e-9);
        assert!((rotation(pivot, p(10.0, 0.0), p(10.0, 1.0), true)).abs() < 1e-9);
    }

    #[test]
    fn scaling_is_the_pointer_s_distance_from_the_pivot_against_the_press() {
        let pivot = p(0.0, 0.0);
        assert_eq!(
            scaling(pivot, p(10.0, 10.0), p(20.0, 5.0), false),
            (2.0, 0.5)
        );
        assert_eq!(
            scaling(pivot, p(10.0, 10.0), p(20.0, 5.0), true),
            (2.0, 2.0)
        );
        // Pressed level with the pivot across: no lever, so that axis stays.
        assert_eq!(
            scaling(pivot, p(10.0, 0.0), p(30.0, 9.0), false),
            (3.0, 1.0)
        );
        // Dragged through it, the factor is held off zero and keeps its sign.
        let (sx, _) = scaling(pivot, p(10.0, 10.0), p(0.0, 10.0), false);
        assert!(sx > 0.0 && sx <= SMALLEST);
    }

    #[test]
    fn a_shear_carries_the_pressed_point_to_the_pointer() {
        let pivot = p(0.0, 0.0);
        let along_x = shearing(pivot, p(0.0, -10.0), p(10.0, -10.0), false);
        assert!(near(along_x.apply(p(0.0, -10.0)), p(10.0, -10.0)));
        assert!(near(along_x.apply(pivot), pivot), "the pivot holds still");

        let along_y = shearing(pivot, p(10.0, 0.0), p(10.0, 5.0), false);
        assert!(near(along_y.apply(p(10.0, 0.0)), p(10.0, 5.0)));
        assert!(near(along_y.apply(pivot), pivot));
    }

    #[test]
    fn an_upright_scale_gives_the_frame_a_new_box_and_keeps_its_place() {
        let (id, origins) = one_frame();
        // About the frame's top left corner, where it really is.
        let map = Transform::scale_about(2.0, 1.0, p(100.0, 100.0));
        let out = applied(&origins, Some(id), map);
        assert_eq!(out[0].1.width, 200.0);
        assert_eq!(out[0].1.height, 50.0);
        assert_eq!(out[0].2, origins[0].2, "the placement is untouched");
    }

    #[test]
    fn a_turn_is_carried_in_the_placement_and_the_box_is_kept() {
        let (id, origins) = one_frame();
        let pivot = p(150.0, 125.0);
        let out = applied(&origins, Some(id), Transform::rotate_about(30.0, pivot));
        assert_eq!(out[0].1, origins[0].1);
        let centre = out[0].2.apply(out[0].1.center());
        assert!(near(centre, pivot), "turned about its own centre, it stays");
        assert!((out[0].2.rotation_degrees() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn several_frames_all_follow_by_placement() {
        let (_, mut origins) = one_frame();
        let mut second = origins[0];
        second.0 = FrameId::from(slotmap::KeyData::from_ffi(1 << 32 | 2));
        origins.push(second);
        let map = Transform::scale_about(2.0, 2.0, p(0.0, 0.0));
        let out = applied(&origins, None, map);
        for (was, now) in origins.iter().zip(&out) {
            assert_eq!(now.1, was.1);
            assert_eq!(now.2, was.2.then(map));
        }
    }
}

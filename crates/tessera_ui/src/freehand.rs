//! Drawing by hand, and tidying what was drawn: InDesign's Pencil, Smooth
//! and Erase tools.
//!
//! - **Pencil**: the pointer's trail, thinned to the points that shape it
//!   (Ramer–Douglas–Peucker) and drawn through them as a smooth curve
//!   (Catmull–Rom, written as cubic Béziers), so a shaky hand gives a clean
//!   line with a handful of anchors rather than hundreds.
//! - **Smooth**: every anchor the brush passes over has its handles
//!   recomputed from its neighbours, so a corner becomes a curve and a kink
//!   straightens; the rest of the path is left exactly as it was.
//! - **Erase**: every segment the brush touches is taken out, and what is
//!   left becomes separate runs of the same path. A closed path erased
//!   anywhere is open.
//!
//! Pure geometry over kurbo paths, so all three are tested without a canvas.

use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point};

/// The points of `trail` that shape it, within `tolerance`: the first, the
/// last, and between them only where the trail strays further than that
/// from the straight line joining its ends.
pub fn simplify(trail: &[Point], tolerance: f64) -> Vec<Point> {
    if trail.len() < 3 {
        return trail.to_vec();
    }
    let mut keep = vec![false; trail.len()];
    keep[0] = true;
    keep[trail.len() - 1] = true;
    let mut stack = vec![(0, trail.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (p, q) = (trail[a], trail[b]);
        let line = q - p;
        let length = line.hypot();
        let mut far = (0.0, a);
        for (i, point) in trail.iter().enumerate().take(b).skip(a + 1) {
            let d = if length < f64::EPSILON {
                (*point - p).hypot()
            } else {
                (line.cross(*point - p)).abs() / length
            };
            if d > far.0 {
                far = (d, i);
            }
        }
        if far.0 > tolerance {
            keep[far.1] = true;
            stack.push((a, far.1));
            stack.push((far.1, b));
        }
    }
    trail
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

/// The handles for a smooth curve through `points` from `i` to `i + 1`:
/// Catmull–Rom, with the ends' missing neighbours taken as the ends
/// themselves, so the curve starts and finishes along its first and last
/// chords.
fn catmull(points: &[Point], i: usize) -> (Point, Point) {
    let p0 = points[i.saturating_sub(1)];
    let p1 = points[i];
    let p2 = points[i + 1];
    let p3 = points[(i + 2).min(points.len() - 1)];
    (p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0)
}

/// A smooth open curve through `points`.
pub fn fit(points: &[Point]) -> BezPath {
    let mut path = BezPath::new();
    let Some(first) = points.first() else {
        return path;
    };
    path.move_to(*first);
    for i in 0..points.len().saturating_sub(1) {
        let (c1, c2) = catmull(points, i);
        path.curve_to(c1, c2, points[i + 1]);
    }
    path
}

/// Each run of `path` as its segments, and whether it closes.
fn runs(path: &BezPath) -> Vec<(Vec<PathSeg>, bool)> {
    let mut out: Vec<(Vec<PathSeg>, bool)> = Vec::new();
    let mut start = Point::ZERO;
    let mut at = Point::ZERO;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.push((Vec::new(), false));
                start = p;
                at = p;
            }
            PathEl::LineTo(p) => {
                if let Some(run) = out.last_mut() {
                    run.0.push(PathSeg::Line(kurbo::Line::new(at, p)));
                }
                at = p;
            }
            PathEl::QuadTo(c, p) => {
                if let Some(run) = out.last_mut() {
                    run.0.push(PathSeg::Quad(kurbo::QuadBez::new(at, c, p)));
                }
                at = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                if let Some(run) = out.last_mut() {
                    run.0.push(PathSeg::Cubic(CubicBez::new(at, c1, c2, p)));
                }
                at = p;
            }
            PathEl::ClosePath => {
                if let Some(run) = out.last_mut() {
                    if (at - start).hypot() > 1e-9 {
                        run.0.push(PathSeg::Line(kurbo::Line::new(at, start)));
                    }
                    run.1 = true;
                }
                at = start;
            }
        }
    }
    out.retain(|(segs, _)| !segs.is_empty());
    out
}

fn push_seg(path: &mut BezPath, seg: PathSeg) {
    match seg {
        PathSeg::Line(l) => path.line_to(l.p1),
        PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
        PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
    }
}

/// `seg` as a cubic, however it was written.
fn as_cubic(seg: PathSeg) -> CubicBez {
    match seg {
        PathSeg::Line(l) => CubicBez::new(
            l.p0,
            l.p0.lerp(l.p1, 1.0 / 3.0),
            l.p0.lerp(l.p1, 2.0 / 3.0),
            l.p1,
        ),
        PathSeg::Quad(q) => q.raise(),
        PathSeg::Cubic(c) => c,
    }
}

/// `path` with every anchor for which `touched` holds made smooth: its
/// handles recomputed from its neighbours, as a Catmull–Rom curve would have
/// them. Segments with neither end touched are kept exactly.
pub fn smooth(path: &BezPath, touched: impl Fn(Point) -> bool) -> BezPath {
    let mut out = BezPath::new();
    for (segs, closed) in runs(path) {
        let mut anchors: Vec<Point> = segs.iter().map(|s| s.start()).collect();
        if let Some(last) = segs.last()
            && !closed
        {
            anchors.push(last.end());
        }
        let n = anchors.len();
        let neighbour = |i: isize| -> Point {
            if closed {
                anchors[i.rem_euclid(n as isize) as usize]
            } else {
                anchors[i.clamp(0, n as isize - 1) as usize]
            }
        };
        out.move_to(anchors[0]);
        for (i, seg) in segs.iter().enumerate() {
            let a = i as isize;
            let (from, to) = (neighbour(a), neighbour(a + 1));
            let (from_touched, to_touched) = (touched(from), touched(to));
            if !from_touched && !to_touched {
                push_seg(&mut out, *seg);
                continue;
            }
            let cubic = as_cubic(*seg);
            let c1 = if from_touched {
                from + (to - neighbour(a - 1)) / 6.0
            } else {
                cubic.p1
            };
            let c2 = if to_touched {
                to - (neighbour(a + 2) - from) / 6.0
            } else {
                cubic.p2
            };
            out.curve_to(c1, c2, to);
        }
        if closed {
            out.close_path();
        }
    }
    out
}

/// `path` with every segment for which `hit` holds taken out. What is left
/// of each run stays in order, split where segments went; a closed run
/// erased anywhere is opened there, its two ends joined into one run.
pub fn erase(path: &BezPath, hit: impl Fn(&PathSeg) -> bool) -> BezPath {
    let mut out = BezPath::new();
    for (segs, closed) in runs(path) {
        let kept: Vec<Option<PathSeg>> = segs.iter().map(|s| (!hit(s)).then_some(*s)).collect();
        if kept.iter().all(Option::is_some) {
            // Untouched: as it was, closed or not.
            out.move_to(segs[0].start());
            for s in &segs {
                push_seg(&mut out, *s);
            }
            if closed {
                out.close_path();
            }
            continue;
        }
        // Rotate a closed run so it starts after an erased segment: then the
        // run's two ends, which were joined, come out as one piece.
        let order: Vec<Option<PathSeg>> = if closed {
            let first_gap = kept.iter().position(Option::is_none).unwrap_or(0);
            kept[first_gap..]
                .iter()
                .chain(kept[..first_gap].iter())
                .copied()
                .collect()
        } else {
            kept
        };
        let mut drawing = false;
        for s in order {
            match s {
                Some(s) => {
                    if !drawing {
                        out.move_to(s.start());
                        drawing = true;
                    }
                    push_seg(&mut out, s);
                }
                None => drawing = false,
            }
        }
    }
    out
}

/// Whether `seg` passes within `radius` of any of `points`.
pub fn touches(seg: &PathSeg, points: &[Point], radius: f64) -> bool {
    points
        .iter()
        .any(|p| seg.nearest(*p, 1e-3).distance_sq <= radius * radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_shaky_trail_is_two_points() {
        let trail: Vec<Point> = (0..50)
            .map(|i| Point::new(i as f64 * 2.0, if i % 2 == 0 { 0.3 } else { -0.3 }))
            .collect();
        let kept = simplify(&trail, 1.0);
        assert_eq!(kept.len(), 2, "{kept:?}");
    }

    #[test]
    fn a_corner_in_a_trail_is_kept() {
        let mut trail: Vec<Point> = (0..20).map(|i| Point::new(i as f64 * 5.0, 0.0)).collect();
        trail.extend((1..20).map(|i| Point::new(95.0, i as f64 * 5.0)));
        let kept = simplify(&trail, 1.0);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[1], Point::new(95.0, 0.0));
    }

    #[test]
    fn the_fitted_curve_passes_through_every_point() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(50.0, 40.0),
            Point::new(100.0, 0.0),
        ];
        let path = fit(&points);
        let ends: Vec<Point> = path.segments().map(|s| s.end()).collect();
        assert_eq!(ends, vec![points[1], points[2]]);
    }

    #[test]
    fn smoothing_a_corner_gives_it_handles_and_leaves_the_rest() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((50.0, 50.0));
        path.line_to((100.0, 0.0));
        path.line_to((150.0, 50.0));
        let corner = Point::new(50.0, 50.0);
        let smoothed = smooth(&path, |p| (p - corner).hypot() < 1.0);
        let segs: Vec<PathSeg> = smoothed.segments().collect();
        assert_eq!(segs.len(), 3, "no anchors added or lost");
        assert!(
            matches!(segs[0], PathSeg::Cubic(_)),
            "into the corner curves"
        );
        assert!(matches!(segs[1], PathSeg::Cubic(_)), "and out of it");
        assert!(
            matches!(segs[2], PathSeg::Line(_)),
            "the far segment is untouched"
        );
        // The two handles at the corner lie on one line through it: smooth.
        let (PathSeg::Cubic(a), PathSeg::Cubic(b)) = (segs[0], segs[1]) else {
            panic!()
        };
        let (into, out) = (corner - a.p2, b.p1 - corner);
        assert!(into.cross(out).abs() < 1e-9);
    }

    #[test]
    fn erasing_the_middle_of_a_line_leaves_two_pieces() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((10.0, 0.0));
        path.line_to((20.0, 0.0));
        path.line_to((30.0, 0.0));
        let erased = erase(&path, |s| touches(s, &[Point::new(15.0, 0.0)], 1.0));
        let moves = erased
            .elements()
            .iter()
            .filter(|e| matches!(e, PathEl::MoveTo(_)))
            .count();
        assert_eq!(moves, 2);
        assert_eq!(erased.segments().count(), 2);
    }

    #[test]
    fn erasing_a_closed_shape_once_opens_it_in_one_piece() {
        let mut square = BezPath::new();
        square.move_to((0.0, 0.0));
        square.line_to((10.0, 0.0));
        square.line_to((10.0, 10.0));
        square.line_to((0.0, 10.0));
        square.close_path();
        let erased = erase(&square, |s| touches(s, &[Point::new(5.0, 0.0)], 1.0));
        assert!(
            !erased
                .elements()
                .iter()
                .any(|e| matches!(e, PathEl::ClosePath))
        );
        assert_eq!(
            erased
                .elements()
                .iter()
                .filter(|e| matches!(e, PathEl::MoveTo(_)))
                .count(),
            1,
            "its ends were joined, so it is one run"
        );
        assert_eq!(erased.segments().count(), 3);
    }

    #[test]
    fn erasing_everything_leaves_nothing() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((10.0, 0.0));
        assert!(erase(&path, |_| true).elements().is_empty());
    }
}

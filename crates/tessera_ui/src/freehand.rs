//! Drawing by hand, and tidying what was drawn: InDesign's Pencil, Smooth
//! and Erase tools.
//!
//! - **Pencil**: the pointer's trail, thinned to the points that shape it
//!   (Ramer–Douglas–Peucker) and drawn through them as a smooth curve
//!   (Catmull–Rom, written as cubic Béziers), so a shaky hand gives a clean
//!   line with a handful of anchors rather than hundreds.
//! - **Smooth**: the stretch of path under the brush is read again, thinned
//!   to the anchors that shape it and drawn as one smooth curve, so wobbles
//!   lose their anchors, a corner becomes a curve, and each pass smooths
//!   further; the rest of the path is left exactly as it was.
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

/// How many points each segment is read at when a stretch is refitted.
const SAMPLES: usize = 8;

/// `path` smoothed where `touched` holds, as Illustrator's and InDesign's
/// Smooth tools do it: the stretch of path around the anchors the brush
/// passed over is read point by point, thinned to the anchors that shape it
/// within `tolerance`, and drawn again through them as one smooth curve.
/// Anchors that only made a wobble are gone; a corner becomes a curve; and
/// a second pass over the same stretch smooths it further. Segments away
/// from the brush are kept exactly, and a path's ends never move.
pub fn smooth(path: &BezPath, touched: impl Fn(Point) -> bool, tolerance: f64) -> BezPath {
    let mut out = BezPath::new();
    for (mut segs, closed) in runs(path) {
        let n = segs.len();
        // Anchor `i` is where segment `i` starts; an open run has one more,
        // its end.
        let mut hit: Vec<bool> = segs.iter().map(|s| touched(s.start())).collect();
        if !closed {
            hit.push(touched(segs[n - 1].end()));
        }
        if !hit.iter().any(|h| *h) {
            out.move_to(segs[0].start());
            for seg in &segs {
                push_seg(&mut out, *seg);
            }
            if closed {
                out.close_path();
            }
            continue;
        }
        if closed {
            match hit.iter().position(|h| !h) {
                // Start the loop at an anchor the brush missed, so the seam
                // is somewhere nothing changes.
                Some(r) => {
                    segs.rotate_left(r);
                    hit.rotate_left(r);
                    hit.push(hit[0]);
                }
                // All of it: the whole loop, refitted as a loop.
                None => {
                    let mut kept = simplify(&sample(&segs), tolerance);
                    kept.pop(); // the end is the start again
                    if kept.len() < 3 {
                        kept = segs.iter().map(PathSeg::start).collect();
                    }
                    let k = kept.len() as isize;
                    let at = |j: isize| kept[j.rem_euclid(k) as usize];
                    out.move_to(kept[0]);
                    for i in 0..k {
                        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                        out.curve_to(p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0, p2);
                    }
                    out.close_path();
                    continue;
                }
            }
        }

        // Each run of touched anchors, widened by one anchor either side:
        // those are the segments that meet at a touched anchor.
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut a = 0;
        while a <= n {
            if !hit[a] {
                a += 1;
                continue;
            }
            let mut b = a;
            while b < n && hit[b + 1] {
                b += 1;
            }
            let span = (a.saturating_sub(1), (b + 1).min(n));
            match spans.last_mut() {
                Some(last) if span.0 < last.1 => last.1 = span.1,
                _ => spans.push(span),
            }
            a = b + 1;
        }

        out.move_to(segs[0].start());
        let mut at = 0;
        for (from, to) in spans {
            for seg in &segs[at..from] {
                push_seg(&mut out, *seg);
            }
            let kept = simplify(&sample(&segs[from..to]), tolerance);
            // The neighbours outside the stretch steer the curve's ends, so
            // it leaves and rejoins the untouched path along it.
            let before = match from {
                0 if closed => segs[n - 1].start(),
                0 => kept[0],
                f => segs[f - 1].start(),
            };
            let after = if to < n {
                segs[to].end()
            } else if closed {
                segs[0].end()
            } else {
                kept[kept.len() - 1]
            };
            let mut steered = Vec::with_capacity(kept.len() + 2);
            steered.push(before);
            steered.extend(&kept);
            steered.push(after);
            for i in 1..steered.len() - 2 {
                let (p0, p1, p2, p3) = (steered[i - 1], steered[i], steered[i + 1], steered[i + 2]);
                out.curve_to(p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0, p2);
            }
            at = to;
        }
        for seg in &segs[at..] {
            push_seg(&mut out, *seg);
        }
        if closed {
            out.close_path();
        }
    }
    out
}

/// Points along `segs`, in order, [`SAMPLES`] to a segment: the first
/// segment's start, and every segment's end.
fn sample(segs: &[PathSeg]) -> Vec<Point> {
    let mut points = Vec::with_capacity(segs.len() * SAMPLES + 1);
    if let Some(first) = segs.first() {
        points.push(first.start());
    }
    for seg in segs {
        for k in 1..=SAMPLES {
            points.push(seg.eval(k as f64 / SAMPLES as f64));
        }
    }
    points
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
        let smoothed = smooth(&path, |p| (p - corner).hypot() < 1.0, 0.5);
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
    fn smoothing_a_wobble_takes_out_the_anchors_that_made_it() {
        // A nearly straight line with a dozen small kinks: the brush along
        // it leaves its two ends and little else.
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        for i in 1..=12 {
            path.line_to((f64::from(i) * 10.0, if i % 2 == 0 { 0.0 } else { 1.5 }));
        }
        let before = path.segments().count();
        let smoothed = smooth(&path, |p| p.x > 5.0 && p.x < 115.0, 3.0);
        let after = smoothed.segments().count();
        assert!(after < before / 2, "{before} segments became {after}");
        // The ends stay where they were.
        let ends = |p: &BezPath| {
            let segs: Vec<PathSeg> = p.segments().collect();
            (segs[0].start(), segs[segs.len() - 1].end())
        };
        assert_eq!(ends(&path), ends(&smoothed));
    }

    #[test]
    fn smoothing_a_closed_shape_all_over_keeps_it_closed_and_round() {
        let smoothed = smooth(&square(), |_| true, 1.0);
        assert!(
            smoothed
                .elements()
                .iter()
                .any(|e| matches!(e, PathEl::ClosePath))
        );
        assert!(smoothed.segments().all(|s| matches!(s, PathSeg::Cubic(_))));
    }

    #[test]
    fn smoothing_one_corner_of_a_square_bends_only_the_sides_that_meet_there() {
        let corner = Point::new(100.0, 100.0);
        let smoothed = smooth(&square(), |p| (p - corner).hypot() < 1.0, 1.0);
        let segs: Vec<PathSeg> = smoothed.segments().collect();
        assert_eq!(segs.len(), 4);
        assert_eq!(
            segs.iter()
                .filter(|s| matches!(s, PathSeg::Cubic(_)))
                .count(),
            2,
            "only the two sides meeting at the corner bend"
        );
    }

    fn square() -> BezPath {
        let mut square = BezPath::new();
        square.move_to((0.0, 0.0));
        square.line_to((100.0, 0.0));
        square.line_to((100.0, 100.0));
        square.line_to((0.0, 100.0));
        square.close_path();
        square
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

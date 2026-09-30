//! The gap tool: the space between objects, taken hold of as a thing.
//!
//! InDesign's Gap tool. Point at the empty strip between two frames — or
//! between a frame and the page's edge — and drag it: the frames on one side
//! grow and those on the other shrink, so the gap moves and keeps its width.
//! With Ctrl the gap itself widens or narrows instead, both sides giving way
//! equally. With Shift only the two frames nearest the pointer take part.
//!
//! Pure arithmetic over upright rectangles, so it is tested without a
//! canvas. A frame that is rotated, sheared or scaled has no straight edge
//! along the gap to move, and is left out, as InDesign leaves it.

use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect};

/// How close two edges must be to count as the same edge, in points.
const EDGE: f64 = 0.01;

/// The narrowest a frame is left by a gap moving into it, in points.
pub const NARROWEST: f64 = 1.0;

/// Which way a gap runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// A strip running up the page between frames side by side: dragged
    /// across.
    Across,
    /// A strip running across the page between frames one above another:
    /// dragged down.
    Down,
}

/// A gap under the pointer: where it is, and who borders it.
#[derive(Debug, Clone, PartialEq)]
pub struct Gap {
    pub axis: Axis,
    /// Its near and far edge along the axis: left then right for `Across`,
    /// top then bottom for `Down`.
    pub from: f64,
    pub to: f64,
    /// How far it runs the other way: top to bottom for `Across`.
    pub span: (f64, f64),
    /// The frames whose far edge is `from`, and those whose near edge is `to`.
    pub before: Vec<FrameId>,
    pub after: Vec<FrameId>,
}

impl Gap {
    /// The gap as a rectangle, for drawing it.
    pub fn rect(&self) -> DocRect {
        match self.axis {
            Axis::Across => DocRect {
                x: self.from,
                y: self.span.0,
                width: self.to - self.from,
                height: self.span.1 - self.span.0,
            },
            Axis::Down => DocRect {
                x: self.span.0,
                y: self.from,
                width: self.span.1 - self.span.0,
                height: self.to - self.from,
            },
        }
    }
}

/// `(near, far)` of `r` along `axis`, and the same the other way.
fn along(r: &DocRect, axis: Axis) -> ((f64, f64), (f64, f64)) {
    let x = (r.x, r.x + r.width);
    let y = (r.y, r.y + r.height);
    match axis {
        Axis::Across => (x, y),
        Axis::Down => (y, x),
    }
}

/// The gap at `p` running one way, if there is one.
///
/// `items` are the frames that may take part, as upright rectangles on the
/// page; `walls` are what bounds a gap without moving — the page's edges.
fn find_on(
    items: &[(FrameId, DocRect)],
    walls: &[DocRect],
    p: DocPoint,
    axis: Axis,
    nearest_only: bool,
) -> Option<Gap> {
    let (at, cross) = match axis {
        Axis::Across => (p.x, p.y),
        Axis::Down => (p.y, p.x),
    };
    // The frames the pointer's line crosses. The pointer inside any of them
    // is not in a gap.
    let crossing: Vec<&(FrameId, DocRect)> = items
        .iter()
        .filter(|(_, r)| {
            let (_, c) = along(r, axis);
            c.0 < cross && cross < c.1
        })
        .collect();
    if crossing.iter().any(|(_, r)| {
        let (a, _) = along(r, axis);
        a.0 <= at && at <= a.1
    }) {
        return None;
    }
    let mut from = f64::NEG_INFINITY;
    let mut to = f64::INFINITY;
    for (_, r) in &crossing {
        let (a, _) = along(r, axis);
        if a.1 <= at {
            from = from.max(a.1);
        }
        if a.0 >= at {
            to = to.min(a.0);
        }
    }
    // The page's own edges, where the pointer is on a page.
    for wall in walls {
        let (a, c) = along(wall, axis);
        if c.0 <= cross && cross <= c.1 && a.0 <= at && at <= a.1 {
            from = from.max(a.0);
            to = to.min(a.1);
        }
    }
    if !from.is_finite() || !to.is_finite() || to - from <= EDGE {
        return None;
    }
    let near = |(_, r): &&(FrameId, DocRect)| along(r, axis);
    let on_line_before: Vec<&(FrameId, DocRect)> = crossing
        .iter()
        .copied()
        .filter(|item| (near(item).0.1 - from).abs() < EDGE)
        .collect();
    let on_line_after: Vec<&(FrameId, DocRect)> = crossing
        .iter()
        .copied()
        .filter(|item| (near(item).0.0 - to).abs() < EDGE)
        .collect();
    if on_line_before.is_empty() && on_line_after.is_empty() {
        return None;
    }
    // Every frame bordering the same gap, not only the ones on the
    // pointer's line: two boxes stacked in a column share the gutter beside
    // them. Grown from the frames the line crosses, through any frame
    // bordering the gap whose extent overlaps what is gathered so far.
    let (mut before, mut after) = (on_line_before.clone(), on_line_after.clone());
    if !nearest_only {
        let mut span = before
            .iter()
            .chain(after.iter())
            .map(|item| near(item).1)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |s, c| {
                (s.0.min(c.0), s.1.max(c.1))
            });
        loop {
            let mut grew = false;
            for item in items {
                let (a, c) = along(&item.1, axis);
                if c.1 <= span.0 || c.0 >= span.1 {
                    continue;
                }
                let is_before = (a.1 - from).abs() < EDGE;
                let is_after = (a.0 - to).abs() < EDGE;
                if is_before && !before.iter().any(|b| b.0 == item.0) {
                    before.push(item);
                } else if is_after && !after.iter().any(|b| b.0 == item.0) {
                    after.push(item);
                } else {
                    continue;
                }
                span = (span.0.min(c.0), span.1.max(c.1));
                grew = true;
            }
            if !grew {
                break;
            }
        }
    }
    let span = before
        .iter()
        .chain(after.iter())
        .map(|item| near(item).1)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |s, c| {
            (s.0.min(c.0), s.1.max(c.1))
        });
    Some(Gap {
        axis,
        from,
        to,
        span,
        before: before.iter().map(|(id, _)| *id).collect(),
        after: after.iter().map(|(id, _)| *id).collect(),
    })
}

/// The gap under `p`, whichever way it runs. Where the pointer is in both —
/// the corner where a gutter meets a row gap — the narrower is taken, as it
/// is the one the pointer is more plainly in.
pub fn find(
    items: &[(FrameId, DocRect)],
    walls: &[DocRect],
    p: DocPoint,
    nearest_only: bool,
) -> Option<Gap> {
    let across = find_on(items, walls, p, Axis::Across, nearest_only);
    let down = find_on(items, walls, p, Axis::Down, nearest_only);
    match (across, down) {
        (Some(a), Some(d)) => Some(if a.to - a.from <= d.to - d.from { a } else { d }),
        (a, d) => a.or(d),
    }
}

/// The frames' new rectangles with the gap moved `by` points along its axis,
/// or — `resize` — widened by `by`, half on each side. Held so no frame is
/// left narrower than [`NARROWEST`] and a gap is never closed past nothing.
pub fn moved(
    gap: &Gap,
    items: &[(FrameId, DocRect)],
    by: f64,
    resize: bool,
) -> Vec<(FrameId, DocRect)> {
    let size = |id: &FrameId| {
        items
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, r)| {
                let (a, _) = along(r, gap.axis);
                a.1 - a.0
            })
            .unwrap_or(0.0)
    };
    let room_before = gap.before.iter().map(size).fold(f64::INFINITY, f64::min) - NARROWEST;
    let room_after = gap.after.iter().map(size).fold(f64::INFINITY, f64::min) - NARROWEST;
    let (near_by, far_by) = if resize {
        // Wider by `by`: the near edge back, the far edge on, half each.
        let half = (by / 2.0)
            .max(-(gap.to - gap.from) / 2.0)
            .min(room_before.max(0.0))
            .min(room_after.max(0.0));
        (-half, half)
    } else {
        let by = by.max(-room_before.max(0.0)).min(room_after.max(0.0));
        (by, by)
    };
    let mut out = Vec::new();
    for (id, r) in items {
        let is_before = gap.before.contains(id);
        let is_after = gap.after.contains(id);
        if !is_before && !is_after {
            continue;
        }
        let mut r = *r;
        match (gap.axis, is_before) {
            (Axis::Across, true) => r.width += near_by,
            (Axis::Across, false) => {
                r.x += far_by;
                r.width -= far_by;
            }
            (Axis::Down, true) => r.height += near_by,
            (Axis::Down, false) => {
                r.y += far_by;
                r.height -= far_by;
            }
        }
        out.push((*id, r));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::KeyData;

    fn id(n: u64) -> FrameId {
        FrameId::from(KeyData::from_ffi(n | (1 << 32)))
    }

    fn r(x: f64, y: f64, w: f64, h: f64) -> DocRect {
        DocRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn two_columns() -> Vec<(FrameId, DocRect)> {
        // A gutter from 100 to 112, two boxes on the left stacked.
        vec![
            (id(1), r(0.0, 0.0, 100.0, 50.0)),
            (id(2), r(0.0, 60.0, 100.0, 50.0)),
            (id(3), r(112.0, 0.0, 100.0, 110.0)),
        ]
    }

    fn at(x: f64, y: f64) -> DocPoint {
        DocPoint { x, y }
    }

    #[test]
    fn the_gutter_between_two_columns_is_found_with_everything_beside_it() {
        let gap = find(&two_columns(), &[], at(106.0, 20.0), false).expect("a gap");
        assert_eq!(gap.axis, Axis::Across);
        assert_eq!((gap.from, gap.to), (100.0, 112.0));
        assert_eq!(gap.before.len(), 2, "both stacked boxes border it");
        assert_eq!(gap.after, vec![id(3)]);
    }

    #[test]
    fn shift_takes_only_the_frames_nearest_the_pointer() {
        let gap = find(&two_columns(), &[], at(106.0, 20.0), true).expect("a gap");
        assert_eq!(gap.before, vec![id(1)]);
    }

    #[test]
    fn inside_a_frame_is_not_a_gap() {
        assert!(find(&two_columns(), &[], at(50.0, 20.0), false).is_none());
    }

    #[test]
    fn the_space_between_stacked_frames_is_a_gap_running_across() {
        let gap = find(&two_columns(), &[], at(50.0, 55.0), false).expect("a gap");
        assert_eq!(gap.axis, Axis::Down);
        assert_eq!((gap.from, gap.to), (50.0, 60.0));
    }

    #[test]
    fn moving_a_gap_keeps_its_width_and_moves_both_sides() {
        let items = two_columns();
        let gap = find(&items, &[], at(106.0, 20.0), false).expect("a gap");
        let moved = moved(&gap, &items, 10.0, false);
        let get = |n| moved.iter().find(|(i, _)| *i == id(n)).expect("moved").1;
        assert_eq!(get(1).width, 110.0);
        assert_eq!(get(2).width, 110.0);
        assert_eq!((get(3).x, get(3).width), (122.0, 90.0));
        assert_eq!(get(3).x - (get(1).x + get(1).width), 12.0, "the same gap");
    }

    #[test]
    fn a_gap_cannot_squeeze_a_frame_to_nothing() {
        let items = two_columns();
        let gap = find(&items, &[], at(106.0, 20.0), false).expect("a gap");
        let moved = moved(&gap, &items, 500.0, false);
        let right = moved.iter().find(|(i, _)| *i == id(3)).expect("moved").1;
        assert_eq!(right.width, NARROWEST);
    }

    #[test]
    fn ctrl_widens_the_gap_half_on_each_side() {
        let items = two_columns();
        let gap = find(&items, &[], at(106.0, 20.0), false).expect("a gap");
        let moved = moved(&gap, &items, 8.0, true);
        let get = |n| moved.iter().find(|(i, _)| *i == id(n)).expect("moved").1;
        assert_eq!(get(1).width, 96.0);
        assert_eq!(get(3).x, 116.0);
    }

    #[test]
    fn the_page_edge_bounds_a_gap_and_does_not_move() {
        // One box, 20 points in from the page's left edge.
        let items = vec![(id(1), r(20.0, 20.0, 100.0, 100.0))];
        let page = r(0.0, 0.0, 400.0, 400.0);
        let gap = find(&items, &[page], at(10.0, 50.0), false).expect("a gap");
        assert_eq!((gap.from, gap.to), (0.0, 20.0));
        assert!(gap.before.is_empty());
        let moved = moved(&gap, &items, -5.0, false);
        assert_eq!(moved[0].1.x, 15.0, "the box's edge follows the gap");
        assert_eq!(moved[0].1.width, 105.0);
    }
}

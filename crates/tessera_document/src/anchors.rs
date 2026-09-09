//! Editing a path by its anchor points.
//!
//! A [`kurbo::BezPath`] is a list of drawing commands, and that is the right way
//! to *draw* one. It is the wrong way to *edit* one: "the third anchor" is not a
//! command, and moving it changes two commands rather than one — the segment
//! arriving at it and the segment leaving it, each of which may be a line or a
//! curve. Everything here exists to make that one idea sayable.
//!
//! ## Nothing here touches a screen
//!
//! Which is the point of the split. Direct selection is a gesture with a lot of
//! pixels in it and very little arithmetic; the arithmetic is all here, where it
//! can be tested, and the pixels are in the view.
//!
//! ## Handles move with their anchor
//!
//! Dragging an anchor takes its control points along. Leaving them behind is the
//! difference between moving a point and reshaping the curve around it, and a
//! path editor that did the second when asked for the first would be unusable
//! for the thing anybody opens it to do.

use kurbo::{BezPath, PathEl, Point};

/// What a point on a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The two segments meeting here are free to turn a corner.
    Corner,
    /// The handles either side are colinear, so the curve runs through
    /// smoothly. Dragging one turns the other.
    Smooth,
}

/// One editable point on a path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Which element of the path ends here.
    ///
    /// The index into `BezPath::elements`, which is what makes an anchor
    /// addressable at all: "the third point" is ambiguous the moment a path is
    /// edited, and an element index is not.
    pub at: usize,
    pub point: Point,
    pub kind: Kind,
}

/// Every anchor on a path, in order.
///
/// A `ClosePath` contributes none: it is a command, not a point — the shape
/// returns to where the last `MoveTo` began, and that anchor is already listed.
/// Counting it would give a path an extra point that cannot be moved.
pub fn anchors(path: &BezPath) -> Vec<Anchor> {
    let elements = path.elements();
    let mut out = Vec::new();

    for (at, element) in elements.iter().enumerate() {
        let point = match element {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
            PathEl::CurveTo(_, _, p) => *p,
            PathEl::QuadTo(_, p) => *p,
            PathEl::ClosePath => continue,
        };
        out.push(Anchor {
            at,
            point,
            kind: kind_at(elements, at),
        });
    }
    out
}

/// Whether the handles either side of an anchor are colinear.
///
/// Measured rather than stored, because the path is the only description of the
/// shape: a flag saying "smooth" that disagreed with the geometry would be a
/// second answer to a question the geometry already answers, and the one that
/// draws would win.
fn kind_at(elements: &[PathEl], at: usize) -> Kind {
    let (Some(PathEl::CurveTo(_, before, joint)), Some(PathEl::CurveTo(after, _, _))) =
        (elements.get(at), elements.get(at + 1))
    else {
        return Kind::Corner;
    };

    let incoming = (joint.x - before.x, joint.y - before.y);
    let outgoing = (after.x - joint.x, after.y - joint.y);
    let cross = incoming.0 * outgoing.1 - incoming.1 * outgoing.0;
    let length = incoming.0.hypot(incoming.1) * outgoing.0.hypot(outgoing.1);

    // A tolerance on the *sine* of the angle between them, so it means the same
    // thing for a long handle as for a short one. Comparing the cross product
    // alone would call a pair of tiny handles smooth whatever their angle.
    if length > 0.0 && (cross / length).abs() < 0.01 {
        Kind::Smooth
    } else {
        Kind::Corner
    }
}

/// Move one anchor, taking the handles either side of it with it.
///
/// Returns the path unchanged if `at` names nothing, rather than panicking:
/// an index arrives from a selection that a document change may have outlived.
pub fn move_anchor(path: &BezPath, at: usize, dx: f64, dy: f64) -> BezPath {
    let mut elements: Vec<PathEl> = path.elements().to_vec();
    let shift = |p: Point| Point::new(p.x + dx, p.y + dy);

    match elements.get(at).copied() {
        // The anchor itself, and the handle that arrives at it.
        Some(PathEl::MoveTo(p)) => elements[at] = PathEl::MoveTo(shift(p)),
        Some(PathEl::LineTo(p)) => elements[at] = PathEl::LineTo(shift(p)),
        Some(PathEl::CurveTo(a, b, p)) => elements[at] = PathEl::CurveTo(a, shift(b), shift(p)),
        Some(PathEl::QuadTo(a, p)) => elements[at] = PathEl::QuadTo(shift(a), shift(p)),
        _ => return path.clone(),
    }

    // And the handle that leaves it, which belongs to the *next* element.
    if let Some(next) = elements.get(at + 1).copied() {
        elements[at + 1] = match next {
            PathEl::CurveTo(a, b, p) => PathEl::CurveTo(shift(a), b, p),
            PathEl::QuadTo(a, p) => PathEl::QuadTo(shift(a), p),
            other => other,
        };
    }

    // A closed path's first anchor is also its last. Moving the `MoveTo`
    // without moving what closes back onto it tears the shape open at the seam
    // — invisibly, until it is filled or exported.
    if at == 0
        && is_closed(&elements)
        && let Some(last_curve) = elements.len().checked_sub(2)
        && let Some(PathEl::CurveTo(a, b, p)) = elements.get(last_curve).copied()
    {
        elements[last_curve] = PathEl::CurveTo(a, shift(b), shift(p));
    }

    BezPath::from_vec(elements)
}

fn is_closed(elements: &[PathEl]) -> bool {
    matches!(elements.last(), Some(PathEl::ClosePath))
}

/// Remove an anchor, joining what was either side of it.
///
/// **A path needs two anchors to be a path.** Removing the last one but one
/// would leave a shape with a single point, which draws as nothing and cannot be
/// selected again — so it is refused, and the caller keeps what it had.
pub fn remove_anchor(path: &BezPath, at: usize) -> Option<BezPath> {
    let mut elements: Vec<PathEl> = path.elements().to_vec();
    if anchors(path).len() <= 2 {
        return None;
    }
    if at >= elements.len() || matches!(elements[at], PathEl::ClosePath) {
        return None;
    }

    // Removing the first anchor makes the second one the start, and a path
    // whose first element is not a `MoveTo` is not a path.
    if at == 0 {
        let point = match elements.get(1).copied() {
            Some(PathEl::LineTo(p)) => p,
            Some(PathEl::CurveTo(_, _, p)) => p,
            Some(PathEl::QuadTo(_, p)) => p,
            _ => return None,
        };
        elements[1] = PathEl::MoveTo(point);
    }
    elements.remove(at);
    Some(BezPath::from_vec(elements))
}

/// Turn a corner into a smooth point, or a smooth one back into a corner.
///
/// Smoothing gives the anchor two handles along the line joining its
/// neighbours, which is what makes the curve run through rather than turn. A
/// corner drops them.
pub fn convert_anchor(path: &BezPath, at: usize) -> BezPath {
    let elements: Vec<PathEl> = path.elements().to_vec();
    let list = anchors(path);
    let Some(anchor) = list.iter().find(|a| a.at == at) else {
        return path.clone();
    };

    match anchor.kind {
        Kind::Smooth => flatten_at(&elements, at),
        Kind::Corner => smooth_at(&elements, at, &list),
    }
}

/// Replace the curves either side of an anchor with straight lines.
fn flatten_at(elements: &[PathEl], at: usize) -> BezPath {
    let mut out = elements.to_vec();
    for slot in [at, at + 1] {
        if let Some(PathEl::CurveTo(_, _, p) | PathEl::QuadTo(_, p)) = out.get(slot).copied() {
            out[slot] = PathEl::LineTo(p);
        }
    }
    BezPath::from_vec(out)
}

/// Give an anchor handles along the line joining its neighbours.
fn smooth_at(elements: &[PathEl], at: usize, list: &[Anchor]) -> BezPath {
    let Some(here) = list.iter().position(|a| a.at == at) else {
        return BezPath::from_vec(elements.to_vec());
    };
    let (Some(before), Some(after)) = (
        here.checked_sub(1).and_then(|i| list.get(i)),
        list.get(here + 1),
    ) else {
        // An end point has only one neighbour, so there is no line through it
        // to be smooth along. Left as it is rather than guessed at.
        return BezPath::from_vec(elements.to_vec());
    };

    let point = list[here].point;
    // A third of the way towards each neighbour, along the line between them:
    // the standard construction, and the one that makes a smoothed polygon look
    // like the curve somebody expected rather than a slightly bent version of
    // what they had.
    let along = (
        (after.point.x - before.point.x) / 6.0,
        (after.point.y - before.point.y) / 6.0,
    );

    let mut out = elements.to_vec();
    let incoming = Point::new(point.x - along.0, point.y - along.1);
    let outgoing = Point::new(point.x + along.0, point.y + along.1);

    if let Some(element) = out.get(at).copied() {
        let start = before.point;
        out[at] = match element {
            PathEl::MoveTo(p) => PathEl::MoveTo(p),
            _ => PathEl::CurveTo(
                Point::new(
                    start.x + (point.x - start.x) / 3.0,
                    start.y + (point.y - start.y) / 3.0,
                ),
                incoming,
                point,
            ),
        };
    }
    if let Some(element) = out.get(at + 1).copied()
        && !matches!(element, PathEl::ClosePath | PathEl::MoveTo(_))
    {
        let end = after.point;
        out[at + 1] = PathEl::CurveTo(
            outgoing,
            Point::new(
                end.x - (end.x - point.x) / 3.0,
                end.y - (end.y - point.y) / 3.0,
            ),
            end,
        );
    }
    BezPath::from_vec(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A three-sided path of straight lines, closed.
    fn triangle() -> BezPath {
        let mut path = BezPath::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        path.line_to(Point::new(50.0, 80.0));
        path.close_path();
        path
    }

    /// Two curves meeting smoothly at (100, 0).
    fn smooth_pair() -> BezPath {
        let mut path = BezPath::new();
        path.move_to(Point::new(0.0, 0.0));
        path.curve_to(
            Point::new(30.0, 0.0),
            Point::new(70.0, 0.0),
            Point::new(100.0, 0.0),
        );
        path.curve_to(
            Point::new(130.0, 0.0),
            Point::new(170.0, 0.0),
            Point::new(200.0, 0.0),
        );
        path
    }

    #[test]
    fn closing_a_path_adds_no_anchor() {
        // `ClosePath` is a command, not a point: the shape returns to where the
        // last `MoveTo` began, and that anchor is already listed. Counting it
        // would give the path a point that cannot be moved.
        assert_eq!(anchors(&triangle()).len(), 3);
    }

    #[test]
    fn an_anchor_knows_which_element_it_is() {
        // "The third point" stops meaning anything the moment a path is edited.
        // An element index does not.
        let found = anchors(&triangle());
        assert_eq!(found[0].at, 0);
        assert_eq!(found[1].at, 1);
        assert_eq!(found[2].at, 2);
    }

    #[test]
    fn moving_an_anchor_takes_its_handles_with_it() {
        // **The difference between moving a point and reshaping the curve round
        // it.** Left behind, the handles pull the curve inside out, and a path
        // editor that did that when asked to move a point would be unusable for
        // the thing anybody opens it to do.
        let path = smooth_pair();
        let moved = move_anchor(&path, 1, 0.0, 40.0);

        let PathEl::CurveTo(_, before, joint) = moved.elements()[1] else {
            panic!("the first curve changed shape");
        };
        let PathEl::CurveTo(after, _, _) = moved.elements()[2] else {
            panic!("the second curve changed shape");
        };
        assert_eq!(joint, Point::new(100.0, 40.0));
        assert_eq!(before, Point::new(70.0, 40.0), "the arriving handle stayed");
        assert_eq!(after, Point::new(130.0, 40.0), "the leaving handle stayed");
    }

    #[test]
    fn moving_an_anchor_that_is_not_there_changes_nothing() {
        // The index comes from a selection, and a selection can outlive the
        // path it was made against.
        let path = triangle();
        assert_eq!(move_anchor(&path, 99, 5.0, 5.0), path);
    }

    #[test]
    fn a_path_keeps_at_least_two_anchors() {
        // One point draws as nothing and cannot be selected again, so removing
        // down to it is refused rather than done.
        let mut line = BezPath::new();
        line.move_to(Point::new(0.0, 0.0));
        line.line_to(Point::new(10.0, 0.0));
        assert!(remove_anchor(&line, 1).is_none());
    }

    #[test]
    fn removing_the_first_anchor_leaves_a_path_that_still_starts_somewhere() {
        // A path whose first element is not a `MoveTo` is not a path, and kurbo
        // will happily hold one.
        let shortened = remove_anchor(&triangle(), 0).expect("removed");
        assert!(
            matches!(shortened.elements().first(), Some(PathEl::MoveTo(_))),
            "the path no longer starts with a move: {:?}",
            shortened.elements().first()
        );
        assert_eq!(anchors(&shortened).len(), 2);
    }

    #[test]
    fn a_smooth_join_is_recognised_as_one() {
        // Measured from the geometry rather than stored beside it: a flag
        // saying "smooth" that disagreed with the shape would be a second
        // answer, and the one that draws would win.
        let found = anchors(&smooth_pair());
        assert_eq!(found[1].kind, Kind::Smooth);
    }

    #[test]
    fn a_corner_is_recognised_as_one() {
        let found = anchors(&triangle());
        assert!(found.iter().all(|a| a.kind == Kind::Corner));
    }

    #[test]
    fn converting_a_smooth_point_squares_it_off() {
        let path = smooth_pair();
        let squared = convert_anchor(&path, 1);
        let found = anchors(&squared);
        assert_eq!(found[1].kind, Kind::Corner);
    }

    #[test]
    fn converting_a_corner_rounds_it() {
        // The anchor stays exactly where it was. Smoothing is about the
        // handles, and a point that moved when asked to become smooth would be
        // a different edit from the one requested.
        let path = triangle();
        let before = anchors(&path)[1].point;
        let rounded = convert_anchor(&path, 1);
        let after = anchors(&rounded);

        assert_eq!(after[1].point, before, "the anchor moved");
        assert_eq!(after[1].kind, Kind::Smooth);
    }

    #[test]
    fn an_end_point_cannot_be_smoothed() {
        // There is no line through it to be smooth along, so it is left as it
        // is rather than guessed at.
        let path = smooth_pair();
        let last = anchors(&path).last().expect("an anchor").at;
        assert_eq!(convert_anchor(&path, last), path);
    }

    #[test]
    fn moving_the_start_of_a_closed_path_keeps_it_closed() {
        // The first anchor is also the last. Moving one without the other tears
        // the shape open at the seam — invisibly, until it is filled.
        let moved = move_anchor(&triangle(), 0, 10.0, 10.0);
        assert!(
            matches!(moved.elements().last(), Some(PathEl::ClosePath)),
            "the path came open"
        );
        assert_eq!(anchors(&moved)[0].point, Point::new(10.0, 10.0));
    }
}

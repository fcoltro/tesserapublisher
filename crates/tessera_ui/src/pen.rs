//! The pen tool's in-progress path.
//!
//! Kept separate from the viewport so the geometry — which anchor produces
//! which bezier segment — is testable without a window. The viewport only
//! feeds clicks and drags into it.

use kurbo::BezPath;
use tessera_geometry::{DocPoint, DocRect};

/// One anchor on the path being drawn.
///
/// `handle_out` is the outgoing control point, in document coordinates. The
/// incoming handle is its mirror through the anchor, which is what makes a
/// dragged point *smooth*: both sides stay collinear by construction rather
/// than by the user keeping them aligned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub point: DocPoint,
    pub handle_out: Option<DocPoint>,
}

impl Anchor {
    pub fn corner(point: DocPoint) -> Self {
        Self {
            point,
            handle_out: None,
        }
    }

    /// The incoming handle: the outgoing one reflected through the anchor.
    pub fn handle_in(&self) -> Option<DocPoint> {
        self.handle_out.map(|h| DocPoint {
            x: 2.0 * self.point.x - h.x,
            y: 2.0 * self.point.y - h.y,
        })
    }
}

/// A path under construction.
#[derive(Debug, Clone, Default)]
pub struct PenPath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

impl PenPath {
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn push(&mut self, anchor: Anchor) {
        self.anchors.push(anchor);
    }

    /// The anchor currently being dragged, so the viewport can set its handle.
    pub fn last_mut(&mut self) -> Option<&mut Anchor> {
        self.anchors.last_mut()
    }

    pub fn first_point(&self) -> Option<DocPoint> {
        self.anchors.first().map(|a| a.point)
    }

    /// A path needs two anchors to draw anything, or three to enclose an area.
    pub fn is_drawable(&self) -> bool {
        self.anchors.len() >= 2
    }

    /// The bounding box of the curve itself.
    ///
    /// The *curve*, not the control-point hull: a cubic bows well inside its
    /// handles, so hull bounds would leave dead space around the frame. It
    /// also has to be exact, because a path frame's geometry is rescaled to
    /// fit its bounds — bounds that did not match the curve would stretch it
    /// the moment it was created.
    pub fn bounds(&self) -> DocRect {
        use kurbo::Shape as _;
        if self.anchors.is_empty() {
            return DocRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            };
        }
        let b = self.to_bezpath_at(0.0, 0.0).bounding_box();
        DocRect {
            x: b.x0,
            y: b.y0,
            width: b.width(),
            height: b.height(),
        }
    }

    /// Build the path in **frame-local** coordinates, relative to
    /// [`PenPath::bounds`], ready for `FrameKind::Path`.
    pub fn to_bezpath(&self) -> BezPath {
        let origin = self.bounds();
        self.to_bezpath_at(origin.x, origin.y)
    }

    /// Build the path offset by `(ox, oy)`. The live preview uses `(0, 0)` to
    /// get document coordinates; the committed frame uses its own origin.
    pub fn to_bezpath_at(&self, ox: f64, oy: f64) -> BezPath {
        let mut path = BezPath::new();
        let Some(first) = self.anchors.first() else {
            return path;
        };
        let at = |p: DocPoint| (p.x - ox, p.y - oy);

        path.move_to(at(first.point));

        for pair in self.anchors.windows(2) {
            segment(&mut path, &pair[0], &pair[1], &at);
        }

        if self.closed && self.anchors.len() >= 3 {
            let last = self.anchors.last().expect("checked non-empty");
            segment(&mut path, last, first, &at);
            path.close_path();
        }

        path
    }
}

/// Emit the segment from `a` to `b`.
///
/// A straight line only when *neither* end has a handle: one handle still
/// curves the segment, which is what makes a corner-to-smooth join work.
fn segment(path: &mut BezPath, a: &Anchor, b: &Anchor, at: &impl Fn(DocPoint) -> (f64, f64)) {
    match (a.handle_out, b.handle_in()) {
        (None, None) => path.line_to(at(b.point)),
        (out, inc) => {
            let c1 = out.unwrap_or(a.point);
            let c2 = inc.unwrap_or(b.point);
            path.curve_to(at(c1), at(c2), at(b.point));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::PathEl;

    fn p(x: f64, y: f64) -> DocPoint {
        DocPoint { x, y }
    }

    #[test]
    fn an_empty_pen_draws_nothing() {
        assert!(PenPath::default().to_bezpath().elements().is_empty());
        assert!(!PenPath::default().is_drawable());
    }

    #[test]
    fn one_anchor_is_not_yet_drawable() {
        let mut pen = PenPath::default();
        pen.push(Anchor::corner(p(0.0, 0.0)));
        assert!(!pen.is_drawable());
    }

    #[test]
    fn two_corner_anchors_make_a_straight_line() {
        let mut pen = PenPath::default();
        pen.push(Anchor::corner(p(0.0, 0.0)));
        pen.push(Anchor::corner(p(10.0, 0.0)));

        let els: Vec<_> = pen.to_bezpath().elements().to_vec();

        assert!(matches!(els[0], PathEl::MoveTo(_)));
        assert!(
            matches!(els[1], PathEl::LineTo(_)),
            "two corners must not curve"
        );
    }

    #[test]
    fn a_handle_turns_the_segment_into_a_curve() {
        let mut pen = PenPath::default();
        pen.push(Anchor {
            point: p(0.0, 0.0),
            handle_out: Some(p(5.0, 5.0)),
        });
        pen.push(Anchor::corner(p(10.0, 0.0)));

        let els: Vec<_> = pen.to_bezpath().elements().to_vec();

        assert!(
            matches!(els[1], PathEl::CurveTo(..)),
            "one handle is enough to curve the segment"
        );
    }

    #[test]
    fn the_incoming_handle_mirrors_the_outgoing_one() {
        let a = Anchor {
            point: p(10.0, 10.0),
            handle_out: Some(p(14.0, 12.0)),
        };
        assert_eq!(a.handle_in(), Some(p(6.0, 8.0)));
    }

    #[test]
    fn a_corner_anchor_has_no_handles_at_all() {
        let a = Anchor::corner(p(1.0, 2.0));
        assert_eq!(a.handle_out, None);
        assert_eq!(a.handle_in(), None);
    }

    #[test]
    fn closing_adds_a_returning_segment_and_a_close() {
        let mut pen = PenPath::default();
        pen.push(Anchor::corner(p(0.0, 0.0)));
        pen.push(Anchor::corner(p(10.0, 0.0)));
        pen.push(Anchor::corner(p(10.0, 10.0)));
        pen.closed = true;

        let els: Vec<_> = pen.to_bezpath().elements().to_vec();

        assert!(matches!(els.last(), Some(PathEl::ClosePath)));
        assert_eq!(els.len(), 5, "move, three segments, close");
    }

    #[test]
    fn two_anchors_cannot_close_into_an_area() {
        let mut pen = PenPath::default();
        pen.push(Anchor::corner(p(0.0, 0.0)));
        pen.push(Anchor::corner(p(10.0, 0.0)));
        pen.closed = true;

        let els: Vec<_> = pen.to_bezpath().elements().to_vec();

        assert!(
            !els.iter().any(|e| matches!(e, PathEl::ClosePath)),
            "a two-point path encloses nothing, so closing it is meaningless"
        );
    }

    #[test]
    fn bounds_cover_the_curve_which_bows_beyond_its_anchors() {
        // Both anchors sit on y = 0, so any height at all proves the bow is
        // included. Bounds drawn round the anchors alone would clip it.
        let mut pen = PenPath::default();
        pen.push(Anchor {
            point: p(0.0, 0.0),
            handle_out: Some(p(0.0, 40.0)),
        });
        pen.push(Anchor::corner(p(10.0, 0.0)));

        let b = pen.bounds();

        assert!(b.height > 5.0, "the bow must widen the bounds: {b:?}");
    }

    #[test]
    fn bounds_do_not_run_out_to_the_handles() {
        // The control point reaches y = 40, but a cubic only reaches about
        // three eighths of that. Hull bounds would leave dead space that the
        // frame's rescaling would then stretch the curve into.
        let mut pen = PenPath::default();
        pen.push(Anchor {
            point: p(0.0, 0.0),
            handle_out: Some(p(0.0, 40.0)),
        });
        pen.push(Anchor::corner(p(10.0, 0.0)));

        assert!(
            pen.bounds().height < 30.0,
            "bounds hugged the handles instead of the curve: {:?}",
            pen.bounds()
        );
    }

    #[test]
    fn the_committed_path_is_frame_local() {
        let mut pen = PenPath::default();
        pen.push(Anchor::corner(p(100.0, 200.0)));
        pen.push(Anchor::corner(p(110.0, 200.0)));

        let els: Vec<_> = pen.to_bezpath().elements().to_vec();

        // The first point must sit at the frame's origin, not at 100,200.
        assert_eq!(els[0], PathEl::MoveTo(kurbo::Point::new(0.0, 0.0)));
    }
}

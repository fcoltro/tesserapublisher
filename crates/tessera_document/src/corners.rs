//! Corner options: how a rectangle's corners are cut.
//!
//! ## One outline, not one per consumer
//!
//! [`Corners::outline`] returns the path, and the renderer and the PDF writer
//! both draw *that*. A rounded corner computed twice is two corners that agree
//! until somebody fixes a rounding error in one of them, and then the export
//! stops matching the screen in a way nobody can see until it is printed.
//!
//! ## Four radii, one shape
//!
//! Per-corner sizes, because a card with one cut corner is a real thing and a
//! single radius cannot express it. One shape for all four, because a rectangle
//! that is rounded at the top and bevelled at the bottom is not a thing anybody
//! has asked a layout tool for, and the control for it would cost more than the
//! feature.

use kurbo::{BezPath, Point};
use serde::{Deserialize, Serialize};
use tessera_geometry::DocRect;

/// What a cut corner looks like.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CornerShape {
    /// A quarter circle. The one everybody means by "rounded".
    #[default]
    Round,
    /// A straight cut across.
    Bevel,
    /// A quarter circle bitten *out* of the corner, which is the shape a
    /// ticket or a tag has.
    Inverse,
}

impl CornerShape {
    pub const ALL: [CornerShape; 3] =
        [CornerShape::Round, CornerShape::Bevel, CornerShape::Inverse];

    pub fn label(self) -> &'static str {
        match self {
            CornerShape::Round => "Rounded",
            CornerShape::Bevel => "Bevelled",
            CornerShape::Inverse => "Inverse",
        }
    }
}

/// How a frame's corners are cut.
///
/// The default is square — every radius zero — so a document written before
/// this existed reads as what it was. That is why no migration step rewrites
/// anything for it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Corners {
    #[serde(default)]
    pub shape: CornerShape,
    /// Clockwise from the top left: top-left, top-right, bottom-right,
    /// bottom-left. The same order CSS uses, because somebody reading this file
    /// by hand has met that order before.
    #[serde(default)]
    pub radii: [f64; 4],
}

/// The most a radius may be, as a share of the shorter side.
///
/// A half is the point at which two corners meet in the middle of an edge and
/// the shape becomes a stadium. Past it the curves cross and the outline folds
/// through itself, which draws as a bow tie.
const MOST: f64 = 0.5;

impl Corners {
    /// Square corners.
    pub const SQUARE: Self = Self {
        shape: CornerShape::Round,
        radii: [0.0; 4],
    };

    /// Every corner the same.
    pub fn uniform(shape: CornerShape, radius: f64) -> Self {
        Self {
            shape,
            radii: [radius; 4],
        }
    }

    /// Whether this cuts anything at all.
    pub fn is_square(&self) -> bool {
        self.radii.iter().all(|r| *r <= 0.0)
    }

    /// Whether all four corners are cut the same.
    pub fn is_uniform(&self) -> bool {
        self.radii.windows(2).all(|pair| pair[0] == pair[1])
    }

    /// The radii this shape can actually use, given its size.
    ///
    /// **Clamped on read, not on write.** A frame scaled down after its corners
    /// were set would otherwise hold radii bigger than itself, and the choice
    /// would be either to draw a bow tie or to silently rewrite what somebody
    /// typed. Clamping here keeps the number they typed and draws the shape that
    /// fits, so scaling back up brings the corner back.
    pub fn effective(&self, bounds: DocRect) -> [f64; 4] {
        let limit = bounds.width.min(bounds.height).max(0.0) * MOST;
        let mut out = [0.0; 4];
        for (at, radius) in self.radii.iter().enumerate() {
            out[at] = radius.max(0.0).min(limit);
        }
        out
    }

    /// The outline, as a closed path in the frame's own space.
    ///
    /// `None` when nothing is cut: the caller already knows how to draw a
    /// rectangle, and a four-segment path for the commonest case is work and
    /// bytes for no difference.
    pub fn outline(&self, bounds: DocRect) -> Option<BezPath> {
        if self.is_square() || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let r = self.effective(bounds);
        let (x0, y0) = (bounds.x, bounds.y);
        let (x1, y1) = (bounds.x + bounds.width, bounds.y + bounds.height);

        let mut path = BezPath::new();
        path.move_to(Point::new(x0 + r[0], y0));

        // Clockwise, in document space, where y increases downward. Each corner
        // is named by the two directions the path is travelling in as it
        // arrives and as it leaves; everything else follows from those, and
        // there is nowhere else for a sign to be wrong.
        path.line_to(Point::new(x1 - r[1], y0));
        self.turn(&mut path, Point::new(x1, y0), r[1], (1.0, 0.0), (0.0, 1.0));

        path.line_to(Point::new(x1, y1 - r[2]));
        self.turn(&mut path, Point::new(x1, y1), r[2], (0.0, 1.0), (-1.0, 0.0));

        path.line_to(Point::new(x0 + r[3], y1));
        self.turn(
            &mut path,
            Point::new(x0, y1),
            r[3],
            (-1.0, 0.0),
            (0.0, -1.0),
        );

        path.line_to(Point::new(x0, y0 + r[0]));
        self.turn(&mut path, Point::new(x0, y0), r[0], (0.0, -1.0), (1.0, 0.0));

        path.close_path();
        Some(path)
    }

    /// One corner: the path is already at the arc's start, and this ends at
    /// the point the next edge runs from.
    ///
    /// `arrive` is the direction the path was travelling as it reached the
    /// corner; `leave` is the direction it will travel out of it. Both are unit
    /// vectors along an axis. Naming them for the motion rather than for the
    /// corner is what keeps the four call sites checkable by eye.
    fn turn(
        &self,
        path: &mut BezPath,
        at: Point,
        radius: f64,
        arrive: (f64, f64),
        leave: (f64, f64),
    ) {
        if radius <= 0.0 {
            path.line_to(at);
            return;
        }
        let start = Point::new(at.x - arrive.0 * radius, at.y - arrive.1 * radius);
        let end = Point::new(at.x + leave.0 * radius, at.y + leave.1 * radius);

        // The circular-arc constant: a cubic with its handles this far along the
        // tangents is a quarter circle to within a ten-thousandth, far finer
        // than any press resolves.
        let k = radius * 0.552_284_75;

        match self.shape {
            CornerShape::Bevel => path.line_to(end),

            // Convex: the arc bows *toward* the corner it replaces, so both
            // handles run along the edges that meet there.
            CornerShape::Round => path.curve_to(
                Point::new(start.x + arrive.0 * k, start.y + arrive.1 * k),
                Point::new(end.x - leave.0 * k, end.y - leave.1 * k),
                end,
            ),

            // Concave: the same quarter circle centred *on* the corner instead,
            // so the handles turn the other way and the shape is bitten into.
            CornerShape::Inverse => path.curve_to(
                Point::new(start.x + leave.0 * k, start.y + leave.1 * k),
                Point::new(end.x - arrive.0 * k, end.y - arrive.1 * k),
                end,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Shape` is what gives a path its bounding box, and only the tests ask.
    use kurbo::Shape;

    fn box_of(w: f64, h: f64) -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn square_corners_draw_no_path() {
        // The caller already knows how to draw a rectangle. A four-segment path
        // for the commonest case is work and bytes for no difference.
        assert!(Corners::SQUARE.outline(box_of(100.0, 50.0)).is_none());
    }

    #[test]
    fn a_radius_bigger_than_the_shape_is_clamped_rather_than_folded() {
        // Past half the shorter side the curves cross and the outline folds
        // through itself, which draws as a bow tie.
        let wild = Corners::uniform(CornerShape::Round, 500.0);
        let fitted = wild.effective(box_of(100.0, 40.0));
        assert!(fitted.iter().all(|r| *r <= 20.0), "{fitted:?}");
    }

    #[test]
    fn clamping_does_not_change_what_was_typed() {
        // Scaling a frame down and back up brings the corner back. Rewriting
        // the radius on the way down would lose it for good.
        let set = Corners::uniform(CornerShape::Round, 40.0);
        let _ = set.effective(box_of(10.0, 10.0));
        assert_eq!(set.radii, [40.0; 4], "the stored radius was rewritten");
    }

    #[test]
    fn each_corner_can_differ() {
        // A card with one cut corner is a real thing, and a single radius cannot
        // express it.
        let mixed = Corners {
            shape: CornerShape::Round,
            radii: [10.0, 0.0, 0.0, 0.0],
        };
        assert!(!mixed.is_square());
        assert!(!mixed.is_uniform());
        assert!(mixed.outline(box_of(100.0, 100.0)).is_some());
    }

    #[test]
    fn the_outline_is_closed() {
        // An open path fills differently from a closed one, and strokes very
        // differently: the join at the start point would be two ends instead.
        for shape in CornerShape::ALL {
            let path = Corners::uniform(shape, 8.0)
                .outline(box_of(80.0, 40.0))
                .expect("outline");
            assert!(
                path.elements()
                    .iter()
                    .any(|el| matches!(el, kurbo::PathEl::ClosePath)),
                "{shape:?} left the outline open"
            );
        }
    }

    #[test]
    fn the_outline_stays_inside_the_frame() {
        // A corner that bulged past the box would overlap its neighbours and
        // break every alignment the frame takes part in. True of the inverse
        // cut as well, which curves the other way but from the same two points.
        for shape in CornerShape::ALL {
            let bounds = box_of(80.0, 40.0);
            let path = Corners::uniform(shape, 10.0).outline(bounds).expect("path");
            let box_ = path.bounding_box();
            assert!(
                box_.x0 >= bounds.x - 0.01
                    && box_.y0 >= bounds.y - 0.01
                    && box_.x1 <= bounds.x + bounds.width + 0.01
                    && box_.y1 <= bounds.y + bounds.height + 0.01,
                "{shape:?} left the frame: {box_:?}"
            );
        }
    }

    #[test]
    fn a_rounded_corner_is_a_quarter_circle() {
        // The cubic constant is what makes it one. A wrong value draws a corner
        // that is visibly not round at large radii and reads as a rendering
        // fault rather than as a number somebody typed.
        //
        // At radius = half the side of a square, the whole outline *is* a
        // circle, so every point on it is one radius from the centre. That is
        // the strongest form this can be checked in, and it needs no reference
        // image.
        let side = 100.0;
        let path = Corners::uniform(CornerShape::Round, side / 2.0)
            .outline(box_of(side, side))
            .expect("path");

        let centre = kurbo::Point::new(side / 2.0, side / 2.0);
        let mut worst: f64 = 0.0;
        kurbo::flatten(path.iter(), 0.01, |el| {
            if let kurbo::PathEl::LineTo(at) | kurbo::PathEl::MoveTo(at) = el {
                worst = worst.max(((at - centre).hypot() - side / 2.0).abs());
            }
        });
        assert!(worst < 0.2, "the corner is out by {worst} points");
    }

    #[test]
    fn corners_round_trip_through_json() {
        let set = Corners {
            shape: CornerShape::Bevel,
            radii: [1.0, 2.0, 3.0, 4.0],
        };
        let text = serde_json::to_string(&set).expect("write");
        let back: Corners = serde_json::from_str(&text).expect("read");
        assert_eq!(back, set);
    }

    #[test]
    fn a_document_written_before_corners_reads_as_square() {
        // Which is why no migration step rewrites anything: the default *is*
        // what those documents meant.
        let back: Corners = serde_json::from_str("{}").expect("read");
        assert!(back.is_square());
    }
}

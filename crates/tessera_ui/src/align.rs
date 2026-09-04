//! Aligning and distributing, as arithmetic over rectangles.
//!
//! Pure functions rather than methods on the application, so the awkward cases
//! — two objects, coincident objects, aligning to the page rather than to the
//! selection — are cheap to pin without a document, a selection or a window.

use tessera_document::nodes::Axis;
use tessera_geometry::DocRect;

/// Which edge, or middle, is being lined up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    HCentre,
    Right,
    Top,
    VCentre,
    Bottom,
}

impl Edge {
    /// Whether this edge moves objects horizontally.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Edge::Left | Edge::HCentre | Edge::Right)
    }
}

/// What the objects are lined up against.
///
/// The distinction matters: aligning to the selection depends on where the
/// objects happen to be, and aligning to the page does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignTo {
    /// The selection's own bounding box.
    #[default]
    Selection,
    /// The active page's type area.
    Margins,
    /// The active page's trim.
    Page,
    /// Every page of the active spread, together.
    Spread,
}

/// The smallest rectangle containing all of `rects`.
pub fn bounding_box(rects: &[DocRect]) -> Option<DocRect> {
    let first = rects.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x + first.width;
    let mut max_y = first.y + first.height;

    for r in rects.iter().skip(1) {
        min_x = min_x.min(r.x);
        min_y = min_y.min(r.y);
        max_x = max_x.max(r.x + r.width);
        max_y = max_y.max(r.y + r.height);
    }

    Some(DocRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

/// How far each rectangle must move to line up on `edge` within `target`.
///
/// Returned in the **input order**, so the caller can zip them against the
/// selection. Sorted output would silently move the wrong objects.
pub fn align_deltas(rects: &[DocRect], target: DocRect, edge: Edge) -> Vec<(f64, f64)> {
    rects
        .iter()
        .map(|r| match edge {
            Edge::Left => (target.x - r.x, 0.0),
            Edge::HCentre => ((target.x + target.width / 2.0) - (r.x + r.width / 2.0), 0.0),
            Edge::Right => ((target.x + target.width) - (r.x + r.width), 0.0),
            Edge::Top => (0.0, target.y - r.y),
            Edge::VCentre => (
                0.0,
                (target.y + target.height / 2.0) - (r.y + r.height / 2.0),
            ),
            Edge::Bottom => (0.0, (target.y + target.height) - (r.y + r.height)),
        })
        .collect()
}

/// How far each rectangle must move so their centres are evenly spaced.
///
/// The outermost two stay put and everything between them is spread evenly.
/// Fewer than three does nothing: with two objects there is nothing between
/// them to space out, and moving either would be a surprise.
pub fn distribute_deltas(rects: &[DocRect], axis: Axis) -> Vec<(f64, f64)> {
    let mut deltas = vec![(0.0, 0.0); rects.len()];
    if rects.len() < 3 {
        return deltas;
    }

    let centre = |r: &DocRect| match axis {
        Axis::Horizontal => r.x + r.width / 2.0,
        Axis::Vertical => r.y + r.height / 2.0,
    };

    // Sorted by position to find the order, but the deltas go back into the
    // caller's order.
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|a, b| {
        centre(&rects[*a])
            .partial_cmp(&centre(&rects[*b]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let first = centre(&rects[order[0]]);
    let last = centre(&rects[order[order.len() - 1]]);
    let step = (last - first) / (order.len() - 1) as f64;

    for (rank, &index) in order.iter().enumerate() {
        let wanted = first + step * rank as f64;
        let delta = wanted - centre(&rects[index]);
        deltas[index] = match axis {
            Axis::Horizontal => (delta, 0.0),
            Axis::Vertical => (0.0, delta),
        };
    }

    deltas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> DocRect {
        DocRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn bounding(rects: &[DocRect]) -> DocRect {
        bounding_box(rects).expect("some rectangles")
    }

    #[test]
    fn aligning_left_moves_every_edge_to_the_leftmost() {
        let rects = [r(10.0, 0.0, 20.0, 10.0), r(50.0, 0.0, 20.0, 10.0)];
        let deltas = align_deltas(&rects, bounding(&rects), Edge::Left);
        assert_eq!(deltas[0], (0.0, 0.0), "the leftmost does not move");
        assert_eq!(deltas[1], (-40.0, 0.0));
    }

    #[test]
    fn aligning_centres_uses_the_targets_middle() {
        let rects = [r(0.0, 0.0, 10.0, 10.0), r(100.0, 0.0, 30.0, 10.0)];
        let target = bounding(&rects);
        let deltas = align_deltas(&rects, target, Edge::HCentre);
        let middle = target.x + target.width / 2.0;
        assert!((rects[0].x + deltas[0].0 + 5.0 - middle).abs() < 1e-9);
        assert!((rects[1].x + deltas[1].0 + 15.0 - middle).abs() < 1e-9);
    }

    #[test]
    fn aligning_to_a_page_ignores_where_the_selection_happens_to_be() {
        // Aligning to the page must not depend on the objects' own extent,
        // which is the whole difference between the two targets.
        let rects = [r(500.0, 0.0, 20.0, 10.0)];
        let page = r(0.0, 0.0, 612.0, 792.0);
        let deltas = align_deltas(&rects, page, Edge::Left);
        assert_eq!(deltas[0], (-500.0, 0.0));
    }

    #[test]
    fn aligning_vertically_leaves_x_alone() {
        let rects = [r(10.0, 5.0, 20.0, 10.0), r(50.0, 90.0, 20.0, 10.0)];
        let deltas = align_deltas(&rects, bounding(&rects), Edge::Top);
        assert!(
            deltas.iter().all(|d| d.0 == 0.0),
            "a vertical align moved x"
        );
    }

    #[test]
    fn distributing_spaces_the_centres_evenly() {
        let rects = [
            r(0.0, 0.0, 10.0, 10.0),
            r(20.0, 0.0, 10.0, 10.0),
            r(100.0, 0.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert_eq!(deltas[0], (0.0, 0.0), "the outermost do not move");
        assert_eq!(deltas[2], (0.0, 0.0));

        let centre = |i: usize| rects[i].x + deltas[i].0 + rects[i].width / 2.0;
        assert!((centre(1) - (centre(0) + centre(2)) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn distributing_returns_deltas_in_the_callers_order_not_sorted_order() {
        // Sorting the output would move the wrong objects, and the mistake
        // would look like the alignment simply being wrong.
        let rects = [
            r(100.0, 0.0, 10.0, 10.0),
            r(0.0, 0.0, 10.0, 10.0),
            r(20.0, 0.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert_eq!(deltas[0], (0.0, 0.0), "the rightmost is an outer one");
        assert_eq!(deltas[1], (0.0, 0.0), "so is the leftmost");
        assert!(deltas[2].0 != 0.0, "the middle one is the one that moves");
    }

    #[test]
    fn distributing_fewer_than_three_does_nothing() {
        let rects = [r(0.0, 0.0, 10.0, 10.0), r(50.0, 0.0, 10.0, 10.0)];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert!(deltas.iter().all(|d| *d == (0.0, 0.0)));
    }

    #[test]
    fn distributing_coincident_objects_does_not_divide_by_zero() {
        let rects = [
            r(0.0, 0.0, 10.0, 10.0),
            r(0.0, 0.0, 10.0, 10.0),
            r(0.0, 0.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert!(deltas.iter().all(|d| d.0.is_finite()));
    }

    #[test]
    fn distributing_vertically_leaves_x_alone() {
        let rects = [
            r(0.0, 0.0, 10.0, 10.0),
            r(0.0, 20.0, 10.0, 10.0),
            r(0.0, 100.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&rects, Axis::Vertical);
        assert!(deltas.iter().all(|d| d.0 == 0.0));
    }

    #[test]
    fn a_bounding_box_of_nothing_is_nothing() {
        assert!(bounding_box(&[]).is_none());
    }
}

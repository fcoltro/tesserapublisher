//! Snapping: the lines a dragged object settles onto.
//!
//! Three parts, kept apart on purpose. [`Lines`] is what a spread offers to
//! snap to — page edges, margins, ruler guides and the other objects on it.
//! [`solve`] is pure arithmetic over that list and knows nothing about a
//! document. The viewport does the third part: it converts a screen-pixel
//! threshold into document units through the zoom and paints what was hit.
//!
//! **The threshold is in pixels, not points.** A snap that grabs from 4pt away
//! is imperceptible at 25% and unshakeable at 800%; a snap that grabs from six
//! *pixels* away feels the same at every zoom, which is what makes it feel
//! like a magnet rather than a fight.

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, SpreadId};
use tessera_document::nodes::Axis;
use tessera_geometry::DocRect;

/// The lines an object can settle onto, in document units.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    /// Lines running top to bottom, at these `x` positions.
    pub vertical: Vec<f64>,
    /// Lines running left to right, at these `y` positions.
    pub horizontal: Vec<f64>,
}

impl Lines {
    fn push(&mut self, axis: Axis, at: f64) {
        let into = match axis {
            Axis::Vertical => &mut self.vertical,
            Axis::Horizontal => &mut self.horizontal,
        };
        // A page edge and a guide dragged onto it are the same line, and
        // offering it twice makes it no stronger — but it does make the
        // indicator draw twice over itself.
        if !into.iter().any(|v| (v - at).abs() < 1e-9) {
            into.push(at);
        }
    }

    /// The left, centre and right of a rectangle, and its top, middle and
    /// bottom.
    fn push_box(&mut self, r: DocRect) {
        self.push(Axis::Vertical, r.x);
        self.push(Axis::Vertical, r.x + r.width / 2.0);
        self.push(Axis::Vertical, r.x + r.width);
        self.push(Axis::Horizontal, r.y);
        self.push(Axis::Horizontal, r.y + r.height / 2.0);
        self.push(Axis::Horizontal, r.y + r.height);
    }

    pub fn is_empty(&self) -> bool {
        self.vertical.is_empty() && self.horizontal.is_empty()
    }
}

/// What a spread offers a dragged object to settle onto.
///
/// `moving` is left out of its own candidate list — an object cannot snap to
/// itself, and a selection cannot snap to the objects being dragged with it.
pub fn lines(doc: &Document, spread: SpreadId, moving: &[FrameId]) -> Lines {
    let mut out = Lines::default();

    for page in doc.pages_of(spread) {
        let Some(bounds) = doc.pages.get(page).map(|p| p.bounds) else {
            continue;
        };
        // The trim: the edge a printed page is cut to, and the strongest line
        // on a spread.
        out.push_box(bounds);

        // The type area. What a layout is actually built against — the trim is
        // where the paper ends, the margins are where the design begins.
        if let Some(margins) = doc.margin_rect(page) {
            out.push_box(margins);
        }

        // The column guides. The strongest lines on a page that has them: a
        // multi-column layout is built against its columns, not against the
        // type area as a whole.
        for column in doc.column_rects(page) {
            out.push(Axis::Vertical, column.x);
            out.push(Axis::Vertical, column.x + column.width);
        }
    }

    for guide in doc.guides_of(spread) {
        out.push(guide.axis, guide.position);
    }

    // Everything else on the spread. Its edges *and* its centres, because
    // lining two objects up by their middles is as common as lining up their
    // left edges and much harder to do by eye.
    for page in doc.pages_of(spread) {
        for frame in doc.frames_on_page(page) {
            if moving.contains(&frame) {
                continue;
            }
            if let Some(bounds) = doc.visual_bounds(frame) {
                out.push_box(bounds);
            }
        }
    }

    out
}

/// A snap: how far to move, and the lines that caught it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Snap {
    /// Added to the movement already asked for.
    pub dx: f64,
    pub dy: f64,
    /// Where to draw the indicator, if anything was caught.
    pub on_x: Option<f64>,
    pub on_y: Option<f64>,
}

impl Snap {
    pub fn caught(&self) -> bool {
        self.on_x.is_some() || self.on_y.is_some()
    }
}

/// The nearest line within `threshold`, and how far it is.
///
/// Nearest rather than first: with a page edge and a guide a point apart, the
/// object should take whichever it is actually closer to. Ties go to the
/// earlier line, which is the stronger one — [`lines`] adds trim, then
/// margins, then guides, then objects.
fn nearest(edges: [f64; 3], candidates: &[f64], threshold: f64) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for edge in edges {
        for line in candidates {
            let delta = line - edge;
            if delta.abs() > threshold {
                continue;
            }
            let better = match best {
                None => true,
                Some((was, _)) => delta.abs() < was.abs() - 1e-9,
            };
            if better {
                best = Some((delta, *line));
            }
        }
    }
    best
}

/// Where `rect` should settle, given what is on offer.
///
/// The two axes are solved apart, which is what lets an object snap its left
/// edge to a margin while its top stays exactly where the pointer put it.
pub fn solve(rect: DocRect, lines: &Lines, threshold: f64) -> Snap {
    let horizontally = nearest(
        [rect.x, rect.x + rect.width / 2.0, rect.x + rect.width],
        &lines.vertical,
        threshold,
    );
    let vertically = nearest(
        [rect.y, rect.y + rect.height / 2.0, rect.y + rect.height],
        &lines.horizontal,
        threshold,
    );

    Snap {
        dx: horizontally.map_or(0.0, |(d, _)| d),
        dy: vertically.map_or(0.0, |(d, _)| d),
        on_x: horizontally.map(|(_, line)| line),
        on_y: vertically.map(|(_, line)| line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::{Frame, FrameKind, Guide};
    use tessera_geometry::Transform;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> DocRect {
        DocRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn lines_at(vertical: &[f64], horizontal: &[f64]) -> Lines {
        Lines {
            vertical: vertical.to_vec(),
            horizontal: horizontal.to_vec(),
        }
    }

    // --- the arithmetic -----------------------------------------------------

    #[test]
    fn an_edge_within_the_threshold_is_pulled_onto_the_line() {
        let snap = solve(rect(98.0, 50.0, 20.0, 20.0), &lines_at(&[100.0], &[]), 5.0);
        assert_eq!(snap.dx, 2.0);
        assert_eq!(snap.on_x, Some(100.0));
    }

    #[test]
    fn an_edge_outside_the_threshold_is_left_alone() {
        // Narrow on purpose. A twenty-wide box at 80 has its *right* edge on
        // 100 exactly, which snaps — correctly, and it is what the first
        // draft of this test caught itself on.
        let snap = solve(rect(80.0, 50.0, 5.0, 20.0), &lines_at(&[100.0], &[]), 5.0);
        assert_eq!(snap.dx, 0.0);
        assert!(!snap.caught());
    }

    #[test]
    fn a_centre_snaps_as_readily_as_an_edge() {
        // Lining two objects up by their middles is as common as by their left
        // edges, and much harder to do by eye.
        let snap = solve(rect(88.0, 0.0, 20.0, 20.0), &lines_at(&[100.0], &[]), 5.0);
        assert_eq!(snap.dx, 2.0, "the centre at 98 went to 100");
    }

    #[test]
    fn a_trailing_edge_snaps_too() {
        let snap = solve(rect(78.0, 0.0, 20.0, 20.0), &lines_at(&[100.0], &[]), 5.0);
        assert_eq!(snap.dx, 2.0, "the right edge at 98 went to 100");
    }

    #[test]
    fn the_nearer_of_two_lines_wins() {
        // A page edge and a guide a point apart: the object takes whichever it
        // is actually closer to, not whichever was offered first.
        let snap = solve(
            rect(99.0, 0.0, 20.0, 20.0),
            &lines_at(&[100.0, 96.0], &[]),
            5.0,
        );
        assert_eq!(snap.on_x, Some(100.0));
    }

    #[test]
    fn a_tie_goes_to_the_stronger_line() {
        // Equidistant, so the earlier line takes it — and `lines` offers trim
        // before margins before guides before objects.
        let snap = solve(
            rect(98.0, 0.0, 4.0, 20.0),
            &lines_at(&[96.0, 104.0], &[]),
            5.0,
        );
        assert_eq!(snap.on_x, Some(96.0));
    }

    #[test]
    fn the_two_axes_are_solved_apart() {
        // What lets an object snap its left edge to a margin while its top
        // stays exactly where the pointer put it.
        let snap = solve(rect(98.0, 33.0, 20.0, 20.0), &lines_at(&[100.0], &[]), 5.0);
        assert_eq!(snap.dx, 2.0);
        assert_eq!(snap.dy, 0.0);
        assert!(snap.on_y.is_none());
    }

    #[test]
    fn nothing_on_offer_is_no_snap() {
        let snap = solve(rect(10.0, 10.0, 5.0, 5.0), &Lines::default(), 5.0);
        assert_eq!(snap, Snap::default());
        assert!(!snap.caught());
    }

    #[test]
    fn a_threshold_of_zero_snaps_only_what_is_already_exact() {
        let on = solve(rect(100.0, 0.0, 20.0, 20.0), &lines_at(&[100.0], &[]), 0.0);
        assert!(on.caught(), "already on the line");
        let off = solve(rect(100.1, 0.0, 20.0, 20.0), &lines_at(&[100.0], &[]), 0.0);
        assert!(!off.caught());
    }

    // --- what a spread offers -----------------------------------------------

    /// A document with margins and one frame on its first page.
    fn a_page() -> (Document, SpreadId, FrameId) {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.setup.margins = tessera_document::nodes::Margins::uniform(36.0);
        doc.reflow_spreads();

        let page = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[page].bounds;
        let layer = doc.default_layer().expect("a layer");
        let frame = doc.add_frame(
            layer,
            Frame {
                bounds: rect(bounds.x + 100.0, bounds.y + 100.0, 50.0, 40.0),
                transform: Transform::IDENTITY,
                kind: FrameKind::Rectangle,
                fill: tessera_color::Color::BLACK,
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
            },
        );
        let spread = doc.spread_of(page).expect("a spread");
        (doc, spread, frame)
    }

    #[test]
    fn a_spread_offers_its_trim() {
        let (doc, spread, _) = a_page();
        let page = doc.pages[doc.page_ids().next().expect("a page")].bounds;
        let lines = lines(&doc, spread, &[]);

        assert!(lines.vertical.contains(&page.x), "the left trim");
        assert!(
            lines.vertical.contains(&(page.x + page.width)),
            "and the right"
        );
    }

    #[test]
    fn a_spread_offers_its_margins() {
        // What a layout is actually built against: the trim is where the paper
        // ends, the margins are where the design begins.
        let (doc, spread, _) = a_page();
        let page = doc.page_ids().next().expect("a page");
        let margins = doc.margin_rect(page).expect("margins");
        let lines = lines(&doc, spread, &[]);

        assert!(lines.vertical.contains(&margins.x));
        assert!(lines.horizontal.contains(&margins.y));
    }

    #[test]
    fn a_spread_offers_its_guides() {
        let (mut doc, spread, _) = a_page();
        doc.add_guide(
            spread,
            Guide {
                axis: Axis::Vertical,
                position: 123.0,
                locked: false,
            },
        );

        assert!(lines(&doc, spread, &[]).vertical.contains(&123.0));
    }

    #[test]
    fn a_spread_offers_the_other_objects_on_it() {
        let (doc, spread, frame) = a_page();
        let bounds = doc.visual_bounds(frame).expect("bounds");
        let lines = lines(&doc, spread, &[]);

        assert!(lines.vertical.contains(&bounds.x), "its left edge");
        assert!(
            lines.vertical.contains(&(bounds.x + bounds.width / 2.0)),
            "and its centre"
        );
    }

    #[test]
    fn an_object_is_left_out_of_its_own_candidates() {
        // An object cannot snap to itself, and a selection cannot snap to the
        // objects being dragged along with it — everything would appear stuck.
        let (doc, spread, frame) = a_page();
        let bounds = doc.visual_bounds(frame).expect("bounds");

        let lines = lines(&doc, spread, &[frame]);

        assert!(!lines.vertical.contains(&bounds.x));
    }

    #[test]
    fn a_line_offered_twice_is_listed_once() {
        // A guide dragged onto a page edge is the same line. Offering it twice
        // makes it no stronger and draws the indicator over itself.
        let (mut doc, spread, _) = a_page();
        let page = doc.pages[doc.page_ids().next().expect("a page")].bounds;
        doc.add_guide(
            spread,
            Guide {
                axis: Axis::Vertical,
                position: page.x,
                locked: false,
            },
        );

        let lines = lines(&doc, spread, &[]);
        assert_eq!(lines.vertical.iter().filter(|v| **v == page.x).count(), 1);
    }

    #[test]
    fn an_empty_spread_still_offers_its_page() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let spread = doc.spread_order[0];

        assert!(!lines(&doc, spread, &[]).is_empty());
    }

    #[test]
    fn a_spread_offers_its_column_guides() {
        // A multi-column layout is built against its columns, not against the
        // type area as a whole.
        let (mut doc, spread, _) = a_page();
        doc.setup.columns = 3;
        doc.setup.column_gutter = 12.0;

        let page = doc.page_ids().next().expect("a page");
        let columns = doc.column_rects(page);
        assert_eq!(columns.len(), 3);

        let lines = lines(&doc, spread, &[]);
        for column in columns {
            assert!(lines.vertical.contains(&column.x), "a column's left edge");
        }
    }

    #[test]
    fn one_column_offers_no_guide_of_its_own() {
        // A single column *is* the type area, and a guide drawn on the margin
        // rule says nothing.
        let (mut doc, _, _) = a_page();
        doc.setup.columns = 1;
        let page = doc.page_ids().next().expect("a page");
        assert!(doc.column_rects(page).is_empty());
    }
}

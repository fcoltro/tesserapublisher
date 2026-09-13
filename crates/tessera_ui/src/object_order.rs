//! The order the keyboard walks a spread's objects in, and what it calls them.
//!
//! Selecting an object was a pointer-only act. Every path into
//! [`Selection::set`](crate::selection::Selection::set) began with a click, so
//! a person who cannot use a mouse could not select an object — and could
//! therefore not move, style, delete or inspect one either. The whole
//! application sat behind a gesture. This module is the ordering that Tab and
//! Shift-Tab walk, and the sentence the canvas reads out when they do.
//!
//! Pure functions over a document, so the awkward cases — an empty spread, a
//! selection left behind on another page, two objects at exactly the same
//! point — are cheap to pin without a window.

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, SpreadId};
use tessera_document::nodes::FrameKind;

/// How far apart two tops may be and still be read as the same row, in points.
///
/// Reading order is not a plain sort by `y`. Two columns whose tops differ by a
/// quarter of a point are, to the eye, side by side; sorting on `y` alone would
/// order them by that difference and walk down one column, across, and back up
/// the other — which is the order the *numbers* are in and not the order the
/// page is in.
///
/// Twelve points is a line of body text: objects whose tops are closer together
/// than one line are level with each other, and objects further apart are not.
/// A measure taken from the thing being laid out, rather than a tolerance
/// chosen because it looked about right.
const ROW: f64 = 12.0;

/// The objects of `spread`, in the order a person reads a page.
///
/// **Reading order, not creation order.** InDesign walks the objects in the
/// order they were drawn. That is an accident of how a layout was built: two
/// documents that look identical walk differently, and inserting one box in the
/// middle of a finished page puts it last. Top-to-bottom then left-to-right is
/// the order the page is *read* in, which is the order somebody who cannot see
/// it needs, and it is a property of the layout rather than of its history.
///
/// Only what a click could reach. [`Document::selectable_order`] is the shared
/// rule — the same list the pointer hit-tests and Select All fills from — so
/// the keyboard and the mouse cannot come to disagree about which objects
/// exist. A locked or hidden layer is out of both, and stays out of both
/// without this module knowing that locking is a thing.
///
/// Groups are one object, not several, for the same reason a click selects the
/// group: that is what grouping is for. Their children are reached the way they
/// always were, by direct selection.
pub fn reading_order(document: &Document, spread: SpreadId) -> Vec<FrameId> {
    let pages = document.pages_of(spread);
    if pages.is_empty() {
        return Vec::new();
    }

    // Where a frame *is* goes through `corners`, never through `bounds`: a
    // rotated object's box is in its own space, and the top-left the eye sees
    // is the top-left of what is drawn.
    let placed = |id: FrameId| -> (f64, f64) {
        document.frame(id).map_or((0.0, 0.0), |frame| {
            let corners = frame.corners();
            let top = corners.iter().fold(f64::INFINITY, |a, p| a.min(p.y));
            let left = corners.iter().fold(f64::INFINITY, |a, p| a.min(p.x));
            (top, left)
        })
    };

    // Paint order, kept alongside each id: it is the tie-break, and reading it
    // out of the shared list rather than re-deriving it keeps the two in step.
    let mut on_spread: Vec<(usize, FrameId)> = document
        .selectable_order()
        .into_iter()
        .enumerate()
        .filter(|(_, id)| {
            document
                .page_of_frame(*id)
                .is_some_and(|page| pages.contains(&page))
        })
        .collect();

    // Sorted by top first, so the banding below meets each row in one pass.
    // `total_cmp` rather than `partial_cmp` and an unwrap: a NaN coordinate is
    // a bug somewhere else, and it should not be one that panics here.
    on_spread.sort_by(|(a_paint, a), (b_paint, b)| {
        placed(*a)
            .0
            .total_cmp(&placed(*b).0)
            .then(a_paint.cmp(b_paint))
    });

    let flush = |row: &mut Vec<(usize, FrameId)>, out: &mut Vec<FrameId>| {
        // Left to right within the row, and paint order between two objects at
        // the same point — so the list is *total*. Two coincident objects with
        // no fixed order between them is a walk that can cycle over the pair
        // and never reach the rest of the page.
        row.sort_by(|(a_paint, a), (b_paint, b)| {
            placed(*a)
                .1
                .total_cmp(&placed(*b).1)
                .then(a_paint.cmp(b_paint))
        });
        out.extend(row.drain(..).map(|(_, id)| id));
    };

    let mut out: Vec<FrameId> = Vec::with_capacity(on_spread.len());
    let mut row: Vec<(usize, FrameId)> = Vec::new();
    let mut row_top = f64::NEG_INFINITY;

    for (paint, id) in on_spread {
        let top = placed(id).0;
        // Measured against the row's **first** member rather than its last.
        // Chaining from the last lets a staircase of objects, each nine points
        // below the one before, join one row that spans the whole page.
        if row.is_empty() {
            row_top = top;
        } else if top - row_top >= ROW {
            flush(&mut row, &mut out);
            row_top = top;
        }
        row.push((paint, id));
    }
    flush(&mut row, &mut out);

    out
}

/// The object Tab, or Shift-Tab, should move to.
///
/// `from` is what is selected now, and `None` covers three cases that behave
/// the same way and should: nothing is selected, several things are, or the one
/// thing selected is on a spread this list does not describe. In all three the
/// walk starts at an end — the first object going forward, the last coming
/// back — because there is no "next" from somewhere that is not one place on
/// the page.
///
/// Wraps, deliberately. A walk that stops dead at the last object leaves
/// somebody pressing a key that does nothing, with no way to tell "the end of
/// the page" from "the keyboard is not working".
pub fn step(order: &[FrameId], from: Option<FrameId>, back: bool) -> Option<FrameId> {
    if order.is_empty() {
        return None;
    }
    let at = from.and_then(|id| order.iter().position(|o| *o == id));
    let next = match (at, back) {
        (Some(i), false) => (i + 1) % order.len(),
        (Some(i), true) => (i + order.len() - 1) % order.len(),
        (None, false) => 0,
        (None, true) => order.len() - 1,
    };
    order.get(next).copied()
}

/// What to call an object out loud.
///
/// The one vocabulary for this. A second list of words for the same kinds would
/// drift from this one, and a screen reader calling a thing a "picture frame"
/// where the interface calls it a "graphic" is describing two applications.
pub fn describe(document: &Document, id: FrameId) -> &'static str {
    match document.frame(id).map(|f| &f.kind) {
        Some(FrameKind::Rectangle) => "Rectangle",
        Some(FrameKind::Ellipse) => "Ellipse",
        Some(FrameKind::Text { .. }) => "Text frame",
        Some(FrameKind::Graphic { placed: Some(_) }) => "Graphic frame",
        // An empty graphic frame is a real thing and worth saying: it is the
        // box drawn to reserve room for a photograph that has not arrived, and
        // "graphic frame" alone would have somebody hunting for artwork that
        // is not there yet.
        Some(FrameKind::Graphic { placed: None }) => "Empty graphic frame",
        Some(FrameKind::Path(_)) => "Path",
        Some(FrameKind::Table(_)) => "Table",
        Some(FrameKind::Group(_)) => "Group",
        None => "Object",
    }
}

/// What the canvas tells a screen reader.
///
/// The canvas contributed **nothing** to the accessibility tree before this: a
/// response built from `allocate_exact_size` carries no `WidgetInfo`, so the
/// page and everything on it were not merely unnamed but absent. A screen
/// reader landing here was told there was nothing to land on.
///
/// The ordinal is the most useful thing it can say. Somebody walking a page
/// with Tab needs to know where they are in the walk and how far it runs;
/// "Text frame" alone says what they have without saying whether pressing Tab
/// again goes on or starts over.
pub fn announce(document: &Document, order: &[FrameId], selection: &[FrameId]) -> String {
    let total = order.len();
    let objects = if total == 1 {
        "1 object".to_string()
    } else {
        format!("{total} objects")
    };

    match selection {
        [] => format!("Page canvas, {objects}. Nothing selected."),
        [one] => {
            let what = describe(document, *one);
            match order.iter().position(|id| id == one) {
                Some(i) => format!("Page canvas, {objects}. {what}, {} of {total}.", i + 1),
                // Selected but not in the walk — an object on another spread,
                // reached from the layers panel. Saying where it is not would
                // be worse than not saying where it is.
                None => format!("Page canvas, {objects}. {what} selected."),
            }
        }
        many => format!("Page canvas, {objects}. {} of them selected.", many.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::Frame;
    use tessera_geometry::{DocRect, Transform};

    /// A document with one page, and a way to put a box on it.
    ///
    /// The default page is 612 × 792 at the origin, so every coordinate here is
    /// comfortably on it — which matters, because a frame is on a page by where
    /// its centre lands and a frame off the page is on no page at all.
    fn page() -> (Document, tessera_document::ids::LayerId, SpreadId) {
        let doc = Document::new();
        let layer = doc.default_layer().expect("a new document has a layer");
        let spread = *doc.spread_order.first().expect("and a spread");
        (doc, layer, spread)
    }

    fn box_at(x: f64, y: f64) -> Frame {
        Frame {
            bounds: DocRect {
                x,
                y,
                width: 40.0,
                height: 40.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: tessera_document::paint::Paint::Solid(tessera_color::Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            anchor: None,
            style: None,
        }
    }

    #[test]
    fn the_walk_reads_the_page_rather_than_replaying_how_it_was_built() {
        // Added bottom-first and right-first, so creation order is the reverse
        // of reading order in both axes. A test built in reading order would
        // pass against a function that did nothing at all.
        let (mut doc, layer, spread) = page();
        let bottom = doc.add_frame(layer, box_at(100.0, 400.0));
        let top_right = doc.add_frame(layer, box_at(300.0, 100.0));
        let top_left = doc.add_frame(layer, box_at(100.0, 100.0));

        assert_eq!(
            reading_order(&doc, spread),
            vec![top_left, top_right, bottom]
        );
    }

    #[test]
    fn two_columns_are_one_row_even_when_their_tops_disagree_slightly() {
        // The case the banding exists for. A quarter of a point apart is level
        // to the eye; sorting on `y` alone puts the right column first because
        // its number is smaller, which walks the page in an order nobody reads
        // it in.
        let (mut doc, layer, spread) = page();
        let left = doc.add_frame(layer, box_at(100.0, 100.25));
        let right = doc.add_frame(layer, box_at(300.0, 100.0));

        assert_eq!(reading_order(&doc, spread), vec![left, right]);
    }

    #[test]
    fn a_staircase_does_not_collapse_into_one_row() {
        // Four boxes, each nine points below the one before: inside the
        // tolerance of its neighbour, outside the tolerance of the box that
        // opened its row.
        let (mut doc, layer, spread) = page();
        let first = doc.add_frame(layer, box_at(400.0, 100.0));
        let second = doc.add_frame(layer, box_at(300.0, 109.0));
        let third = doc.add_frame(layer, box_at(200.0, 118.0));
        let fourth = doc.add_frame(layer, box_at(100.0, 127.0));

        // Two rows of two, which is the whole point. Banding from each row's
        // **first** member closes the row at `third`, eighteen points below
        // `first`; banding from the previous member instead would chain all
        // four into one row spanning twenty-seven points and read a descending
        // staircase as a single line — `[fourth, third, second, first]`.
        assert_eq!(
            reading_order(&doc, spread),
            vec![second, first, fourth, third]
        );
    }

    #[test]
    fn a_locked_layer_is_out_of_the_walk_as_it_is_out_of_a_click() {
        // The keyboard must not reach what the pointer cannot. Locking a layer
        // is how a background is kept visible and untouched, and a Tab that
        // walked into it would be a way to move something the mouse refuses to.
        let (mut doc, layer, spread) = page();
        let reachable = doc.add_frame(layer, box_at(100.0, 100.0));

        let locked = doc.add_layer("Background");
        let hidden_away = doc.add_frame(locked, box_at(100.0, 300.0));
        doc.layers.get_mut(locked).expect("layer").locked = true;

        let order = reading_order(&doc, spread);
        assert_eq!(order, vec![reachable]);
        assert!(!order.contains(&hidden_away));
    }

    #[test]
    fn a_rotated_object_is_placed_where_it_is_drawn() {
        // `bounds` is in the frame's own space, so a rotation leaves it saying
        // where the frame *would* be if it had never turned. The two readings
        // have to be made to disagree or this proves nothing: rotating a box
        // about its own top-left corner, for one, leaves its drawn top exactly
        // where `bounds.y` already was.
        let (mut doc, layer, spread) = page();
        let upright = doc.add_frame(layer, box_at(100.0, 120.0));

        let mut turned = box_at(100.0, 100.0);
        // A quarter turn about a point forty points to this box's left swings
        // it down: its corners land between y=140 and y=180 while its `bounds`
        // still read y=100. So `bounds` orders it first and the paper orders it
        // second, and only one of those is what a reader sees.
        turned.transform =
            Transform::rotate_about(90.0, tessera_geometry::DocPoint { x: 60.0, y: 100.0 });
        let swung = doc.add_frame(layer, turned);

        assert_eq!(reading_order(&doc, spread), vec![upright, swung]);
    }

    #[test]
    fn the_walk_wraps_in_both_directions() {
        let a = FrameId::default();
        let (mut doc, layer, _) = page();
        let one = doc.add_frame(layer, box_at(10.0, 10.0));
        let two = doc.add_frame(layer, box_at(10.0, 100.0));
        let order = vec![one, two];

        assert_eq!(step(&order, Some(one), false), Some(two));
        assert_eq!(step(&order, Some(two), false), Some(one), "forward wraps");
        assert_eq!(step(&order, Some(one), true), Some(two), "back wraps");

        // Nothing selected starts at the near end going each way, and a
        // selection this list has never heard of is treated the same: there is
        // no "next" from somewhere that is not on the page.
        assert_eq!(step(&order, None, false), Some(one));
        assert_eq!(step(&order, None, true), Some(two));
        assert_eq!(step(&order, Some(a), false), Some(one));

        assert_eq!(step(&[], None, false), None, "an empty page goes nowhere");
    }

    #[test]
    fn the_canvas_says_where_in_the_walk_the_selection_is() {
        let (mut doc, layer, spread) = page();
        let first = doc.add_frame(layer, box_at(100.0, 100.0));
        let second = doc.add_frame(layer, box_at(100.0, 300.0));
        let order = reading_order(&doc, spread);

        assert_eq!(
            announce(&doc, &order, &[]),
            "Page canvas, 2 objects. Nothing selected."
        );
        // The ordinal is the point: it says both what is held and whether Tab
        // goes on or starts over.
        assert_eq!(
            announce(&doc, &order, &[second]),
            "Page canvas, 2 objects. Rectangle, 2 of 2."
        );
        assert_eq!(
            announce(&doc, &order, &[first, second]),
            "Page canvas, 2 objects. 2 of them selected."
        );
    }

    #[test]
    fn one_object_is_not_announced_as_one_objects() {
        // Small, and the kind of thing a screen reader reads out loud every
        // time the canvas takes focus.
        let (mut doc, layer, spread) = page();
        doc.add_frame(layer, box_at(100.0, 100.0));

        assert_eq!(
            announce(&doc, &reading_order(&doc, spread), &[]),
            "Page canvas, 1 object. Nothing selected."
        );
    }
}

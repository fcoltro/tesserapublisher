//! Adjust layout: what is on a page follows the page when its size or its
//! margins change.
//!
//! InDesign's Layout ▸ Adjust Layout. The page's edges and its margins are
//! the lines a layout is built on, and a layout keeps its relationship to
//! them: a box set flush to the margins is still flush to them after the
//! margins move, a folio against the foot is still against the foot, and a
//! picture in the middle of the type area is still in the middle of it.
//!
//! Along each axis, per object:
//!
//! - an edge that sat on one of the four lines — the page's edge, the
//!   margin, the other margin, the other edge — goes to that line's new
//!   place;
//! - with both edges on lines, the object is resized between them;
//! - with one, it keeps its size and hangs from that edge;
//! - with neither, it keeps its size and its centre moves in proportion
//!   within whichever band it was in — margin to margin, or edge to margin.
//!
//! Only upright objects are resized: a rotated, sheared or scaled one, a
//! group, is moved by its centre and keeps its shape. A ruler guide moves in
//! proportion with the page it crosses, as a free object's centre does, so a
//! guide on a margin stays on it.

use tessera_geometry::DocRect;

use crate::document::Document;
use crate::ids::{FrameId, PageId, SpreadId};
use crate::nodes::Axis;

/// How close an edge must be to a line to be on it, in points: the width of
/// a snap, near enough that nobody put it there by accident.
const ON: f64 = 0.5;

/// A page as it was before a change: its trim and margin boxes, and what
/// stood on it.
#[derive(Debug, Clone)]
pub struct PageBefore {
    pub page: PageId,
    pub trim: DocRect,
    pub margins: DocRect,
    pub frames: Vec<FrameId>,
    /// The ruler guides that cross this page: their spread, their place in
    /// its list, which way they run, and where they were from the page's top
    /// left along the axis they cut.
    pub guides: Vec<(SpreadId, usize, Axis, f64)>,
}

/// The four lines along one axis, page-relative: edge, margin, margin, edge.
type Lines = [f64; 4];

fn lines_x(trim: DocRect, margins: DocRect) -> Lines {
    [
        0.0,
        margins.x - trim.x,
        margins.x + margins.width - trim.x,
        trim.width,
    ]
}

fn lines_y(trim: DocRect, margins: DocRect) -> Lines {
    [
        0.0,
        margins.y - trim.y,
        margins.y + margins.height - trim.y,
        trim.height,
    ]
}

/// `v` carried from `old` lines to `new`, in proportion within the band it
/// is in; past the page it keeps its distance from the nearer edge.
fn carry(v: f64, old: Lines, new: Lines) -> f64 {
    if v <= old[0] {
        return new[0] + (v - old[0]);
    }
    if v >= old[3] {
        return new[3] + (v - old[3]);
    }
    for i in 0..3 {
        let (a, b) = (old[i], old[i + 1]);
        if v <= b {
            let t = if b - a > f64::EPSILON {
                (v - a) / (b - a)
            } else {
                0.0
            };
            return new[i] + t * (new[i + 1] - new[i]);
        }
    }
    v
}

/// Which line `v` is on, if any.
fn on_line(v: f64, old: Lines) -> Option<usize> {
    old.iter().position(|l| (v - l).abs() < ON)
}

/// One axis of an object from `lo` to `hi`, moved to the new lines as the
/// module describes. Returns the new `(lo, hi)`.
pub fn adjust_axis(lo: f64, hi: f64, old: Lines, new: Lines, resizable: bool) -> (f64, f64) {
    let size = hi - lo;
    match (on_line(lo, old), on_line(hi, old)) {
        (Some(a), Some(b)) if resizable && a != b => (new[a], new[b]),
        (Some(a), _) => (new[a], new[a] + size),
        (_, Some(b)) => (new[b] - size, new[b]),
        (None, None) => {
            let centre = carry((lo + hi) / 2.0, old, new);
            (centre - size / 2.0, centre + size / 2.0)
        }
    }
}

impl Document {
    /// Every page as it is now, for [`Document::adjust_layout`] to compare
    /// with after a change. Taken **before** the change, while each object
    /// is still on the page it was put on.
    pub fn layout_before(&self) -> Vec<PageBefore> {
        self.pages
            .keys()
            .filter_map(|page| {
                Some(PageBefore {
                    page,
                    trim: self.pages.get(page)?.bounds,
                    margins: self.margin_rect(page)?,
                    frames: self
                        .frames_on_page(page)
                        .into_iter()
                        .filter(|f| self.frames.get(*f).is_some_and(|f| f.anchor.is_none()))
                        .collect(),
                    guides: self.guides_crossing(page),
                })
            })
            .collect()
    }

    /// The ruler guides that belong to `page` for adjusting: a horizontal
    /// guide to its spread's first page, a vertical one to the page it
    /// crosses, or — off every page — to the nearest.
    fn guides_crossing(&self, page: PageId) -> Vec<(SpreadId, usize, Axis, f64)> {
        let Some(spread) = self.spread_of(page) else {
            return Vec::new();
        };
        let pages = self.pages_of(spread);
        let Some(trim) = self.pages.get(page).map(|p| p.bounds) else {
            return Vec::new();
        };
        let distance = |p: PageId, x: f64| {
            self.pages.get(p).map_or(f64::INFINITY, |p| {
                let b = p.bounds;
                if x < b.x {
                    b.x - x
                } else if x > b.x + b.width {
                    x - (b.x + b.width)
                } else {
                    0.0
                }
            })
        };
        self.guides_of(spread)
            .iter()
            .enumerate()
            .filter_map(|(index, guide)| {
                let mine = match guide.axis {
                    Axis::Horizontal => pages.first() == Some(&page),
                    Axis::Vertical => {
                        pages.iter().copied().min_by(|a, b| {
                            distance(*a, guide.position).total_cmp(&distance(*b, guide.position))
                        }) == Some(page)
                    }
                };
                mine.then(|| {
                    let along = match guide.axis {
                        Axis::Horizontal => guide.position - trim.y,
                        Axis::Vertical => guide.position - trim.x,
                    };
                    (spread, index, guide.axis, along)
                })
            })
            .collect()
    }

    /// Carry what stood on each page to the page's new size and margins.
    ///
    /// Called after the change. The spreads have been laid out again by
    /// then and every page's contents moved with its page, so each object is
    /// where it was **relative to its page** and the arithmetic is done in
    /// page-relative points.
    pub fn adjust_layout(&mut self, before: &[PageBefore]) {
        for was in before {
            let Some(trim) = self.pages.get(was.page).map(|p| p.bounds) else {
                continue;
            };
            let Some(margins) = self.margin_rect(was.page) else {
                continue;
            };
            let (ox, oy) = (
                lines_x(was.trim, was.margins),
                lines_y(was.trim, was.margins),
            );
            let (nx, ny) = (lines_x(trim, margins), lines_y(trim, margins));
            if ox == nx && oy == ny {
                continue;
            }
            for &id in &was.frames {
                self.adjust_frame(id, trim, (ox, oy), (nx, ny));
            }
            for &(spread, index, axis, along) in &was.guides {
                let position = match axis {
                    Axis::Horizontal => trim.y + carry(along, oy, ny),
                    Axis::Vertical => trim.x + carry(along, ox, nx),
                };
                if let Some(guide) = self
                    .spreads
                    .get_mut(spread)
                    .and_then(|s| s.guides.get_mut(index))
                {
                    guide.position = position;
                }
            }
        }
        self.touch();
    }

    fn adjust_frame(
        &mut self,
        id: FrameId,
        trim: DocRect,
        old: (Lines, Lines),
        new: (Lines, Lines),
    ) {
        let Some(frame) = self.frames.get(id) else {
            return;
        };
        let corners = frame.corners();
        let upright = (corners[0].y - corners[1].y).abs() < 1e-6
            && (corners[0].x - corners[3].x).abs() < 1e-6
            && ((corners[1].x - corners[0].x) - frame.bounds.width).abs() < 1e-6
            && ((corners[3].y - corners[0].y) - frame.bounds.height).abs() < 1e-6
            && !matches!(frame.kind, crate::nodes::FrameKind::Group(_));
        // Where it is on its page, as an upright box: its own box for an
        // upright frame, the box round its corners for any other.
        let xs = corners.map(|c| c.x - trim.x);
        let ys = corners.map(|c| c.y - trim.y);
        let (x0, x1) = (
            xs.iter().copied().fold(f64::INFINITY, f64::min),
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let (y0, y1) = (
            ys.iter().copied().fold(f64::INFINITY, f64::min),
            ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let (nx0, nx1) = adjust_axis(x0, x1, old.0, new.0, upright);
        let (ny0, ny1) = adjust_axis(y0, y1, old.1, new.1, upright);
        if upright {
            if let Some(f) = self.frames.get_mut(id) {
                f.bounds.x += nx0 - x0;
                f.bounds.y += ny0 - y0;
                f.bounds.width = (nx1 - nx0).max(1.0);
                f.bounds.height = (ny1 - ny0).max(1.0);
            }
        } else {
            self.translate_deeply(id, nx0 - x0, ny0 - y0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A page 600 wide with 36-point margins, going to 400 wide with 50.
    const OLD: Lines = [0.0, 36.0, 564.0, 600.0];
    const NEW: Lines = [0.0, 50.0, 350.0, 400.0];

    #[test]
    fn a_box_flush_to_both_margins_stays_flush_to_both() {
        assert_eq!(adjust_axis(36.0, 564.0, OLD, NEW, true), (50.0, 350.0));
    }

    #[test]
    fn a_box_on_one_margin_keeps_its_size_and_that_margin() {
        assert_eq!(adjust_axis(36.0, 136.0, OLD, NEW, true), (50.0, 150.0));
        assert_eq!(adjust_axis(464.0, 564.0, OLD, NEW, true), (250.0, 350.0));
    }

    #[test]
    fn a_folio_against_the_far_edge_goes_with_it() {
        assert_eq!(adjust_axis(580.0, 600.0, OLD, NEW, true), (380.0, 400.0));
    }

    #[test]
    fn a_free_box_keeps_its_size_and_its_place_in_proportion() {
        // Centred in the type area before, centred in it after.
        let (a, b) = adjust_axis(250.0, 350.0, OLD, NEW, true);
        assert!((b - a - 100.0).abs() < 1e-9, "same size");
        assert!(((a + b) / 2.0 - 200.0).abs() < 1e-9, "still in the middle");
    }

    #[test]
    fn what_cannot_be_resized_is_moved_between_its_lines() {
        let (a, b) = adjust_axis(36.0, 564.0, OLD, NEW, false);
        assert_eq!(
            (a, b),
            (50.0, 578.0),
            "hung from its near margin, same size"
        );
    }

    #[test]
    fn a_guide_on_the_margin_stays_on_the_margin() {
        use crate::nodes::Guide;
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let spread = doc.spread_of(page).expect("a spread");
        let margins = doc.margin_rect(page).expect("margins");
        let trim = doc.pages[page].bounds;
        let right_margin = margins.x + margins.width;
        doc.add_guide(
            spread,
            Guide {
                axis: Axis::Vertical,
                position: right_margin,
                locked: false,
            },
        );
        let before = doc.layout_before();
        doc.set_page_size(trim.width + 120.0, trim.height);
        doc.adjust_layout(&before);
        let now = doc.margin_rect(page).expect("margins");
        let guide = doc.guides_of(spread)[0];
        assert!(
            (guide.position - (now.x + now.width)).abs() < 1e-6,
            "{} vs {}",
            guide.position,
            now.x + now.width
        );
    }

    #[test]
    fn a_page_that_grows_and_its_margin_box_move_what_is_on_them() {
        use crate::nodes::{Frame, FrameKind};
        use crate::paint::Paint;
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let margins = doc.margin_rect(page).expect("margins");
        let trim = doc.pages[page].bounds;
        let layer = doc.default_layer().expect("a layer");
        // Flush to the margins across, free down.
        let id = doc.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: margins.x,
                    y: trim.y + 200.0,
                    width: margins.width,
                    height: 50.0,
                },
                transform: Default::default(),
                kind: FrameKind::Rectangle,
                fill: Paint::default(),
                stroke: None,
                wrap: Default::default(),
                blend: crate::blending::Blending::PLAIN,
                corners: crate::corners::Corners::SQUARE,
                shadow: None,
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
            },
        );
        let before = doc.layout_before();
        doc.set_page_size(trim.width + 100.0, trim.height);
        doc.adjust_layout(&before);
        let now = doc.margin_rect(page).expect("margins");
        let f = doc.frame(id).expect("frame");
        assert!((f.bounds.x - now.x).abs() < 1e-6);
        assert!(
            (f.bounds.width - now.width).abs() < 1e-6,
            "still margin to margin"
        );
        assert!((f.bounds.height - 50.0).abs() < 1e-6);
    }
}

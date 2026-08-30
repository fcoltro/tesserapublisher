//! Page and spread placement on the infinite pasteboard.
//!
//! A document is a sequence of pages; a *spread* is the group of pages a reader
//! sees at once. With facing pages on, page 1 stands alone on the right (a
//! recto), and every later spread pairs a verso and a recto across a spine —
//! the arrangement print layout tools use so a designer sees the page pairing
//! exactly as it will be bound.
//!
//! The placement maths here is pure: it takes document settings and a page
//! count and returns coordinates. [`PageGuides`] additionally derives
//! `Component` so a page entity can override the document defaults.

use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

/// Where a page sits on the pasteboard, in document units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PagePlacement {
    /// One-based page number, as shown to the user.
    pub page_number: u32,
    /// Which spread this page belongs to.
    pub spread_index: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// True for a verso (left-hand) page in a facing-pages spread.
    pub is_left: bool,
}

impl PagePlacement {
    /// The page's outer edge — the trim box.
    pub fn trim_rect(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.x + self.width, self.y + self.height)
    }

    /// The trim box grown by `bleed` on every side.
    pub fn bleed_rect(&self, bleed: f32) -> (f32, f32, f32, f32) {
        (
            self.x - bleed,
            self.y - bleed,
            self.x + self.width + bleed,
            self.y + self.height + bleed,
        )
    }
}

/// How pages are arranged relative to one another.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpreadLayout {
    /// When true, pages pair across a spine after the first.
    pub facing_pages: bool,
    pub page_width: f32,
    pub page_height: f32,
    /// Vertical gap between consecutive spreads on the pasteboard.
    pub spread_gap: f32,
}

impl Default for SpreadLayout {
    fn default() -> Self {
        Self {
            facing_pages: true,
            page_width: 595.0,  // A4 at 72 dpi
            page_height: 842.0,
            spread_gap: 60.0,
        }
    }
}

impl SpreadLayout {
    /// The spread a one-based page number belongs to.
    ///
    /// With facing pages, page 1 is a lone recto, so pages 2 and 3 share
    /// spread 1, pages 4 and 5 share spread 2, and so on.
    pub fn spread_of(&self, page_number: u32) -> u32 {
        if !self.facing_pages {
            return page_number.saturating_sub(1);
        }
        page_number / 2
    }

    /// Whether a page falls on the left (verso) side of its spread.
    ///
    /// Only even pages are versos, and only when facing pages are on.
    pub fn is_left_page(&self, page_number: u32) -> bool {
        self.facing_pages && page_number % 2 == 0
    }

    /// Places one page on the pasteboard.
    ///
    /// The spine sits at `page_width` so every spread's spine lines up
    /// vertically, which is what makes a document scroll cleanly.
    pub fn place(&self, page_number: u32) -> PagePlacement {
        let spread_index = self.spread_of(page_number);
        let is_left = self.is_left_page(page_number);

        let x = if !self.facing_pages {
            0.0
        } else if is_left {
            0.0
        } else {
            self.page_width
        };

        PagePlacement {
            page_number,
            spread_index,
            x,
            y: spread_index as f32 * (self.page_height + self.spread_gap),
            width: self.page_width,
            height: self.page_height,
            is_left,
        }
    }

    /// Places every page in a document of `page_count` pages.
    pub fn place_all(&self, page_count: u32) -> Vec<PagePlacement> {
        (1..=page_count).map(|n| self.place(n)).collect()
    }

    /// The bounds enclosing every page, useful for fit-to-document.
    ///
    /// Returns `None` for an empty document.
    pub fn document_bounds(&self, page_count: u32) -> Option<(f32, f32, f32, f32)> {
        if page_count == 0 {
            return None;
        }
        let placements = self.place_all(page_count);
        let min_x = placements.iter().fold(f32::MAX, |acc, p| acc.min(p.x));
        let min_y = placements.iter().fold(f32::MAX, |acc, p| acc.min(p.y));
        let max_x = placements
            .iter()
            .fold(f32::MIN, |acc, p| acc.max(p.x + p.width));
        let max_y = placements
            .iter()
            .fold(f32::MIN, |acc, p| acc.max(p.y + p.height));
        Some((min_x, min_y, max_x, max_y))
    }
}

/// Margin and column guides for a page, in document units.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageGuides {
    pub margin_top: f32,
    pub margin_bottom: f32,
    /// Margin nearest the spine. Mirrors across a facing-pages spread.
    pub margin_inside: f32,
    /// Margin nearest the outer edge.
    pub margin_outside: f32,
    pub columns: u32,
    /// Space between columns.
    pub gutter: f32,
}

impl Default for PageGuides {
    fn default() -> Self {
        Self {
            margin_top: 36.0,
            margin_bottom: 36.0,
            margin_inside: 54.0,
            margin_outside: 36.0,
            columns: 1,
            gutter: 12.0,
        }
    }
}

impl PageGuides {
    /// The text area of a page, accounting for which side of the spine it is on.
    ///
    /// Inside and outside margins swap between versos and rectos, which is why
    /// they are stored by role rather than as left and right.
    pub fn content_rect(&self, page: &PagePlacement) -> (f32, f32, f32, f32) {
        let (left_margin, right_margin) = if page.is_left {
            // A verso's spine is on its right.
            (self.margin_outside, self.margin_inside)
        } else {
            (self.margin_inside, self.margin_outside)
        };

        (
            page.x + left_margin,
            page.y + self.margin_top,
            page.x + page.width - right_margin,
            page.y + page.height - self.margin_bottom,
        )
    }

    /// The x ranges of each column within a page's content area.
    pub fn column_ranges(&self, page: &PagePlacement) -> Vec<(f32, f32)> {
        let (left, _, right, _) = self.content_rect(page);
        let columns = self.columns.max(1);
        let available = (right - left) - self.gutter * (columns - 1) as f32;
        let column_width = (available / columns as f32).max(0.0);

        (0..columns)
            .map(|i| {
                let start = left + i as f32 * (column_width + self.gutter);
                (start, start + column_width)
            })
            .collect()
    }
}

/// A vertical rhythm that typography can lock onto, in document units.
///
/// The grid is a ladder of evenly spaced baselines. A text frame with
/// `snap_to_baseline` set has every line pulled onto the nearest rung *below*
/// where it naturally fell, which is what keeps columns and facing pages in
/// register with one another.
///
/// The origin is per page, measured from the top margin, so pages with
/// different margins still start their text at the same optical position and
/// a spread reads as one ruled sheet.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BaselineGrid {
    /// Distance between consecutive baselines. Zero or less disables the grid.
    pub increment: f32,
    /// Offset from the top margin down to the first baseline.
    pub start: f32,
    /// Whether the grid is drawn on the canvas. Snapping is independent of this
    /// so a designer can work against an invisible grid.
    pub visible: bool,
}

impl Default for BaselineGrid {
    fn default() -> Self {
        Self {
            increment: 12.0,
            start: 0.0,
            visible: false,
        }
    }
}

impl BaselineGrid {
    /// Whether the grid can be snapped to at all.
    ///
    /// A non-positive increment would make every rung the same line, so it is
    /// treated as "no grid" rather than as an error.
    pub fn is_active(&self) -> bool {
        self.increment > 0.0
    }

    /// The y of the first baseline on a page, in document units.
    ///
    /// Measured from the page's top margin, so the grid inherits any per-page
    /// margin override rather than assuming the document default.
    pub fn origin_for(&self, page: &PagePlacement, guides: &PageGuides) -> f32 {
        let (_, content_top, _, _) = guides.content_rect(page);
        content_top + self.start
    }

    /// Leading rounded up to a whole number of increments.
    ///
    /// Rounding is always upward: locking to the grid may open text up but
    /// never tightens it, so lines cannot be pushed into collision. Type set
    /// looser than one increment simply takes as many rungs as it needs.
    pub fn snapped_leading(&self, natural_leading: f32) -> f32 {
        if !self.is_active() {
            return natural_leading;
        }
        let rungs = (natural_leading / self.increment).ceil().max(1.0);
        rungs * self.increment
    }

    /// How far a baseline must move down to land on the grid.
    ///
    /// Always non-negative: a baseline is pushed to the next rung at or below
    /// its natural position, matching how print layout tools align to a grid.
    /// A baseline already exactly on a rung does not move.
    pub fn shift_onto_grid(&self, origin: f32, baseline: f32) -> f32 {
        if !self.is_active() {
            return 0.0;
        }
        let offset = baseline - origin;
        let rungs = (offset / self.increment).ceil();
        let target = origin + rungs * self.increment;
        (target - baseline).max(0.0)
    }

    /// The baselines falling within `[top, bottom]`, for drawing the grid.
    ///
    /// Bounded so a malformed increment cannot produce an unbounded list.
    pub fn lines_between(&self, origin: f32, top: f32, bottom: f32) -> Vec<f32> {
        if !self.is_active() || bottom <= top {
            return Vec::new();
        }
        let first_rung = ((top - origin) / self.increment).ceil();
        let mut lines = Vec::new();
        let mut y = origin + first_rung * self.increment;
        while y <= bottom && lines.len() < 10_000 {
            lines.push(y);
            y += self.increment;
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> SpreadLayout {
        SpreadLayout {
            facing_pages: true,
            page_width: 100.0,
            page_height: 200.0,
            spread_gap: 20.0,
        }
    }

    #[test]
    fn page_one_stands_alone_as_a_recto() {
        // The binding convention: a document opens on a right-hand page.
        let layout = layout();
        let page = layout.place(1);

        assert_eq!(page.spread_index, 0);
        assert!(!page.is_left, "page 1 must be a recto");
        assert_eq!(page.x, 100.0, "page 1 sits right of the spine");
    }

    #[test]
    fn pages_two_and_three_share_a_spread() {
        let layout = layout();
        let two = layout.place(2);
        let three = layout.place(3);

        assert_eq!(two.spread_index, three.spread_index);
        assert!(two.is_left, "even pages are versos");
        assert!(!three.is_left);
        assert_eq!(two.y, three.y, "a spread's pages share a baseline");
    }

    #[test]
    fn spines_align_across_spreads() {
        // Every recto starts at the spine, so the spine is a straight vertical
        // line down the whole document.
        let layout = layout();
        assert_eq!(layout.place(1).x, layout.place(3).x);
        assert_eq!(layout.place(3).x, layout.place(5).x);
        assert_eq!(layout.place(2).x, layout.place(4).x);
    }

    #[test]
    fn spreads_stack_downwards_with_a_gap() {
        let layout = layout();
        let first = layout.place(1);
        let second = layout.place(2);

        assert_eq!(first.y, 0.0);
        assert_eq!(second.y, 220.0, "page height plus the spread gap");
    }

    #[test]
    fn non_facing_documents_give_every_page_its_own_spread() {
        let layout = SpreadLayout {
            facing_pages: false,
            ..layout()
        };

        assert_eq!(layout.place(1).spread_index, 0);
        assert_eq!(layout.place(2).spread_index, 1);
        assert_eq!(layout.place(3).spread_index, 2);
        assert!(!layout.place(2).is_left, "no versos without facing pages");
        assert_eq!(layout.place(2).x, 0.0);
    }

    #[test]
    fn bleed_grows_the_trim_box_on_every_side() {
        let page = layout().place(1);
        let (x0, y0, x1, y1) = page.bleed_rect(5.0);
        let (tx0, ty0, tx1, ty1) = page.trim_rect();

        assert_eq!(x0, tx0 - 5.0);
        assert_eq!(y0, ty0 - 5.0);
        assert_eq!(x1, tx1 + 5.0);
        assert_eq!(y1, ty1 + 5.0);
    }

    #[test]
    fn document_bounds_span_every_page() {
        let layout = layout();
        let (min_x, min_y, max_x, max_y) = layout.document_bounds(3).unwrap();

        assert_eq!(min_x, 0.0, "the verso of spread 1 is the leftmost edge");
        assert_eq!(min_y, 0.0);
        assert_eq!(max_x, 200.0, "spine plus one page width");
        assert_eq!(max_y, 420.0);
    }

    #[test]
    fn an_empty_document_has_no_bounds() {
        assert!(layout().document_bounds(0).is_none());
    }

    #[test]
    fn margins_mirror_across_the_spine() {
        // The decisive property of facing-page margins: the wide inside margin
        // must always sit against the spine, on both sides.
        let layout = layout();
        let guides = PageGuides {
            margin_inside: 30.0,
            margin_outside: 10.0,
            ..Default::default()
        };

        let verso = guides.content_rect(&layout.place(2));
        let recto = guides.content_rect(&layout.place(3));

        // The verso's spine is on its right, so its right margin is the wide one.
        let verso_page = layout.place(2);
        assert_eq!(verso.0, verso_page.x + 10.0, "verso outer margin is narrow");
        assert_eq!(
            verso.2,
            verso_page.x + verso_page.width - 30.0,
            "verso inner margin is wide"
        );

        // The recto's spine is on its left.
        let recto_page = layout.place(3);
        assert_eq!(recto.0, recto_page.x + 30.0, "recto inner margin is wide");
    }

    #[test]
    fn columns_divide_the_content_area_with_gutters() {
        let page = layout().place(1);
        let guides = PageGuides {
            margin_inside: 10.0,
            margin_outside: 10.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            columns: 3,
            gutter: 10.0,
        };

        let ranges = guides.column_ranges(&page);
        assert_eq!(ranges.len(), 3);

        // Content area is 80 wide; two 10pt gutters leave 60 for three columns.
        for (start, end) in &ranges {
            assert!((end - start - 20.0).abs() < 1e-3, "expected 20pt columns");
        }
        // Columns must not overlap and must stay inside the content area.
        assert!(ranges[0].1 <= ranges[1].0);
        assert!(ranges[1].1 <= ranges[2].0);
        assert!((ranges[2].1 - (page.x + page.width - 10.0)).abs() < 1e-3);
    }

    #[test]
    fn a_single_column_fills_the_content_area() {
        let page = layout().place(1);
        let guides = PageGuides::default();
        let ranges = guides.column_ranges(&page);
        let (left, _, right, _) = guides.content_rect(&page);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], (left, right));
    }

    #[test]
    fn the_grid_starts_at_the_top_margin() {
        // Origin is per page, so a page with a deeper top margin starts lower —
        // that is what keeps pages with different margins optically aligned.
        let page = layout().place(1);
        let guides = PageGuides { margin_top: 40.0, ..Default::default() };
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        assert_eq!(grid.origin_for(&page, &guides), page.y + 40.0);
    }

    #[test]
    fn the_start_offset_pushes_the_first_baseline_down() {
        let page = layout().place(1);
        let guides = PageGuides { margin_top: 40.0, ..Default::default() };
        let grid = BaselineGrid { increment: 12.0, start: 6.0, visible: true };

        assert_eq!(grid.origin_for(&page, &guides), page.y + 46.0);
    }

    #[test]
    fn leading_rounds_up_to_a_whole_increment() {
        // The decisive rule: 16pt type at 1.4 leading is 22.4pt, which must
        // open up to two 12pt rungs rather than tightening to one.
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        assert_eq!(grid.snapped_leading(22.4), 24.0);
        assert_eq!(grid.snapped_leading(12.0), 12.0, "an exact fit does not grow");
        assert_eq!(grid.snapped_leading(12.1), 24.0);
    }

    #[test]
    fn leading_never_tightens() {
        // Type set looser than the grid takes more rungs; it is never squeezed
        // back down, which would push lines into collision.
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        assert!(grid.snapped_leading(30.0) >= 30.0);
        assert_eq!(grid.snapped_leading(30.0), 36.0);
    }

    #[test]
    fn leading_smaller_than_the_increment_takes_one_full_rung() {
        // 9pt type on a 12pt grid is set at 12pt, not at its natural leading.
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        assert_eq!(grid.snapped_leading(10.8), 12.0);
        assert_eq!(grid.snapped_leading(0.5), 12.0);
    }

    #[test]
    fn a_baseline_is_pushed_down_to_the_next_rung() {
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        // Origin 100, rungs at 100, 112, 124. A baseline at 115 goes to 124.
        assert_eq!(grid.shift_onto_grid(100.0, 115.0), 9.0);
    }

    #[test]
    fn a_baseline_already_on_a_rung_does_not_move() {
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        assert_eq!(grid.shift_onto_grid(100.0, 112.0), 0.0);
    }

    #[test]
    fn a_baseline_above_the_origin_is_pulled_down_onto_it() {
        // A frame sitting above the top margin still has its first line land on
        // the grid rather than being left floating.
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        assert_eq!(grid.shift_onto_grid(100.0, 95.0), 5.0);
    }

    #[test]
    fn the_shift_is_never_upward() {
        // Text must not be dragged up out of its frame to reach a rung.
        let grid = BaselineGrid { increment: 12.0, start: 0.0, visible: true };

        for baseline in [95.0, 100.0, 101.0, 111.9, 112.0, 200.0] {
            assert!(grid.shift_onto_grid(100.0, baseline) >= 0.0);
        }
    }

    #[test]
    fn grid_lines_cover_a_range() {
        let grid = BaselineGrid { increment: 10.0, start: 0.0, visible: true };
        let lines = grid.lines_between(100.0, 100.0, 130.0);

        assert_eq!(lines, vec![100.0, 110.0, 120.0, 130.0]);
    }

    #[test]
    fn grid_lines_start_inside_the_range_when_the_origin_is_above_it() {
        let grid = BaselineGrid { increment: 10.0, start: 0.0, visible: true };
        let lines = grid.lines_between(0.0, 25.0, 45.0);

        assert_eq!(lines, vec![30.0, 40.0], "no rung may fall outside the range");
    }

    #[test]
    fn an_inverted_range_yields_no_lines() {
        let grid = BaselineGrid { increment: 10.0, start: 0.0, visible: true };

        assert!(grid.lines_between(0.0, 50.0, 20.0).is_empty());
    }

    #[test]
    fn a_non_positive_increment_disables_the_grid() {
        // Guards the degenerate settings a user can type into the panel: every
        // rung would be the same line, and lines_between would not terminate.
        for increment in [0.0, -12.0] {
            let grid = BaselineGrid { increment, start: 0.0, visible: true };

            assert!(!grid.is_active());
            assert_eq!(grid.snapped_leading(22.4), 22.4, "leading passes through");
            assert_eq!(grid.shift_onto_grid(0.0, 37.0), 0.0, "nothing to snap to");
            assert!(grid.lines_between(0.0, 0.0, 1000.0).is_empty());
        }
    }

    #[test]
    fn zero_columns_is_treated_as_one() {
        // Guards against a divide-by-zero from a malformed document.
        let page = layout().place(1);
        let guides = PageGuides {
            columns: 0,
            ..Default::default()
        };

        assert_eq!(guides.column_ranges(&page).len(), 1);
    }
}

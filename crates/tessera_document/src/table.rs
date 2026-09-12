//! Tables: a grid of cells, each of which is a piece of text.
//!
//! ## A cell holds a story, not a string
//!
//! [`Cell::story`] is a [`StoryId`] like a text frame's, so a cell gets the
//! whole text model for nothing: character and paragraph styles, runs, the
//! shaper, the caret, find and change. A table whose cells held `String` would
//! be a second, poorer typesetting system inside the first, and every feature
//! added to text would have to be added to it again.
//!
//! ## Spans are recorded once, on the cell that spans
//!
//! The grid is `rows * columns` slots, row-major and complete — no ragged rows,
//! because a ragged grid makes "the cell at row 4, column 2" a search rather
//! than an index, and every operation on a table is addressed that way.
//!
//! A cell that covers its neighbours says so in [`Cell::span`]; the slots it
//! covers are [`Slot::Covered`]. Recording it twice — a span on the owner *and*
//! a back-reference on each covered slot — is the kind of arrangement that
//! drifts, and this codebase has paid for that twice already (the note on
//! `TextLayout::next` is about the same mistake). [`Table::spans_are_sound`] is
//! the invariant every operation here preserves, in the same spirit as
//! `Story::runs_are_sound`.
//!
//! ## What a row's height means
//!
//! [`Table::rows`] is a **minimum**. A row grows to fit the tallest cell in it,
//! which is decided by the layout pass and not stored: a height written down
//! here would be a second copy of a fact the shaper owns, and it would be wrong
//! the moment a typeface changed.

use serde::{Deserialize, Serialize};

use crate::ids::StoryId;
use crate::nodes::{Insets, Stroke, VerticalJustify};
use crate::paint::Paint;

/// How many columns and rows one cell covers.
///
/// `(1, 1)` is an ordinary cell, and is what [`Default`] gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub columns: u16,
    pub rows: u16,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            columns: 1,
            rows: 1,
        }
    }
}

impl Span {
    pub fn is_single(self) -> bool {
        self.columns <= 1 && self.rows <= 1
    }
}

/// One cell that draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub story: StoryId,
    #[serde(default)]
    pub span: Span,
    /// The cell's own background, under its text.
    #[serde(default)]
    pub fill: Option<Paint>,
    /// The space between the cell's edge and its text.
    ///
    /// Per cell rather than per table: a heading row wants more air than the
    /// body, and a table that could not say so would need a second table.
    #[serde(default)]
    pub inset: Insets,
    #[serde(default)]
    pub vertical: VerticalJustify,
}

impl Cell {
    pub fn new(story: StoryId) -> Self {
        Self {
            story,
            span: Span::default(),
            fill: None,
            // Two points, which is about what a rule needs to stop touching
            // the letters beside it. Zero would be typographically wrong on
            // every table anybody makes.
            inset: Insets::uniform(2.0),
            vertical: VerticalJustify::Top,
        }
    }
}

/// One position in the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Slot {
    Cell(Cell),
    /// Covered by a cell above or to the left of it that spans over here.
    Covered,
}

impl Slot {
    pub fn cell(&self) -> Option<&Cell> {
        match self {
            Slot::Cell(cell) => Some(cell),
            Slot::Covered => None,
        }
    }

    pub fn cell_mut(&mut self) -> Option<&mut Cell> {
        match self {
            Slot::Cell(cell) => Some(cell),
            Slot::Covered => None,
        }
    }
}

/// A grid of cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Column widths in points. Never empty.
    ///
    /// Widths rather than proportions: a table is set to a measure, and a
    /// column a designer has dragged to 40mm must stay 40mm when a column is
    /// added beside it rather than being renormalised out from under them.
    pub columns: Vec<f64>,
    /// The **minimum** height of each row, in points. Never empty.
    pub rows: Vec<f64>,
    /// `rows.len() * columns.len()` slots, row-major.
    pub cells: Vec<Slot>,
    /// The rule drawn between and around cells.
    ///
    /// One stroke for the whole table to begin with. Per-edge strokes are what
    /// a table eventually needs and they are a much larger model; this is the
    /// half that makes a table legible, and it is honest about being that.
    #[serde(default)]
    pub stroke: Option<Stroke>,
}

impl Table {
    pub fn columns(&self) -> usize {
        self.columns.len()
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// The index into `cells` of a position in the grid.
    pub fn index(&self, row: usize, column: usize) -> Option<usize> {
        (row < self.rows() && column < self.columns()).then(|| row * self.columns() + column)
    }

    pub fn at(&self, row: usize, column: usize) -> Option<&Slot> {
        self.cells.get(self.index(row, column)?)
    }

    pub fn at_mut(&mut self, row: usize, column: usize) -> Option<&mut Slot> {
        let at = self.index(row, column)?;
        self.cells.get_mut(at)
    }

    /// Every story the table holds, in reading order.
    ///
    /// What a search, a spell check or a style sweep walks. Covered slots hold
    /// none, which is the point of them.
    pub fn stories(&self) -> impl Iterator<Item = StoryId> + '_ {
        self.cells.iter().filter_map(|s| s.cell().map(|c| c.story))
    }

    /// The total width of the columns a cell at this position covers.
    ///
    /// **The span's width, not the column's.** Text in a merged cell is set
    /// across everything it covers; measuring it against one column would wrap
    /// it into a ribbon a third of the width it is drawn at.
    pub fn span_width(&self, row: usize, column: usize) -> Option<f64> {
        let cell = self.at(row, column)?.cell()?;
        let last = (column + usize::from(cell.span.columns.max(1))).min(self.columns());
        Some(self.columns[column..last].iter().sum())
    }

    /// Whether every span is recorded consistently.
    ///
    /// The invariant the whole module preserves: a spanning cell's footprint is
    /// exactly the slots marked [`Slot::Covered`], no slot is covered twice, and
    /// no span runs off the edge of the grid. A table that fails this draws
    /// cells on top of each other or leaves holes, and the symptom appears far
    /// from the operation that caused it.
    pub fn spans_are_sound(&self) -> bool {
        if self.cells.len() != self.rows() * self.columns() {
            return false;
        }
        if self.columns.is_empty() || self.rows.is_empty() {
            return false;
        }

        // Painted by the cells that claim them, then compared against the
        // slots that say they are covered.
        let mut claimed = vec![false; self.cells.len()];
        for row in 0..self.rows() {
            for column in 0..self.columns() {
                let Some(Slot::Cell(cell)) = self.at(row, column) else {
                    continue;
                };
                let (down, across) = (
                    usize::from(cell.span.rows.max(1)),
                    usize::from(cell.span.columns.max(1)),
                );
                if row + down > self.rows() || column + across > self.columns() {
                    return false; // runs off the grid
                }
                for r in row..row + down {
                    for c in column..column + across {
                        let at = r * self.columns() + c;
                        let owner = r == row && c == column;
                        if claimed[at] {
                            return false; // two cells claim one slot
                        }
                        claimed[at] = true;
                        let covered = matches!(self.cells[at], Slot::Covered);
                        if owner == covered {
                            // The owner must be a cell; everything else in the
                            // footprint must be marked covered.
                            return false;
                        }
                    }
                }
            }
        }
        // A covered slot nobody claims is an orphan: it draws nothing and can
        // never be reached, which is a hole in the table.
        claimed.iter().all(|c| *c)
    }
}

/// A plain grid, every cell its own story.
///
/// `make_story` is handed in rather than a document being passed down, because
/// `tessera_document`'s own types do not reach into its `Document` — the same
/// arrangement every other node here uses.
pub fn new(
    rows: usize,
    columns: usize,
    width: f64,
    mut make_story: impl FnMut() -> StoryId,
) -> Table {
    let rows = rows.max(1);
    let columns = columns.max(1);
    let each = if width > 0.0 {
        width / columns as f64
    } else {
        72.0
    };
    Table {
        columns: vec![each; columns],
        // Twelve points: one line of body text plus its insets, which is what
        // an empty row should look like rather than a hairline.
        rows: vec![12.0; rows],
        cells: (0..rows * columns)
            .map(|_| Slot::Cell(Cell::new(make_story())))
            .collect(),
        stroke: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_table(rows: usize, columns: usize) -> Table {
        let mut next = 0u32;
        new(rows, columns, 300.0, || {
            next += 1;
            StoryId::default()
        })
    }

    #[test]
    fn a_new_table_is_sound_and_divides_its_width() {
        let table = a_table(3, 4);
        assert!(table.spans_are_sound());
        assert_eq!(table.cells.len(), 12);
        assert_eq!(table.columns.len(), 4);
        let total: f64 = table.columns.iter().sum();
        assert!((total - 300.0).abs() < 1e-9, "columns must fill the width");
    }

    #[test]
    fn a_position_maps_to_one_slot() {
        let table = a_table(3, 4);
        assert_eq!(table.index(0, 0), Some(0));
        assert_eq!(table.index(2, 3), Some(11));
        assert_eq!(table.index(3, 0), None, "off the bottom");
        assert_eq!(table.index(0, 4), None, "off the right");
    }

    /// Merge the cell at `(row, column)` over `span`, marking what it covers.
    fn merge(table: &mut Table, row: usize, column: usize, span: Span) {
        if let Some(Slot::Cell(cell)) = table.at_mut(row, column) {
            cell.span = span;
        }
        for r in row..row + usize::from(span.rows) {
            for c in column..column + usize::from(span.columns) {
                if (r, c) != (row, column)
                    && let Some(slot) = table.at_mut(r, c)
                {
                    *slot = Slot::Covered;
                }
            }
        }
    }

    #[test]
    fn a_merged_cell_covers_exactly_its_footprint() {
        let mut table = a_table(3, 4);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 2,
                rows: 2,
            },
        );
        assert!(table.spans_are_sound());

        assert!(table.at(0, 0).unwrap().cell().is_some(), "the owner draws");
        for (r, c) in [(0, 1), (1, 0), (1, 1)] {
            assert!(
                table.at(r, c).unwrap().cell().is_none(),
                "({r},{c}) must be covered"
            );
        }
        assert!(
            table.at(0, 2).unwrap().cell().is_some(),
            "the cell beside the span is untouched"
        );
    }

    #[test]
    fn a_span_running_off_the_grid_is_not_sound() {
        // The check earns its keep here: a span two columns wide starting in
        // the last column would index into the next row, so cells would be
        // drawn on top of each other a row down.
        let mut table = a_table(2, 2);
        if let Some(Slot::Cell(cell)) = table.at_mut(0, 1) {
            cell.span = Span {
                columns: 2,
                rows: 1,
            };
        }
        assert!(!table.spans_are_sound());
    }

    #[test]
    fn a_slot_covered_by_nobody_is_not_sound() {
        // A hole: it draws nothing and no cell owns it, so it can never be
        // reached or typed into.
        let mut table = a_table(2, 2);
        *table.at_mut(1, 1).unwrap() = Slot::Covered;
        assert!(!table.spans_are_sound());
    }

    #[test]
    fn two_cells_may_not_claim_one_slot() {
        let mut table = a_table(2, 2);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 2,
                rows: 1,
            },
        );
        // A second cell reaching into the first one's footprint.
        if let Some(Slot::Cell(cell)) = table.at_mut(1, 0) {
            cell.span = Span {
                columns: 1,
                rows: 2,
            };
        }
        assert!(!table.spans_are_sound());
    }

    #[test]
    fn a_merged_cell_is_measured_across_everything_it_covers() {
        // The bug this prevents: text in a cell spanning three columns, set to
        // the width of one, wraps into a ribbon a third of the width it is
        // actually drawn at.
        let mut table = a_table(1, 3);
        let one = table.span_width(0, 0).expect("a width");
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 3,
                rows: 1,
            },
        );
        let merged = table.span_width(0, 0).expect("a width");
        assert!(
            (merged - one * 3.0).abs() < 1e-9,
            "{merged} should be three columns, not {one}"
        );
    }

    #[test]
    fn covered_slots_hold_no_story() {
        // What makes `stories()` the right thing for a search to walk: a
        // merged cell's text must be offered once, not once per slot.
        let mut table = a_table(2, 2);
        assert_eq!(table.stories().count(), 4);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 2,
                rows: 2,
            },
        );
        assert_eq!(table.stories().count(), 1);
    }
}

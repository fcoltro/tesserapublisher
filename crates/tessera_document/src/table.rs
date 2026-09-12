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

impl Table {
    /// Put a row in at `at`, pushing the rest down.
    ///
    /// **A cell spanning across the new boundary grows to keep covering it.**
    /// Leaving the span alone would tear a hole through the middle of a merged
    /// cell, so the grid would stop being sound the moment a row was added
    /// through a merge somebody made earlier.
    pub fn insert_row(&mut self, at: usize, mut make_story: impl FnMut() -> StoryId) {
        let at = at.min(self.rows());
        let columns = self.columns();

        for row in 0..self.rows() {
            for column in 0..columns {
                let Some(Slot::Cell(cell)) = self.at_mut(row, column) else {
                    continue;
                };
                let down = usize::from(cell.span.rows.max(1));
                if row < at && row + down > at {
                    cell.span.rows = cell.span.rows.saturating_add(1);
                }
            }
        }

        // A slot in the new row is covered exactly where a cell above reaches
        // through it — which, after the widening above, is what the row above
        // now says.
        let fresh: Vec<Slot> = (0..columns)
            .map(|column| {
                let reaches = at.checked_sub(1).is_some_and(|above| {
                    self.reaching_down(above, column) || self.is_covered(above, column)
                });
                if reaches {
                    Slot::Covered
                } else {
                    Slot::Cell(Cell::new(make_story()))
                }
            })
            .collect();

        self.rows.insert(at, 12.0);
        let index = at * columns;
        self.cells.splice(index..index, fresh);
        self.heal();
    }

    fn is_covered(&self, row: usize, column: usize) -> bool {
        matches!(self.at(row, column), Some(Slot::Covered))
    }

    /// Whether the cell starting here reaches past its own row.
    fn reaching_down(&self, row: usize, column: usize) -> bool {
        self.at(row, column)
            .and_then(|s| s.cell())
            .is_some_and(|c| c.span.rows > 1)
    }

    /// Whether the cell starting here reaches past its own column.
    fn reaching_right(&self, row: usize, column: usize) -> bool {
        self.at(row, column)
            .and_then(|s| s.cell())
            .is_some_and(|c| c.span.columns > 1)
    }

    /// Take a row out. Refuses to leave the table with none.
    pub fn remove_row(&mut self, at: usize) -> bool {
        if self.rows() <= 1 || at >= self.rows() {
            return false;
        }
        let columns = self.columns();
        for row in 0..self.rows() {
            for column in 0..columns {
                let Some(Slot::Cell(cell)) = self.at_mut(row, column) else {
                    continue;
                };
                let down = usize::from(cell.span.rows.max(1));
                if row <= at && row + down > at && down > 1 {
                    cell.span.rows -= 1;
                }
            }
        }
        self.rows.remove(at);
        let index = at * columns;
        self.cells.drain(index..index + columns);
        self.heal();
        true
    }

    /// Put a column in at `at`, widening any span that crosses the boundary.
    pub fn insert_column(
        &mut self,
        at: usize,
        width: f64,
        mut make_story: impl FnMut() -> StoryId,
    ) {
        let at = at.min(self.columns());
        for row in 0..self.rows() {
            for column in 0..self.columns() {
                let Some(Slot::Cell(cell)) = self.at_mut(row, column) else {
                    continue;
                };
                let across = usize::from(cell.span.columns.max(1));
                if column < at && column + across > at {
                    cell.span.columns = cell.span.columns.saturating_add(1);
                }
            }
        }

        let columns = self.columns();
        let mut fresh: Vec<Slot> = Vec::with_capacity(self.rows());
        for row in 0..self.rows() {
            let reaches = at
                .checked_sub(1)
                .is_some_and(|left| self.reaching_right(row, left) || self.is_covered(row, left));
            fresh.push(if reaches {
                Slot::Covered
            } else {
                Slot::Cell(Cell::new(make_story()))
            });
        }
        // Back to front, so an insertion does not shift the rows below it out
        // from under the index being computed for them.
        for (row, slot) in fresh.into_iter().enumerate().rev() {
            self.cells.insert(row * columns + at, slot);
        }
        self.columns.insert(at, width.max(1.0));
        self.heal();
    }

    /// Take a column out. Refuses to leave the table with none.
    pub fn remove_column(&mut self, at: usize) -> bool {
        if self.columns() <= 1 || at >= self.columns() {
            return false;
        }
        for row in 0..self.rows() {
            for column in 0..self.columns() {
                let Some(Slot::Cell(cell)) = self.at_mut(row, column) else {
                    continue;
                };
                let across = usize::from(cell.span.columns.max(1));
                if column <= at && column + across > at && across > 1 {
                    cell.span.columns -= 1;
                }
            }
        }
        let columns = self.columns();
        for row in (0..self.rows()).rev() {
            self.cells.remove(row * columns + at);
        }
        self.columns.remove(at);
        self.heal();
        true
    }

    /// Merge a rectangle of slots into the cell at its top-left corner.
    ///
    /// Returns the stories of the cells that were absorbed, so the caller can
    /// take them out of the document: a story nothing refers to is a leak the
    /// file then carries forever.
    pub fn merge(&mut self, row: usize, column: usize, span: Span) -> Vec<StoryId> {
        let down = usize::from(span.rows.max(1));
        let across = usize::from(span.columns.max(1));
        if row + down > self.rows() || column + across > self.columns() {
            return Vec::new();
        }

        let mut absorbed = Vec::new();
        for r in row..row + down {
            for c in column..column + across {
                if (r, c) == (row, column) {
                    continue;
                }
                if let Some(slot) = self.at_mut(r, c) {
                    if let Slot::Cell(cell) = slot {
                        absorbed.push(cell.story);
                    }
                    *slot = Slot::Covered;
                }
            }
        }
        if let Some(Slot::Cell(cell)) = self.at_mut(row, column) {
            cell.span = span;
        }
        absorbed
    }

    /// Undo a merge: the cell keeps its story, the rest become empty cells.
    pub fn split(&mut self, row: usize, column: usize, mut make_story: impl FnMut() -> StoryId) {
        let Some(cell) = self.at(row, column).and_then(|s| s.cell()) else {
            return;
        };
        if cell.span.is_single() {
            return;
        }
        let (down, across) = (
            usize::from(cell.span.rows.max(1)),
            usize::from(cell.span.columns.max(1)),
        );
        if let Some(Slot::Cell(cell)) = self.at_mut(row, column) {
            cell.span = Span::default();
        }
        for r in row..(row + down).min(self.rows()) {
            for c in column..(column + across).min(self.columns()) {
                if (r, c) != (row, column)
                    && let Some(slot) = self.at_mut(r, c)
                {
                    *slot = Slot::Cell(Cell::new(make_story()));
                }
            }
        }
    }

    /// Give every slot no spanning cell covers a cell of its own.
    ///
    /// Called after a removal, where a covered slot can be orphaned by the
    /// disappearance of whatever covered it. An orphan draws nothing and can
    /// never be typed into, so the table would have a hole in it — and
    /// `spans_are_sound` would start failing somewhere far from the removal.
    ///
    /// The replacement carries a default story id, which refers to nothing.
    /// That is deliberate and is why the callers in `tessera_ui` mint one
    /// afterwards: this module cannot reach a document to make one, and a
    /// wrong id is caught by the cell drawing nothing, where a silently
    /// shared id would have two cells editing one story.
    fn heal(&mut self) {
        let columns = self.columns();
        let rows = self.rows();
        if columns == 0 || rows == 0 {
            return;
        }
        let mut claimed = vec![false; self.cells.len()];
        for row in 0..rows {
            for column in 0..columns {
                let Some(cell) = self.at(row, column).and_then(|s| s.cell()) else {
                    continue;
                };
                let down = usize::from(cell.span.rows.max(1)).min(rows - row);
                let across = usize::from(cell.span.columns.max(1)).min(columns - column);
                for r in row..row + down {
                    for c in column..column + across {
                        claimed[r * columns + c] = true;
                    }
                }
            }
        }
        for (at, was_claimed) in claimed.into_iter().enumerate() {
            if !was_claimed {
                self.cells[at] = Slot::Cell(Cell::new(StoryId::default()));
            }
        }
    }

    /// Every cell whose story is the default id, which refers to nothing.
    ///
    /// What a caller asks after a structural change so it can mint a real
    /// story for each. See [`Table::heal`].
    pub fn cells_needing_a_story(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for row in 0..self.rows() {
            for column in 0..self.columns() {
                if self
                    .at(row, column)
                    .and_then(|s| s.cell())
                    .is_some_and(|c| c.story == StoryId::default())
                {
                    out.push((row, column));
                }
            }
        }
        out
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

    // --- structural changes, which are where spans break ---------------------

    #[test]
    fn a_row_added_through_a_merge_grows_it_rather_than_tearing_it() {
        // The bug this prevents: insert a row through the middle of a cell
        // that spans two, leave the span at two, and the new row punches a
        // hole through the merge — the grid stops being sound somewhere the
        // person never touched.
        let mut table = a_table(3, 2);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 1,
                rows: 3,
            },
        );
        assert!(table.spans_are_sound());

        table.insert_row(1, StoryId::default);

        assert!(table.spans_are_sound(), "the grid must survive the insert");
        let spanned = table.at(0, 0).unwrap().cell().expect("the merged cell");
        assert_eq!(spanned.span.rows, 4, "the merge must cover the new row too");
    }

    #[test]
    fn a_row_added_outside_a_merge_leaves_it_alone() {
        let mut table = a_table(3, 2);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 1,
                rows: 2,
            },
        );
        table.insert_row(3, StoryId::default);

        assert!(table.spans_are_sound());
        assert_eq!(table.at(0, 0).unwrap().cell().unwrap().span.rows, 2);
        assert_eq!(table.rows(), 4);
    }

    #[test]
    fn a_column_added_through_a_merge_widens_it() {
        let mut table = a_table(2, 3);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 3,
                rows: 1,
            },
        );
        table.insert_column(1, 50.0, StoryId::default);

        assert!(table.spans_are_sound());
        assert_eq!(table.at(0, 0).unwrap().cell().unwrap().span.columns, 4);
        assert_eq!(table.columns(), 4);
        assert_eq!(table.columns[1], 50.0, "the new column keeps its width");
    }

    #[test]
    fn removing_a_row_shrinks_a_merge_that_ran_through_it() {
        let mut table = a_table(3, 2);
        merge(
            &mut table,
            0,
            0,
            Span {
                columns: 1,
                rows: 3,
            },
        );
        assert!(table.remove_row(1));

        assert!(table.spans_are_sound());
        assert_eq!(table.rows(), 2);
        assert_eq!(table.at(0, 0).unwrap().cell().unwrap().span.rows, 2);
    }

    #[test]
    fn removing_the_row_a_merge_starts_in_leaves_no_orphans() {
        // The slots the merge covered have nothing covering them any more.
        // Left as `Covered` they would be holes: drawing nothing, reachable by
        // nothing, and `spans_are_sound` failing far from the removal.
        let mut table = a_table(3, 2);
        merge(
            &mut table,
            1,
            0,
            Span {
                columns: 2,
                rows: 2,
            },
        );
        assert!(table.remove_row(1));

        assert!(
            table.spans_are_sound(),
            "every orphaned slot must become a cell again"
        );
        assert_eq!(table.rows(), 2);
    }

    #[test]
    fn a_table_refuses_to_lose_its_last_row_or_column() {
        // A table with no rows is not a table, and every operation on one
        // would then be indexing into nothing.
        let mut table = a_table(1, 1);
        assert!(!table.remove_row(0));
        assert!(!table.remove_column(0));
        assert_eq!(table.rows(), 1);
        assert_eq!(table.columns(), 1);
    }

    #[test]
    fn merging_reports_the_stories_it_absorbed() {
        // So the caller can take them out of the document. A story nothing
        // refers to is a leak the file carries forever.
        let mut table = a_table(2, 2);
        let absorbed = table.merge(
            0,
            0,
            Span {
                columns: 2,
                rows: 2,
            },
        );
        assert_eq!(absorbed.len(), 3, "three cells were covered");
        assert!(table.spans_are_sound());
        assert_eq!(table.stories().count(), 1);
    }

    #[test]
    fn merging_off_the_edge_does_nothing_at_all() {
        let mut table = a_table(2, 2);
        let before = table.clone();
        let absorbed = table.merge(
            1,
            1,
            Span {
                columns: 2,
                rows: 2,
            },
        );
        assert!(absorbed.is_empty());
        assert_eq!(table, before, "a refused merge must change nothing");
    }

    #[test]
    fn splitting_gives_every_covered_slot_a_cell_back() {
        let mut table = a_table(2, 2);
        table.merge(
            0,
            0,
            Span {
                columns: 2,
                rows: 2,
            },
        );
        table.split(0, 0, StoryId::default);

        assert!(table.spans_are_sound());
        assert_eq!(table.stories().count(), 4);
        assert!(table.at(0, 0).unwrap().cell().unwrap().span.is_single());
    }

    #[test]
    fn a_healed_cell_asks_for_a_story() {
        // `heal` cannot mint one — this module never reaches a document — so
        // it leaves a default id and says so. A caller that ignored this would
        // leave two cells sharing one story, and typing in either would show
        // in both.
        let mut table = a_table(3, 2);
        merge(
            &mut table,
            1,
            0,
            Span {
                columns: 1,
                rows: 2,
            },
        );
        assert!(table.remove_row(1));
        assert!(!table.cells_needing_a_story().is_empty());
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

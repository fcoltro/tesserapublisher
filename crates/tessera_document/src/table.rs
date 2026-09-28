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
    /// The rule on each side, where it is not the table's. Boxed: four
    /// strokes would make every cell, and every covered slot beside it, the
    /// size of the rare cell that has them.
    #[serde(default, skip_serializing_if = "plain_edges")]
    pub edges: Box<CellEdges>,
    /// The cell style it takes, over the one its row's region takes.
    #[serde(default)]
    pub style: Option<crate::ids::CellStyleId>,
    /// The properties somebody set on this cell by hand, which keep their
    /// own values whatever a style says.
    #[serde(default, skip_serializing_if = "CellLocal::is_none")]
    pub local: CellLocal,
}

/// Which of a cell's properties were set on it by hand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellLocal {
    #[serde(default)]
    pub fill: bool,
    #[serde(default)]
    pub inset: bool,
    #[serde(default)]
    pub vertical: bool,
    #[serde(default)]
    pub edges: bool,
}

impl CellLocal {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

/// Which of a table's own properties were set on it by hand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableLocal {
    #[serde(default)]
    pub stroke: bool,
    #[serde(default)]
    pub alternating: bool,
}

impl TableLocal {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
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
            edges: Box::default(),
            style: None,
            local: CellLocal::default(),
        }
    }
}

fn plain_edges(edges: &CellEdges) -> bool {
    edges.is_plain()
}

/// The rules on a cell's four sides.
///
/// `None` draws the table's own stroke there; a stroke draws that instead,
/// and a stroke of no width draws nothing — how a heading row loses the rule
/// between its cells, or a total gets a heavier one above it.
///
/// **An edge is shared.** A cell's right side is its neighbour's left, and
/// only one line is drawn there: the side that is set wins, and where both
/// are, the cell below or to the right does, as it is the later one read.
/// Setting an edge through a command sets both sides, so the two never
/// disagree in a document a person made; the rule is for one that does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CellEdges {
    #[serde(default)]
    pub top: Option<Stroke>,
    #[serde(default)]
    pub right: Option<Stroke>,
    #[serde(default)]
    pub bottom: Option<Stroke>,
    #[serde(default)]
    pub left: Option<Stroke>,
}

impl CellEdges {
    /// Every side the table's.
    pub fn is_plain(&self) -> bool {
        self.top.is_none() && self.right.is_none() && self.bottom.is_none() && self.left.is_none()
    }
}

/// Rows filled in turn: `first` rows in one colour, then `next` in another,
/// and round again — InDesign's alternating fills. The first `skip_first`
/// rows (a heading) and the last `skip_last` (a total) are left out, and a
/// cell's own fill wins over the pattern, so a highlighted cell stays
/// highlighted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlternatingFills {
    pub first: u16,
    #[serde(default)]
    pub first_fill: Option<Paint>,
    pub next: u16,
    #[serde(default)]
    pub next_fill: Option<Paint>,
    #[serde(default)]
    pub skip_first: u16,
    #[serde(default)]
    pub skip_last: u16,
}

impl AlternatingFills {
    /// Every other row: one row in `fill`, one plain.
    pub fn every_other_row(fill: Paint) -> Self {
        Self {
            first: 1,
            first_fill: Some(fill),
            next: 1,
            next_fill: None,
            skip_first: 0,
            skip_last: 0,
        }
    }

    /// The fill row `row` of `rows` takes from the pattern, if any.
    pub fn fill_for_row(&self, row: usize, rows: usize) -> Option<&Paint> {
        let skip_first = usize::from(self.skip_first);
        let skip_last = usize::from(self.skip_last);
        if row < skip_first || row + skip_last >= rows {
            return None;
        }
        let (first, next) = (usize::from(self.first), usize::from(self.next));
        let cycle = first + next;
        if cycle == 0 {
            return None;
        }
        if (row - skip_first) % cycle < first {
            self.first_fill.as_ref()
        } else {
            self.next_fill.as_ref()
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
    /// The rule drawn between and around cells, where a cell's own
    /// [`CellEdges`] do not say otherwise.
    #[serde(default)]
    pub stroke: Option<Stroke>,
    /// Rows filled in turn, under any cell's own fill. Boxed for the size of
    /// every frame kind, as a cell's edges are.
    #[serde(default)]
    pub alternating: Option<Box<AlternatingFills>>,
    /// The table style it takes.
    #[serde(default)]
    pub style: Option<crate::ids::TableStyleId>,
    /// The table's own properties set by hand, which a style does not move.
    #[serde(default, skip_serializing_if = "TableLocal::is_none")]
    pub local: TableLocal,
    /// How many rows at the top are the heading, and at the foot the
    /// footing: the rows a table style gives their own cell style, and that
    /// repeat when a table runs on across frames.
    #[serde(default)]
    pub header_rows: u16,
    #[serde(default)]
    pub footer_rows: u16,
    /// The frames the table runs on into when its own has no more room, in
    /// order: each a [`crate::nodes::FrameKind::TablePart`]. Empty, the whole
    /// table is set in its own frame, as every table was before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<crate::ids::FrameId>,
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
        alternating: None,
        style: None,
        local: TableLocal::default(),
        header_rows: 0,
        footer_rows: 0,
        parts: Vec::new(),
    }
}

/// One side of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];
}

/// What one unit of a grid line — a column's width of a row boundary, or a
/// row's height of a column boundary — is drawn as.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rule<'a> {
    /// Inside a cell that spans across it: nothing is drawn.
    Inside,
    /// Between two cells, or on the table's edge: this stroke, if any.
    Drawn(Option<&'a Stroke>),
}

impl CellEdges {
    pub fn side(&self, side: Side) -> Option<&Stroke> {
        match side {
            Side::Top => self.top.as_ref(),
            Side::Right => self.right.as_ref(),
            Side::Bottom => self.bottom.as_ref(),
            Side::Left => self.left.as_ref(),
        }
    }

    pub fn side_mut(&mut self, side: Side) -> &mut Option<Stroke> {
        match side {
            Side::Top => &mut self.top,
            Side::Right => &mut self.right,
            Side::Bottom => &mut self.bottom,
            Side::Left => &mut self.left,
        }
    }
}

impl Table {
    /// Which cell covers each slot, as its row and column: row-major, one
    /// per slot. A covered slot says nothing of its owner in the model, so
    /// this is worked out from the spans rather than stored twice.
    pub fn owners(&self) -> Vec<Option<(usize, usize)>> {
        let (rows, columns) = (self.rows(), self.columns());
        let mut owners = vec![None; rows * columns];
        for row in 0..rows {
            for column in 0..columns {
                let Some(Slot::Cell(cell)) = self.at(row, column) else {
                    continue;
                };
                let down = usize::from(cell.span.rows.max(1));
                let across = usize::from(cell.span.columns.max(1));
                for r in row..(row + down).min(rows) {
                    for c in column..(column + across).min(columns) {
                        owners[r * columns + c] = Some((row, column));
                    }
                }
            }
        }
        owners
    }

    fn owning_cell(&self, owner: Option<(usize, usize)>) -> Option<&Cell> {
        let (row, column) = owner?;
        self.at(row, column)?.cell()
    }

    /// The row boundary above `row` (`0..=rows`), under `column`: what is
    /// drawn there, from the cell below's top, the cell above's bottom, or
    /// the table's stroke. `owners` is [`Table::owners`], worked out once.
    pub fn rule_across(
        &self,
        owners: &[Option<(usize, usize)>],
        row: usize,
        column: usize,
    ) -> Rule<'_> {
        let columns = self.columns();
        let above = row
            .checked_sub(1)
            .and_then(|r| owners.get(r * columns + column))
            .copied()
            .flatten();
        let below = (row < self.rows())
            .then(|| owners.get(row * columns + column))
            .flatten()
            .copied()
            .flatten();
        if above.is_some() && above == below {
            return Rule::Inside;
        }
        let set = self
            .owning_cell(below)
            .and_then(|c| c.edges.top.as_ref())
            .or_else(|| {
                self.owning_cell(above)
                    .and_then(|c| c.edges.bottom.as_ref())
            });
        Rule::Drawn(set.or(self.stroke.as_ref()))
    }

    /// The column boundary left of `column` (`0..=columns`), beside `row`,
    /// the same way.
    pub fn rule_down(
        &self,
        owners: &[Option<(usize, usize)>],
        row: usize,
        column: usize,
    ) -> Rule<'_> {
        let columns = self.columns();
        let left = column
            .checked_sub(1)
            .and_then(|c| owners.get(row * columns + c))
            .copied()
            .flatten();
        let right = (column < columns)
            .then(|| owners.get(row * columns + column))
            .flatten()
            .copied()
            .flatten();
        if left.is_some() && left == right {
            return Rule::Inside;
        }
        let set = self
            .owning_cell(right)
            .and_then(|c| c.edges.left.as_ref())
            .or_else(|| self.owning_cell(left).and_then(|c| c.edges.right.as_ref()));
        Rule::Drawn(set.or(self.stroke.as_ref()))
    }

    /// Draw `side` of the cell at `row`, `column` as `stroke` — `None` for
    /// the table's own — and the facing side of every neighbour along it,
    /// so the one line both share says one thing.
    pub fn set_side(&mut self, row: usize, column: usize, side: Side, stroke: Option<Stroke>) {
        let owners = self.owners();
        let (rows, columns) = (self.rows(), self.columns());
        let Some(Slot::Cell(cell)) = self.at(row, column) else {
            return;
        };
        let down = usize::from(cell.span.rows.max(1)).min(rows - row);
        let across = usize::from(cell.span.columns.max(1)).min(columns - column);
        // The slots on the far side of this edge, and the side they face it
        // with.
        let (facing, neighbours): (Side, Vec<(usize, usize)>) = match side {
            Side::Top => (
                Side::Bottom,
                row.checked_sub(1)
                    .map(|r| (column..column + across).map(|c| (r, c)).collect())
                    .unwrap_or_default(),
            ),
            Side::Bottom => (
                Side::Top,
                if row + down < rows {
                    (column..column + across).map(|c| (row + down, c)).collect()
                } else {
                    Vec::new()
                },
            ),
            Side::Left => (
                Side::Right,
                column
                    .checked_sub(1)
                    .map(|c| (row..row + down).map(|r| (r, c)).collect())
                    .unwrap_or_default(),
            ),
            Side::Right => (
                Side::Left,
                if column + across < columns {
                    (row..row + down).map(|r| (r, column + across)).collect()
                } else {
                    Vec::new()
                },
            ),
        };
        let mut owning: Vec<(usize, usize)> = neighbours
            .into_iter()
            .filter_map(|(r, c)| owners[r * columns + c])
            .collect();
        owning.dedup();
        // Set by hand, on both sides: a style no longer moves these edges.
        for (r, c) in owning {
            if let Some(cell) = self.at_mut(r, c).and_then(Slot::cell_mut) {
                *cell.edges.side_mut(facing) = stroke.clone();
                cell.local.edges = true;
            }
        }
        if let Some(cell) = self.at_mut(row, column).and_then(Slot::cell_mut) {
            *cell.edges.side_mut(side) = stroke;
            cell.local.edges = true;
        }
    }

    /// Put the rows from `from` on in the order `order` gives — each entry
    /// the index, counted from `from`, of the row that goes there — every row
    /// keeping its cells and its height.
    ///
    /// `false`, changing nothing, when `order` is not an arrangement of those
    /// rows, or when a cell among them spans rows: a row cannot move away
    /// from the rows it shares a cell with, and splitting the cell to let it
    /// would change the table rather than sort it.
    pub fn reorder_rows(&mut self, from: usize, order: &[usize]) -> bool {
        let (rows, columns) = (self.rows(), self.columns());
        if from > rows || order.len() != rows - from {
            return false;
        }
        let mut seen = order.to_vec();
        seen.sort_unstable();
        if seen.iter().enumerate().any(|(i, &n)| i != n) {
            return false;
        }
        let owners = self.owners();
        for row in from..rows {
            for column in 0..columns {
                if owners[row * columns + column].is_some_and(|(r, _)| r != row) {
                    return false;
                }
                if let Some(Slot::Cell(cell)) = self.at(row, column)
                    && cell.span.rows > 1
                {
                    return false;
                }
            }
        }
        let (cells, heights) = (self.cells.clone(), self.rows.clone());
        for (i, &source) in order.iter().enumerate() {
            let (to, source) = (from + i, from + source);
            self.rows[to] = heights[source];
            for column in 0..columns {
                self.cells[to * columns + column] = cells[source * columns + column].clone();
            }
        }
        true
    }

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
        if row.saturating_add(down) > self.rows()
            || column.saturating_add(across) > self.columns()
            || self.at(row, column).and_then(Slot::cell).is_none()
        {
            return Vec::new();
        }

        // A merge may absorb whole spans, never only their origins or tails.
        // Validate before changing slots so a refusal preserves every story.
        for r in 0..self.rows() {
            for c in 0..self.columns() {
                let Some(cell) = self.at(r, c).and_then(Slot::cell) else {
                    continue;
                };
                let bottom = r + usize::from(cell.span.rows.max(1));
                let right = c + usize::from(cell.span.columns.max(1));
                let overlaps =
                    r < row + down && bottom > row && c < column + across && right > column;
                if overlaps
                    && (r < row || c < column || bottom > row + down || right > column + across)
                {
                    return Vec::new();
                }
            }
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
            cell.span = Span {
                rows: span.rows.max(1),
                columns: span.columns.max(1),
            };
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

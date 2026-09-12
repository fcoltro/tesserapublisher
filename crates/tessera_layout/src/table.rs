//! Laying a table's cells out, and finding how tall its rows have to be.
//!
//! ## A row's height is computed, never stored
//!
//! [`tessera_document::table::Table::rows`] holds a **minimum**. What a row
//! actually takes is whatever its tallest cell needs, and that is a fact the
//! shaper owns: it depends on the typeface, the size, the leading and the
//! column width, every one of which can change without the table being told.
//! Writing the answer into the document would be a second copy of it, and the
//! second copy is the one that goes stale.
//!
//! ## Two passes, because a span cannot be measured in the first
//!
//! A cell covering one row contributes its height to that row directly. A cell
//! covering three has no single row to contribute to — its content has to fit
//! the three together, and which of them grows is a choice. The rows are
//! therefore settled from the single-row cells first, and spanning cells are
//! then given room by growing the **last** row they cover, which is the row
//! whose boundary moves without disturbing anything already measured above it.

use tessera_document::document::Document;
use tessera_document::table::{Slot, Table};
use tessera_geometry::DocRect;
use tessera_text::shape::{Column, ShapedText};
use tessera_text::{Shaper, Story};

/// One cell, laid out in the frame's own space.
// No `PartialEq`: `ShapedText` has none, and a laid-out table is compared by
// the facts tests care about — edges, bounds, overset — rather than glyph for
// glyph.
#[derive(Debug, Clone)]
pub struct LaidCell {
    /// Which slot this is, so an editor can map a click back to the model.
    pub row: usize,
    pub column: usize,
    /// The cell's whole box, insets included, relative to the frame's origin.
    pub bounds: DocRect,
    /// The box the text was flowed into: `bounds` less the cell's insets.
    pub text_area: DocRect,
    pub shaped: ShapedText,
    /// Lines that did not fit. A cell cannot pass text on to anywhere, so any
    /// overset here is text the reader will never see.
    pub overset_lines: usize,
    /// The cell's background, already resolved through the swatch table.
    ///
    /// **On the cell, not in a list beside it.** A `Vec<Option<Paint>>` running
    /// parallel to `cells` is one reordering away from painting every cell the
    /// colour of its neighbour, and nothing would catch it.
    pub fill: Option<tessera_document::paint::Paint>,
    /// The colour this cell's text is set in, resolved the same way.
    pub color: tessera_color::Color,
}

/// A whole table, laid out.
#[derive(Debug, Clone, Default)]
pub struct LaidTable {
    pub cells: Vec<LaidCell>,
    /// The x of every column boundary, `columns + 1` of them, from zero.
    pub column_edges: Vec<f64>,
    /// The y of every row boundary, `rows + 1` of them, from zero.
    pub row_edges: Vec<f64>,
}

impl LaidTable {
    /// The whole grid's size.
    pub fn size(&self) -> (f64, f64) {
        (
            self.column_edges.last().copied().unwrap_or(0.0),
            self.row_edges.last().copied().unwrap_or(0.0),
        )
    }
}

/// Lay a table out, shaping every cell.
///
/// `story_of` resolves a cell's id to its text, so this does not need to know
/// how a document stores stories — and so a composing input method can splice
/// its preview in exactly as it does for a text frame.
pub fn lay_out(
    table: &Table,
    doc: &Document,
    shaper: &mut Shaper,
    mut story_of: impl FnMut(tessera_document::ids::StoryId) -> Option<Story>,
) -> LaidTable {
    let columns = table.columns();
    let rows = table.rows();
    if columns == 0 || rows == 0 {
        return LaidTable::default();
    }

    // Column boundaries: fixed, because a column's width is a decision the
    // designer made and nothing here may revise it.
    let mut column_edges = Vec::with_capacity(columns + 1);
    let mut x = 0.0;
    column_edges.push(0.0);
    for width in &table.columns {
        x += width.max(0.0);
        column_edges.push(x);
    }

    // Shape every owning cell once, at the width it will really be set to.
    struct Measured {
        row: usize,
        column: usize,
        shaped: ShapedText,
        // The height the text needs, insets included.
        needs: f64,
        down: usize,
        // Taken from the cell's own first run, exactly as a text frame takes
        // its colour, so a cell set in red is red.
        color: tessera_color::Color,
    }
    let mut measured = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let Some(Slot::Cell(cell)) = table.at(row, column) else {
                continue;
            };
            let Some(story) = story_of(cell.story) else {
                continue;
            };
            let width = table.span_width(row, column).unwrap_or(0.0);
            let inner = (width - cell.inset.left - cell.inset.right).max(0.0);
            let color = story
                .runs
                .first()
                .map(|run| story.resolve_run(run, doc))
                .and_then(|f| f.colour)
                .unwrap_or(tessera_color::Color::BLACK);
            let shaped = shaper.shape(&story, doc, inner);
            let needs = shaped.height + cell.inset.top + cell.inset.bottom;
            measured.push(Measured {
                row,
                column,
                shaped,
                needs,
                down: usize::from(cell.span.rows.max(1)),
                color,
            });
        }
    }

    // Pass one: the rows that single-row cells decide.
    let mut heights: Vec<f64> = table.rows.iter().map(|h| h.max(0.0)).collect();
    for m in measured.iter().filter(|m| m.down == 1) {
        heights[m.row] = heights[m.row].max(m.needs);
    }

    // Pass two: a spanning cell grows the last row it covers, if the rows it
    // already has are not enough between them.
    for m in measured.iter().filter(|m| m.down > 1) {
        let last = (m.row + m.down - 1).min(rows - 1);
        let have: f64 = heights[m.row..=last].iter().sum();
        if m.needs > have {
            heights[last] += m.needs - have;
        }
    }

    let mut row_edges = Vec::with_capacity(rows + 1);
    let mut y = 0.0;
    row_edges.push(0.0);
    for height in &heights {
        y += height;
        row_edges.push(y);
    }

    // Now the rows are settled, flow each cell's lines into the box it really
    // has. Shaping was done at the right width already, so this only decides
    // which lines fit and where they sit vertically.
    let mut cells = Vec::with_capacity(measured.len());
    for m in measured {
        let Some(Slot::Cell(cell)) = table.at(m.row, m.column) else {
            continue;
        };
        let last_row = (m.row + m.down - 1).min(rows - 1);
        let bounds = DocRect {
            x: column_edges[m.column],
            y: row_edges[m.row],
            width: table.span_width(m.row, m.column).unwrap_or(0.0),
            height: row_edges[last_row + 1] - row_edges[m.row],
        };
        let text_area = DocRect {
            x: bounds.x + cell.inset.left,
            y: bounds.y + cell.inset.top,
            width: (bounds.width - cell.inset.left - cell.inset.right).max(0.0),
            height: (bounds.height - cell.inset.top - cell.inset.bottom).max(0.0),
        };

        let flowed = tessera_text::shape::flow_justified(
            m.shaped,
            &[Column {
                x: text_area.x,
                y: text_area.y,
                width: text_area.width,
                height: text_area.height,
            }],
            vertical_of(cell.vertical),
        );

        cells.push(LaidCell {
            row: m.row,
            column: m.column,
            bounds,
            text_area,
            shaped: flowed.text,
            overset_lines: flowed.overset_lines,
            fill: cell.fill.as_ref().map(|p| doc.resolve_paint(p)),
            color: doc.resolve_colour(&m.color),
        });
    }

    LaidTable {
        cells,
        column_edges,
        row_edges,
    }
}

/// The document's enum mapped onto the shaper's, as `resolve_one` does for a
/// text frame. Two enums rather than one because `tessera_text` knows nothing
/// about documents.
fn vertical_of(v: tessera_document::nodes::VerticalJustify) -> tessera_text::shape::Vertical {
    use tessera_document::nodes::VerticalJustify as V;
    use tessera_text::shape::Vertical;
    match v {
        V::Top => Vertical::Top,
        V::Centre => Vertical::Centre,
        V::Bottom => Vertical::Bottom,
        V::Justify => Vertical::Justify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::ids::StoryId;
    use tessera_document::table::{Span, new};

    /// A table whose cells all hold `text`, and the document holding them.
    fn a_table(rows: usize, columns: usize, text: &str) -> (Document, Table) {
        let mut doc = Document::default();
        let table = new(rows, columns, 300.0, || {
            doc.stories.insert(Story::new(text))
        });
        (doc, table)
    }

    fn lay(doc: &Document, table: &Table) -> LaidTable {
        let mut shaper = Shaper::new();
        let stories: Vec<(StoryId, Story)> =
            doc.stories.iter().map(|(id, s)| (id, s.clone())).collect();
        lay_out(table, doc, &mut shaper, |id| {
            stories
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, s)| s.clone())
        })
    }

    #[test]
    fn every_cell_is_laid_out_once_and_inside_the_grid() {
        let (doc, table) = a_table(3, 4, "x");
        let laid = lay(&doc, &table);

        assert_eq!(laid.cells.len(), 12);
        assert_eq!(laid.column_edges.len(), 5, "one more edge than columns");
        assert_eq!(laid.row_edges.len(), 4);
        let (w, h) = laid.size();
        assert!((w - 300.0).abs() < 1e-9, "the grid fills its measure");
        assert!(h > 0.0);
    }

    #[test]
    fn cells_tile_without_gaps_or_overlaps() {
        // The property that makes a grid a grid. A cell that started anywhere
        // but its column's edge would leave a seam the rules draw into.
        let (doc, table) = a_table(2, 3, "x");
        let laid = lay(&doc, &table);

        for cell in &laid.cells {
            assert_eq!(cell.bounds.x, laid.column_edges[cell.column]);
            assert_eq!(cell.bounds.y, laid.row_edges[cell.row]);
            assert_eq!(
                cell.bounds.x + cell.bounds.width,
                laid.column_edges[cell.column + 1]
            );
        }
    }

    #[test]
    fn a_row_grows_to_fit_the_tallest_cell_in_it() {
        // The stored height is a minimum. A row that kept it would clip the
        // one cell in the table with two lines in it.
        let (mut doc, table) = a_table(2, 2, "one line");
        // Give one cell enough text to wrap several times in a 150pt column.
        let long = "a rather long sentence that will certainly wrap more than once here";
        if let Some(Slot::Cell(cell)) = table.at(1, 0)
            && let Some(story) = doc.stories.get_mut(cell.story)
        {
            *story = Story::new(long);
        }
        let laid = lay(&doc, &table);

        let first = laid.row_edges[1] - laid.row_edges[0];
        let second = laid.row_edges[2] - laid.row_edges[1];
        assert!(
            second > first,
            "the row with more text must be taller: {second} vs {first}"
        );
    }

    #[test]
    fn a_stored_height_is_a_floor_not_a_ceiling() {
        let (doc, mut table) = a_table(1, 1, "x");
        table.rows[0] = 200.0;
        let laid = lay(&doc, &table);
        assert!(laid.row_edges[1] >= 200.0);
    }

    #[test]
    fn a_merged_cell_is_set_across_every_column_it_covers() {
        // The bug this prevents: text shaped to one column's width and drawn
        // across three, so a heading wraps into a ribbon down the left.
        let (doc, mut table) = a_table(1, 3, "a heading that spans the whole table");
        let narrow = lay(&doc, &table);
        let one_column = narrow.cells[0].text_area.width;

        if let Some(Slot::Cell(cell)) = table.at_mut(0, 0) {
            cell.span = Span {
                columns: 3,
                rows: 1,
            };
        }
        for c in 1..3 {
            *table.at_mut(0, c).unwrap() = Slot::Covered;
        }
        assert!(table.spans_are_sound());

        let wide = lay(&doc, &table);
        assert_eq!(wide.cells.len(), 1, "covered slots lay out nothing");
        assert!(
            wide.cells[0].text_area.width > one_column * 2.0,
            "a spanned cell must be set across its whole span"
        );
    }

    #[test]
    fn a_cell_spanning_rows_is_given_room_by_the_last_row_it_covers() {
        // Growing the first row instead would move every boundary below it,
        // undoing measurements already taken.
        let (mut doc, mut table) = a_table(3, 2, "x");
        if let Some(Slot::Cell(cell)) = table.at_mut(0, 0) {
            cell.span = Span {
                columns: 1,
                rows: 2,
            };
        }
        *table.at_mut(1, 0).unwrap() = Slot::Covered;
        assert!(table.spans_are_sound());

        let tall = "a long piece of copy in a merged cell that needs several lines to set";
        if let Some(Slot::Cell(cell)) = table.at(0, 0)
            && let Some(story) = doc.stories.get_mut(cell.story)
        {
            *story = Story::new(tall);
        }
        let laid = lay(&doc, &table);
        let merged = laid
            .cells
            .iter()
            .find(|c| (c.row, c.column) == (0, 0))
            .expect("the merged cell");

        assert_eq!(
            merged.bounds.height,
            laid.row_edges[2] - laid.row_edges[0],
            "the cell must cover both of its rows exactly"
        );
        assert_eq!(merged.overset_lines, 0, "and be tall enough for its text");
    }

    #[test]
    fn an_empty_table_lays_out_nothing_rather_than_panicking() {
        let doc = Document::default();
        let table = Table {
            columns: Vec::new(),
            rows: Vec::new(),
            cells: Vec::new(),
            stroke: None,
        };
        let laid = lay(&doc, &table);
        assert_eq!(laid.size(), (0.0, 0.0));
    }
}

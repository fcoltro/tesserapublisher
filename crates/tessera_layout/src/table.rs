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
use tessera_document::nodes::Stroke;
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
    /// The lines to draw, each with its own stroke, colours resolved.
    /// Worked out here, once, so the screen and the PDF draw the same rules
    /// rather than each deciding which edges a merged cell hides.
    pub rules: Vec<LaidRule>,
    /// Rows no frame had room for: counted on the last frame a table runs
    /// on into, as text overset is counted on the last frame of a thread.
    pub overset_rows: usize,
}

/// One straight rule, from one point to another in the frame's own space.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidRule {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub stroke: Stroke,
}

impl LaidTable {
    /// Half the widest rule: how far the table's ink reaches past its grid,
    /// since a rule is centred on its edge.
    pub fn rule_reach(&self) -> f64 {
        self.rules
            .iter()
            .map(|r| r.stroke.width / 2.0)
            .fold(0.0, f64::max)
    }
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
///
/// `styles` is what the cells are shaped against: the document's styles and
/// what the page the table stands on says its markers read as, so a page
/// number, a chapter number or a merge field in a cell reads as it would in
/// a text frame on the same page.
pub fn lay_out(
    table: &Table,
    doc: &Document,
    styles: &dyn tessera_text::story::Styles,
    shaper: &mut Shaper,
    mut story_of: impl FnMut(tessera_document::ids::StoryId) -> Option<Story>,
) -> LaidTable {
    // Its styles resolved into plain values first, so nothing below needs
    // to know a table can have them.
    let table = &*doc.styled_table(table);
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
            let color = doc.resolve_colour(&color);
            let mut shaped = shaper.shape(&story, styles, inner);
            shaped.resolve_colours(|c| doc.resolve_colour(c));
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
            // The cell's own, else the row's turn in the alternating fills.
            fill: cell
                .fill
                .as_ref()
                .or_else(|| {
                    table
                        .alternating
                        .as_ref()
                        .and_then(|a| a.fill_for_row(m.row, rows))
                })
                .map(|p| doc.resolve_paint(p)),
            color: doc.resolve_colour(&m.color),
        });
    }

    let rules = rules_of(table, doc, &column_edges, &row_edges);
    LaidTable {
        cells,
        column_edges,
        row_edges,
        rules,
        overset_rows: 0,
    }
}

/// One frame's share of a table that runs on across frames: the heading
/// rows, the body rows that fit, and the footing rows — `index` 0 being the
/// table's own frame and each after it one of [`Table::parts`], whose
/// heights are `capacities`, in order.
///
/// The whole table is laid out once to learn its rows' heights, and the
/// body is shared out a frame at a time: as many rows as the frame holds
/// under its heading and over its footing, and never fewer than one, so a
/// row taller than a frame still moves on. Rows that share a cell spanning
/// them go together. What the last frame cannot hold is counted in
/// [`LaidTable::overset_rows`].
///
/// Each share is then laid out as a table of its own, so rules, fills,
/// styles and the page's variables all work as they do for a whole table.
/// The alternating fills are written into the cells first: shared out, a
/// row's turn in the pattern would otherwise restart on every page.
pub fn lay_out_part(
    table: &Table,
    doc: &Document,
    styles: &dyn tessera_text::story::Styles,
    shaper: &mut Shaper,
    mut story_of: impl FnMut(tessera_document::ids::StoryId) -> Option<Story>,
    capacities: &[f64],
    index: usize,
) -> LaidTable {
    let mut whole = doc.styled_table(table).into_owned();
    bake_alternating(&mut whole);
    // Styled already: laying it out again must not style it a second time.
    whole.style = None;
    let full = lay_out(&whole, doc, styles, shaper, &mut story_of);
    let (shares, overset) = split_rows(&whole, &full, capacities);
    let Some(body) = shares.get(index) else {
        return LaidTable::default();
    };
    let part = part_table(&whole, body.clone());
    let mut laid = lay_out(&part, doc, styles, shaper, story_of);
    if index + 1 == capacities.len() {
        laid.overset_rows = overset;
    }
    laid
}

/// Write each row's turn in the alternating fills into its cells that have
/// no fill of their own, and stop the pattern.
fn bake_alternating(table: &mut Table) {
    let Some(pattern) = table.alternating.take() else {
        return;
    };
    let rows = table.rows();
    for row in 0..rows {
        let Some(fill) = pattern.fill_for_row(row, rows).cloned() else {
            continue;
        };
        for column in 0..table.columns() {
            if let Some(Slot::Cell(cell)) = table.at_mut(row, column)
                && cell.fill.is_none()
            {
                cell.fill = Some(fill.clone());
            }
        }
    }
}

/// The body rows each frame shows, and how many rows none had room for.
pub fn split_rows(
    table: &Table,
    laid: &LaidTable,
    capacities: &[f64],
) -> (Vec<std::ops::Range<usize>>, usize) {
    let rows = table.rows();
    let header = usize::from(table.header_rows).min(rows);
    let footer = usize::from(table.footer_rows).min(rows - header);
    let heights: Vec<f64> = laid.row_edges.windows(2).map(|w| w[1] - w[0]).collect();
    let sum = |range: std::ops::Range<usize>| -> f64 {
        heights.get(range).map_or(0.0, |h| h.iter().sum())
    };
    let (head, foot) = (sum(0..header), sum(rows - footer..rows));

    // The body in pieces that cannot be split: a boundary a cell spans
    // across keeps the rows either side of it together.
    let owners = table.owners();
    let columns = table.columns();
    let breakable = |row: usize| {
        (0..columns).all(|c| {
            let (above, below) = (owners[row * columns + c], owners[(row + 1) * columns + c]);
            above.is_none() || above != below
        })
    };
    let body = header..rows - footer;
    let mut units = Vec::new();
    let mut start = body.start;
    for row in body.clone() {
        if row + 1 == body.end || breakable(row) {
            units.push(start..row + 1);
            start = row + 1;
        }
    }

    let mut shares = Vec::with_capacity(capacities.len());
    let mut next = 0;
    for capacity in capacities {
        let room = capacity - head - foot;
        let begin = units.get(next).map_or(body.end, |u| u.start);
        let mut end = begin;
        let mut used = 0.0;
        while let Some(unit) = units.get(next) {
            let height = sum(unit.clone());
            if end > begin && used + height > room + 1e-6 {
                break;
            }
            used += height;
            end = unit.end;
            next += 1;
        }
        shares.push(begin..end);
    }
    let left = units.get(next).map_or(0, |u| body.end - u.start);
    (shares, left)
}

/// The heading rows, the body rows `body`, and the footing rows, as a table
/// of their own.
fn part_table(table: &Table, body: std::ops::Range<usize>) -> Table {
    let rows = table.rows();
    let header = usize::from(table.header_rows).min(rows);
    let footer = usize::from(table.footer_rows).min(rows - header);
    let chosen: Vec<usize> = (0..header).chain(body).chain(rows - footer..rows).collect();
    let columns = table.columns();
    let mut part = table.clone();
    part.rows = chosen.iter().map(|&r| table.rows[r]).collect();
    part.cells = chosen
        .iter()
        .flat_map(|&r| (0..columns).map(move |c| r * columns + c))
        .map(|i| table.cells[i].clone())
        .collect();
    part.parts = Vec::new();
    part
}

/// Every rule the table draws, as straight runs.
///
/// Each grid line is read a unit at a time — a column's width of a row
/// boundary, a row's height of a column boundary — and a unit inside a
/// spanning cell draws nothing, so a merged cell is not crossed by the lines
/// of the cells it swallowed. Neighbouring units drawn with the same stroke
/// are joined into one run, so a dash pattern runs on unbroken across a row
/// and the PDF says one line where it means one.
fn rules_of(
    table: &Table,
    doc: &Document,
    column_edges: &[f64],
    row_edges: &[f64],
) -> Vec<LaidRule> {
    use tessera_document::table::Rule;
    let owners = table.owners();
    let (rows, columns) = (table.rows(), table.columns());
    let mut rules: Vec<LaidRule> = Vec::new();
    let mut push = |from: (f64, f64), to: (f64, f64), stroke: Option<&Stroke>| {
        let Some(stroke) = stroke.filter(|s| s.width > 0.0) else {
            return;
        };
        // Joined onto the run before when it carries straight on in the same
        // stroke.
        if let Some(last) = rules.last_mut()
            && last.to == from
            && last.stroke == *stroke
            && (last.from.0 == to.0 || last.from.1 == to.1)
        {
            last.to = to;
            return;
        }
        rules.push(LaidRule {
            from,
            to,
            stroke: stroke.clone(),
        });
    };
    for (row, &y) in row_edges.iter().enumerate().take(rows + 1) {
        for column in 0..columns {
            if let Rule::Drawn(stroke) = table.rule_across(&owners, row, column) {
                push(
                    (column_edges[column], y),
                    (column_edges[column + 1], y),
                    stroke,
                );
            }
        }
    }
    for (column, &x) in column_edges.iter().enumerate().take(columns + 1) {
        for row in 0..rows {
            if let Rule::Drawn(stroke) = table.rule_down(&owners, row, column) {
                push((x, row_edges[row]), (x, row_edges[row + 1]), stroke);
            }
        }
    }
    for rule in &mut rules {
        rule.stroke.color = doc.resolve_colour(&rule.stroke.color);
    }
    rules
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
        lay_out(table, doc, doc, &mut shaper, |id| {
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
    fn a_merged_cell_is_not_crossed_by_the_rules_of_the_cells_it_swallowed() {
        use tessera_color::Color;
        let (doc, mut table) = a_table(2, 3, "x");
        table.stroke = Some(Stroke::new(Color::BLACK, 0.5));
        let whole_grid = lay(&doc, &table);
        // Three runs across (top, middle, bottom) and four down, each joined
        // into one line from end to end.
        assert_eq!(whole_grid.rules.len(), 7, "{:?}", whole_grid.rules);

        table.merge(
            0,
            0,
            Span {
                columns: 3,
                rows: 1,
            },
        );
        let merged = lay(&doc, &table);
        let (x1, x2) = (merged.column_edges[1], merged.column_edges[2]);
        let y1 = merged.row_edges[1];
        for rule in &merged.rules {
            let vertical = rule.from.0 == rule.to.0;
            let inner_column = rule.from.0 == x1 || rule.from.0 == x2;
            if vertical && inner_column {
                assert!(
                    rule.from.1 >= y1 - 1e-9,
                    "an inner column rule crosses the merged top row: {rule:?}"
                );
            }
        }
    }

    #[test]
    fn a_cell_s_own_side_wins_over_the_table_rule_and_no_width_draws_nothing() {
        use tessera_color::Color;
        use tessera_document::table::Side;
        let (doc, mut table) = a_table(2, 2, "x");
        table.stroke = Some(Stroke::new(Color::BLACK, 0.5));
        // A heavy rule above the second row, under the first cell only.
        table.set_side(1, 0, Side::Top, Some(Stroke::new(Color::BLACK, 2.0)));
        // No rule between the two cells of the first row.
        table.set_side(0, 0, Side::Right, Some(Stroke::new(Color::BLACK, 0.0)));
        let laid = lay(&doc, &table);
        let y1 = laid.row_edges[1];
        let x1 = laid.column_edges[1];
        let heavy: Vec<_> = laid
            .rules
            .iter()
            .filter(|r| r.stroke.width == 2.0)
            .collect();
        assert_eq!(heavy.len(), 1, "{:?}", laid.rules);
        assert_eq!(heavy[0].from, (0.0, y1));
        assert_eq!(heavy[0].to, (x1, y1), "under the first cell only");
        assert!(
            !laid
                .rules
                .iter()
                .any(|r| r.from.0 == x1 && r.to.0 == x1 && r.from.1 < y1),
            "no rule between the first row's cells"
        );
    }

    #[test]
    fn rows_are_filled_in_turn_past_the_heading_and_a_cell_s_own_fill_wins() {
        use tessera_color::Color;
        use tessera_document::paint::Paint;
        use tessera_document::table::AlternatingFills;
        let (doc, mut table) = a_table(5, 1, "x");
        let grey = Paint::Solid(Color::Rgb {
            r: 0.9,
            g: 0.9,
            b: 0.9,
            a: 1.0,
        });
        let red = Paint::Solid(Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        table.alternating = Some(Box::new(AlternatingFills {
            skip_first: 1,
            ..AlternatingFills::every_other_row(grey.clone())
        }));
        if let Some(Slot::Cell(cell)) = table.at_mut(3, 0) {
            cell.fill = Some(red.clone());
        }
        let laid = lay(&doc, &table);
        let fill_of = |row: usize| {
            laid.cells
                .iter()
                .find(|c| c.row == row)
                .and_then(|c| c.fill.clone())
        };
        assert_eq!(fill_of(0), None, "the heading is skipped");
        assert_eq!(fill_of(1), Some(grey.clone()));
        assert_eq!(fill_of(2), None);
        assert_eq!(fill_of(3), Some(red), "the cell's own fill wins");
        assert_eq!(fill_of(4), None);
    }

    #[test]
    fn a_running_table_shares_its_body_by_height_keeps_spans_together_and_counts_the_rest() {
        let (doc, mut table) = a_table(10, 2, "x");
        // Every row twenty points, whatever the text needs.
        table.rows = vec![20.0; 10];
        table.header_rows = 1;
        let laid = lay(&doc, &table);
        assert!(
            laid.row_edges
                .windows(2)
                .all(|w| (w[1] - w[0] - 20.0).abs() < 1e-9)
        );

        // Seventy points a frame: the heading takes twenty, two body rows fit.
        let (shares, left) = split_rows(&table, &laid, &[70.0, 70.0, 70.0]);
        assert_eq!(shares, [1..3, 3..5, 5..7]);
        assert_eq!(left, 3, "rows 7, 8 and 9 have nowhere to go");

        // A cell over rows 3 and 4 keeps them together, even where the pair
        // is taller than the room: a frame always takes something.
        table.merge(
            3,
            0,
            Span {
                columns: 1,
                rows: 2,
            },
        );
        let laid = lay(&doc, &table);
        let (shares, _) = split_rows(&table, &laid, &[50.0, 50.0, 50.0, 50.0]);
        assert_eq!(shares, [1..2, 2..3, 3..5, 5..6]);
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
            alternating: None,
            style: None,
            local: tessera_document::table::TableLocal::default(),
            header_rows: 0,
            footer_rows: 0,
            parts: Vec::new(),
        };
        let laid = lay(&doc, &table);
        assert_eq!(laid.size(), (0.0, 0.0));
    }
}

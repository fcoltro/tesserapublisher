//! Tables made from text and data, turned back into text, and sorted.
//!
//! The arithmetic of each, kept apart from the commands that use it so it
//! can be tested as plain functions: which piece of a story goes in which
//! cell, what a table reads as when it is text again, and which order rows
//! sort into.

use std::cmp::Ordering;
use std::ops::Range;

use tessera_document::document::Document;
use tessera_document::table::{Slot, Table};
use tessera_text::Story;

/// The part of `story` in `range`, formatting and all.
///
/// Cut from a copy rather than rebuilt from the text, so every run keeps its
/// character format and every paragraph its style — a bold price stays bold
/// in its cell.
pub fn excerpt(story: &Story, range: Range<usize>) -> Story {
    let mut piece = story.clone();
    let end = piece.text.len();
    if range.end < end {
        piece.delete_range(range.end..end);
    }
    if range.start > 0 {
        piece.delete_range(0..range.start);
    }
    piece
}

/// `story` cut into rows and cells as InDesign's "Convert Text to Table"
/// cuts it: a paragraph is a row, a tab starts the next cell. Rows short of
/// the widest are padded with empty cells when the table is made.
pub fn cells_of(story: &Story) -> Vec<Vec<Story>> {
    let mut rows = Vec::new();
    for paragraph in story.paragraph_ranges() {
        // The paragraph without the break that ends it.
        let text = &story.text[paragraph.clone()];
        let end = paragraph.start + text.trim_end_matches('\n').len();
        let mut cells = Vec::new();
        let mut start = paragraph.start;
        for (offset, c) in story.text[paragraph.start..end].char_indices() {
            if c == '\t' {
                let at = paragraph.start + offset;
                cells.push(excerpt(story, start..at));
                start = at + 1;
            }
        }
        cells.push(excerpt(story, start..end));
        rows.push(cells);
    }
    // A story that ends with a paragraph break has an empty last paragraph,
    // which is not a row anybody meant.
    if rows
        .last()
        .is_some_and(|r| r.len() == 1 && r[0].text.is_empty())
        && rows.len() > 1
    {
        rows.pop();
    }
    rows
}

/// What `table` reads as, as text: its rows as paragraphs and its cells
/// separated by tabs, each row in the paragraph style of its first cell.
/// Returns the text, the style of each row, and whether any cell had
/// character formatting that plain text leaves behind — to be said out
/// loud rather than dropped silently.
pub fn text_of(
    doc: &Document,
    table: &Table,
) -> (
    String,
    Vec<Option<tessera_text::story::ParagraphStyleId>>,
    bool,
) {
    let mut text = String::new();
    let mut styles = Vec::new();
    let mut formatted = false;
    for row in 0..table.rows() {
        if row > 0 {
            text.push('\n');
        }
        let mut first = true;
        let mut style = None;
        for column in 0..table.columns() {
            let Some(Slot::Cell(cell)) = table.at(row, column) else {
                continue;
            };
            if !first {
                text.push('\t');
            }
            let story = doc.story(cell.story);
            let words = story.map_or("", |s| s.text.as_str());
            // A paragraph break inside a cell would start a new row.
            text.push_str(&words.replace('\n', " "));
            if let Some(story) = story {
                if first {
                    style = story.common_paragraph_style(0..story.text.len()).0;
                }
                formatted |= story.has_character_overrides(0..story.text.len());
            }
            first = false;
        }
        styles.push(style);
    }
    (text, styles, formatted)
}

/// The order rows sort into by `keys`, one per row: numbers as numbers,
/// words ignoring case, and a stable sort so rows that tie keep their order.
pub fn sort_order(keys: &[String], descending: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by(|&a, &b| {
        let ordering = compare(&keys[a], &keys[b]);
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    order
}

/// Two cells' words compared as a person would: "9" before "10", "£4.50"
/// before "£12", "apple" beside "Apple", and an empty cell last.
fn compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim(), b.trim());
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// A cell's words as a number, if that is what they are: a currency sign or
/// a percent is allowed around it, thousands separated by commas, and a
/// lone decimal comma read as a point.
fn number(words: &str) -> Option<f64> {
    let digits =
        words.trim_matches(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.' || c == ','));
    if digits.is_empty() || !digits.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let plain = match (digits.matches(',').count(), digits.contains('.')) {
        // "2,50": the comma is the decimal mark.
        (1, false) if digits.split(',').nth(1).is_some_and(|d| d.len() != 3) => {
            digits.replace(',', ".")
        }
        // "1,250" or "1,250.50": commas separate thousands.
        _ => digits.replace(',', ""),
    };
    plain.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rows: &[Vec<Story>]) -> Vec<Vec<&str>> {
        rows.iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn paragraphs_are_rows_and_tabs_are_cells() {
        let story = Story::new("Tea\t2.50\nCoffee\t3.00\tlarge\n");
        let rows = cells_of(&story);
        assert_eq!(
            texts(&rows),
            [vec!["Tea", "2.50"], vec!["Coffee", "3.00", "large"]],
            "the break at the very end is not a row"
        );
    }

    #[test]
    fn a_cell_keeps_its_formatting() {
        use tessera_text::story::CharacterFormat;
        let mut story = Story::new("Tea\t2.50");
        story.apply_character_format(
            4..8,
            &CharacterFormat {
                weight: Some(700),
                ..CharacterFormat::default()
            },
        );
        let rows = cells_of(&story);
        assert!(!rows[0][0].has_character_overrides(0..3), "Tea is plain");
        assert!(
            rows[0][1].has_character_overrides(0..4),
            "the price stays bold"
        );
    }

    use crate::app::TesseraApp;
    use crate::command::{Command, apply};
    use tessera_document::ids::FrameId;
    use tessera_document::nodes::FrameKind;
    use tessera_geometry::DocRect;

    fn table_of(state: &TesseraApp, id: FrameId) -> Table {
        match state.active().document().frame(id).map(|f| f.kind.clone()) {
            Some(FrameKind::Table(table)) => table,
            other => panic!("not a table: {other:?}"),
        }
    }

    fn words(state: &TesseraApp, table: &Table) -> Vec<Vec<String>> {
        (0..table.rows())
            .map(|r| {
                (0..table.columns())
                    .filter_map(|c| table.at(r, c).and_then(|s| s.cell()))
                    .map(|cell| {
                        state
                            .active()
                            .document()
                            .story(cell.story)
                            .map(|s| s.text.clone())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect()
    }

    fn a_text_frame(state: &mut TesseraApp, text: &str) -> FrameId {
        let b = state.first_page_bounds();
        apply(
            state,
            Command::AddTextFrame(DocRect {
                x: b.x + 40.0,
                y: b.y + 40.0,
                width: 300.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("the frame");
        apply(
            state,
            Command::SetText {
                id,
                text: text.to_owned(),
            },
        );
        id
    }

    #[test]
    fn text_becomes_a_table_in_its_place_and_back_and_each_is_one_undo() {
        let mut state = TesseraApp::headless();
        let text = a_text_frame(&mut state, "Item\tPrice\nTea\t2.50\nCoffee");
        let frames_before = state.active().document().frames.len();

        apply(&mut state, Command::ConvertTextToTable { id: text });
        let table_id = state.active().selection.single().expect("the table");
        assert!(
            state.active().document().frame(text).is_none(),
            "the text frame went"
        );
        assert_eq!(state.active().document().frames.len(), frames_before);
        let table = table_of(&state, table_id);
        assert_eq!(
            words(&state, &table),
            [
                vec!["Item", "Price"],
                vec!["Tea", "2.50"],
                vec!["Coffee", ""]
            ],
            "a short row is padded"
        );

        apply(&mut state, Command::ConvertTableToText { id: table_id });
        let back = state.active().selection.single().expect("the text frame");
        let Some(FrameKind::Text { story, .. }) = state
            .active()
            .document()
            .frame(back)
            .map(|f| f.kind.clone())
        else {
            panic!("a text frame");
        };
        assert_eq!(
            state
                .active()
                .document()
                .story(story)
                .map(|s| s.text.as_str()),
            Some("Item\tPrice\nTea\t2.50\nCoffee\t")
        );

        apply(&mut state, Command::Undo);
        apply(&mut state, Command::Undo);
        assert!(
            state.active().document().frame(text).is_some(),
            "two undos bring the text frame back"
        );
    }

    #[test]
    fn rows_sort_below_the_heading_and_a_row_spanning_cell_refuses() {
        let mut state = TesseraApp::headless();
        let b = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddTableFromData {
                bounds: DocRect {
                    x: b.x + 40.0,
                    y: b.y + 40.0,
                    width: 300.0,
                    height: 60.0,
                },
                cells: [
                    ["Item", "Price"],
                    ["Tea", "10"],
                    ["Cake", "9"],
                    ["Bun", "2,50"],
                ]
                .map(|r| r.map(String::from).to_vec())
                .to_vec(),
            },
        );
        let id = state.active().selection.single().expect("the table");
        apply(
            &mut state,
            Command::SortTableRows {
                id,
                column: 1,
                descending: false,
                skip: 1,
            },
        );
        let table = table_of(&state, id);
        let firsts: Vec<String> = words(&state, &table)
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
        assert_eq!(
            firsts,
            ["Item", "Bun", "Cake", "Tea"],
            "by price, as numbers"
        );

        let mut spanning = table.clone();
        spanning.merge(
            1,
            0,
            tessera_document::table::Span {
                columns: 1,
                rows: 2,
            },
        );
        assert!(
            !spanning.reorder_rows(1, &[2, 1, 0]),
            "a cell over two rows"
        );
    }

    #[test]
    fn a_data_file_is_placed_as_a_table_and_what_was_changed_is_said() {
        let mut state = TesseraApp::headless();
        let path = std::env::temp_dir().join(format!("tessera-data-{}.csv", std::process::id()));
        std::fs::write(&path, "Name;@Photo\nAna;ana.jpg\n\nBo;bo.jpg;extra\n").expect("write");
        crate::file_ops::place_data_file_as_table(&mut state, &path);
        let id = state.active().selection.single().expect("the table");
        let table = table_of(&state, id);
        assert_eq!(
            words(&state, &table),
            [
                vec!["Name", "@Photo", "Column 3"],
                vec!["Ana", "ana.jpg", ""],
                vec!["Bo", "bo.jpg", "extra"],
            ]
        );
        let said = state
            .status
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_default();
        assert!(said.contains("2 rows of 3"), "{said}");
        assert!(said.contains("blank row"), "{said}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_long_table_flows_onto_new_pages_with_its_heading_on_each_and_undoes_as_one() {
        use tessera_layout::resolve::ResolvedKind;
        let mut state = TesseraApp::headless();
        let pages_before = state.active().document().page_ids().count();
        let b = state.first_page_bounds();
        let mut cells = vec![vec!["Item".to_owned(), "Price".to_owned()]];
        cells.extend((1..=60).map(|n| vec![format!("Thing {n}"), format!("{n}.00")]));
        apply(
            &mut state,
            Command::AddTableFromData {
                bounds: DocRect {
                    x: b.x + 36.0,
                    y: b.y + 36.0,
                    width: 300.0,
                    height: 200.0,
                },
                cells,
            },
        );
        let id = state.active().selection.single().expect("the table");
        apply(
            &mut state,
            Command::SetTableRegions {
                id,
                header: 1,
                footer: 0,
            },
        );
        apply(&mut state, Command::FlowTable { id });

        let table = table_of(&state, id);
        assert!(table.parts.len() >= 2, "{} parts", table.parts.len());
        assert_eq!(
            state.active().document().page_ids().count(),
            pages_before + table.parts.len(),
            "a page for each part"
        );

        // Every row is somewhere, the heading at the top of each frame.
        let first_row = |kind: &ResolvedKind| -> Vec<u32> {
            let ResolvedKind::Table { laid, .. } = kind else {
                panic!("a table");
            };
            let cell = laid
                .cells
                .iter()
                .find(|c| c.row == 0 && c.column == 0)
                .expect("a first row");
            cell.shaped
                .lines
                .iter()
                .flat_map(|l| l.glyphs().map(|g| g.glyph_id))
                .collect()
        };
        let resolved = state.resolve_active().clone();
        let head = resolved.items.iter().find(|i| i.frame == id).expect("head");
        let heading = first_row(&head.kind);
        let mut shown = 0;
        for part in &table.parts {
            let item = resolved
                .items
                .iter()
                .find(|i| i.frame == *part)
                .expect("each part is laid out");
            assert_eq!(first_row(&item.kind), heading, "the heading repeats");
            let ResolvedKind::Table { laid, .. } = &item.kind else {
                unreachable!();
            };
            shown += laid.row_edges.len() - 2; // less the heading
            if Some(part) == table.parts.last() {
                assert_eq!(laid.overset_rows, 0, "nothing left over");
            }
        }
        let ResolvedKind::Table { laid, .. } = &head.kind else {
            unreachable!();
        };
        shown += laid.row_edges.len() - 2;
        assert_eq!(shown, 60, "each of the sixty rows exactly once");

        // One undo takes the whole flow back.
        apply(&mut state, Command::Undo);
        assert!(table_of(&state, id).parts.is_empty());
        assert_eq!(state.active().document().page_ids().count(), pages_before);
    }

    #[test]
    fn numbers_sort_as_numbers_and_words_ignore_case() {
        let keys: Vec<String> = ["10", "9", "£4.50", "", "2,50", "1,250"]
            .map(String::from)
            .to_vec();
        let order = sort_order(&keys, false);
        let sorted: Vec<&str> = order.iter().map(|&i| keys[i].as_str()).collect();
        assert_eq!(sorted, ["2,50", "£4.50", "9", "10", "1,250", ""]);

        let words: Vec<String> = ["pear", "Apple", "banana", "apple"]
            .map(String::from)
            .to_vec();
        let order = sort_order(&words, false);
        let sorted: Vec<&str> = order.iter().map(|&i| words[i].as_str()).collect();
        assert_eq!(
            sorted,
            ["Apple", "apple", "banana", "pear"],
            "a tie keeps its order"
        );
        let down = sort_order(&words, true);
        assert_eq!(words[down[0]], "pear");
    }
}

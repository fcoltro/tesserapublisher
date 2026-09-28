//! Making the merged pages: one copy of the template's pages per record.
//!
//! **One document, pages duplicated in it**, rather than a document per
//! record joined afterwards. A duplicated page owns deep copies of its text
//! (`Document::duplicate_page`), shares the template's styles, swatches and
//! parents by the same ids, and says which copy came from which frame — so a
//! record's picture frame is found from the template's, and nothing has to
//! be matched up across documents whose ids collide. The same merged
//! document is opened as a new document or laid out and written as one PDF.
//!
//! What is filled, in each record's pages: every field marker becomes the
//! record's words in the marker's own formatting, a line holding only fields
//! that are all empty goes (InDesign's "remove blank lines for empty
//! fields"), and a picture field's frame gets the picture its path names.
//! What cannot be merged is said, not skipped: a field left in a parent
//! page or in text threaded from another page still reads as its name, and
//! the report counts them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, PageId, StoryId};
use tessera_document::merge::values_for;
use tessera_document::nodes::FrameKind;
use tessera_import::delimited::Data;
use tessera_text::Story;
use tessera_text::variables::Marker;

/// The merged document, and what a person needs told about it.
pub struct Merged {
    pub document: Document,
    pub records: usize,
    /// How many pages the merged document has.
    pub pages: usize,
    /// Which record, from zero, each merged frame shows: how an overset
    /// frame is traced to its record when a page holds several.
    pub frame_records: HashMap<FrameId, usize>,
    /// What could not be done, record by record, in words.
    pub notes: Vec<String>,
}

/// Several records to a page: one record's objects repeated across and down
/// the page, `across` and `down` points apart, then onto a new page —
/// labels, badges, a catalogue grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub across: f64,
    pub down: f64,
}

/// For each record, the copy of each template frame it shows.
type Copies = Vec<HashMap<FrameId, FrameId>>;

/// A copy of the template's pages per record, one after another.
fn copies_by_page(doc: &mut Document, records: usize) -> Result<Copies, String> {
    let pages: Vec<PageId> = doc.page_ids().collect();
    // Record one fills the template's own pages; each after it a copy of
    // them, moved to the end so the records run in order.
    let originals: Vec<FrameId> = pages
        .iter()
        .flat_map(|p| doc.frames_on_page(*p))
        .flat_map(|f| doc.descendants(f))
        .collect();
    let mut copies: Copies = vec![originals.iter().map(|f| (*f, *f)).collect()];
    for _ in 1..records {
        let mut map = HashMap::new();
        for page in &pages {
            let (copy, pairs) = doc
                .duplicate_page_mapped(*page)
                .ok_or("A page of the template could not be copied.")?;
            let last = doc.page_ids().count() - 1;
            doc.move_page(copy, last);
            map.extend(pairs);
        }
        copies.push(map);
    }
    Ok(copies)
}

/// A copy of the objects on the template's first page per record, set out
/// in rows across the page from where the template has them and onto new
/// pages as they fill, inside the page's right and bottom margins.
fn copies_in_grid(
    doc: &mut Document,
    records: usize,
    grid: Grid,
    notes: &mut Vec<String>,
) -> Result<Copies, String> {
    let pages: Vec<PageId> = doc.page_ids().collect();
    let first = *pages.first().ok_or("The template has no page.")?;
    if pages.len() > 1 {
        notes.push(format!(
            "Several records to a page merges the first page only; the other {} \
             w{} left out.",
            pages.len() - 1,
            if pages.len() == 2 { "as" } else { "ere" }
        ));
        for page in pages.iter().skip(1).rev() {
            for frame in doc.frames_on_page(*page) {
                doc.remove_frame(frame);
            }
            doc.remove_page(*page);
        }
    }
    let roots = doc.frames_on_page(first);
    // One record's box: everything on the page, as it is seen.
    let bounds: Vec<tessera_geometry::DocRect> =
        roots.iter().filter_map(|f| doc.visual_bounds(*f)).collect();
    let Some(record) = bounds.iter().copied().reduce(|a, b| {
        let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
        let (x1, y1) = (
            (a.x + a.width).max(b.x + b.width),
            (a.y + a.height).max(b.y + b.height),
        );
        tessera_geometry::DocRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }) else {
        return Err("The template's first page has nothing on it to repeat.".into());
    };
    let page = doc.pages[first].bounds;
    let margins = doc.setup.margins;
    let (step_x, step_y) = (record.width + grid.across, record.height + grid.down);
    let room_x = page.x + page.width - margins.outside - record.x;
    let room_y = page.y + page.height - margins.bottom - record.y;
    let fits = |room: f64, size: f64, step: f64| {
        if step <= 0.0 || room < size {
            1
        } else {
            (((room - size) / step).floor() as usize + 1).max(1)
        }
    };
    let across = fits(room_x, record.width, step_x);
    let down = fits(room_y, record.height, step_y);
    let per_page = across * down;

    let originals: Vec<FrameId> = roots.iter().flat_map(|f| doc.descendants(*f)).collect();
    let mut copies: Copies = vec![originals.iter().map(|f| (*f, *f)).collect()];
    let mut record_pages = vec![first];
    for k in 1..records {
        let (sheet, slot) = (k / per_page, k % per_page);
        while record_pages.len() <= sheet {
            record_pages.push(doc.add_page());
        }
        let on = doc.pages[record_pages[sheet]].bounds;
        let dx = (on.x - page.x) + (slot % across) as f64 * step_x;
        let dy = (on.y - page.y) + (slot / across) as f64 * step_y;
        copies.push(doc.copy_frames_mapped(&roots, dx, dy).into_iter().collect());
    }
    Ok(copies)
}

/// `story` with its field markers replaced by `values`, each value in its
/// marker's own formatting. With `remove_blank`, a paragraph holding only
/// fields — and space — whose values are all empty is taken out whole, so an
/// address with no second line does not print a gap.
pub fn fill_story(story: &Story, values: &[String], remove_blank: bool) -> Story {
    let mut out = story.clone();
    let value = |n: u8| values.get(usize::from(n)).map_or("", String::as_str);
    // One paragraph at a time, the ranges read afresh after each: taking the
    // last paragraph takes the break before it too, which shortens the one
    // before, so ranges read once would run past the end.
    while remove_blank && let Some(range) = blank_paragraph(&out, values) {
        // The paragraph and the break that ends it; the last one has none,
        // so it takes the break before it instead.
        let cut = if out.text[range.clone()].ends_with('\n') || range.start == 0 {
            range
        } else {
            range.start - 1..range.end
        };
        out.delete_range(cut);
    }
    let markers: Vec<(usize, usize, u8)> = out
        .text
        .char_indices()
        .filter_map(|(at, c)| match Marker::of(c) {
            Some(Marker::Field(n)) => Some((at, c.len_utf8(), n)),
            _ => None,
        })
        .collect();
    for (at, len, n) in markers.into_iter().rev() {
        // Inserted just after the marker, so the words take the marker's
        // formatting rather than the character before it; then the marker
        // goes.
        out.insert_text(at + len, value(n));
        out.delete_range(at..at + len);
    }
    out
}

/// The last paragraph of `story` holding only fields — and space — whose
/// values are all empty.
fn blank_paragraph(story: &Story, values: &[String]) -> Option<std::ops::Range<usize>> {
    let value = |n: u8| values.get(usize::from(n)).map_or("", String::as_str);
    story.paragraph_ranges().into_iter().rev().find(|range| {
        let mut fields = false;
        let mut empty = true;
        let mut words = false;
        for c in story.text[range.clone()].chars() {
            match Marker::of(c) {
                Some(Marker::Field(n)) => {
                    fields = true;
                    empty &= value(n).trim().is_empty();
                }
                _ if c.is_whitespace() => {}
                _ => words = true,
            }
        }
        fields && empty && !words
    })
}

/// Every story a frame shows: a text frame's, and each of a table's cells.
fn stories_of(doc: &Document, frame: FrameId) -> Vec<StoryId> {
    match doc.frame(frame).map(|f| &f.kind) {
        Some(FrameKind::Text { story, .. }) => vec![*story],
        Some(FrameKind::Table(table)) => table.stories().collect(),
        _ => Vec::new(),
    }
}

fn has_fields(story: &Story) -> bool {
    story
        .text
        .chars()
        .any(|c| matches!(Marker::of(c), Some(Marker::Field(_))))
}

/// The template merged with every record of `data`. Relative picture paths
/// are read from `data_dir`, the data file's own folder, where a spreadsheet
/// of photographs keeps them.
pub fn merge(
    template: &Document,
    data: &Data,
    data_dir: Option<&Path>,
    remove_blank: bool,
    grid: Option<Grid>,
) -> Result<Merged, String> {
    let source = template
        .data_merge
        .clone()
        .ok_or("The document names no data file to merge.")?;
    if data.records.is_empty() {
        return Err("The data file has no records to merge.".into());
    }
    let names: Vec<String> = data.fields.iter().map(|f| f.name.clone()).collect();

    let mut doc = template.clone();
    doc.set_data_source(None);
    doc.set_merge_record(None);
    let mut notes = Vec::new();
    let records = match grid {
        Some(grid) => copies_in_grid(&mut doc, data.records.len(), grid, &mut notes)?,
        None => copies_by_page(&mut doc, data.records.len())?,
    };
    for (k, (record, frames)) in data.records.iter().zip(&records).enumerate() {
        let values = values_for(&source, &names, record);
        // A story shown by two frames of one page is filled once.
        let mut filled: HashSet<StoryId> = HashSet::new();
        for frame in frames.values() {
            for story in stories_of(&doc, *frame) {
                if !filled.insert(story) {
                    continue;
                }
                if let Some(text) = doc.stories.get_mut(story)
                    && has_fields(text)
                {
                    *text = fill_story(text, &values, remove_blank);
                }
            }
        }
        for picture in &source.pictures {
            let Some(frame) = frames.get(&picture.frame) else {
                continue;
            };
            let words = values
                .get(usize::from(picture.field))
                .map_or("", |v| v.trim());
            if words.is_empty() {
                continue;
            }
            let path = match data_dir {
                Some(dir) if Path::new(words).is_relative() => dir.join(words),
                _ => Path::new(words).to_path_buf(),
            };
            if !path.is_file() {
                notes.push(format!(
                    "Record {}: no picture at {}.",
                    k + 1,
                    path.display()
                ));
                continue;
            }
            match crate::command::measured(&path) {
                Ok(link) => {
                    let link = doc.add_link(link);
                    doc.place(*frame, link, tessera_document::graphic::Fit::Proportionally);
                }
                Err(error) => notes.push(format!("Record {}: {error}", k + 1)),
            }
        }
    }

    let left = doc.stories.values().filter(|s| has_fields(s)).count();
    if left > 0 {
        notes.push(format!(
            "{left} stor{} still hold fields and print their names: fields on a parent \
             page, or in text threaded in from another page, are not merged.",
            if left == 1 { "y" } else { "ies" }
        ));
    }
    doc.touch();
    let frame_records = records
        .iter()
        .enumerate()
        .flat_map(|(k, frames)| frames.values().map(move |f| (*f, k)))
        .collect();
    Ok(Merged {
        pages: doc.page_ids().count(),
        document: doc,
        records: data.records.len(),
        frame_records,
        notes,
    })
}

/// The records whose text came out overset in `resolved`, a layout of the
/// merged document: numbered from one, as a person counts them.
pub fn overset_records(
    merged: &Merged,
    resolved: &tessera_layout::resolve::ResolvedDocument,
) -> Vec<usize> {
    use tessera_layout::resolve::ResolvedKind;
    let mut records: Vec<usize> = resolved
        .items
        .iter()
        .filter(|item| match &item.kind {
            ResolvedKind::Text { overset_lines, .. } => *overset_lines > 0,
            ResolvedKind::Table { laid, .. } => {
                laid.overset_rows > 0 || laid.cells.iter().any(|c| c.overset_lines > 0)
            }
            _ => false,
        })
        .filter_map(|item| merged.frame_records.get(&item.frame))
        .map(|k| k + 1)
        .collect();
    records.sort_unstable();
    records.dedup();
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::merge::{DataSource, MergeField};
    use tessera_geometry::DocRect;

    fn field(n: u8) -> String {
        Marker::Field(n).character().to_string()
    }

    fn values(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn fields_become_the_record_s_words_in_the_marker_s_formatting() {
        use tessera_text::story::CharacterFormat;
        let mut story = Story::new(format!("Dear {},", field(0)));
        // The marker alone is bold.
        story.apply_character_format(
            5..5 + field(0).len(),
            &CharacterFormat {
                weight: Some(700),
                ..CharacterFormat::default()
            },
        );
        let filled = fill_story(&story, &values(&["Ana"]), true);
        assert_eq!(filled.text, "Dear Ana,");
        assert!(filled.has_character_overrides(5..8), "the name is bold");
        assert!(!filled.has_character_overrides(0..5), "\"Dear \" is not");
    }

    #[test]
    fn a_line_of_only_empty_fields_goes_and_one_with_words_stays() {
        let text = format!("{}\n{}\n{}, {}", field(0), field(1), field(2), field(3));
        let story = Story::new(text);
        let filled = fill_story(&story, &values(&["Ana Lima", "", "Lisbon", ""]), true);
        assert_eq!(
            filled.text, "Ana Lima\nLisbon, ",
            "no gap where line two was"
        );
        let kept = fill_story(&story, &values(&["Ana Lima", "", "Lisbon", ""]), false);
        assert_eq!(kept.text, "Ana Lima\n\nLisbon, ");
        // The last line empty takes the break before it.
        let tail = Story::new(format!("Name\n{}", field(0)));
        assert_eq!(fill_story(&tail, &values(&[""]), true).text, "Name");
    }

    #[test]
    fn several_records_to_a_page_repeat_across_then_down_then_onto_a_new_page() {
        use crate::command::{Command, apply};
        let mut state = crate::app::TesseraApp::headless();
        let page = state.first_page_bounds();
        let margins = state.active().document().setup.margins;
        // One badge in the page's top left, inside the margins.
        let badge = DocRect {
            x: page.x + margins.inside,
            y: page.y + margins.top,
            width: 150.0,
            height: 60.0,
        };
        apply(&mut state, Command::AddTextFrame(badge));
        let id = state.active().selection.single().expect("the frame");
        apply(&mut state, Command::SetText { id, text: field(0) });
        apply(
            &mut state,
            Command::SetDataSource(Some(DataSource {
                path: "people.csv".into(),
                fields: vec![MergeField {
                    name: "Name".into(),
                    image: false,
                }],
                pictures: Vec::new(),
            })),
        );
        let doc = state.active().document().clone();
        let grid = Grid {
            across: 12.0,
            down: 12.0,
        };
        let names = |n: usize| {
            let mut csv = "Name\n".to_owned();
            for i in 1..=n {
                csv.push_str(&format!("Person {i}\n"));
            }
            tessera_import::delimited::read(csv.as_bytes()).expect("data")
        };

        let merged = merge(&doc, &names(10), None, true, Some(grid)).expect("merged");
        assert_eq!(merged.pages, 1, "ten badges fit one page");
        let frames: Vec<(tessera_geometry::DocRect, String)> = merged
            .document
            .frames_on_page(merged.document.page_ids().next().expect("a page"))
            .into_iter()
            .filter_map(|f| {
                let frame = merged.document.frame(f)?;
                let FrameKind::Text { story, .. } = &frame.kind else {
                    return None;
                };
                Some((frame.bounds, merged.document.story(*story)?.text.clone()))
            })
            .collect();
        assert_eq!(frames.len(), 10);
        // The second record sits one badge and a gap to the right.
        let second = frames
            .iter()
            .find(|(_, words)| words == "Person 2")
            .expect("record two");
        assert!((second.0.x - (badge.x + 150.0 + 12.0)).abs() < 1e-6);
        assert!((second.0.y - badge.y).abs() < 1e-6, "on the same row");
        // Every record once, in its own place.
        let mut places: Vec<(i64, i64)> = frames
            .iter()
            .map(|(b, _)| (b.x.round() as i64, b.y.round() as i64))
            .collect();
        places.sort_unstable();
        places.dedup();
        assert_eq!(places.len(), 10);
        assert_eq!(merged.frame_records.len(), 10);

        let many = merge(&doc, &names(40), None, true, Some(grid)).expect("merged");
        assert_eq!(many.pages, 2, "forty badges need a second page");
    }

    #[test]
    fn every_record_gets_its_own_pages_in_order() {
        use crate::command::{Command, apply};
        let mut state = crate::app::TesseraApp::headless();
        let bounds = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: bounds.x + 20.0,
                y: bounds.y + 20.0,
                width: 200.0,
                height: 40.0,
            }),
        );
        let id = state.active().selection.single().expect("the frame");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: format!("Hello {}", field(0)),
            },
        );
        apply(
            &mut state,
            Command::SetDataSource(Some(DataSource {
                path: "people.csv".into(),
                fields: vec![MergeField {
                    name: "Name".into(),
                    image: false,
                }],
                pictures: Vec::new(),
            })),
        );
        let doc = state.active().document().clone();
        let data = tessera_import::delimited::read(b"Name\nAna\nBo\nCy\n").expect("data");

        let merged = merge(&doc, &data, None, true, None).expect("merged");
        assert_eq!(merged.records, 3);
        assert_eq!(merged.document.page_ids().count(), 3, "a page a record");
        let texts: Vec<String> = merged
            .document
            .page_ids()
            .flat_map(|p| merged.document.frames_on_page(p))
            .filter_map(|f| match merged.document.frame(f).map(|f| &f.kind) {
                Some(FrameKind::Text { story, .. }) => {
                    merged.document.story(*story).map(|s| s.text.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["Hello Ana", "Hello Bo", "Hello Cy"]);
        assert!(
            merged.document.data_merge.is_none(),
            "the result is not a template"
        );
        assert!(merged.notes.is_empty(), "{:?}", merged.notes);
        assert!(
            doc.stories.values().any(has_fields),
            "the template itself is untouched"
        );
    }
}

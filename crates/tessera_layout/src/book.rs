//! What a book means for its documents: pages numbered on from one to the
//! next, one contents listing every chapter's headings, and one PDF.
//!
//! Nothing here knows about files. The caller loads the documents the book
//! lists and hands them over; what comes back is the documents changed, a
//! story, or a resolved document the PDF writer can take as it takes any
//! other.

use tessera_document::contents::Level;
use tessera_document::document::Document;
use tessera_document::sections::Section;
use tessera_geometry::{DocRect, Transform};
use tessera_text::story::{Hyperlink, ParagraphStyleId};

use crate::contents::{Generated, Paragraph, assemble, headings};
use crate::resolve::{LinkTarget, ResolvedDocument};

/// Number each document's pages on from the one before: the second
/// document's first page takes the number after the first's last, and so
/// on. A document already beginning a section keeps the section and takes
/// the number; one that does not gains a section on its first page. The
/// first document is left as it is — it is where the count starts.
///
/// Returns which documents changed, so the caller saves only those.
pub fn continue_numbering(documents: &mut [Document]) -> Vec<bool> {
    let mut changed = vec![false; documents.len()];
    let mut next: Option<u32> = None;
    for (index, doc) in documents.iter_mut().enumerate() {
        let first = doc.page_ids().next();
        if let Some(start) = next
            && let Some(first) = first
        {
            let mut sections = doc.sections.clone();
            match sections.iter_mut().find(|s| s.first == first) {
                Some(section) => {
                    if section.start != Some(start) {
                        section.start = Some(start);
                        changed[index] = true;
                    }
                }
                None => {
                    let mut section = Section::starting_at(first);
                    section.start = Some(start);
                    sections.insert(0, section);
                    changed[index] = true;
                }
            }
            if changed[index] {
                doc.set_sections(sections);
            }
        }
        // Where the next document picks up: after this one's last page.
        next = doc
            .page_numbers()
            .last()
            .map(|(_, n)| n.ordinal.saturating_add(1))
            .or(next);
    }
    changed
}

/// The air between one document's pages and the next's in the combined
/// geometry — enough that nothing on the last page of one reaches the
/// first of the next.
const GAP: f64 = 200.0;

/// Every document's pages and items as one resolved document, each
/// document set below the one before, so the PDF writer — which puts an
/// item on the page its geometry says — writes one file with every page
/// in order. An item's own coordinates are untouched: the move rides on
/// its transform, which is what places it on the page.
pub fn combine(parts: Vec<ResolvedDocument>) -> ResolvedDocument {
    let mut out = ResolvedDocument::default();
    let mut dy = 0.0;
    for part in parts {
        let pages_before = out.pages.len();
        let top = part
            .pages
            .iter()
            .map(|p| p.slug.y.min(p.bleed.y).min(p.bounds.y))
            .fold(f64::INFINITY, f64::min);
        let bottom = part
            .pages
            .iter()
            .map(|p| {
                (p.slug.y + p.slug.height)
                    .max(p.bleed.y + p.bleed.height)
                    .max(p.bounds.y + p.bounds.height)
            })
            .fold(f64::NEG_INFINITY, f64::max);
        if part.pages.is_empty() {
            continue;
        }
        let shift = dy - top;
        let moved = |r: DocRect| DocRect {
            y: r.y + shift,
            ..r
        };
        for page in part.pages {
            out.pages.push(crate::resolve::ResolvedPage {
                bounds: moved(page.bounds),
                margins: moved(page.margins),
                bleed: moved(page.bleed),
                slug: moved(page.slug),
                columns: page.columns.into_iter().map(moved).collect(),
            });
        }
        for mut item in part.items {
            item.transform = Transform::from_affine(
                kurbo::Affine::translate((0.0, shift)) * item.transform.to_affine(),
            );
            item.spread_area = item.spread_area.map(moved);
            for link in &mut item.links {
                link.rects = link.rects.iter().map(|r| moved(*r)).collect();
                if let LinkTarget::Page(index) = &mut link.target {
                    *index += pages_before;
                }
            }
            out.items.push(item);
        }
        for mut bookmark in part.bookmarks {
            bookmark.page += pages_before;
            out.bookmarks.push(bookmark);
        }
        dy += bottom - top + GAP;
    }
    out
}

/// The contents of the whole book, from the recipe of the document it is
/// placed in: every document's headings in the styles the recipe names,
/// matched across documents by name, each with the page label its own
/// document gives it. Entries in the placing document link to their
/// headings; entries elsewhere cannot, and do not.
///
/// `entries` is every document in the book with its resolve, in order;
/// `placed_in` says which of them holds the contents.
pub fn table_of_contents(
    entries: &[(&Document, &ResolvedDocument)],
    placed_in: usize,
    title: &str,
    title_style: Option<ParagraphStyleId>,
    levels: &[Level],
    measure: f32,
) -> Generated {
    let Some((home, _)) = entries.get(placed_in) else {
        return Generated {
            story: assemble(Vec::new(), measure),
            destinations: Vec::new(),
        };
    };
    let name_of = |doc: &Document, id: ParagraphStyleId| -> Option<String> {
        doc.paragraph_styles.get(id).map(|s| s.name.clone())
    };
    let level_names: Vec<Option<String>> = levels.iter().map(|l| name_of(home, l.style)).collect();

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    if !title.is_empty() {
        paragraphs.push((title.to_owned(), title_style, false, None, 0));
    }
    let mut destinations = Vec::new();
    let mut n = 0usize;
    for (index, (doc, resolved)) in entries.iter().enumerate() {
        // This document's ids for the recipe's styles, by name; a level
        // whose style this document has not got lists nothing from it.
        let here: Vec<(usize, ParagraphStyleId)> = level_names
            .iter()
            .enumerate()
            .filter_map(|(level, name)| {
                let name = name.as_ref()?;
                doc.paragraph_styles
                    .iter()
                    .find(|(_, s)| &s.name == name)
                    .map(|(id, _)| (level, id))
            })
            .collect();
        let styles: Vec<ParagraphStyleId> = here.iter().map(|(_, id)| *id).collect();
        for heading in headings(doc, resolved, &styles) {
            let label = doc.page_label(heading.page).unwrap_or_default();
            let level = here
                .iter()
                .find(|(_, id)| *id == heading.style)
                .map(|(level, _)| *level);
            let entry_style = level
                .and_then(|l| levels.get(l))
                .and_then(|l| l.entry_style);
            n += 1;
            let link = if index == placed_in {
                let name = format!("Contents {n}: {}", heading.text);
                destinations.push((name.clone(), heading.page));
                Some(Hyperlink::Destination(name))
            } else {
                None
            };
            paragraphs.push((
                format!("{}\t{label}", heading.text),
                entry_style,
                true,
                link,
                0,
            ));
        }
    }
    Generated {
        story: assemble(paragraphs, measure),
        destinations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::{Frame, FrameKind};
    use tessera_text::Shaper;
    use tessera_text::story::{ParagraphStyle, Story};

    fn chapter(pages: usize, heading: &str) -> Document {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        for _ in 1..pages {
            doc.add_page();
        }
        let style = doc.add_paragraph_style(ParagraphStyle {
            name: "Chapter".into(),
            based_on: None,
            format: Default::default(),
        });
        let mut story = Story::new(heading);
        story.paragraphs[0].style = Some(style);
        let story = doc.add_story(story);
        let first = doc.page_ids().next().unwrap();
        let bounds = doc.pages[first].bounds;
        let layer = doc.default_layer().unwrap();
        doc.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: bounds.x + 20.0,
                    y: bounds.y + 20.0,
                    width: 300.0,
                    height: 100.0,
                },
                kind: FrameKind::text(story),
                transform: Transform::IDENTITY,
                fill: tessera_document::paint::Paint::Solid(tessera_color::Color::BLACK),
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
                blend: tessera_document::blending::Blending::PLAIN,
                corners: tessera_document::corners::Corners::SQUARE,
                shadow: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
            },
        );
        doc
    }

    #[test]
    fn chapters_number_on_from_one_to_the_next() {
        let mut docs = vec![chapter(3, "One"), chapter(2, "Two"), chapter(4, "Three")];
        let changed = continue_numbering(&mut docs);
        assert_eq!(changed, vec![false, true, true]);
        let labels = |doc: &Document| -> Vec<String> {
            doc.page_numbers()
                .into_iter()
                .map(|(_, n)| n.label)
                .collect()
        };
        assert_eq!(labels(&docs[0]), vec!["1", "2", "3"]);
        assert_eq!(labels(&docs[1]), vec!["4", "5"]);
        assert_eq!(labels(&docs[2]), vec!["6", "7", "8", "9"]);
        // Again: nothing to change.
        assert_eq!(continue_numbering(&mut docs), vec![false, false, false]);
        // A chapter that already begins a section keeps it — its roman
        // numerals — and takes the number.
        let first = docs[1].page_ids().next().unwrap();
        let mut section = Section::starting_at(first);
        section.style = tessera_text::story::Numbering::LowerRoman;
        docs[1].set_sections(vec![section]);
        continue_numbering(&mut docs);
        assert_eq!(labels(&docs[1]), vec!["iv", "v"]);
    }

    #[test]
    fn combined_documents_stack_below_one_another_with_every_page_in_order() {
        let mut shaper = Shaper::new();
        let a = chapter(2, "One");
        let b = chapter(1, "Two");
        let ra = crate::resolve(&a, &mut shaper);
        let rb = crate::resolve(&b, &mut shaper);
        let combined = combine(vec![ra.clone(), rb.clone()]);
        assert_eq!(combined.pages.len(), 3);
        assert_eq!(combined.items.len(), ra.items.len() + rb.items.len());
        // The second document's page sits below the first's last.
        let last_of_a = ra.pages.last().unwrap().bounds;
        let first_of_b = combined.pages[2].bounds;
        assert!(first_of_b.y > last_of_a.y + last_of_a.height);
        // And its item moved with it: the frame's own bounds are as they
        // were, and its transform carries the move.
        let item = combined.items.last().unwrap();
        let original = rb.items.last().unwrap();
        assert_eq!(item.bounds, original.bounds);
        let moved = item.transform.apply(tessera_geometry::DocPoint {
            x: item.bounds.x,
            y: item.bounds.y,
        });
        assert!(
            (moved.y - (original.bounds.y + (first_of_b.y - rb.pages[0].bounds.y))).abs() < 1e-6
        );
    }

    #[test]
    fn a_book_s_contents_lists_every_chapter_s_headings_with_its_own_page_labels() {
        let mut shaper = Shaper::new();
        let mut docs = vec![chapter(3, "One"), chapter(2, "Two")];
        continue_numbering(&mut docs);
        let resolved: Vec<ResolvedDocument> = docs
            .iter()
            .map(|d| crate::resolve(d, &mut shaper))
            .collect();
        let entries: Vec<(&Document, &ResolvedDocument)> =
            docs.iter().zip(resolved.iter()).collect();
        let style = docs[0]
            .paragraph_styles
            .iter()
            .find(|(_, s)| s.name == "Chapter")
            .map(|(id, _)| id)
            .unwrap();
        let levels = vec![Level {
            style,
            entry_style: None,
        }];
        let generated = table_of_contents(&entries, 0, "Contents", None, &levels, 0.0);
        assert_eq!(
            generated.story.text, "Contents\nOne\t1\nTwo\t4",
            "the second chapter's heading, on the page number the book gives it"
        );
        // Only the placing document's entry links anywhere.
        assert_eq!(generated.destinations.len(), 1);
    }
}

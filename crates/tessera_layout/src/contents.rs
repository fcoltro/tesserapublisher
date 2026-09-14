//! What landed where: the facts a table of contents and an index are made of.
//!
//! Both are lists of *page labels* — a heading and the page it starts on, a
//! topic and every page it is mentioned on — and neither is knowable from the
//! model alone, because which page a paragraph starts on is a fact about the
//! layout. So both are read off a resolved document, the way running headers
//! are, and turned into a story whoever asked can put in a frame.
//!
//! Reading order is page order, then paint order within a page, then line
//! order within a frame: right for one body thread, and the least surprising
//! answer for anything else.

use tessera_document::contents::Level;
use tessera_document::document::Document;
use tessera_document::ids::{PageId, StoryId};
use tessera_document::nodes::FrameKind;
use tessera_text::story::{
    Hyperlink, ParagraphRun, ParagraphStyleId, Run, Story, TabAlignment, TabStop,
};
use tessera_text::variables::expand;

use crate::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};

/// A paragraph in one of the wanted styles, and the page its first line fell on.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub story: StoryId,
    /// Stored offset of the paragraph's start.
    pub start: usize,
    pub style: ParagraphStyleId,
    /// The paragraph's words, markers expanded, without the newline.
    pub text: String,
    pub page: PageId,
}

/// A topic and a page it is mentioned on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub topic: String,
    pub page: PageId,
}

/// The resolved text items, in reading order, with the page each is on.
fn in_reading_order<'a>(doc: &Document, resolved: &'a ResolvedDocument) -> Vec<&'a ResolvedItem> {
    let pages: Vec<PageId> = doc.page_ids().collect();
    let mut items: Vec<&ResolvedItem> = resolved
        .items
        .iter()
        .filter(|i| matches!(i.kind, ResolvedKind::Text { .. }) && i.on.is_some())
        .collect();
    // Stable, so paint order survives within a page.
    items.sort_by_key(|i| pages.iter().position(|p| Some(*p) == i.on));
    items
}

/// Every paragraph in one of `styles`, with the page it begins on.
///
/// A paragraph counts once, on the page holding its first line; a heading
/// that turns over a page is listed where it starts, as any contents page
/// would list it.
pub fn headings(
    doc: &Document,
    resolved: &ResolvedDocument,
    styles: &[ParagraphStyleId],
) -> Vec<Heading> {
    let mut out: Vec<Heading> = Vec::new();
    for item in in_reading_order(doc, resolved) {
        let (Some(page), ResolvedKind::Text { shaped, .. }) = (item.on, &item.kind) else {
            continue;
        };
        let Some(FrameKind::Text { story: id, .. }) = doc.frame(item.frame).map(|f| &f.kind) else {
            continue;
        };
        let Some(story) = doc.story(*id) else {
            continue;
        };
        // The paragraphs, not the paragraph runs: two headings in a row
        // share one run and are still two headings.
        let paragraphs = story.paragraph_ranges();
        for line in &shaped.lines {
            if line.range.is_empty() {
                continue; // a footnote's line
            }
            for range in &paragraphs {
                // Its first line: the paragraph starts inside this line.
                if !line.range.contains(&range.start) {
                    continue;
                }
                let Some(style) = story
                    .paragraph_run_at(range.start)
                    .and_then(|p| p.style)
                    .filter(|s| styles.contains(s))
                else {
                    continue;
                };
                if out.iter().any(|h| h.story == *id && h.start == range.start) {
                    continue;
                }
                let text = expand(&story.text[range.clone()], None);
                out.push(Heading {
                    story: *id,
                    start: range.start,
                    style,
                    text: text.trim_end_matches('\n').to_owned(),
                    page,
                });
            }
        }
    }
    out
}

/// Every index marker, with the page its line fell on.
pub fn mentions(doc: &Document, resolved: &ResolvedDocument) -> Vec<Mention> {
    let mut out = Vec::new();
    for item in in_reading_order(doc, resolved) {
        let (Some(page), ResolvedKind::Text { shaped, .. }) = (item.on, &item.kind) else {
            continue;
        };
        let Some(FrameKind::Text { story: id, .. }) = doc.frame(item.frame).map(|f| &f.kind) else {
            continue;
        };
        let Some(story) = doc.story(*id) else {
            continue;
        };
        let offsets = story.index_offsets();
        for line in &shaped.lines {
            if line.range.is_empty() {
                continue;
            }
            for (n, at) in offsets.iter().enumerate() {
                if !line.range.contains(at) {
                    continue;
                }
                let Some(entry) = story.index_entries.get(n) else {
                    continue;
                };
                if entry.topic.trim().is_empty() {
                    continue;
                }
                out.push(Mention {
                    topic: entry.topic.trim().to_owned(),
                    page,
                });
            }
        }
    }
    out
}

/// Build the contents story: a title, then one line per heading — its words,
/// a tab, and its page label — with a right-aligned, dot-leadered tab stop at
/// `measure` so the numbers line up at the margin.
pub fn table_of_contents(
    doc: &Document,
    resolved: &ResolvedDocument,
    title: &str,
    title_style: Option<ParagraphStyleId>,
    levels: &[Level],
    measure: f32,
) -> Generated {
    let styles: Vec<ParagraphStyleId> = levels.iter().map(|l| l.style).collect();
    let found = headings(doc, resolved, &styles);

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    if !title.is_empty() {
        paragraphs.push((title.to_owned(), title_style, false, None));
    }
    let mut destinations = Vec::new();
    for (n, heading) in found.iter().enumerate() {
        let label = doc.page_label(heading.page).unwrap_or_default();
        let entry_style = levels
            .iter()
            .find(|l| l.style == heading.style)
            .and_then(|l| l.entry_style);
        // Each entry links to its heading's page through a destination
        // named for it — numbered as well, so two chapters with one title
        // do not share a destination.
        let name = format!("Contents {}: {}", n + 1, heading.text);
        destinations.push((name.clone(), heading.page));
        paragraphs.push((
            format!("{}\t{label}", heading.text),
            entry_style,
            true,
            Some(Hyperlink::Destination(name)),
        ));
    }
    Generated {
        story: assemble(paragraphs, measure),
        destinations,
    }
}

/// A generated story, and the destinations its links name.
#[derive(Debug, Clone)]
pub struct Generated {
    pub story: Story,
    /// Name and page, for the caller to record with `set_destination`.
    pub destinations: Vec<(String, PageId)>,
}

/// One paragraph to assemble: its words, style, whether it is tabbed, and
/// what it links to.
type Paragraph = (String, Option<ParagraphStyleId>, bool, Option<Hyperlink>);

/// Build the index story: topics sorted, each with the labels of every page
/// it is mentioned on, once per page, in page order.
pub fn index(doc: &Document, resolved: &ResolvedDocument, title: &str) -> Story {
    let pages: Vec<PageId> = doc.page_ids().collect();
    let mut by_topic: Vec<(String, Vec<PageId>)> = Vec::new();
    for mention in mentions(doc, resolved) {
        let slot = match by_topic.iter_mut().find(|(t, _)| *t == mention.topic) {
            Some(slot) => slot,
            None => {
                by_topic.push((mention.topic.clone(), Vec::new()));
                by_topic.last_mut().expect("just pushed")
            }
        };
        if !slot.1.contains(&mention.page) {
            slot.1.push(mention.page);
        }
    }
    by_topic.sort_by_key(|(t, _)| t.to_lowercase());

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    if !title.is_empty() {
        paragraphs.push((title.to_owned(), None, false, None));
    }
    for (topic, mut on) in by_topic {
        on.sort_by_key(|p| pages.iter().position(|q| q == p));
        let labels: Vec<String> = on.iter().filter_map(|p| doc.page_label(*p)).collect();
        paragraphs.push((format!("{topic}\t{}", labels.join(", ")), None, true, None));
    }
    assemble(paragraphs, 0.0)
}

/// Paragraphs into a story, each with its style; the tabbed ones with a
/// right stop at `measure` when there is a measure to stop at.
fn assemble(paragraphs: Vec<Paragraph>, measure: f32) -> Story {
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut links: Vec<Run> = Vec::new();
    for (i, (words, style, tabbed, link)) in paragraphs.iter().enumerate() {
        let start = text.len();
        text.push_str(words);
        // The link covers the words and not the break after them.
        links.push(Run {
            range: start..text.len(),
            style: None,
            local: tessera_text::story::CharacterFormat {
                link: link.clone(),
                ..Default::default()
            },
        });
        if i + 1 < paragraphs.len() {
            text.push('\n');
            links.push(Run::plain(text.len() - 1..text.len()));
        }
        let mut local = tessera_text::story::ParagraphFormat::default();
        if *tabbed && measure > 0.0 {
            local.tab_stops = Some(vec![TabStop {
                position: measure,
                alignment: TabAlignment::Right,
                leader: Some('.'),
            }]);
        }
        runs.push(ParagraphRun {
            range: start..text.len(),
            style: *style,
            local,
        });
    }
    let mut story = Story::new(text);
    if !story.text.is_empty() {
        story.runs = links.into_iter().filter(|r| !r.range.is_empty()).collect();
        story.paragraphs = runs;
        story.merge_equal_neighbours();
        debug_assert!(story.runs_are_sound());
    }
    story
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::Frame;
    use tessera_geometry::{DocRect, Transform};
    use tessera_text::Shaper;
    use tessera_text::story::{ParagraphFormat, ParagraphStyle};

    fn text_frame(
        doc: &mut Document,
        page: PageId,
        story: StoryId,
        dy: f64,
    ) -> tessera_document::ids::FrameId {
        let bounds = doc.pages[page].bounds;
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: bounds.x + 20.0,
                    y: bounds.y + dy,
                    width: 300.0,
                    height: 200.0,
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
            },
        )
    }

    fn heading_style(doc: &mut Document) -> ParagraphStyleId {
        doc.add_paragraph_style(ParagraphStyle {
            name: "Heading".into(),
            based_on: None,
            format: ParagraphFormat::default(),
        })
    }

    #[test]
    fn headings_are_listed_with_the_page_they_start_on() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let heading = heading_style(&mut doc);
        let pages: Vec<PageId> = {
            let second = doc.add_page();
            vec![doc.page_ids().next().unwrap(), second]
        };
        let mut one = Story::new("Alpha\nbody");
        one.set_paragraph_style(0..6, Some(heading));
        let mut two = Story::new("body\nBeta");
        two.set_paragraph_style(5..9, Some(heading));
        let one = doc.add_story(one);
        let two = doc.add_story(two);
        text_frame(&mut doc, pages[0], one, 40.0);
        text_frame(&mut doc, pages[1], two, 40.0);

        let mut shaper = Shaper::new();
        let resolved = crate::resolve(&doc, &mut shaper);
        let found = headings(&doc, &resolved, &[heading]);
        let listed: Vec<(&str, PageId)> = found.iter().map(|h| (h.text.as_str(), h.page)).collect();
        assert_eq!(listed, vec![("Alpha", pages[0]), ("Beta", pages[1])]);

        let toc = table_of_contents(
            &doc,
            &resolved,
            "Contents",
            None,
            &[Level {
                style: heading,
                entry_style: None,
            }],
            300.0,
        );
        assert_eq!(toc.destinations.len(), 2, "one destination per heading");
        assert_eq!(toc.destinations[1].1, pages[1]);
        let toc = toc.story;
        assert_eq!(toc.text, "Contents\nAlpha\t1\nBeta\t2");
        assert!(toc.runs_are_sound());
        let linked = toc
            .runs
            .iter()
            .filter(|r| matches!(r.local.link, Some(Hyperlink::Destination(_))))
            .count();
        assert_eq!(linked, 2, "each entry links to its page");
        assert_eq!(toc.paragraph_ranges().len(), 3);
        assert!(
            toc.paragraph_run_at(9).unwrap().local.tab_stops.is_some(),
            "entries carry the right tab"
        );
        assert!(
            toc.paragraphs[0].local.tab_stops.is_none(),
            "the title does not"
        );
    }

    #[test]
    fn the_index_lists_each_topic_once_with_its_pages_in_order() {
        use tessera_text::variables::Marker;
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let second = doc.add_page();
        let first = doc.page_ids().next().unwrap();
        let e = Marker::IndexEntry.character();
        let mut one = Story::new(format!("type{e} and ink{e} and type{e}"));
        one.index_entries[0].topic = "Type".into();
        one.index_entries[1].topic = "Ink".into();
        one.index_entries[2].topic = "Type".into();
        let mut two = Story::new(format!("more type{e}"));
        two.index_entries[0].topic = "Type".into();
        let one = doc.add_story(one);
        let two = doc.add_story(two);
        text_frame(&mut doc, first, one, 40.0);
        text_frame(&mut doc, second, two, 40.0);

        let mut shaper = Shaper::new();
        let resolved = crate::resolve(&doc, &mut shaper);
        let story = index(&doc, &resolved, "");
        assert_eq!(story.text, "Ink\t1\nType\t1, 2");
    }
}

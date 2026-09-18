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
use tessera_text::variables::{Marker, expand};

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

/// A line of a story as laid out: the page it fell on and what of the
/// story it holds.
type PlacedLine = (PageId, std::ops::Range<usize>);

/// Every index marker, with the page its line fell on — and, for an entry
/// that reaches past its marker, every page through to where it reaches.
pub fn mentions(doc: &Document, resolved: &ResolvedDocument) -> Vec<Mention> {
    use tessera_text::story::IndexSpan;

    // Every line of every story, with its page, in reading order: a story
    // threaded through several frames has its lines on several pages, and
    // an entry that reaches to the end of the story reaches across them.
    let mut lines_of: Vec<(StoryId, Vec<PlacedLine>)> = Vec::new();
    for item in in_reading_order(doc, resolved) {
        let (Some(page), ResolvedKind::Text { shaped, .. }) = (item.on, &item.kind) else {
            continue;
        };
        let Some(FrameKind::Text { story: id, .. }) = doc.frame(item.frame).map(|f| &f.kind) else {
            continue;
        };
        let slot = match lines_of.iter_mut().find(|(s, _)| s == id) {
            Some(slot) => slot,
            None => {
                lines_of.push((*id, Vec::new()));
                lines_of.last_mut().expect("just pushed")
            }
        };
        slot.1.extend(
            shaped
                .lines
                .iter()
                .filter(|l| !l.range.is_empty())
                .map(|l| (page, l.range.clone())),
        );
    }

    let mut out = Vec::new();
    // A marker reads as nothing, so a line beginning with one begins, by
    // its stored range, at the character after it. The marker is on the
    // line holding the position just past it — see the same test in
    // `running::Running::read_anchors`.
    let width = Marker::IndexEntry.character().len_utf8();
    for (id, lines) in &lines_of {
        let Some(story) = doc.story(*id) else {
            continue;
        };
        let paragraphs = story.paragraph_ranges();
        for (n, at) in story.index_offsets().into_iter().enumerate() {
            let Some(entry) = story.index_entries.get(n) else {
                continue;
            };
            if entry.topic.trim().is_empty() {
                continue;
            }
            let past = at + width;
            let Some(first) = lines
                .iter()
                .position(|(_, r)| r.start <= past && past <= r.end)
            else {
                continue;
            };
            // Where the mention stops: on its own line, or at the end of a
            // later paragraph or the story — the last line that begins
            // before that offset.
            let end = match entry.span {
                IndexSpan::Here => None,
                IndexSpan::ToEndOfStory => Some(story.text.len()),
                IndexSpan::Paragraphs(more) => {
                    let own = paragraphs
                        .iter()
                        .position(|r| r.contains(&at) || r.end == at);
                    own.map(|p| {
                        let last = (p + more as usize).min(paragraphs.len().saturating_sub(1));
                        paragraphs[last].end
                    })
                }
            };
            let last = match end {
                Some(end) => lines
                    .iter()
                    .rposition(|(_, r)| r.start < end.max(1))
                    .unwrap_or(first)
                    .max(first),
                None => first,
            };
            let topic = entry.topic.trim().to_owned();
            let mut on: Vec<PageId> = Vec::new();
            for (page, _) in &lines[first..=last] {
                if !on.contains(page) {
                    on.push(*page);
                }
            }
            // Every page between the first and the last counts, whether or
            // not a line of this story fell on it: a subject that runs from
            // 12 to 15 is on 13 and 14 too.
            let order: Vec<PageId> = doc.page_ids().collect();
            let (Some(a), Some(b)) = (
                on.first().and_then(|p| order.iter().position(|q| q == p)),
                on.last().and_then(|p| order.iter().position(|q| q == p)),
            ) else {
                continue;
            };
            for page in &order[a.min(b)..=a.max(b)] {
                out.push(Mention {
                    topic: topic.clone(),
                    page: *page,
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
        paragraphs.push((title.to_owned(), title_style, false, None, 0));
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
            0,
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
/// One paragraph of a generated story: its words, the style it is set in,
/// whether it carries a right tab for a page label, the link on it, and how
/// many levels it is nested — a sub-topic under its topic.
pub(crate) type Paragraph = (
    String,
    Option<ParagraphStyleId>,
    bool,
    Option<Hyperlink>,
    usize,
);

/// Build the index story: topics sorted, each with the labels of every page
/// it is mentioned on, once per page, in page order.
pub fn index(doc: &Document, resolved: &ResolvedDocument, title: &str) -> Story {
    let pages: Vec<PageId> = doc.page_ids().collect();
    // Keyed by the topic's levels: "Type: Serif" is a page under
    // ["Type", "Serif"], and "Type" alone under ["Type"].
    let mut by_topic: Vec<(Vec<String>, Vec<PageId>)> = Vec::new();
    for mention in mentions(doc, resolved) {
        let levels = tessera_text::story::IndexEntry {
            topic: mention.topic.clone(),
            span: Default::default(),
        }
        .levels();
        if levels.is_empty() {
            continue;
        }
        let slot = match by_topic.iter_mut().find(|(t, _)| *t == levels) {
            Some(slot) => slot,
            None => {
                by_topic.push((levels, Vec::new()));
                by_topic.last_mut().expect("just pushed")
            }
        };
        if !slot.1.contains(&mention.page) {
            slot.1.push(mention.page);
        }
    }
    // A sub-topic needs its topic above it, listed even when nothing is
    // filed under the topic itself.
    let mut parents: Vec<Vec<String>> = Vec::new();
    for (levels, _) in &by_topic {
        for depth in 1..levels.len() {
            let parent = levels[..depth].to_vec();
            if !by_topic.iter().any(|(t, _)| *t == parent) && !parents.contains(&parent) {
                parents.push(parent);
            }
        }
    }
    by_topic.extend(parents.into_iter().map(|p| (p, Vec::new())));
    // Sorted by the levels, case aside, so a sub-topic follows its topic
    // and the sub-topics of one topic are in order among themselves.
    by_topic.sort_by_cached_key(|(t, _)| t.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>());

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    if !title.is_empty() {
        paragraphs.push((title.to_owned(), None, false, None, 0));
    }
    for (levels, mut on) in by_topic {
        on.sort_by_key(|p| pages.iter().position(|q| q == p));
        let name = levels.last().cloned().unwrap_or_default();
        let depth = levels.len() - 1;
        if on.is_empty() {
            paragraphs.push((name, None, false, None, depth));
        } else {
            let labels = page_ranges(doc, &pages, &on);
            paragraphs.push((format!("{name}\t{labels}"), None, true, None, depth));
        }
    }
    assemble(paragraphs, 0.0)
}

/// The endnotes: every story's notes in reading order, each numbered as
/// its reference is — from the footnote options' start, once through the
/// story — and set as a paragraph, the number where the note's own marker
/// was. A story's notes are listed under the story's first frame, so a
/// thread contributes once.
pub fn endnotes(doc: &Document, resolved: &ResolvedDocument, title: &str) -> Story {
    let options = doc.footnotes;
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    if !title.is_empty() {
        paragraphs.push((title.to_owned(), None, false, None, 0));
    }
    let mut seen: Vec<StoryId> = Vec::new();
    for item in in_reading_order(doc, resolved) {
        let Some(FrameKind::Text { story: id, .. }) = doc.frame(item.frame).map(|f| &f.kind) else {
            continue;
        };
        if seen.contains(id) {
            continue;
        }
        seen.push(*id);
        let Some(story) = doc.story(*id) else {
            continue;
        };
        for (n, note) in story.footnotes.iter().enumerate() {
            let number = n as u32 + options.start_at.max(1);
            let label = options.numbering.label(number);
            let read = expand(
                &note.text,
                Some(&tessera_text::variables::Variables::for_footnote_labelled(
                    n as u32 + 1,
                    label,
                )),
            );
            paragraphs.push((read, None, false, None, 0));
        }
    }
    assemble(paragraphs, 0.0)
}

/// "1, 3–5, 8": the pages a topic is on, with runs of neighbours joined
/// by an en dash, as every index sets them. Pages in different sections
/// are never joined — "iv–2" would be nonsense — so a run is neighbours in
/// the reading order *and* in the same section.
fn page_ranges(doc: &Document, order: &[PageId], on: &[PageId]) -> String {
    let numbered = doc.page_numbers();
    let info = |p: PageId| {
        numbered
            .iter()
            .find(|(id, _)| *id == p)
            .map(|(_, n)| (n.label.clone(), n.section))
    };
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < on.len() {
        let Some((start_label, section)) = info(on[i]) else {
            i += 1;
            continue;
        };
        let mut j = i;
        while j + 1 < on.len() {
            let here = order.iter().position(|p| *p == on[j]);
            let next = order.iter().position(|p| *p == on[j + 1]);
            let neighbours = matches!((here, next), (Some(a), Some(b)) if b == a + 1);
            let same_section = info(on[j + 1]).is_some_and(|(_, s)| s == section);
            if neighbours && same_section {
                j += 1;
            } else {
                break;
            }
        }
        if j > i {
            let end_label = info(on[j]).map(|(l, _)| l).unwrap_or_default();
            out.push(format!("{start_label}\u{2013}{end_label}"));
        } else {
            out.push(start_label);
        }
        i = j + 1;
    }
    out.join(", ")
}

/// Paragraphs into a story, each with its style; the tabbed ones with a
/// right stop at `measure` when there is a measure to stop at.
pub(crate) fn assemble(paragraphs: Vec<Paragraph>, measure: f32) -> Story {
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut links: Vec<Run> = Vec::new();
    for (i, (words, style, tabbed, link, depth)) in paragraphs.iter().enumerate() {
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
        // A nested entry steps in by a pica a level, as every index sets
        // its sub-entries.
        if *depth > 0 {
            local.indent_left = Some(12.0 * *depth as f32);
        }
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
        assert_eq!(story.text, "Ink\t1\nType\t1\u{2013}2", "neighbours join");
    }

    #[test]
    fn endnotes_list_every_story_s_notes_in_reading_order_numbered_as_cited() {
        use tessera_document::footnotes::{FootnoteNumbering, FootnoteOptions, NotePlacement};
        use tessera_text::variables::Marker;
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let second = doc.add_page();
        let first = doc.page_ids().next().unwrap();
        let r = Marker::FootnoteReference.character();
        let mut one = Story::new(format!("A claim{r} and another{r}."));
        for (note, words) in one.footnotes.iter_mut().zip(["First source.", "Second."]) {
            let end = note.text.len();
            note.insert_text(end, words);
        }
        let mut two = Story::new(format!("Later{r}."));
        let end = two.footnotes[0].text.len();
        two.footnotes[0].insert_text(end, "Third, in its own story.");
        // Placed on the pages the other way round: reading order, not the
        // order the stories were made in, decides the list.
        let one = doc.add_story(one);
        let two = doc.add_story(two);
        text_frame(&mut doc, second, one, 40.0);
        text_frame(&mut doc, first, two, 40.0);
        doc.set_footnote_options(FootnoteOptions {
            placement: NotePlacement::End,
            numbering: FootnoteNumbering::LowerRoman,
            ..Default::default()
        });

        let mut shaper = Shaper::new();
        let resolved = crate::resolve(&doc, &mut shaper);
        let story = endnotes(&doc, &resolved, "Notes");
        assert_eq!(
            story.text,
            "Notes
i	Third, in its own story.
i	First source.
ii	Second.",
            "each story's notes, counted from one, in the order the pages read"
        );

        // And nothing at the foot of either frame: the notes are the list's.
        for item in &resolved.items {
            let ResolvedKind::Text { shaped, .. } = &item.kind else {
                continue;
            };
            assert!(
                shaped.lines.iter().all(|l| !l.range.is_empty()),
                "no note lines set at the foot"
            );
        }
        // Set at the foot again, the same document puts them back.
        doc.set_footnote_options(FootnoteOptions::default());
        let resolved = crate::resolve(&doc, &mut shaper);
        let at_foot = resolved.items.iter().any(|item| {
            matches!(&item.kind, ResolvedKind::Text { shaped, .. }
                if shaped.lines.iter().any(|l| l.range.is_empty()))
        });
        assert!(at_foot);
    }

    #[test]
    fn sub_topics_nest_under_their_topic_and_a_span_reaches_across_pages() {
        use tessera_text::story::IndexSpan;
        use tessera_text::variables::Marker;
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let first = doc.page_ids().next().unwrap();
        let second = doc.add_page();
        let third = doc.add_page();
        let e = Marker::IndexEntry.character();
        // Enough copy that the story runs from the first page, through a
        // frame on the second, onto the third.
        let filler = "words and words and words and words and words and words\n".repeat(45);
        let mut long = Story::new(format!("{e}Serif faces{e} here. {filler}the end."));
        long.index_entries[0].topic = "Type: Serif".into();
        long.index_entries[0].span = IndexSpan::ToEndOfStory;
        long.index_entries[1].topic = "Type".into();
        let long = doc.add_story(long);
        let a = text_frame(&mut doc, first, long, 40.0);
        let b = text_frame(&mut doc, second, long, 40.0);
        let c = text_frame(&mut doc, third, long, 40.0);
        assert!(doc.thread(a, b) && doc.thread(b, c));

        let mut shaper = Shaper::new();
        let resolved = crate::resolve(&doc, &mut shaper);
        // The story must really reach the third page for the span to mean
        // anything: checked rather than assumed.
        let reaches_third = resolved.items.iter().any(|i| {
            i.frame == c
                && matches!(&i.kind, ResolvedKind::Text { shaped, .. } if !shaped.lines.is_empty())
        });
        assert!(reaches_third, "the fixture must thread onto the third page");

        let story = index(&doc, &resolved, "");
        assert_eq!(
            story.text, "Type\t1\nSerif\t1\u{2013}3",
            "the topic, then its sub-topic, reaching from the marker to the story's end"
        );
        assert_eq!(
            story.paragraphs[1].local.indent_left,
            Some(12.0),
            "stepped in one level"
        );
        assert_eq!(story.paragraphs[0].local.indent_left, None);

        // A sub-topic with no topic of its own still gets the heading.
        let mut only_sub = Story::new(format!("{e}Bold"));
        only_sub.index_entries[0].topic = "Weight: Bold".into();
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let first = doc.page_ids().next().unwrap();
        let only_sub = doc.add_story(only_sub);
        text_frame(&mut doc, first, only_sub, 40.0);
        let resolved = crate::resolve(&doc, &mut shaper);
        let story = index(&doc, &resolved, "");
        assert_eq!(story.text, "Weight\nBold\t1");
    }

    #[test]
    fn a_span_of_paragraphs_reaches_to_the_end_of_the_last_one() {
        use tessera_text::story::IndexSpan;
        use tessera_text::variables::Marker;
        let e = Marker::IndexEntry.character();
        let mut story = Story::new(format!("{e}One.\nTwo.\nThree.\nFour."));
        story.index_entries[0].topic = "Counting".into();
        story.index_entries[0].span = IndexSpan::Paragraphs(1);
        // The paragraph after the marker's ends before "Three.".
        let ranges = story.paragraph_ranges();
        assert_eq!(ranges.len(), 4);
        assert!(story.text[..ranges[1].end].ends_with("Two.\n"));
        // On one page, one page: the reach across pages is proven above;
        // this pins that a span of paragraphs indexes at all.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let first = doc.page_ids().next().unwrap();
        let story = doc.add_story(story);
        text_frame(&mut doc, first, story, 40.0);
        let mut shaper = Shaper::new();
        let resolved = crate::resolve(&doc, &mut shaper);
        assert_eq!(index(&doc, &resolved, "").text, "Counting\t1");
    }

    #[test]
    fn an_index_marker_at_the_start_of_a_line_is_still_on_the_page() {
        // A marker reads as nothing, so a line that begins with one begins,
        // by its stored range, at the character after it — and a test for
        // "is the marker's offset inside the line" said no. Marking a topic
        // at the head of a paragraph is the ordinary case, not the edge.
        use tessera_text::variables::Marker;
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let first = doc.page_ids().next().unwrap();
        let e = Marker::IndexEntry.character();
        let mut one = Story::new(format!(
            "{e}Type at the head.
{e}Ink at the head of the next."
        ));
        one.index_entries[0].topic = "Type".into();
        one.index_entries[1].topic = "Ink".into();
        let one = doc.add_story(one);
        text_frame(&mut doc, first, one, 40.0);

        let mut shaper = Shaper::new();
        let resolved = crate::resolve(&doc, &mut shaper);
        let story = index(&doc, &resolved, "");
        assert_eq!(
            story.text,
            "Ink	1
Type	1"
        );
    }

    #[test]
    fn page_runs_join_within_a_section_and_not_across_one() {
        use tessera_document::sections::Section;
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        for _ in 0..5 {
            doc.add_page();
        }
        let pages: Vec<PageId> = doc.page_ids().collect();
        // 1 2 3 | 4 5 6 -> the second section restarts at 1.
        doc.set_sections(vec![Section::starting_at(pages[3])]);
        assert_eq!(
            page_ranges(
                &doc,
                &pages,
                &[pages[0], pages[1], pages[2], pages[4], pages[5]]
            ),
            "1\u{2013}3, 2\u{2013}3"
        );
        assert_eq!(
            page_ranges(&doc, &pages, &[pages[2], pages[3]]),
            "3, 1",
            "neighbours in different sections stay apart"
        );
        assert_eq!(page_ranges(&doc, &pages, &[pages[0], pages[2]]), "1, 3");
    }
}

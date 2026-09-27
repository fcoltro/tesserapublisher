//! Text that runs out of frames flows onto new pages.
//!
//! A manuscript placed into one frame is the first page of a book and three
//! hundred pages of overset. What a person means by placing it is "set all
//! of this", so the thread is carried on: a page after the page its last
//! frame is on, a text frame in that page's margins threaded on from the
//! last, and again, until the story fits — InDesign's autoflow, with the new
//! pages added at the end of the story as its smart text reflow adds them.
//!
//! **Pages are added in batches, not one at a time.** Laying the story out
//! after every page would lay a book out once per page. So one page is added
//! first, to learn how much a page holds; then as many as the rest of the
//! story needs at that rate; then the handful a heading or a picture
//! changes; and whatever was added past the end of the text is taken away.

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, PageId};
use tessera_document::nodes::FrameKind;
use tessera_geometry::Transform;
use tessera_text::shape::Shaper;

use crate::resolve::{ResolvedKind, resolve_only};

/// The most pages one flow adds: a guard against a story that could never
/// fit — a word wider than the page — rather than a limit anyone meets.
pub const MOST_PAGES: usize = 2000;

/// What flowing a story did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flow {
    /// The pages added, in reading order.
    pub pages: Vec<PageId>,
    /// The frames added to them, in thread order.
    pub frames: Vec<FrameId>,
    /// Whether the story fits now.
    pub fits: bool,
}

/// What a frame holds of its story, laid out where it stands.
struct Held {
    /// Lines that did not fit anywhere.
    overset: usize,
    /// Bytes of the story it holds.
    bytes: usize,
    /// Where in the story its text ends; `None` when it holds nothing.
    ends: Option<usize>,
}

fn held(doc: &Document, shaper: &mut Shaper, frame: FrameId) -> Option<Held> {
    let page = doc.page_of_frame(frame)?;
    let laid = resolve_only(doc, shaper, &[page]);
    let item = laid.items.iter().find(|item| item.frame == frame)?;
    let ResolvedKind::Text {
        shaped,
        overset_lines,
        ..
    } = &item.kind
    else {
        return None;
    };
    let first = shaped.lines.first().map(|l| l.range.start);
    let last = shaped.lines.last().map(|l| l.range.end);
    Some(Held {
        overset: *overset_lines,
        bytes: match (first, last) {
            (Some(a), Some(b)) => b.saturating_sub(a),
            _ => 0,
        },
        ends: last,
    })
}

/// Carry the thread `frame` belongs to onto new pages until its story fits,
/// adding at most `most` pages. Nothing is added when it fits already, or
/// when `frame` is not a text frame on a page.
pub fn flow_onto_new_pages(
    doc: &mut Document,
    shaper: &mut Shaper,
    frame: FrameId,
    most: usize,
) -> Flow {
    let mut flow = Flow::default();
    let Some(FrameKind::Text { story, .. }) = doc.frame(frame).map(|f| f.kind.clone()) else {
        return flow;
    };
    let Some(total) = doc.story(story).map(|s| s.text.len()) else {
        return flow;
    };
    loop {
        let Some(&last) = doc.thread_of(frame).last() else {
            return flow;
        };
        let Some(now) = held(doc, shaper, last) else {
            return flow;
        };
        if now.overset == 0 {
            flow.fits = true;
            break;
        }
        let room = most.saturating_sub(flow.pages.len());
        // A page of ours that held nothing will not be followed by one that
        // holds something: the story cannot fit, and more pages would not
        // change that.
        if room == 0 || (flow.frames.contains(&last) && now.bytes == 0) {
            break;
        }
        // One page first, to learn what a page holds; then the rest at that
        // rate. A frame of somebody else's — a small box on page one — says
        // nothing about a page's worth.
        let count = if flow.frames.contains(&last) {
            let left = total.saturating_sub(now.ends.unwrap_or(0));
            left.div_ceil(now.bytes.max(1)).clamp(1, room)
        } else {
            1
        };
        let Some(page) = doc.page_of_frame(last) else {
            return flow;
        };
        let pages = doc.insert_pages(Some(page), count, None);
        let mut before = last;
        for page in &pages {
            let Some(id) = frame_on(doc, last, *page) else {
                continue;
            };
            doc.thread(before, id);
            flow.frames.push(id);
            before = id;
        }
        flow.pages.extend(pages);
    }
    trim(doc, shaper, &mut flow);
    flow
}

/// A text frame filling `page`'s margins, laid out as `like` is — its
/// columns, gutter, inset and object style — on `like`'s layer, threaded to
/// nothing yet.
fn frame_on(doc: &mut Document, like: FrameId, page: PageId) -> Option<FrameId> {
    let mut frame = doc.frame(like)?.clone();
    let layer = doc.layer_of_frame(like).or_else(|| doc.default_layer())?;
    frame.bounds = doc.margin_rect(page)?;
    frame.transform = Transform::IDENTITY;
    frame.anchor = None;
    frame.hidden = false;
    frame.locked = false;
    if let FrameKind::Text { layout, .. } = &mut frame.kind {
        layout.next = None;
    }
    Some(doc.add_frame(layer, frame))
}

/// Take away the pages added past the end of the text: a batch sized at
/// one page's rate overshoots when later pages hold more.
fn trim(doc: &mut Document, shaper: &mut Shaper, flow: &mut Flow) {
    let laid = resolve_only(doc, shaper, &flow.pages);
    let empty: Vec<FrameId> = flow
        .frames
        .iter()
        .copied()
        .filter(|id| {
            laid.items.iter().any(|item| {
                item.frame == *id
                    && matches!(&item.kind, ResolvedKind::Text { shaped, .. } if shaped.lines.is_empty())
            })
        })
        .collect();
    // Only a page that holds nothing but its empty frame goes.
    let going: Vec<PageId> = flow
        .pages
        .iter()
        .copied()
        .filter(|page| {
            let on = doc.frames_on_page(*page);
            !on.is_empty() && on.iter().all(|f| empty.contains(f))
        })
        .collect();
    if going.is_empty() {
        return;
    }
    doc.remove_pages(&going);
    flow.pages.retain(|p| !going.contains(p));
    flow.frames.retain(|f| doc.frame(*f).is_some());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::Frame;

    fn frame(bounds: tessera_geometry::DocRect) -> Frame {
        crate::resolve::tests_support::frame(bounds)
    }

    /// A one-page document with `paragraphs` paragraphs of sixty words in a
    /// frame filling its margins, and anything else wanted on the page.
    fn manuscript(paragraphs: usize) -> (Document, FrameId) {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let layer = doc.default_layer().expect("layer");
        let words =
            "The harbour wakes before the town does and the boats leave in the grey light. "
                .repeat(4);
        let text = vec![words; paragraphs].join("\n");
        let story = doc.add_story(tessera_text::story::Story::new(&text));
        let page = doc.page_ids().next().expect("a page");
        let mut first = frame(doc.margin_rect(page).expect("margins"));
        first.kind = FrameKind::text(story);
        let id = doc.add_frame(layer, first);
        (doc, id)
    }

    fn overset(doc: &Document, frame: FrameId) -> usize {
        let last = *doc.thread_of(frame).last().expect("a thread");
        held(doc, &mut Shaper::new(), last)
            .expect("laid out")
            .overset
    }

    #[test]
    fn an_overset_story_flows_onto_as_many_pages_as_it_needs_and_no_more() {
        let (mut doc, first) = manuscript(60);
        assert!(overset(&doc, first) > 0);
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
        assert!(flow.fits);
        assert_eq!(overset(&doc, first), 0);
        let pages = doc.page_ids().count();
        assert_eq!(pages, 1 + flow.pages.len());
        assert!(flow.pages.len() > 2, "{} pages", flow.pages.len());
        // Every frame of the thread holds something: none was left empty at
        // the end.
        let chain = doc.thread_of(first);
        assert_eq!(chain.len(), pages);
        let mut shaper = Shaper::new();
        for id in &chain {
            assert!(held(&doc, &mut shaper, *id).expect("laid out").bytes > 0);
        }
        // One page fewer would not do.
        let last_page = *flow.pages.last().expect("pages");
        let mut shorter = doc.clone();
        shorter.remove_page(last_page);
        assert!(overset(&shorter, first) > 0, "the last page is needed");
    }

    #[test]
    fn each_new_page_has_a_frame_in_its_margins_laid_out_as_the_first() {
        let (mut doc, first) = manuscript(20);
        if let Some(FrameKind::Text { layout, .. }) = doc.frame_mut(first).map(|f| &mut f.kind) {
            layout.columns = 2;
            layout.gutter = 14.0;
        }
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
        assert!(flow.fits && !flow.frames.is_empty());
        for (page, id) in flow.pages.iter().zip(&flow.frames) {
            let frame = doc.frame(*id).expect("frame");
            assert_eq!(Some(frame.bounds), doc.margin_rect(*page));
            assert_eq!(doc.page_of_frame(*id), Some(*page));
            let FrameKind::Text { layout, .. } = &frame.kind else {
                panic!("a text frame");
            };
            assert_eq!((layout.columns, layout.gutter), (2, 14.0));
        }
    }

    #[test]
    fn new_pages_follow_the_page_the_story_ends_on_and_push_the_rest_on() {
        let (mut doc, first) = manuscript(20);
        let after = doc.add_page();
        doc.reflow_spreads();
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
        let order: Vec<PageId> = doc.page_ids().collect();
        assert_eq!(
            order.last(),
            Some(&after),
            "the page after the story stays after it"
        );
        assert_eq!(&order[1..order.len() - 1], flow.pages.as_slice());
    }

    #[test]
    fn a_story_that_fits_adds_nothing() {
        let (mut doc, first) = manuscript(1);
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
        assert_eq!(
            flow,
            Flow {
                fits: true,
                ..Flow::default()
            }
        );
        assert_eq!(doc.page_ids().count(), 1);
    }

    #[test]
    fn a_small_frame_is_followed_by_page_sized_ones_not_by_pages_at_its_rate() {
        let (mut doc, first) = manuscript(12);
        doc.frame_mut(first).expect("frame").bounds.height = 60.0;
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
        assert!(flow.fits);
        // Twelve paragraphs fill a few pages, not the dozens a sixty-point
        // box's rate would have asked for — and none were left over.
        assert!(flow.pages.len() < 8, "{} pages", flow.pages.len());
        let mut shaper = Shaper::new();
        for id in &flow.frames {
            assert!(held(&doc, &mut shaper, *id).expect("laid out").bytes > 0);
        }
    }

    #[test]
    fn pages_added_at_a_sparse_pages_rate_are_taken_away_again() {
        // The first pages in large type, the rest small: the page that
        // teaches the rate holds a fraction of what the pages after it do,
        // so the batch sized at its rate adds too many.
        let (mut doc, first) = manuscript(80);
        let story = match doc.frame(first).map(|f| f.kind.clone()) {
            Some(FrameKind::Text { story, .. }) => story,
            _ => panic!("a text frame"),
        };
        let text = doc.story(story).expect("story");
        let big = text.text.len() / 6;
        doc.story_mut(story).expect("story").apply_character_format(
            0..big,
            &tessera_text::story::CharacterFormat {
                size: Some(40.0),
                ..Default::default()
            },
        );
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
        assert!(flow.fits);
        let mut shaper = Shaper::new();
        for id in doc.thread_of(first) {
            assert!(
                held(&doc, &mut shaper, id).expect("laid out").bytes > 0,
                "a page was left holding nothing"
            );
        }
        assert_eq!(doc.page_ids().count(), 1 + flow.pages.len());
    }

    #[test]
    fn a_flow_stops_at_the_most_pages_it_may_add() {
        let (mut doc, first) = manuscript(150);
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, 2);
        assert!(!flow.fits);
        assert_eq!(flow.pages.len(), 2);
    }

    #[test]
    fn the_last_frame_of_a_longer_thread_is_the_one_carried_on() {
        let (mut doc, first) = manuscript(30);
        let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, 1);
        let middle = flow.frames[0];
        // Asked of any frame of the thread, it carries on from its end.
        let more = flow_onto_new_pages(&mut doc, &mut Shaper::new(), middle, MOST_PAGES);
        assert!(more.fits);
        assert_eq!(doc.thread_of(first).len(), 2 + more.frames.len());
        assert_eq!(overset(&doc, first), 0);
    }

    #[test]
    #[ignore]
    fn cost() {
        for paragraphs in [300, 1200] {
            let (mut doc, first) = manuscript(paragraphs);
            let start = std::time::Instant::now();
            let flow = flow_onto_new_pages(&mut doc, &mut Shaper::new(), first, MOST_PAGES);
            eprintln!(
                "{paragraphs} paragraphs: {} pages in {:?}",
                flow.pages.len() + 1,
                start.elapsed()
            );
        }
    }
}

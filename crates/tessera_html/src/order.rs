//! What goes out, in what order.

use std::collections::HashSet;

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, StoryId};
use tessera_document::nodes::FrameKind;

/// One thing in the reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// A story, once, where its first frame is.
    Story(StoryId),
    /// A picture frame with something placed in it.
    Picture(FrameId),
    /// A table.
    Table(FrameId),
}

/// Every block of the document in reading order: page by page, and on each
/// page top to bottom, then left to right, by where each object's box
/// begins. Objects on parent pages, hidden ones and ones anchored in text
/// are not blocks of their own.
pub fn reading_order(doc: &Document) -> Vec<Block> {
    let mut out = Vec::new();
    let mut stories: HashSet<StoryId> = HashSet::new();
    for page in doc.page_ids() {
        let mut here: Vec<(f64, f64, FrameId)> = Vec::new();
        for id in doc.frames_on_page(page) {
            collect(doc, id, &mut here);
        }
        // Top to bottom, then left to right, a point's grace either way so
        // two boxes set level read left to right.
        here.sort_by(|a, b| {
            let (ay, by) = ((a.0 / 2.0).round(), (b.0 / 2.0).round());
            ay.total_cmp(&by).then(a.1.total_cmp(&b.1))
        });
        for (_, _, id) in here {
            let Some(frame) = doc.frame(id) else { continue };
            match &frame.kind {
                FrameKind::Text { story, .. } => {
                    if stories.insert(*story) {
                        out.push(Block::Story(*story));
                    }
                }
                FrameKind::Path(_) => {
                    if let Some(carried) = doc.path_text(id)
                        && stories.insert(carried.story)
                    {
                        out.push(Block::Story(carried.story));
                    }
                }
                FrameKind::Graphic { placed: Some(_) } => out.push(Block::Picture(id)),
                FrameKind::Table(_) => out.push(Block::Table(id)),
                _ => {}
            }
        }
    }
    out
}

/// `id` and, for a group, what is in it, with where each begins.
fn collect(doc: &Document, id: FrameId, out: &mut Vec<(f64, f64, FrameId)>) {
    if doc.is_hidden(id) {
        return;
    }
    let Some(frame) = doc.frame(id) else { return };
    if frame.anchor.is_some() {
        return;
    }
    if let FrameKind::Group(children) = &frame.kind {
        for child in children {
            collect(doc, *child, out);
        }
        return;
    }
    if let Some(b) = doc.visual_bounds(id) {
        out.push((b.y, b.x, id));
    }
}

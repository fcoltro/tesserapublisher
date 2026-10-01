//! The content conveyor: InDesign's Content Collector and Content Placer.
//!
//! One tool, B, as InDesign has it. Collecting, a click on an object puts a
//! copy of it on the conveyor — as it is at that moment, from a snapshot of
//! the document, so editing the original afterwards does not change what was
//! collected. Pressing B again turns the tool to placing: a click lays the
//! next item on the page with its top left at the pointer, and — unless the
//! conveyor is set to keep them — takes it off.
//!
//! The same snapshot copy and paste use ([`crate::app::Clipboard`]), so a
//! placed item brings its stories, its links and its styles with it exactly
//! as a pasted one does.

use tessera_document::ids::FrameId;
use tessera_geometry::DocPoint;

use crate::app::{Clipboard, TesseraApp};

/// What the conveyor holds, and which way the tool is working.
#[derive(Default)]
pub struct Conveyor {
    pub items: Vec<Clipboard>,
    /// Placing rather than collecting.
    pub placing: bool,
    /// Leave each item on the conveyor after placing it, to place again.
    pub keep: bool,
    /// Place copies as linked content, tied to their originals, when the
    /// original is in the document being placed into. See
    /// [`tessera_document::content_link`].
    pub link: bool,
}

/// Put a copy of `id` at the end of the conveyor.
pub fn collect(state: &mut TesseraApp, id: FrameId) {
    let source = std::sync::Arc::new(state.active().document().clone());
    if source.frame(id).is_none() {
        return;
    }
    let from = Some(state.active);
    state.conveyor.items.push(Clipboard {
        source,
        root: id,
        from,
    });
    let n = state.conveyor.items.len();
    state.status = Some(crate::app::Status::info(match n {
        1 => "1 item on the conveyor; press B to place".to_string(),
        n => format!("{n} items on the conveyor; press B to place"),
    }));
}

/// A line for the conveyor's list: what kind of thing an item is, and for
/// text the first words of it.
pub fn describe(item: &Clipboard) -> String {
    use tessera_document::nodes::FrameKind;
    let Some(frame) = item.source.frame(item.root) else {
        return "Gone".to_string();
    };
    match &frame.kind {
        FrameKind::Text { story, .. } => {
            let words: String = item
                .source
                .story(*story)
                .map(|s| {
                    s.text
                        .chars()
                        .filter(|c| tessera_text::variables::Marker::of(*c).is_none())
                        .take(28)
                        .collect()
                })
                .unwrap_or_default();
            let words = words.replace('\n', " ");
            if words.trim().is_empty() {
                "Text frame".to_string()
            } else {
                format!("Text: {}", words.trim())
            }
        }
        FrameKind::Graphic {
            placed: Some(placed),
            ..
        } => item
            .source
            .links
            .get(placed.link)
            .and_then(|l| l.path.file_name())
            .map_or("Picture".to_string(), |n| {
                format!("Picture: {}", n.to_string_lossy())
            }),
        FrameKind::Graphic { .. } => "Empty picture box".to_string(),
        FrameKind::Group(children) => format!("Group of {}", children.len()),
        FrameKind::Table(_) => "Table".to_string(),
        FrameKind::Rectangle => "Rectangle".to_string(),
        FrameKind::Ellipse => "Ellipse".to_string(),
        _ => "Shape".to_string(),
    }
}

/// Where an item's top left is, in its snapshot: the offset a placement
/// needs to bring it to the pointer.
pub fn top_left(item: &Clipboard) -> Option<DocPoint> {
    let frame = item.source.frame(item.root)?;
    let corners = frame.corners();
    Some(DocPoint {
        x: corners.iter().map(|c| c.x).fold(f64::INFINITY, f64::min),
        y: corners.iter().map(|c| c.y).fold(f64::INFINITY, f64::min),
    })
}

/// The size of the next item to place, for the ghost under the pointer.
pub fn next_size(state: &TesseraApp) -> Option<(f64, f64)> {
    let item = state.conveyor.items.first()?;
    let frame = item.source.frame(item.root)?;
    let corners = frame.corners();
    let (x0, x1) = corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |s, c| {
            (s.0.min(c.x), s.1.max(c.x))
        });
    let (y0, y1) = corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |s, c| {
            (s.0.min(c.y), s.1.max(c.y))
        });
    Some((x1 - x0, y1 - y0))
}

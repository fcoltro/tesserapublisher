//! Linked content: a copy that stays tied to its original.
//!
//! InDesign's Content Placer with "Create Link". The copy is an ordinary
//! frame — moved, resized, recoloured as any other — and the document
//! remembers which frame it came from and what that original looked like
//! when they were linked, as a fingerprint. When the original has changed
//! since, the copy is out of date; updating it gives it the original's
//! appearance and content again, where it stands.
//!
//! **A fingerprint, not a copy.** Holding the original as it was would be a
//! second description of a fact the original already holds, and would grow
//! the file by every linked thing twice over. The fingerprint is FNV-1a over
//! the original's serialized appearance and content, which is the same on
//! every machine and every build — a hasher seeded per process would call
//! every link out of date on the next launch.
//!
//! Single frames only: a rectangle, an ellipse, a path, a picture box, a
//! text frame. A group or a table is placed as a plain copy, because its
//! parts are frames and stories of their own that a link would have to
//! follow one by one.

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::ids::FrameId;
use crate::nodes::FrameKind;

/// One copy and the original it follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentLink {
    pub original: FrameId,
    pub copy: FrameId,
    /// The original's fingerprint when they last agreed.
    pub fingerprint: u64,
}

/// Where a linked copy stands with its original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    UpToDate,
    /// The original has changed since the copy last took it.
    Modified,
    /// The original has been deleted; the copy is on its own in all but name.
    Gone,
}

/// FNV-1a, 64-bit: the same answer for the same bytes everywhere, forever.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Whether a frame of this kind can be linked.
pub fn linkable(kind: &FrameKind) -> bool {
    matches!(
        kind,
        FrameKind::Rectangle
            | FrameKind::Ellipse
            | FrameKind::Path(_)
            | FrameKind::Graphic { .. }
            | FrameKind::Text { .. }
    )
}

impl Document {
    /// The fingerprint of `id`'s appearance and content: everything but where
    /// it stands, which a copy is allowed to differ in.
    pub fn content_fingerprint(&self, id: FrameId) -> Option<u64> {
        let mut frame = self.frames.get(id)?.clone();
        frame.transform = tessera_geometry::Transform::IDENTITY;
        frame.bounds.x = 0.0;
        frame.bounds.y = 0.0;
        frame.anchor = None;
        let story = match &frame.kind {
            FrameKind::Text { story, .. } => {
                let s = self.story(*story).cloned();
                // The story's own id is the frame's business, not its look.
                if let FrameKind::Text { story, .. } = &mut frame.kind {
                    *story = Default::default();
                }
                s
            }
            _ => None,
        };
        let mut bytes = serde_json::to_vec(&frame).ok()?;
        if let Some(story) = story {
            bytes.extend(serde_json::to_vec(&story).ok()?);
        }
        Some(fnv1a(&bytes))
    }

    /// Tie `copy` to `original`, as they are now. False when either is gone
    /// or cannot be linked; an earlier link of `copy` is replaced.
    pub fn link_content(&mut self, original: FrameId, copy: FrameId) -> bool {
        let kinds = (self.frames.get(original), self.frames.get(copy));
        let (Some(a), Some(b)) = kinds else {
            return false;
        };
        if original == copy || !linkable(&a.kind) || !linkable(&b.kind) {
            return false;
        }
        let Some(fingerprint) = self.content_fingerprint(original) else {
            return false;
        };
        self.content_links.retain(|l| l.copy != copy);
        self.content_links.push(ContentLink {
            original,
            copy,
            fingerprint,
        });
        self.touch();
        true
    }

    /// The link `copy` follows, if it is a linked copy.
    pub fn content_link_of(&self, copy: FrameId) -> Option<&ContentLink> {
        self.content_links.iter().find(|l| l.copy == copy)
    }

    /// Where `copy` stands with its original, if it is a linked copy.
    pub fn content_link_state(&self, copy: FrameId) -> Option<LinkState> {
        let link = self.content_link_of(copy)?;
        Some(match self.content_fingerprint(link.original) {
            None => LinkState::Gone,
            Some(now) if now == link.fingerprint => LinkState::UpToDate,
            Some(_) => LinkState::Modified,
        })
    }

    /// Give `copy` its original's appearance and content again, where it
    /// stands: its placement and its box's corner are kept, everything else
    /// is the original's. A text frame keeps its own story, rewritten with
    /// the original's words and formatting.
    pub fn update_linked_content(&mut self, copy: FrameId) -> bool {
        let Some(link) = self.content_link_of(copy).cloned() else {
            return false;
        };
        let (Some(original), Some(mine)) = (
            self.frames.get(link.original).cloned(),
            self.frames.get(copy).cloned(),
        ) else {
            return false;
        };
        let mut updated = original.clone();
        updated.transform = mine.transform;
        updated.bounds.x = mine.bounds.x;
        updated.bounds.y = mine.bounds.y;
        updated.anchor = mine.anchor;
        updated.hidden = mine.hidden;
        updated.locked = mine.locked;
        if let (FrameKind::Text { story: theirs, .. }, FrameKind::Text { story: own, .. }) =
            (&original.kind, &mine.kind)
        {
            let Some(words) = self.story(*theirs).cloned() else {
                return false;
            };
            if let Some(story) = self.story_mut(*own) {
                *story = words;
            }
            if let FrameKind::Text { story, .. } = &mut updated.kind {
                *story = *own;
            }
        }
        if let Some(frame) = self.frames.get_mut(copy) {
            *frame = updated;
        }
        let fingerprint = self
            .content_fingerprint(link.original)
            .unwrap_or(link.fingerprint);
        if let Some(l) = self.content_links.iter_mut().find(|l| l.copy == copy) {
            l.fingerprint = fingerprint;
        }
        self.touch();
        true
    }

    /// Make `copy` an ordinary frame again.
    pub fn unlink_content(&mut self, copy: FrameId) -> bool {
        let before = self.content_links.len();
        self.content_links.retain(|l| l.copy != copy);
        let changed = self.content_links.len() != before;
        if changed {
            self.touch();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Paint;
    use tessera_color::Color;
    use tessera_geometry::DocRect;

    fn boxed(doc: &mut Document, x: f64) -> FrameId {
        let layer = doc.default_layer().expect("a layer");
        doc.add_frame(
            layer,
            crate::nodes::Frame {
                bounds: DocRect {
                    x,
                    y: 10.0,
                    width: 50.0,
                    height: 20.0,
                },
                transform: Default::default(),
                kind: FrameKind::Rectangle,
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
                wrap: Default::default(),
                blend: crate::blending::Blending::PLAIN,
                corners: crate::corners::Corners::SQUARE,
                shadow: None,
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
                overprint: Default::default(),
            },
        )
    }

    #[test]
    fn the_fingerprint_is_fixed_across_runs() {
        // Written to the file, so it must not move between builds: FNV-1a of
        // a known string, against its published value.
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn a_copy_is_up_to_date_until_its_original_changes() {
        let mut doc = Document::new();
        let original = boxed(&mut doc, 10.0);
        let copy = boxed(&mut doc, 200.0);
        assert!(doc.link_content(original, copy));
        assert_eq!(doc.content_link_state(copy), Some(LinkState::UpToDate));

        // Moving the original is not a change to what it is.
        doc.frame_mut(original).unwrap().bounds.x = 40.0;
        assert_eq!(doc.content_link_state(copy), Some(LinkState::UpToDate));

        doc.frame_mut(original).unwrap().fill = Paint::Solid(Color::WHITE);
        assert_eq!(doc.content_link_state(copy), Some(LinkState::Modified));
    }

    #[test]
    fn updating_takes_the_original_s_look_and_keeps_the_copy_where_it_is() {
        let mut doc = Document::new();
        let original = boxed(&mut doc, 10.0);
        let copy = boxed(&mut doc, 200.0);
        doc.link_content(original, copy);
        doc.frame_mut(original).unwrap().fill = Paint::Solid(Color::WHITE);
        doc.frame_mut(original).unwrap().bounds.width = 90.0;

        assert!(doc.update_linked_content(copy));
        let f = doc.frame(copy).unwrap();
        assert_eq!(f.fill, Paint::Solid(Color::WHITE));
        assert_eq!(f.bounds.width, 90.0, "its size is the original's");
        assert_eq!(f.bounds.x, 200.0, "its place is its own");
        assert_eq!(doc.content_link_state(copy), Some(LinkState::UpToDate));
    }

    #[test]
    fn a_deleted_original_leaves_the_copy_on_its_own_and_unlinking_forgets() {
        let mut doc = Document::new();
        let original = boxed(&mut doc, 10.0);
        let copy = boxed(&mut doc, 200.0);
        doc.link_content(original, copy);
        doc.remove_frame(original);
        assert_eq!(doc.content_link_state(copy), Some(LinkState::Gone));
        assert!(doc.unlink_content(copy));
        assert_eq!(doc.content_link_state(copy), None);
    }
}

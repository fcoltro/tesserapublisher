//! What is currently selected.
//!
//! An ordered set rather than a `Vec`: order matters (the first-selected
//! frame is the alignment key in later milestones) but a frame must never
//! appear twice, which shift-clicking would otherwise cause.

use tessera_document::ids::FrameId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    frames: Vec<FrameId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn contains(&self, id: FrameId) -> bool {
        self.frames.contains(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = FrameId> + '_ {
        self.frames.iter().copied()
    }

    pub fn as_slice(&self) -> &[FrameId] {
        &self.frames
    }

    /// The one selected frame, or `None` when nothing or several are selected.
    ///
    /// Used wherever an operation is only meaningful on a single frame — the
    /// inspector's geometry fields, entering text edit — so those places
    /// cannot silently act on just the first of many.
    pub fn single(&self) -> Option<FrameId> {
        match self.frames.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The frame an operation should act on when any one will do.
    pub fn primary(&self) -> Option<FrameId> {
        self.frames.first().copied()
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// Replace the selection with exactly this frame.
    pub fn set(&mut self, id: FrameId) {
        self.frames.clear();
        self.frames.push(id);
    }

    /// Add a frame, ignoring a repeat.
    pub fn add(&mut self, id: FrameId) {
        if !self.contains(id) {
            self.frames.push(id);
        }
    }

    /// Add or remove — what shift-clicking does.
    pub fn toggle(&mut self, id: FrameId) {
        if let Some(i) = self.frames.iter().position(|f| *f == id) {
            self.frames.remove(i);
        } else {
            self.frames.push(id);
        }
    }

    pub fn remove(&mut self, id: FrameId) {
        self.frames.retain(|f| *f != id);
    }

    pub fn replace_all(&mut self, ids: impl IntoIterator<Item = FrameId>) {
        self.frames.clear();
        for id in ids {
            self.add(id);
        }
    }

    /// Drop anything no longer in the document.
    ///
    /// Undo and open replace the document wholesale, and a selection pointing
    /// at frames that no longer exist would draw handles around nothing.
    pub fn retain_existing(&mut self, document: &tessera_document::document::Document) {
        self.frames.retain(|id| document.frame(*id).is_some());
    }
}

impl FromIterator<FrameId> for Selection {
    fn from_iter<T: IntoIterator<Item = FrameId>>(iter: T) -> Self {
        let mut s = Self::default();
        for id in iter {
            s.add(id);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::document::Document;
    use tessera_document::nodes::{Frame, FrameKind};
    use tessera_geometry::DocRect;

    fn doc_with_two() -> (Document, FrameId, FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let frame = || Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            kind: FrameKind::Rectangle,
            rotation: 0.0,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        };
        let a = doc.add_frame(layer, frame());
        let b = doc.add_frame(layer, frame());
        (doc, a, b)
    }

    #[test]
    fn a_fresh_selection_is_empty() {
        let s = Selection::default();
        assert!(s.is_empty());
        assert_eq!(s.single(), None);
        assert_eq!(s.primary(), None);
    }

    #[test]
    fn setting_replaces_rather_than_adds() {
        let (_, a, b) = doc_with_two();
        let mut s = Selection::default();
        s.set(a);
        s.set(b);
        assert_eq!(s.len(), 1);
        assert_eq!(s.single(), Some(b));
    }

    #[test]
    fn adding_the_same_frame_twice_keeps_one() {
        let (_, a, _) = doc_with_two();
        let mut s = Selection::default();
        s.add(a);
        s.add(a);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn toggling_adds_then_removes() {
        let (_, a, _) = doc_with_two();
        let mut s = Selection::default();
        s.toggle(a);
        assert!(s.contains(a));
        s.toggle(a);
        assert!(!s.contains(a));
    }

    #[test]
    fn single_is_none_when_several_are_selected() {
        let (_, a, b) = doc_with_two();
        let mut s = Selection::default();
        s.add(a);
        s.add(b);
        assert_eq!(s.single(), None, "several selected is not 'one'");
        assert_eq!(s.primary(), Some(a), "but there is still a first");
    }

    #[test]
    fn order_follows_the_order_frames_were_added() {
        let (_, a, b) = doc_with_two();
        let mut s = Selection::default();
        s.add(b);
        s.add(a);
        assert_eq!(s.as_slice(), &[b, a]);
    }

    #[test]
    fn stale_frames_are_dropped_after_the_document_changes() {
        let (mut doc, a, b) = doc_with_two();
        let mut s = Selection::default();
        s.add(a);
        s.add(b);

        doc.remove_frame(a);
        s.retain_existing(&doc);

        assert_eq!(
            s.as_slice(),
            &[b],
            "a selection must not outlive the frames it points at"
        );
    }
}

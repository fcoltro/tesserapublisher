//! Snapshot-based undo.
//!
//! Decision D5. The document is a plain clonable struct (decision D1), so
//! snapshots are cheap. Snapshots also cannot develop the class of bug where
//! an inverse operation is subtly wrong — or, as happened in the previous
//! codebase, where an operation never got an inverse at all and was silently
//! not undoable.

use std::collections::VecDeque;

use crate::document::Document;

pub struct History {
    past: VecDeque<Document>,
    future: Vec<Document>,
    limit: usize,
}

impl History {
    /// `limit` bounds how many snapshots are kept. Memory-bounding on total
    /// size arrives when documents are large enough to need it.
    pub fn new(limit: usize) -> Self {
        Self {
            past: VecDeque::new(),
            future: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Call immediately *before* mutating the document.
    pub fn record(&mut self, doc: &Document) {
        self.past.push_back(doc.clone());
        while self.past.len() > self.limit {
            self.past.pop_front();
        }
        self.future.clear();
    }

    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let previous = self.past.pop_back()?;
        self.future.push(current.clone());
        Some(previous)
    }

    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let next = self.future.pop()?;
        self.past.push_back(current.clone());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::nodes::{Frame, FrameKind};
    use tessera_geometry::DocRect;

    fn frame() -> Frame {
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            kind: FrameKind::Rectangle,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn a_fresh_history_can_neither_undo_nor_redo() {
        let history = History::new(50);
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn undo_restores_the_state_before_the_change() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut history = History::new(50);

        history.record(&doc);
        doc.add_frame(layer, frame());
        assert_eq!(doc.frames.len(), 1);

        let restored = history.undo(&doc).expect("undo available");
        assert_eq!(restored.frames.len(), 0);
    }

    #[test]
    fn redo_reapplies_what_undo_took_away() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut history = History::new(50);

        history.record(&doc);
        doc.add_frame(layer, frame());
        let undone = history.undo(&doc).expect("undo");
        let redone = history.redo(&undone).expect("redo");

        assert_eq!(redone.frames.len(), 1);
    }

    #[test]
    fn a_new_change_discards_the_redo_stack() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut history = History::new(50);

        history.record(&doc);
        doc.add_frame(layer, frame());
        let mut doc = history.undo(&doc).expect("undo");
        assert!(history.can_redo());

        history.record(&doc);
        doc.add_frame(layer, frame());
        assert!(!history.can_redo());
    }

    #[test]
    fn the_stack_is_bounded() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut history = History::new(3);
        for _ in 0..10 {
            history.record(&doc);
            doc.add_frame(layer, frame());
        }
        assert_eq!(history.undo_depth(), 3);
    }

    #[test]
    fn removing_a_frame_is_undoable_too() {
        // The previous codebase never made add-page or remove-page undoable,
        // because nobody wrote their inverses. Snapshots remove that whole
        // failure mode: there is no inverse to forget.
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, frame());
        let mut history = History::new(50);

        history.record(&doc);
        doc.remove_frame(id);
        assert!(doc.frame(id).is_none());

        let restored = history.undo(&doc).expect("undo");
        assert!(
            restored.frame(id).is_some(),
            "the removed frame must come back, under its original id"
        );
    }
}

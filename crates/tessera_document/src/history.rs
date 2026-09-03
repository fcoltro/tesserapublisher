//! Snapshot-based undo.
//!
//! Decision D5. The document is a plain clonable struct (decision D1), so
//! snapshots are cheap. Snapshots also cannot develop the class of bug where
//! an inverse operation is subtly wrong — or, as happened in the previous
//! codebase, where an operation never got an inverse at all and was silently
//! not undoable.
//!
//! What snapshots cost is memory, and a count alone does not bound it: two
//! hundred snapshots of a two-page flyer is nothing, and two hundred of a
//! two-hundred-page catalogue is not. So the stack is bounded by **both** a
//! count and an estimated total size, and the oldest entries go first.
//!
//! One entry is always kept, however large. An undo stack that refused to hold
//! even the last step would be worse than one that used too much memory.

use std::collections::VecDeque;

use crate::document::Document;

/// How much the undo stack may hold before the oldest entries are dropped.
///
/// Generous enough that ordinary work never reaches it, small enough that a
/// large document cannot fill memory with its own history.
pub const DEFAULT_BUDGET_BYTES: usize = 256 * 1024 * 1024;

pub struct History {
    /// Each snapshot with its estimated size, so trimming does not have to
    /// re-measure documents it is about to throw away.
    past: VecDeque<(Document, usize)>,
    future: Vec<Document>,
    limit: usize,
    budget: usize,
    held: usize,
}

impl History {
    /// `limit` bounds how many snapshots are kept, and
    /// [`DEFAULT_BUDGET_BYTES`] how much they may weigh between them.
    pub fn new(limit: usize) -> Self {
        Self::with_budget(limit, DEFAULT_BUDGET_BYTES)
    }

    pub fn with_budget(limit: usize, budget: usize) -> Self {
        Self {
            past: VecDeque::new(),
            future: Vec::new(),
            limit: limit.max(1),
            budget,
            held: 0,
        }
    }

    /// Call immediately *before* mutating the document.
    pub fn record(&mut self, doc: &Document) {
        let size = doc.footprint();
        self.past.push_back((doc.clone(), size));
        self.held += size;
        self.trim();
        self.future.clear();
    }

    /// Drop the oldest entries until the stack is within both bounds.
    fn trim(&mut self) {
        while self.past.len() > self.limit || (self.held > self.budget && self.past.len() > 1) {
            match self.past.pop_front() {
                Some((_, size)) => self.held = self.held.saturating_sub(size),
                None => break,
            }
        }
    }

    /// The estimated size of everything the stack is holding, in bytes.
    pub fn held_bytes(&self) -> usize {
        self.held
    }

    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let (previous, size) = self.past.pop_back()?;
        self.held = self.held.saturating_sub(size);
        self.future.push(current.clone());
        Some(previous)
    }

    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let next = self.future.pop()?;
        let size = current.footprint();
        self.past.push_back((current.clone(), size));
        self.held += size;
        self.trim();
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
    use tessera_geometry::{DocRect, Transform};

    fn frame() -> Frame {
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn a_document_reports_a_footprint_that_grows_with_its_contents() {
        let mut doc = Document::new();
        let empty = doc.footprint();
        let layer = doc.default_layer().expect("layer");
        for _ in 0..50 {
            doc.add_frame(layer, frame());
        }
        assert!(
            doc.footprint() > empty,
            "fifty frames should weigh more than none"
        );
    }

    #[test]
    fn text_counts_towards_the_footprint() {
        // Text is the part that really scales in a page-layout document, so a
        // bound that ignored it would not bound anything that matters.
        let mut doc = Document::new();
        let before = doc.footprint();
        doc.add_story(tessera_text::story::Story::new("x".repeat(10_000)));
        assert!(doc.footprint() >= before + 10_000);
    }

    #[test]
    fn the_stack_stops_growing_once_it_reaches_its_budget() {
        // The reason this exists: two hundred snapshots of a two-hundred-page
        // catalogue is not the same amount of memory as two hundred of a
        // flyer, and a count alone cannot tell them apart.
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        for _ in 0..200 {
            doc.add_frame(layer, frame());
        }

        let budget = doc.footprint() * 4;
        let mut history = History::with_budget(100, budget);
        for _ in 0..50 {
            history.record(&doc);
        }

        assert!(
            history.held_bytes() <= budget,
            "held {} against a budget of {budget}",
            history.held_bytes()
        );
        assert!(
            history.undo_depth() < 50,
            "the count limit alone would have kept all fifty"
        );
    }

    #[test]
    fn one_step_is_always_kept_however_big_the_document() {
        // Refusing to hold even the last step would be worse than using too
        // much memory.
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        for _ in 0..100 {
            doc.add_frame(layer, frame());
        }

        let mut history = History::with_budget(100, 1);
        history.record(&doc);
        history.record(&doc);

        assert_eq!(history.undo_depth(), 1);
        assert!(history.can_undo());
    }

    #[test]
    fn a_small_document_still_gets_the_full_count() {
        let doc = Document::new();
        let mut history = History::with_budget(10, super::DEFAULT_BUDGET_BYTES);
        for _ in 0..25 {
            history.record(&doc);
        }
        assert_eq!(history.undo_depth(), 10, "the count bound still applies");
    }

    #[test]
    fn undoing_gives_the_memory_back() {
        let doc = Document::new();
        let mut history = History::new(10);
        history.record(&doc);
        let held = history.held_bytes();
        assert!(held > 0);
        let _ = history.undo(&doc);
        assert!(
            history.held_bytes() < held,
            "the popped snapshot should stop counting"
        );
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

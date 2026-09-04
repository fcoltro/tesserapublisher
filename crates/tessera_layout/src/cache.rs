//! Resolving only when the document has actually changed.
//!
//! [`crate::resolve::resolve`] walks every frame and lays out every story. The
//! viewport needs its output on every painted frame — sixty times a second,
//! whether or not anything moved — and running it that often is the difference
//! between an application that stays responsive with a real document in it and
//! one that does not.
//!
//! [`Document`] already bumps a revision counter on every mutation, which is
//! exactly the question this needs answered: *has anything changed?* So the
//! cache is that counter and the last answer.
//!
//! Note what this does **not** cover. Panning and zooming do not touch the
//! document, so they resolve nothing — but they do rebuild the Vello scene,
//! because the camera is baked into it. Culling to the visible spread is the
//! next thing to do here, and needs page structure that does not exist yet.

use tessera_document::document::Document;
use tessera_text::shape::Shaper;

use crate::resolve::{self, ResolvedDocument};

#[derive(Debug, Default)]
pub struct ResolveCache {
    /// The revision the held answer was resolved from.
    at: Option<u64>,
    resolved: ResolvedDocument,
    resolves: u64,
}

impl ResolveCache {
    /// The resolved document, resolving it first if the document has moved on.
    pub fn get(&mut self, document: &Document, shaper: &mut Shaper) -> &ResolvedDocument {
        let revision = document.revision();
        if self.at != Some(revision) {
            self.resolved = resolve::resolve(document, shaper);
            self.at = Some(revision);
            self.resolves += 1;
        }
        &self.resolved
    }

    /// How many times the document has really been resolved. For tests, and
    /// for a diagnostics panel later.
    pub fn resolves(&self) -> u64 {
        self.resolves
    }

    /// Throw the held answer away.
    ///
    /// For when something *outside* the document changes what resolving would
    /// produce — a font becoming available, say. Nothing does that yet, which
    /// is why it is worth having one obvious place to reach for when something
    /// does, rather than discovering the cache has gone stale.
    pub fn invalidate(&mut self) {
        self.at = None;
    }
}

#[cfg(test)]
mod tests {
    /// The regression this cache once caused.
    ///
    /// The page rectangles travel inside the resolved document, so a change
    /// to the page setup has to invalidate the cache like any other. It did
    /// not: `setup` was written straight into the field and the revision
    /// never moved, so the canvas kept drawing the old margins until
    /// something else — moving an object — happened to bump the counter.
    ///
    /// Found by using the application, which is what the hand checks are for.
    #[test]
    fn changing_the_page_setup_invalidates_the_cache() {
        use tessera_document::nodes::Margins;

        let mut doc = Document::new();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();

        let before = cache.get(&doc, &mut shaper).pages[0].margins;

        let mut setup = doc.setup;
        setup.margins = Margins::uniform(36.0);
        doc.set_setup(setup);

        let after = cache.get(&doc, &mut shaper).pages[0].margins;
        assert_ne!(
            before, after,
            "the cache handed back the margins from before the change"
        );
        assert_eq!(cache.resolves(), 2, "and it really did resolve again");
    }

    use super::*;
    use tessera_color::Color;
    use tessera_document::nodes::{Frame, FrameKind};
    use tessera_geometry::{DocRect, Transform};

    fn document_with_a_frame() -> (Document, tessera_document::ids::FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                kind: FrameKind::Rectangle,
                transform: Transform::IDENTITY,
                fill: Color::BLACK,
                stroke: None,
            },
        );
        (doc, id)
    }

    #[test]
    fn an_unchanged_document_is_resolved_once_however_often_it_is_asked_for() {
        // The whole point: a still canvas repainting at sixty frames a second
        // must not re-lay-out the document sixty times a second.
        let (doc, _) = document_with_a_frame();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();

        for _ in 0..10 {
            let _ = cache.get(&doc, &mut shaper);
        }
        assert_eq!(cache.resolves(), 1);
    }

    #[test]
    fn changing_the_document_resolves_it_again() {
        let (mut doc, id) = document_with_a_frame();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();

        let _ = cache.get(&doc, &mut shaper);
        doc.frame_mut(id).expect("frame").bounds.width = 200.0;
        let _ = cache.get(&doc, &mut shaper);

        assert_eq!(cache.resolves(), 2);
    }

    #[test]
    fn the_answer_it_hands_back_is_the_current_one() {
        // A cache that returned a stale document would be worse than no cache.
        let (mut doc, id) = document_with_a_frame();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();

        assert_eq!(cache.get(&doc, &mut shaper).items[0].bounds.width, 100.0);
        doc.frame_mut(id).expect("frame").bounds.width = 200.0;
        assert_eq!(cache.get(&doc, &mut shaper).items[0].bounds.width, 200.0);
    }

    #[test]
    fn a_document_that_loses_a_frame_loses_it_from_the_answer_too() {
        let (mut doc, id) = document_with_a_frame();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();

        assert_eq!(cache.get(&doc, &mut shaper).items.len(), 1);
        doc.remove_frame(id);
        assert!(cache.get(&doc, &mut shaper).items.is_empty());
    }

    #[test]
    fn invalidating_forces_one_more_resolve() {
        let (doc, _) = document_with_a_frame();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();

        let _ = cache.get(&doc, &mut shaper);
        let _ = cache.get(&doc, &mut shaper);
        assert_eq!(cache.resolves(), 1);

        cache.invalidate();
        let _ = cache.get(&doc, &mut shaper);
        assert_eq!(cache.resolves(), 2);
    }

    #[test]
    fn a_fresh_cache_resolves_an_empty_document_rather_than_returning_nothing() {
        // Revision zero is a real revision, not "never resolved".
        let doc = Document::new();
        let mut shaper = Shaper::new();
        let mut cache = ResolveCache::default();
        assert_eq!(cache.resolves(), 0);
        let _ = cache.get(&doc, &mut shaper);
        assert_eq!(
            cache.resolves(),
            1,
            "revision 0 must still be resolved once"
        );
    }
}

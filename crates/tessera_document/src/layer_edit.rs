//! Objects hidden, locked and arranged one at a time, as the Layers panel
//! shows them: under their layer, in front-to-back order.

use crate::{
    document::Document,
    ids::{FrameId, LayerId},
};

impl Document {
    /// Hide objects, or show them again. How many changed.
    pub fn set_frames_hidden(&mut self, ids: &[FrameId], hidden: bool) -> usize {
        self.set_frame_flag(ids, |frame| &mut frame.hidden, hidden)
    }

    /// Lock objects, or unlock them. How many changed.
    pub fn set_frames_locked(&mut self, ids: &[FrameId], locked: bool) -> usize {
        self.set_frame_flag(ids, |frame| &mut frame.locked, locked)
    }

    fn set_frame_flag(
        &mut self,
        ids: &[FrameId],
        flag: impl Fn(&mut crate::nodes::Frame) -> &mut bool,
        to: bool,
    ) -> usize {
        let mut changed = 0;
        for id in ids {
            if let Some(frame) = self.frames.get_mut(*id) {
                let at = flag(frame);
                if *at != to {
                    *at = to;
                    changed += 1;
                }
            }
        }
        if changed > 0 {
            self.touch();
        }
        changed
    }

    /// Put objects on `layer`, together and in the order they are drawn in
    /// now, so they stand at `index` in its back-to-front list — counted in
    /// the list as it is before they move, as a drop marker is drawn.
    ///
    /// One operation for both of a Layers panel's drags: along a layer's
    /// list is a change of what is in front of what, onto another layer is
    /// a change of layer as well, and nothing moves on the page either way.
    /// Only objects a layer holds directly can go: a group's members go with
    /// their group.
    ///
    /// Whether anything changed.
    pub fn arrange_frames(&mut self, ids: &[FrameId], layer: LayerId, index: usize) -> bool {
        if !self.layers.contains_key(layer) {
            return false;
        }
        // In the order they are drawn now, across every layer.
        let moving: Vec<FrameId> = self
            .layer_ids()
            .filter_map(|l| self.layers.get(l))
            .flat_map(|l| l.frames.iter().copied())
            .filter(|f| ids.contains(f))
            .collect();
        if moving.is_empty() {
            return false;
        }
        let was: Vec<(LayerId, Vec<FrameId>)> = self
            .layer_ids()
            .map(|l| (l, self.layers[l].frames.clone()))
            .collect();
        let taken = self.layers[layer]
            .frames
            .iter()
            .take(index)
            .filter(|f| moving.contains(f))
            .count();
        for (id, _) in &was {
            if let Some(l) = self.layers.get_mut(*id) {
                l.frames.retain(|f| !moving.contains(f));
            }
        }
        let target = &mut self.layers[layer].frames;
        let at = index.saturating_sub(taken).min(target.len());
        target.splice(at..at, moving);
        if was
            .iter()
            .all(|(l, frames)| self.layers[*l].frames == *frames)
        {
            // Put back exactly where they were: nothing to record.
            return false;
        }
        self.touch();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{Frame, FrameKind};

    fn a_frame() -> Frame {
        Frame {
            bounds: tessera_geometry::DocRect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            kind: FrameKind::Rectangle,
            transform: tessera_geometry::Transform::IDENTITY,
            fill: crate::paint::Paint::Solid(tessera_color::Color::BLACK),
            stroke: None,
            wrap: crate::nodes::TextWrap::None,
            blend: crate::blending::Blending::PLAIN,
            corners: crate::corners::Corners::SQUARE,
            shadow: None,
            anchor: None,
            style: None,
            hidden: false,
            locked: false,
        }
    }

    /// A document with two layers, three frames on the first.
    fn a_document() -> (Document, LayerId, LayerId, Vec<FrameId>) {
        let mut doc = Document::new();
        let first = doc.default_layer().expect("a layer");
        let frames = (0..3).map(|_| doc.add_frame(first, a_frame())).collect();
        let second = doc.add_layer("Second");
        (doc, first, second, frames)
    }

    #[test]
    fn a_hidden_object_is_not_drawn_touched_or_wrapped_round() {
        let (mut doc, _, _, frames) = a_document();
        assert_eq!(doc.set_frames_hidden(&frames[1..2], true), 1);
        assert!(!doc.top_level_order().contains(&frames[1]), "not drawn");
        assert!(!doc.paint_order().contains(&frames[1]));
        assert!(!doc.selectable_order().contains(&frames[1]), "not touched");
        assert_eq!(
            doc.set_frames_hidden(&frames[1..2], true),
            0,
            "already hidden"
        );
        doc.set_frames_hidden(&frames, false);
        assert_eq!(doc.top_level_order(), frames);
    }

    #[test]
    fn a_locked_object_is_drawn_but_not_touched() {
        let (mut doc, _, _, frames) = a_document();
        doc.set_frames_locked(&frames[..1], true);
        assert!(doc.top_level_order().contains(&frames[0]), "drawn");
        assert!(!doc.selectable_order().contains(&frames[0]), "not touched");
    }

    #[test]
    fn a_document_with_nothing_hidden_is_written_as_before() {
        // The flags are left out when false, so every file written before
        // them reads — and writes — the same.
        let (mut doc, _, _, frames) = a_document();
        let plain = serde_json::to_string(&doc.frames[frames[0]]).expect("json");
        assert!(!plain.contains("hidden") && !plain.contains("locked"));
        doc.set_frames_locked(&frames[..1], true);
        let locked = serde_json::to_string(&doc.frames[frames[0]]).expect("json");
        assert!(locked.contains("\"locked\":true"));
        let back: Frame = serde_json::from_str(&plain).expect("read back");
        assert!(!back.hidden && !back.locked);
    }

    #[test]
    fn objects_move_along_their_layer_as_a_block() {
        let (mut doc, first, _, f) = a_document();
        // The back one to the front: index 3 is past the end.
        assert!(doc.arrange_frames(&[f[0]], first, 3));
        assert_eq!(doc.layers[first].frames, [f[1], f[2], f[0]]);
        // Two of them to the back, in the order they were drawn.
        assert!(doc.arrange_frames(&[f[0], f[2]], first, 0));
        assert_eq!(doc.layers[first].frames, [f[2], f[0], f[1]]);
        assert!(!doc.arrange_frames(&[f[2]], first, 0), "already there");
        // Into the gap before the third, counted before it moves: the one
        // taken out of the list ahead of the gap closes it up by one.
        assert!(doc.arrange_frames(&[f[2]], first, 2));
        assert_eq!(doc.layers[first].frames, [f[0], f[2], f[1]]);
    }

    #[test]
    fn objects_dropped_on_another_layer_go_to_it_where_they_are_dropped() {
        let (mut doc, first, second, f) = a_document();
        let there = doc.add_frame(second, a_frame());
        let before = doc.frames[f[1]].bounds;
        assert!(doc.arrange_frames(&[f[1]], second, 1));
        assert_eq!(doc.layers[first].frames, [f[0], f[2]]);
        assert_eq!(doc.layers[second].frames, [there, f[1]]);
        assert_eq!(doc.frames[f[1]].bounds, before, "nothing moves on the page");
    }
}

//! The document: arenas of nodes, addressed by typed key.

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use tessera_geometry::{DocPoint, DocRect};

use crate::ids::{FrameId, LayerId, PageId, SpreadId, StoryId};
use crate::nodes::{Frame, Layer, Page, Spread};
use tessera_text::story::Story;

/// Stories are addressed by id and live at the document level, so a threaded
/// story flows through many frames while existing exactly once.
pub type StoryMap = slotmap::SlotMap<StoryId, Story>;

/// Where a frame should move within its layer's paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZMove {
    Forward,
    Backward,
    ToFront,
    ToBack,
}

/// US Letter in points — the default new-document size.
const DEFAULT_PAGE: DocRect = DocRect {
    x: 0.0,
    y: 0.0,
    width: 612.0,
    height: 792.0,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub frames: SlotMap<FrameId, Frame>,
    pub layers: SlotMap<LayerId, Layer>,
    pub pages: SlotMap<PageId, Page>,
    pub spreads: SlotMap<SpreadId, Spread>,
    /// Text content, referenced by `FrameKind::Text`. **Part of the document,
    /// so it is saved with it** — text held anywhere else would vanish on
    /// save, which is precisely the class of bug this rebuild exists to fix.
    pub stories: StoryMap,
    /// Spread paint and navigation order.
    pub spread_order: Vec<SpreadId>,
    /// Bumped on every mutation. The renderer rebuilds its scene only when
    /// this moves, so panning the camera does not rebuild anything.
    ///
    /// Not serialized: a document loaded from disk starts fresh at zero, and
    /// a revision counter carried across sessions would mean nothing.
    #[serde(skip)]
    revision: u64,
}

impl Document {
    pub fn new() -> Self {
        let mut doc = Self {
            frames: SlotMap::with_key(),
            layers: SlotMap::with_key(),
            pages: SlotMap::with_key(),
            spreads: SlotMap::with_key(),
            stories: StoryMap::with_key(),
            spread_order: Vec::new(),
            revision: 0,
        };

        let layer = doc.layers.insert(Layer {
            name: "Layer 1".to_string(),
            visible: true,
            locked: false,
            frames: Vec::new(),
        });
        let page = doc.pages.insert(Page {
            bounds: DEFAULT_PAGE,
            layers: vec![layer],
        });
        let spread = doc.spreads.insert(Spread { pages: vec![page] });
        doc.spread_order.push(spread);

        doc
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn spread_ids(&self) -> impl Iterator<Item = SpreadId> + '_ {
        self.spread_order.iter().copied()
    }

    pub fn page_ids(&self) -> impl Iterator<Item = PageId> + '_ {
        self.spread_ids()
            .filter_map(|s| self.spreads.get(s))
            .flat_map(|s| s.pages.iter().copied())
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.page_ids()
            .filter_map(|p| self.pages.get(p))
            .flat_map(|p| p.layers.iter().copied())
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// The layer new frames go onto. `None` only for a document whose pages
    /// have all been removed, which milestone 0 cannot produce.
    pub fn default_layer(&self) -> Option<LayerId> {
        self.layer_ids().next()
    }

    pub fn first_page_bounds(&self) -> DocRect {
        self.page_ids()
            .next()
            .and_then(|p| self.pages.get(p))
            .map_or(DEFAULT_PAGE, |p| p.bounds)
    }

    pub fn add_frame(&mut self, layer: LayerId, frame: Frame) -> FrameId {
        let id = self.frames.insert(frame);
        if let Some(l) = self.layers.get_mut(layer) {
            l.frames.push(id);
        }
        self.revision += 1;
        id
    }

    pub fn remove_frame(&mut self, id: FrameId) {
        self.frames.remove(id);
        for layer in self.layers.values_mut() {
            layer.frames.retain(|f| *f != id);
        }
        self.revision += 1;
    }

    pub fn add_story(&mut self, story: Story) -> StoryId {
        self.revision += 1;
        self.stories.insert(story)
    }

    pub fn story(&self, id: StoryId) -> Option<&Story> {
        self.stories.get(id)
    }

    /// Bumps the revision on the assumption the caller mutates.
    pub fn story_mut(&mut self, id: StoryId) -> Option<&mut Story> {
        self.revision += 1;
        self.stories.get_mut(id)
    }

    pub fn frame(&self, id: FrameId) -> Option<&Frame> {
        self.frames.get(id)
    }

    /// Bumps the revision on the assumption the caller mutates. Callers that
    /// only want to read must use [`Document::frame`].
    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        self.revision += 1;
        self.frames.get_mut(id)
    }

    /// Move a frame within its layer's paint order.
    ///
    /// Order lives in `Layer::frames` rather than in a z-index field on the
    /// frame, so "in front of" is a property of the list and cannot fall out
    /// of sync with itself.
    pub fn move_in_z(&mut self, id: FrameId, how: ZMove) -> bool {
        let Some(layer) = self.layers.values_mut().find(|l| l.frames.contains(&id)) else {
            return false;
        };
        let Some(from) = layer.frames.iter().position(|f| *f == id) else {
            return false;
        };
        let last = layer.frames.len() - 1;
        let to = match how {
            ZMove::Forward => (from + 1).min(last),
            ZMove::Backward => from.saturating_sub(1),
            ZMove::ToFront => last,
            ZMove::ToBack => 0,
        };
        if to == from {
            return false;
        }
        let frame = layer.frames.remove(from);
        layer.frames.insert(to, frame);
        self.revision += 1;
        true
    }

    /// Back-to-front paint order across every visible layer.
    pub fn paint_order(&self) -> Vec<FrameId> {
        self.layer_ids()
            .filter_map(|l| self.layers.get(l))
            .filter(|l| l.visible)
            .flat_map(|l| l.frames.iter().copied())
            .collect()
    }

    /// The topmost frame containing the point, or `None`.
    pub fn hit_test(&self, point: DocPoint) -> Option<FrameId> {
        self.paint_order().into_iter().rev().find(|id| {
            self.frames
                .get(*id)
                .is_some_and(|f| f.bounds.contains(point))
        })
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{Frame, FrameKind};
    use tessera_color::Color;
    use tessera_geometry::{DocPoint, DocRect};

    fn rect_frame() -> Frame {
        Frame {
            bounds: DocRect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
            },
            kind: FrameKind::Rectangle,
            fill: Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn a_new_document_has_one_spread_with_one_page_and_one_layer() {
        let doc = Document::new();
        assert_eq!(doc.spread_ids().count(), 1);
        assert_eq!(doc.page_ids().count(), 1);
        assert_eq!(doc.layer_ids().count(), 1);
    }

    #[test]
    fn a_new_document_is_us_letter_in_points() {
        let doc = Document::new();
        let page = doc.first_page_bounds();
        assert_eq!((page.width, page.height), (612.0, 792.0));
    }

    #[test]
    fn an_added_frame_can_be_read_back() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.frame(id).expect("frame exists").bounds.width, 100.0);
    }

    #[test]
    fn adding_a_frame_advances_the_revision() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let before = doc.revision();
        doc.add_frame(layer, rect_frame());
        assert!(doc.revision() > before);
    }

    #[test]
    fn a_removed_frame_is_gone_from_the_arena_and_from_its_layer() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        doc.remove_frame(id);
        assert!(doc.frame(id).is_none());
        assert!(doc.paint_order().is_empty());
    }

    #[test]
    fn hit_test_finds_a_frame_under_the_point_and_nothing_outside_it() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 5.0, y: 5.0 }), None);
    }

    #[test]
    fn hit_test_returns_the_topmost_frame() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let _under = doc.add_frame(layer, rect_frame());
        let over = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }), Some(over));
    }

    #[test]
    fn a_hidden_layer_contributes_nothing_to_paint_order() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        doc.add_frame(layer, rect_frame());
        doc.layers.get_mut(layer).expect("layer").visible = false;
        assert!(doc.paint_order().is_empty());
    }

    #[test]
    fn a_hidden_layer_cannot_be_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        doc.add_frame(layer, rect_frame());
        doc.layers.get_mut(layer).expect("layer").visible = false;
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }), None);
    }

    #[test]
    fn bringing_a_frame_forward_swaps_it_with_the_one_above() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, rect_frame());
        let b = doc.add_frame(layer, rect_frame());

        assert!(doc.move_in_z(a, ZMove::Forward));

        assert_eq!(doc.paint_order(), vec![b, a]);
    }

    #[test]
    fn sending_to_back_puts_a_frame_first_in_paint_order() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, rect_frame());
        let b = doc.add_frame(layer, rect_frame());
        let c = doc.add_frame(layer, rect_frame());

        assert!(doc.move_in_z(c, ZMove::ToBack));

        assert_eq!(doc.paint_order(), vec![c, a, b]);
    }

    #[test]
    fn moving_the_frontmost_frame_forward_changes_nothing() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, rect_frame());
        let b = doc.add_frame(layer, rect_frame());

        assert!(!doc.move_in_z(b, ZMove::Forward), "already at the front");

        assert_eq!(doc.paint_order(), vec![a, b]);
    }

    #[test]
    fn z_order_decides_which_frame_a_click_finds() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let under = doc.add_frame(layer, rect_frame());
        let over = doc.add_frame(layer, rect_frame());
        let point = DocPoint { x: 50.0, y: 40.0 };

        assert_eq!(doc.hit_test(point), Some(over));
        doc.move_in_z(over, ZMove::ToBack);
        assert_eq!(
            doc.hit_test(point),
            Some(under),
            "the stack really reordered"
        );
    }

    #[test]
    fn moving_an_unknown_frame_reports_failure_rather_than_panicking() {
        let mut doc = Document::new();
        assert!(!doc.move_in_z(FrameId::default(), ZMove::ToFront));
    }
}

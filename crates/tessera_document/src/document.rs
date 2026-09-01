//! The document: arenas of nodes, addressed by typed key.

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use tessera_geometry::{DocPoint, DocRect};

use crate::ids::{FrameId, LayerId, PageId, SpreadId, StoryId};
use crate::nodes::{Frame, FrameKind, Layer, Page, Spread};
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

    /// Remove a frame, and everything inside it if it is a group.
    pub fn remove_frame(&mut self, id: FrameId) {
        for victim in self.descendants(id) {
            self.frames.remove(victim);
            for layer in self.layers.values_mut() {
                layer.frames.retain(|f| *f != victim);
            }
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

    /// Back-to-front order of the frames a layer owns directly.
    ///
    /// Groups appear as themselves here, not as their children — this is the
    /// order z-moves and hit-testing work in.
    pub fn top_level_order(&self) -> Vec<FrameId> {
        self.layer_ids()
            .filter_map(|l| self.layers.get(l))
            .filter(|l| l.visible)
            .flat_map(|l| l.frames.iter().copied())
            .collect()
    }

    /// Back-to-front paint order, with groups expanded into their children.
    ///
    /// A group has no appearance of its own, so it never appears here; only
    /// the leaves that actually draw do.
    pub fn paint_order(&self) -> Vec<FrameId> {
        let mut out = Vec::new();
        for id in self.top_level_order() {
            self.push_leaves(id, &mut out);
        }
        out
    }

    fn push_leaves(&self, id: FrameId, out: &mut Vec<FrameId>) {
        match self.frames.get(id).map(|f| &f.kind) {
            Some(FrameKind::Group(children)) => {
                for child in children.clone() {
                    self.push_leaves(child, out);
                }
            }
            Some(_) => out.push(id),
            None => {}
        }
    }

    /// Every frame inside `id`, including `id` itself.
    pub fn descendants(&self, id: FrameId) -> Vec<FrameId> {
        let mut out = vec![id];
        if let Some(FrameKind::Group(children)) = self.frames.get(id).map(|f| &f.kind) {
            for child in children.clone() {
                out.extend(self.descendants(child));
            }
        }
        out
    }

    /// The bounds a frame occupies — for a group, the union of its children.
    pub fn effective_bounds(&self, id: FrameId) -> Option<DocRect> {
        let frame = self.frames.get(id)?;
        let FrameKind::Group(children) = &frame.kind else {
            return Some(frame.bounds);
        };
        let mut union: Option<DocRect> = None;
        for child in children {
            let Some(b) = self.effective_bounds(*child) else {
                continue;
            };
            union = Some(match union {
                None => b,
                Some(u) => {
                    let x0 = u.x.min(b.x);
                    let y0 = u.y.min(b.y);
                    let x1 = (u.x + u.width).max(b.x + b.width);
                    let y1 = (u.y + u.height).max(b.y + b.height);
                    DocRect {
                        x: x0,
                        y: y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    }
                }
            });
        }
        union
    }

    /// Move a frame, carrying a group's children with it.
    pub fn translate_frame(&mut self, id: FrameId, dx: f64, dy: f64) {
        for leaf in self.descendants(id) {
            if let Some(f) = self.frames.get_mut(leaf) {
                f.bounds.x += dx;
                f.bounds.y += dy;
            }
        }
        self.revision += 1;
    }

    /// Collect `ids` into a new group, inserted where the frontmost of them
    /// sat. Returns `None` for fewer than two frames — a group of one is not
    /// a group.
    pub fn group(&mut self, ids: &[FrameId]) -> Option<FrameId> {
        let order = self.top_level_order();
        let mut members: Vec<FrameId> = order
            .iter()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        if members.len() < 2 {
            return None;
        }

        let layer_id = self.layer_ids().find(|l| {
            self.layers
                .get(*l)
                .is_some_and(|layer| layer.frames.contains(&members[0]))
        })?;
        let position = self
            .layers
            .get(layer_id)?
            .frames
            .iter()
            .position(|f| Some(f) == members.last())?;

        let bounds = members
            .iter()
            .filter_map(|id| self.effective_bounds(*id))
            .fold(None::<DocRect>, |acc, b| {
                Some(match acc {
                    None => b,
                    Some(u) => {
                        let x0 = u.x.min(b.x);
                        let y0 = u.y.min(b.y);
                        let x1 = (u.x + u.width).max(b.x + b.width);
                        let y1 = (u.y + u.height).max(b.y + b.height);
                        DocRect {
                            x: x0,
                            y: y0,
                            width: x1 - x0,
                            height: y1 - y0,
                        }
                    }
                })
            })?;

        let group = self.frames.insert(Frame {
            bounds,
            kind: FrameKind::Group(std::mem::take(&mut members)),
            rotation: 0.0,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        });

        let layer = self.layers.get_mut(layer_id)?;
        // Children leave the layer's list; the group takes their place, at
        // the frontmost member's position so the stack does not jump.
        let FrameKind::Group(children) = &self.frames[group].kind else {
            unreachable!("just inserted a group")
        };
        let children = children.clone();
        // The insertion index must be computed against the list AS IT WILL BE
        // once the children are gone, not as it is now: removing them shifts
        // everything after them down.
        let at = layer
            .frames
            .iter()
            .take(position)
            .filter(|f| !children.contains(f))
            .count();
        layer.frames.retain(|f| !children.contains(f));
        layer.frames.insert(at.min(layer.frames.len()), group);

        self.revision += 1;
        Some(group)
    }

    /// Dissolve a group, returning its children to the layer in its place.
    pub fn ungroup(&mut self, id: FrameId) -> Vec<FrameId> {
        let Some(FrameKind::Group(children)) = self.frames.get(id).map(|f| f.kind.clone()) else {
            return Vec::new();
        };

        let Some(layer_id) = self.layer_ids().find(|l| {
            self.layers
                .get(*l)
                .is_some_and(|layer| layer.frames.contains(&id))
        }) else {
            return Vec::new();
        };

        if let Some(layer) = self.layers.get_mut(layer_id)
            && let Some(at) = layer.frames.iter().position(|f| *f == id)
        {
            layer.frames.remove(at);
            for (offset, child) in children.iter().enumerate() {
                layer.frames.insert(at + offset, *child);
            }
        }

        self.frames.remove(id);
        self.revision += 1;
        children
    }

    /// The topmost frame containing the point, or `None`.
    ///
    /// A rotated frame is tested by rotating the *point* backwards into the
    /// frame's own space, rather than by building a rotated polygon. Same
    /// answer, one line, and it stays correct for any future transform that
    /// is invertible.
    pub fn hit_test(&self, point: DocPoint) -> Option<FrameId> {
        // Top level, not paint order: clicking a grouped object selects the
        // GROUP, which is what grouping is for.
        self.top_level_order()
            .into_iter()
            .rev()
            .find(|id| self.hits_anywhere(*id, point))
    }

    fn hits_anywhere(&self, id: FrameId, point: DocPoint) -> bool {
        self.descendants(id).into_iter().any(|leaf| {
            self.frames
                .get(leaf)
                .is_some_and(|f| !matches!(f.kind, FrameKind::Group(_)) && hits(f, point))
        })
    }
}

/// Whether `point` falls inside `frame`, accounting for its rotation.
fn hits(frame: &Frame, point: DocPoint) -> bool {
    let local = point.rotated_about(frame.bounds.center(), -frame.rotation);
    frame.bounds.contains(local)
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
            rotation: 0.0,
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

    /// A long thin bar, so rotating it moves real area around.
    fn bar() -> Frame {
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 45.0,
                width: 100.0,
                height: 10.0,
            },
            kind: FrameKind::Rectangle,
            rotation: 0.0,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn an_unrotated_frame_hit_tests_as_before() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 10.0 }), None);
    }

    #[test]
    fn rotating_a_frame_moves_where_it_can_be_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());

        // Upright, the bar spans x 0..100 at y 45..55, centred on (50, 50).
        // Turned a quarter turn it spans y 0..100 at x 45..55.
        doc.frames.get_mut(id).expect("frame").rotation = 90.0;

        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 10.0 }),
            Some(id),
            "the bar now reaches up the page"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 10.0, y: 50.0 }),
            None,
            "and no longer reaches across it"
        );
    }

    #[test]
    fn the_centre_of_a_rotated_frame_is_always_a_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());
        for angle in [0.0, 17.0, 45.0, 90.0, 180.0, -33.0] {
            doc.frames.get_mut(id).expect("frame").rotation = angle;
            assert_eq!(
                doc.hit_test(DocPoint { x: 50.0, y: 50.0 }),
                Some(id),
                "rotation {angle} lost its own centre"
            );
        }
    }

    /// A rectangle at a given position, 20x20.
    fn at(x: f64, y: f64) -> Frame {
        Frame {
            bounds: DocRect {
                x,
                y,
                width: 20.0,
                height: 20.0,
            },
            kind: FrameKind::Rectangle,
            rotation: 0.0,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    /// Two rectangles, side by side, in one layer.
    fn two_apart() -> (Document, FrameId, FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        (doc, a, b)
    }

    #[test]
    fn grouping_needs_at_least_two_frames() {
        let (mut doc, a, _) = two_apart();
        assert!(doc.group(&[a]).is_none(), "a group of one is not a group");
        assert!(doc.group(&[]).is_none());
    }

    #[test]
    fn a_group_takes_the_union_of_its_children() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");
        let bounds = doc.effective_bounds(g).expect("bounds");
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.width, 120.0, "0..20 and 100..120");
    }

    #[test]
    fn grouping_does_not_change_what_is_painted() {
        let (mut doc, a, b) = two_apart();
        let before = doc.paint_order();
        doc.group(&[a, b]).expect("grouped");
        assert_eq!(
            doc.paint_order(),
            before,
            "a group has no appearance of its own, so the leaves are unchanged"
        );
    }

    #[test]
    fn the_group_replaces_its_children_at_the_top_level() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");
        let top = doc.top_level_order();
        assert_eq!(top, vec![g], "children leave the layer's own list");
    }

    #[test]
    fn clicking_a_grouped_object_selects_the_group() {
        let (mut doc, a, b) = two_apart();
        assert_eq!(doc.hit_test(DocPoint { x: 10.0, y: 10.0 }), Some(a));

        let g = doc.group(&[a, b]).expect("grouped");

        assert_eq!(
            doc.hit_test(DocPoint { x: 10.0, y: 10.0 }),
            Some(g),
            "the group answers for its children"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 110.0, y: 10.0 }),
            Some(g),
            "either child, same answer"
        );
    }

    #[test]
    fn the_gap_between_grouped_objects_is_not_a_hit() {
        // A group is its children, not their bounding box. Clicking the empty
        // space between two grouped objects must miss.
        let (mut doc, a, b) = two_apart();
        doc.group(&[a, b]).expect("grouped");
        assert_eq!(doc.hit_test(DocPoint { x: 60.0, y: 10.0 }), None);
    }

    #[test]
    fn moving_a_group_carries_its_children() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");

        doc.translate_frame(g, 5.0, 7.0);

        assert_eq!(doc.frame(a).expect("a").bounds.x, 5.0);
        assert_eq!(doc.frame(b).expect("b").bounds.x, 105.0);
        assert_eq!(doc.frame(a).expect("a").bounds.y, 7.0);
    }

    #[test]
    fn ungrouping_returns_the_children_in_place() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");

        let freed = doc.ungroup(g);

        assert_eq!(freed, vec![a, b]);
        assert_eq!(doc.top_level_order(), vec![a, b]);
        assert!(doc.frame(g).is_none(), "the group itself is gone");
    }

    #[test]
    fn grouping_then_ungrouping_restores_the_stack() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        let c = doc.add_frame(layer, at(200.0, 0.0));
        let before = doc.top_level_order();

        let g = doc.group(&[a, b]).expect("grouped");
        doc.ungroup(g);

        assert_eq!(doc.top_level_order(), before);
        assert_eq!(before, vec![a, b, c]);
    }

    #[test]
    fn deleting_a_group_deletes_its_children_too() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");

        doc.remove_frame(g);

        assert!(
            doc.frame(a).is_none(),
            "an orphaned child would be invisible"
        );
        assert!(doc.frame(b).is_none());
        assert!(doc.paint_order().is_empty());
    }

    #[test]
    fn ungrouping_something_that_is_not_a_group_does_nothing() {
        let (mut doc, a, _) = two_apart();
        assert!(doc.ungroup(a).is_empty());
        assert!(doc.frame(a).is_some(), "and does not destroy it");
    }

    #[test]
    fn a_group_can_be_moved_in_z_as_one_object() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        let c = doc.add_frame(layer, at(200.0, 0.0));

        let g = doc.group(&[a, b]).expect("grouped");
        assert_eq!(doc.top_level_order(), vec![g, c]);

        doc.move_in_z(g, ZMove::ToFront);

        assert_eq!(doc.top_level_order(), vec![c, g]);
        assert_eq!(
            doc.paint_order(),
            vec![c, a, b],
            "the children move with it, keeping their order"
        );
    }

    #[test]
    fn nested_groups_report_every_descendant() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        let c = doc.add_frame(layer, at(200.0, 0.0));

        let inner = doc.group(&[a, b]).expect("inner");
        let outer = doc.group(&[inner, c]).expect("outer");

        let mut found = doc.descendants(outer);
        found.sort_by_key(|k| format!("{k:?}"));
        let mut expected = vec![outer, inner, a, b, c];
        expected.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(found, expected);
        assert_eq!(doc.paint_order(), vec![a, b, c], "leaves only, in order");
    }
}

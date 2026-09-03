//! The document: arenas of nodes, addressed by typed key.

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use tessera_geometry::{DocPoint, DocRect, Transform};

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

    /// The axis-aligned box a frame really covers, rotation included.
    ///
    /// [`Document::effective_bounds`] unions children's *unrotated* boxes,
    /// which is right for asking how big a shape is and wrong for drawing a
    /// box around one: a frame turned 45 degrees sticks out well past its own
    /// bounds. Grouping needs the second question answered, or the new group's
    /// box starts out too small.
    pub fn visual_bounds(&self, id: FrameId) -> Option<DocRect> {
        let mut union: Option<(f64, f64, f64, f64)> = None;
        for leaf in self.descendants(id) {
            let Some(frame) = self.frames.get(leaf) else {
                continue;
            };
            if matches!(frame.kind, FrameKind::Group(_)) {
                continue;
            }
            for p in frame.corners() {
                union = Some(match union {
                    None => (p.x, p.y, p.x, p.y),
                    Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
                });
            }
        }
        union.map(|(x0, y0, x1, y1)| DocRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// Roughly how much memory this document holds, in bytes.
    ///
    /// Used to bound the undo stack, which holds whole snapshots. Deliberately
    /// an estimate: walking every allocation exactly would cost more than the
    /// bound is worth, and a bound only has to be the right order of
    /// magnitude to stop a large document filling memory with its own history.
    ///
    /// Counts the things that actually scale — frame count, path complexity,
    /// text length — and ignores the fixed overhead of the arenas themselves.
    pub fn footprint(&self) -> usize {
        use std::mem::{size_of, size_of_val};

        let mut bytes = self.frames.len() * size_of::<Frame>();
        for frame in self.frames.values() {
            bytes += match &frame.kind {
                FrameKind::Path(path) => size_of_val(path.elements()),
                FrameKind::Group(children) => children.len() * size_of::<FrameId>(),
                _ => 0,
            };
        }

        for story in self.stories.values() {
            bytes += story.text.len() + story.style.family.len() + size_of::<Story>();
        }

        for layer in self.layers.values() {
            bytes += layer.name.len() + layer.frames.len() * size_of::<FrameId>();
        }
        bytes += self.pages.len() * size_of::<Page>();
        bytes += self.spreads.len() * size_of::<Spread>();
        bytes += self.spread_order.len() * size_of::<SpreadId>();

        bytes
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

        // The group's own box is authoritative from here on: the interface
        // draws it, and transforms update it alongside the children. So it has
        // to start out enclosing what is actually on screen, rotation and all.
        let bounds = members
            .iter()
            .filter_map(|id| self.visual_bounds(*id))
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
            transform: Transform::IDENTITY,
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
    /// A frame's geometry as a path in **document** coordinates, rotation
    /// applied.
    ///
    /// What the frame actually draws, as opposed to the box it draws inside.
    /// A group returns `None`: it has no geometry, only children.
    pub fn outline(&self, id: FrameId) -> Option<kurbo::BezPath> {
        use kurbo::Shape as _;

        let frame = self.frames.get(id)?;
        let b = frame.bounds;
        let rect = kurbo::Rect::new(b.x, b.y, b.x + b.width, b.y + b.height);

        let mut path = match &frame.kind {
            FrameKind::Rectangle | FrameKind::Text { .. } => rect.to_path(ACCURACY),
            FrameKind::Ellipse => kurbo::Ellipse::from_rect(rect).to_path(ACCURACY),
            FrameKind::Path(p) => {
                let mut placed = crate::path::fit_to_bounds(p, b);
                // `fit_to_bounds` answers frame-locally; this is the one place
                // that wants the answer in document space.
                placed.apply_affine(kurbo::Affine::translate((b.x, b.y)));
                placed
            }
            FrameKind::Group(_) => return None,
        };

        path.apply_affine(frame.transform.to_affine());
        Some(path)
    }

    /// The top-level frames a rubber band over `area` catches.
    ///
    /// By content, not by bounding box — the same rule clicking follows. A
    /// marquee that caught anything whose box it grazed would sweep up a
    /// pen-drawn curve from well outside the ink, which is exactly what
    /// clicking was fixed not to do.
    ///
    /// Top-level, also like clicking: a marquee over part of a group takes the
    /// group, because that is what grouping means.
    pub fn frames_touching(&self, area: DocRect) -> Vec<FrameId> {
        self.top_level_order()
            .into_iter()
            .filter(|id| self.touches_anywhere(*id, area))
            .collect()
    }

    fn touches_anywhere(&self, id: FrameId, area: DocRect) -> bool {
        self.descendants(id).into_iter().any(|leaf| {
            let Some(frame) = self.frames.get(leaf) else {
                return false;
            };
            self.outline(leaf)
                .is_some_and(|path| touches(&path, area, is_filled(&frame.kind)))
        })
    }

    /// The frontmost frame `point` lands on, or `None`.
    ///
    /// `tolerance`, in document units, is how far outside a shape's edge still
    /// counts. The viewport passes a few screen pixels converted through the
    /// zoom, so a hairline stays clickable however far out the view is.
    pub fn hit_test(&self, point: DocPoint, tolerance: f64) -> Option<FrameId> {
        // Top level, not paint order: clicking a grouped object selects the
        // GROUP, which is what grouping is for.
        self.top_level_order()
            .into_iter()
            .rev()
            .find(|id| self.hits_anywhere(*id, point, tolerance))
    }

    fn hits_anywhere(&self, id: FrameId, point: DocPoint, tolerance: f64) -> bool {
        self.descendants(id).into_iter().any(|leaf| {
            self.frames.get(leaf).is_some_and(|f| {
                !matches!(f.kind, FrameKind::Group(_)) && hits(f, point, tolerance)
            })
        })
    }
}

/// Whether `point` lands on `frame`, accounting for its rotation.
///
/// The shape decides, not the bounding box. A box test is right for a
/// rectangle and wrong for everything else: it hands an ellipse its corners,
/// and it lets a pen-drawn curve claim the whole rectangle it happens to span,
/// so clicking empty space well away from the ink selects it.
fn hits(frame: &Frame, point: DocPoint, tolerance: f64) -> bool {
    let bounds = frame.bounds;
    // Into the frame's own space, where its geometry is described.
    let local = frame.to_local(point);

    match &frame.kind {
        // A text frame is a box, and an empty one still has to be clickable.
        FrameKind::Rectangle | FrameKind::Text { .. } => grown(bounds, tolerance).contains(local),

        FrameKind::Ellipse => {
            let (rx, ry) = (
                bounds.width / 2.0 + tolerance,
                bounds.height / 2.0 + tolerance,
            );
            if rx <= 0.0 || ry <= 0.0 {
                return grown(bounds, tolerance).contains(local);
            }
            let centre = bounds.center();
            let nx = (local.x - centre.x) / rx;
            let ny = (local.y - centre.y) / ry;
            nx * nx + ny * ny <= 1.0
        }

        // Proximity to the ink, not to the box. A path frame renders as a
        // stroke and never as a fill (see `tessera_layout::resolve`), so the
        // empty middle of a closed pen shape belongs to whatever is behind it
        // — which is what an unfilled path means in every layout tool.
        FrameKind::Path(path) => {
            use kurbo::ParamCurveNearest as _;

            let reach = tolerance + frame.stroke.as_ref().map_or(1.0, |s| s.width) / 2.0;
            // `fit_to_bounds` answers in frame-local coordinates, so the
            // point has to be asked the same way.
            let fitted = crate::path::fit_to_bounds(path, bounds);
            let at = kurbo::Point::new(local.x - bounds.x, local.y - bounds.y);
            // Cheap rejection first: a path cannot be nearer than its own box.
            if !grown(bounds, reach).contains(local) {
                return false;
            }
            fitted
                .segments()
                .any(|seg| seg.nearest(at, ACCURACY).distance_sq <= reach * reach)
        }

        // A group is a container. `hits_anywhere` asks its children instead.
        FrameKind::Group(_) => false,
    }
}

/// Whether a marquee lying wholly inside this shape should catch it.
///
/// A filled shape swallows the band; an unfilled outline does not, for the
/// same reason clicking its empty middle does not select it.
fn is_filled(kind: &FrameKind) -> bool {
    matches!(
        kind,
        FrameKind::Rectangle | FrameKind::Ellipse | FrameKind::Text { .. }
    )
}

/// Whether the rubber band over `area` touches `path`.
///
/// Three ways it can: the band contains part of the path, the path crosses an
/// edge of the band, or — for a filled shape — the band is entirely inside it.
fn touches(path: &kurbo::BezPath, area: DocRect, filled: bool) -> bool {
    use kurbo::{ParamCurve as _, Shape as _};

    let band = kurbo::Rect::new(area.x, area.y, area.x + area.width, area.y + area.height);
    let corners = [
        (band.x0, band.y0),
        (band.x1, band.y0),
        (band.x1, band.y1),
        (band.x0, band.y1),
    ];
    let edges: Vec<kurbo::Line> = (0..4)
        .map(|i| kurbo::Line::new(corners[i], corners[(i + 1) % 4]))
        .collect();

    for seg in path.segments() {
        if band.contains(seg.eval(0.0)) || band.contains(seg.eval(1.0)) {
            return true;
        }
        // Exact against the curve, not against a flattened approximation of
        // it: a band clipping the bulge of a tight arc must still catch it.
        if edges
            .iter()
            .any(|edge| !seg.intersect_line(*edge).is_empty())
        {
            return true;
        }
    }

    filled && path.winding(band.center()) != 0
}

/// How precisely a curve's nearest point is found, in document units. Well
/// below anything a pointer can express, and far cheaper than exact.
const ACCURACY: f64 = 0.05;

/// `bounds` grown by `by` on every side.
fn grown(bounds: DocRect, by: f64) -> DocRect {
    DocRect {
        x: bounds.x - by,
        y: bounds.y - by,
        width: bounds.width + by * 2.0,
        height: bounds.height + by * 2.0,
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
            transform: Transform::IDENTITY,
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
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }, 0.0), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 5.0, y: 5.0 }, 0.0), None);
    }

    #[test]
    fn hit_test_returns_the_topmost_frame() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let _under = doc.add_frame(layer, rect_frame());
        let over = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }, 0.0), Some(over));
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
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }, 0.0), None);
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

        assert_eq!(doc.hit_test(point, 0.0), Some(over));
        doc.move_in_z(over, ZMove::ToBack);
        assert_eq!(
            doc.hit_test(point, 0.0),
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
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn an_unrotated_frame_hit_tests_as_before() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 10.0 }, 0.0), None);
    }

    #[test]
    fn rotating_a_frame_moves_where_it_can_be_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());

        // Upright, the bar spans x 0..100 at y 45..55, centred on (50, 50).
        // Turned a quarter turn it spans y 0..100 at x 45..55.
        let frame = doc.frames.get_mut(id).expect("frame");
        frame.transform = Transform::rotate_about(90.0, frame.bounds.center());

        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 10.0 }, 0.0),
            Some(id),
            "the bar now reaches up the page"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 10.0, y: 50.0 }, 0.0),
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
            let frame = doc.frames.get_mut(id).expect("frame");
            frame.transform = Transform::rotate_about(angle, frame.bounds.center());
            assert_eq!(
                doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
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
            transform: Transform::IDENTITY,
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

    // --- shape-precise hit testing -------------------------------------

    fn shape(kind: FrameKind, bounds: DocRect) -> Frame {
        Frame {
            bounds,
            kind,
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    fn square() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }
    }

    fn with(kind: FrameKind) -> (Document, FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, shape(kind, square()));
        (doc, id)
    }

    #[test]
    fn an_ellipse_does_not_claim_its_corners() {
        // The bug this pins: every kind was hit-tested against its bounding
        // box, so the empty corner of a circle selected it.
        let (doc, id) = with(FrameKind::Ellipse);
        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
            Some(id),
            "the middle is still inside"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 3.0, y: 3.0 }, 0.0),
            None,
            "the corner of the box is outside the ellipse"
        );
    }

    /// A diagonal stroke from corner to corner of `square`.
    fn diagonal() -> FrameKind {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 100.0));
        FrameKind::Path(path)
    }

    #[test]
    fn a_path_is_hit_on_its_ink_and_not_across_its_box() {
        // The reported bug: a curve drawn with the pen could be selected by
        // clicking anywhere inside the rectangle it happened to span.
        let (doc, id) = with(diagonal());
        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
            Some(id),
            "on the line"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 90.0, y: 10.0 }, 0.0),
            None,
            "well off the line but inside its box"
        );
    }

    #[test]
    fn a_closed_path_does_not_claim_its_empty_middle() {
        // A path frame renders as a stroke and never as a fill, so its inside
        // belongs to whatever is behind it.
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        path.line_to((100.0, 100.0));
        path.line_to((0.0, 100.0));
        path.close_path();
        let (doc, id) = with(FrameKind::Path(path));

        assert_eq!(doc.hit_test(DocPoint { x: 0.5, y: 50.0 }, 0.0), Some(id));
        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
            None,
            "the middle of an unfilled outline is not the outline"
        );
    }

    #[test]
    fn tolerance_makes_a_hairline_clickable_without_making_it_a_box() {
        let (doc, id) = with(diagonal());
        let just_off = DocPoint { x: 53.0, y: 50.0 };
        assert_eq!(doc.hit_test(just_off, 0.0), None, "no tolerance, no hit");
        assert_eq!(
            doc.hit_test(just_off, 4.0),
            Some(id),
            "a few units of slack catches it"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 90.0, y: 10.0 }, 4.0),
            None,
            "but slack must not restore the bounding box"
        );
    }

    #[test]
    fn a_rotated_path_is_hit_where_it_was_drawn_to() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut frame = shape(diagonal(), square());
        frame.transform = Transform::rotate_about(90.0, frame.bounds.center());
        let id = doc.add_frame(layer, frame);

        // The centre is on the line whatever the angle...
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0), Some(id));
        // ...and the far corner, which the rotated line now passes through.
        assert_eq!(doc.hit_test(DocPoint { x: 10.0, y: 90.0 }, 2.0), Some(id));
        assert_eq!(
            doc.hit_test(DocPoint { x: 90.0, y: 90.0 }, 2.0),
            None,
            "the corner the rotated line moved away from"
        );
    }

    #[test]
    fn a_text_frame_is_still_a_box_even_when_it_is_empty() {
        // An empty text frame has no ink at all. Hit-testing it by its
        // content would make it impossible to select or delete.
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(tessera_text::story::Story::default());
        let id = doc.add_frame(layer, shape(FrameKind::Text { story }, square()));
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0), Some(id));
    }

    // --- the marquee catches content, not boxes --------------------------

    fn band(x: f64, y: f64, width: f64, height: f64) -> DocRect {
        DocRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_marquee_across_a_curve_catches_it() {
        let (doc, id) = with(diagonal());
        // A band straddling the middle of the diagonal.
        assert_eq!(
            doc.frames_touching(band(40.0, 40.0, 20.0, 20.0)),
            vec![id],
            "the band crosses the ink"
        );
    }

    #[test]
    fn a_marquee_in_the_empty_part_of_a_curves_box_catches_nothing() {
        // The reported bug: selecting by bounding box swept up a pen-drawn
        // curve from a corner of its box the ink never reaches.
        let (doc, _) = with(diagonal());
        assert!(
            doc.frames_touching(band(80.0, 5.0, 15.0, 15.0)).is_empty(),
            "the band is inside the box but nowhere near the curve"
        );
    }

    #[test]
    fn a_marquee_wholly_inside_a_filled_shape_still_catches_it() {
        // Otherwise a rubber band drawn in the middle of a large rectangle
        // would select nothing, which is not what any layout tool does.
        let (doc, id) = with(FrameKind::Rectangle);
        assert_eq!(doc.frames_touching(band(40.0, 40.0, 5.0, 5.0)), vec![id]);
    }

    #[test]
    fn a_marquee_inside_an_unfilled_outline_catches_nothing() {
        // The same rule clicking follows: an unfilled path is its outline.
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        path.line_to((100.0, 100.0));
        path.line_to((0.0, 100.0));
        path.close_path();
        let (doc, _) = with(FrameKind::Path(path));
        assert!(doc.frames_touching(band(40.0, 40.0, 5.0, 5.0)).is_empty());
    }

    #[test]
    fn a_marquee_that_misses_entirely_catches_nothing() {
        let (doc, _) = with(FrameKind::Rectangle);
        assert!(
            doc.frames_touching(band(500.0, 500.0, 10.0, 10.0))
                .is_empty()
        );
    }

    #[test]
    fn a_marquee_over_part_of_a_group_takes_the_whole_group() {
        // Top-level, exactly as clicking is. Selecting one child out of a
        // group by rubber band would contradict what grouping means — and
        // paint order, which this used to walk, only ever yields children.
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");
        assert_eq!(
            doc.frames_touching(band(-5.0, -5.0, 20.0, 20.0)),
            vec![g],
            "the band touches only the first child"
        );
    }

    #[test]
    fn a_marquee_catches_a_rotated_frame_where_it_really_is() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut frame = shape(FrameKind::Rectangle, square());
        frame.transform = Transform::rotate_about(45.0, frame.bounds.center());
        let id = doc.add_frame(layer, frame);

        // A 45-degree square reaches further along the axes through its
        // centre than its unrotated bounds do.
        let reach = 50.0 * 2.0_f64.sqrt();
        let outside = 50.0 + reach - 4.0;
        assert_eq!(
            doc.frames_touching(band(outside, 48.0, 3.0, 3.0)),
            vec![id],
            "the corner swung out to here"
        );
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
        assert_eq!(doc.hit_test(DocPoint { x: 10.0, y: 10.0 }, 0.0), Some(a));

        let g = doc.group(&[a, b]).expect("grouped");

        assert_eq!(
            doc.hit_test(DocPoint { x: 10.0, y: 10.0 }, 0.0),
            Some(g),
            "the group answers for its children"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 110.0, y: 10.0 }, 0.0),
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
        assert_eq!(doc.hit_test(DocPoint { x: 60.0, y: 10.0 }, 0.0), None);
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

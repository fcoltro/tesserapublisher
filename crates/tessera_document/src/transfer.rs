//! Copy owned frame trees and remap document-local resources together.
use std::collections::{HashMap, HashSet};

use tessera_color::Color;
use tessera_geometry::Transform;
use tessera_text::story::{CharacterFormat, CharacterStyleId, ParagraphStyleId, Styles};

use crate::ids::{LinkId, ObjectStyleId};
use crate::paint::Paint;
use crate::{Document, FrameId, FrameKind, LayerId, StoryId};

impl Document {
    /// Copy a selection as one graph, preserving shared stories within the copy.
    pub fn copy_frames(
        &mut self,
        roots: &[FrameId],
        layer: LayerId,
        dx: f64,
        dy: f64,
    ) -> Vec<FrameId> {
        let source = self.clone();
        self.import_frames(&source, roots, layer, dx, dy, true)
    }

    /// Import from a frozen source. Only a same-document copy may reuse resource IDs.
    pub fn import_frames(
        &mut self,
        source: &Document,
        roots: &[FrameId],
        layer: LayerId,
        dx: f64,
        dy: f64,
        same_document: bool,
    ) -> Vec<FrameId> {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(frame) = source.frame(id) else {
                continue;
            };
            ids.push(id);
            match &frame.kind {
                FrameKind::Group(children) => pending.extend(children),
                FrameKind::Text { story, .. } => {
                    pending.extend(source.anchors_in(*story).frames.iter().map(|(_, id)| id))
                }
                FrameKind::Table(table) => {
                    for cell in table.cells.iter().filter_map(|slot| slot.cell()) {
                        pending.extend(
                            source
                                .anchors_in(cell.story)
                                .frames
                                .iter()
                                .map(|(_, id)| id),
                        );
                    }
                }
                _ => {}
            }
        }
        let frame_map: HashMap<_, _> = ids
            .iter()
            .map(|id| (*id, self.frames.insert(source.frames[*id].clone())))
            .collect();
        let mut transfer = Transfer {
            source,
            target: self,
            same_document,
            stories: HashMap::new(),
            characters: HashMap::new(),
            paragraphs: HashMap::new(),
            objects: HashMap::new(),
            links: HashMap::new(),
            swatches: HashMap::new(),
        };
        for id in &ids {
            let mut frame = source.frames[*id].clone();
            // Compose a document-space translation, preserving rotations and shears.
            if frame.transform.is_identity() {
                frame.bounds.x += dx;
                frame.bounds.y += dy;
            } else {
                frame.transform = frame.transform.then(Transform::translate(dx, dy));
            }
            frame.style = frame.style.and_then(|id| transfer.object(id));
            transfer.paint(&mut frame.fill);
            if let Some(s) = &mut frame.stroke {
                transfer.color(&mut s.color);
            }
            if let Some(s) = &mut frame.shadow {
                transfer.color(&mut s.colour);
            }
            match &mut frame.kind {
                FrameKind::Group(children) => {
                    *children = children
                        .iter()
                        .filter_map(|id| frame_map.get(id).copied())
                        .collect()
                }
                FrameKind::Text { story, layout } => {
                    *story = transfer.story(*story);
                    layout.next = layout.next.and_then(|id| frame_map.get(&id).copied());
                }
                FrameKind::Table(table) => {
                    for slot in &mut table.cells {
                        if let Some(cell) = slot.cell_mut() {
                            cell.story = transfer.story(cell.story);
                            if let Some(fill) = &mut cell.fill {
                                transfer.paint(fill);
                            }
                        }
                    }
                    if let Some(s) = &mut table.stroke {
                        transfer.color(&mut s.color);
                    }
                }
                FrameKind::Graphic { placed: Some(p) } => p.link = transfer.link(p.link),
                _ => {}
            }
            if let Some(anchor) = &mut frame.anchor {
                if let Some(story) = transfer.stories.get(&anchor.story) {
                    anchor.story = *story;
                } else {
                    frame.anchor = None;
                }
            }
            transfer.target.frames[frame_map[id]] = frame;
        }
        // Anchors can be visited before their host; remap after every story is known.
        for id in &ids {
            if let Some(mut anchor) = source.frames[*id].anchor {
                anchor.story = match transfer.stories.get(&anchor.story) {
                    Some(id) => *id,
                    None => continue,
                };
                transfer.target.frames[frame_map[id]].anchor = Some(anchor);
            }
        }
        let children: HashSet<_> = ids
            .iter()
            .flat_map(|id| match &source.frames[*id].kind {
                FrameKind::Group(c) => c.clone(),
                _ => vec![],
            })
            .collect();
        let roots: Vec<_> = roots
            .iter()
            .filter(|id| !children.contains(id))
            .filter_map(|id| frame_map.get(id).copied())
            .collect();
        if let Some(layer) = transfer.target.layers.get_mut(layer) {
            layer.frames.extend(roots.iter().copied());
            layer.frames.extend(
                ids.iter()
                    .filter(|id| !children.contains(id) && source.frames[**id].anchor.is_some())
                    .filter_map(|id| frame_map.get(id).copied())
                    .filter(|id| !roots.contains(id)),
            );
        }
        transfer.target.touch();
        roots
    }
}

struct Transfer<'a> {
    source: &'a Document,
    target: &'a mut Document,
    same_document: bool,
    stories: HashMap<StoryId, StoryId>,
    characters: HashMap<CharacterStyleId, CharacterStyleId>,
    paragraphs: HashMap<ParagraphStyleId, ParagraphStyleId>,
    objects: HashMap<ObjectStyleId, ObjectStyleId>,
    links: HashMap<LinkId, LinkId>,
    swatches: HashMap<String, String>,
}
impl Transfer<'_> {
    fn color(&mut self, color: &mut Color) {
        if self.same_document {
            return;
        }
        match color {
            Color::Spot { fallback, .. } => self.color(fallback),
            Color::Swatch { name, .. } => {
                if let Some(mapped) = self.swatches.get(name) {
                    *name = mapped.clone();
                    return;
                }
                let Some(mut swatch) = self.source.swatch(name).cloned() else {
                    return;
                };
                let original = name.clone();
                let mut mapped = original.clone();
                let mut suffix = 2;
                while self
                    .target
                    .swatch(&mapped)
                    .is_some_and(|existing| existing != &swatch)
                {
                    mapped = format!("{original} (copy {suffix})");
                    suffix += 1;
                }
                self.swatches.insert(original, mapped.clone());
                self.color(&mut swatch.colour);
                swatch.name = mapped.clone();
                if self.target.swatch(&mapped).is_none() {
                    self.target.swatches.push(swatch);
                }
                *name = mapped;
            }
            _ => {}
        }
    }
    fn paint(&mut self, paint: &mut Paint) {
        match paint {
            Paint::Solid(c) => self.color(c),
            Paint::Gradient(g) => {
                let mut stops = g.stops().to_vec();
                for stop in &mut stops {
                    self.color(&mut stop.colour);
                }
                g.set_stops(stops);
            }
        }
    }
    fn character_format(&mut self, f: &mut CharacterFormat) {
        if let Some(c) = &mut f.colour {
            self.color(c);
        }
    }
    fn character(&mut self, id: CharacterStyleId) -> Option<CharacterStyleId> {
        if self.same_document {
            return Some(id);
        }
        if let Some(mapped) = self.characters.get(&id) {
            return Some(*mapped);
        }
        let mut style = self.source.character_styles.get(id)?.clone();
        let mapped = self.target.character_styles.insert(style.clone());
        self.characters.insert(id, mapped);
        style.based_on = style.based_on.and_then(|id| self.character(id));
        self.character_format(&mut style.format);
        self.target.character_styles[mapped] = style;
        Some(mapped)
    }
    fn paragraph(&mut self, id: ParagraphStyleId) -> Option<ParagraphStyleId> {
        if self.same_document {
            return Some(id);
        }
        if let Some(mapped) = self.paragraphs.get(&id) {
            return Some(*mapped);
        }
        let mut style = self.source.paragraph_styles.get(id)?.clone();
        let mapped = self.target.paragraph_styles.insert(style.clone());
        self.paragraphs.insert(id, mapped);
        style.based_on = style.based_on.and_then(|id| self.paragraph(id));
        if style.based_on.is_none() {
            style.format.character = style.format.character.over(&self.source.document_default());
        }
        self.character_format(&mut style.format.character);
        self.target.paragraph_styles[mapped] = style;
        Some(mapped)
    }
    fn object(&mut self, id: ObjectStyleId) -> Option<ObjectStyleId> {
        if self.same_document {
            return Some(id);
        }
        if let Some(mapped) = self.objects.get(&id) {
            return Some(*mapped);
        }
        let mut style = self.source.object_styles.get(id)?.clone();
        let mapped = self.target.object_styles.insert(style.clone());
        self.objects.insert(id, mapped);
        self.target.object_style_order.push(mapped);
        style.based_on = style.based_on.and_then(|id| self.object(id));
        if let Some(f) = &mut style.format.fill {
            self.paint(f);
        }
        if let Some(Some(s)) = &mut style.format.stroke {
            self.color(&mut s.color);
        }
        if let Some(Some(s)) = &mut style.format.shadow {
            self.color(&mut s.colour);
        }
        self.target.object_styles[mapped] = style;
        Some(mapped)
    }
    fn story(&mut self, id: StoryId) -> StoryId {
        if let Some(mapped) = self.stories.get(&id) {
            return *mapped;
        }
        let mut story = self.source.story(id).cloned().unwrap_or_default();
        for run in &mut story.runs {
            run.style = run.style.and_then(|id| self.character(id));
            self.character_format(&mut run.local);
        }
        for para in &mut story.paragraphs {
            para.style = para.style.and_then(|id| self.paragraph(id));
            if !self.same_document && para.style.is_none() {
                para.local.character = para.local.character.over(&self.source.document_default());
            }
            self.character_format(&mut para.local.character);
        }
        let mapped = self.target.add_story(story);
        self.stories.insert(id, mapped);
        mapped
    }
    fn link(&mut self, id: LinkId) -> LinkId {
        if self.same_document {
            return id;
        }
        if let Some(mapped) = self.links.get(&id) {
            return *mapped;
        }
        let Some(link) = self.source.links.get(id).cloned() else {
            return LinkId::default();
        };
        let mapped = self.target.add_link(link);
        self.links.insert(id, mapped);
        mapped
    }
}

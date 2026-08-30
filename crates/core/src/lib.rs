pub mod camera;
pub mod components;
pub mod geometry;
pub mod layout;
pub mod snapping;
pub mod history;

pub use camera::*;
pub use components::*;
pub use geometry::*;
pub use layout::*;
pub use snapping::*;
pub use history::*;

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

/// Serialized representation of an entity snapshot for IPC transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub id: u32,
    pub position: Option<Position>,
    pub size: Option<Size>,
    pub transform: Option<Transform>,
    pub bounding_box: Option<BoundingBox>,
    pub z_index: Option<i32>,
    pub style: Option<Style>,
    pub frame: Option<Frame>,
    pub text_content: Option<TextContent>,
    pub parent_id: Option<u32>,
}

/// Hit test result from point raycasting against ECS BoundingBoxes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitTestResult {
    pub entity_id: u32,
    pub name: String,
    pub frame_type: FrameType,
    pub z_index: i32,
    pub bounding_box: BoundingBox,
}

/// Node representing a frame in the document tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameNode {
    pub id: u32,
    pub name: String,
    pub frame_type: FrameType,
    pub transform: Transform,
    pub size: Size,
    pub z_index: i32,
    pub bounding_box: BoundingBox,
    pub style: Style,
    pub text: Option<String>,
}

/// Node representing a layer in the document tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerNode {
    pub id: u32,
    pub name: String,
    pub z_index: i32,
    pub is_visible: bool,
    pub is_locked: bool,
    pub frames: Vec<FrameNode>,
}

/// Node representing a page in the document tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageNode {
    pub id: u32,
    pub page_number: u32,
    pub width: f32,
    pub height: f32,
    pub layers: Vec<LayerNode>,
}

/// Structured document hierarchy for UI trees and inspector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentTreeSnapshot {
    pub document_id: u32,
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub dpi: f32,
    pub bleed: f32,
    pub pages: Vec<PageNode>,
    pub total_entities: usize,
}

/// History status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryStatus {
    pub undo_count: usize,
    pub redo_count: usize,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Thread-safe managed application state wrapping Bevy ECS World in RwLock
pub struct AppState {
    pub world: RwLock<World>,
    pub history: Mutex<HistoryStack>,
    pub camera: RwLock<Camera>,
    pub scene_revision: AtomicU64,
    /// Snap lines hit by the gesture currently in flight, for on-canvas feedback.
    ///
    /// Transient UI state rather than document state, so it lives beside the
    /// world instead of in it and is never recorded in history.
    pub active_snap: Mutex<Option<SnapResult>>,
}

impl AppState {
    /// Initializes a new AppState containing an initialized Document hierarchy
    pub fn new() -> Self {
        let mut world = World::new();
        let history = HistoryStack::new(100);
        let camera = Camera::default();

        // Scaffold initial Document -> Page -> Layer structure
        let document = Document {
            title: "Tessera Document 1".to_string(),
            ..Default::default()
        };
        let spread_layout = document.spread_layout();
        let page_guides = document.guides;
        let doc_entity = world.spawn(document).id();

        let placement = spread_layout.place(1);
        let page_entity = world
            .spawn((
                Page {
                    page_number: placement.page_number,
                    width: placement.width,
                    height: placement.height,
                    spread_index: placement.spread_index,
                },
                page_guides,
                BelongsTo(doc_entity),
            ))
            .id();

        let _layer_entity = world
            .spawn((
                Layer {
                    name: "Layer 1".to_string(),
                    z_index: 0,
                    is_visible: true,
                    is_locked: false,
                },
                BelongsTo(page_entity),
            ))
            .id();

        Self {
            world: RwLock::new(world),
            history: Mutex::new(history),
            camera: RwLock::new(camera),
            active_snap: Mutex::new(None),
            scene_revision: AtomicU64::new(1),
        }
    }

    pub fn get_scene_revision(&self) -> u64 {
        self.scene_revision.load(Ordering::Acquire)
    }

    pub fn increment_scene_revision(&self) -> u64 {
        self.scene_revision.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn get_camera(&self) -> Camera {
        self.camera.read().map(|c| *c).unwrap_or_default()
    }

    pub fn pan_camera(&self, dx: f32, dy: f32) -> Camera {
        if let Ok(mut cam) = self.camera.write() {
            cam.pan_by(dx, dy);
            *cam
        } else {
            Camera::default()
        }
    }

    pub fn zoom_camera(&self, screen_x: f32, screen_y: f32, factor: f32) -> Camera {
        if let Ok(mut cam) = self.camera.write() {
            cam.zoom_at(screen_x, screen_y, factor);
            *cam
        } else {
            Camera::default()
        }
    }

    pub fn fit_page_view(&self, viewport_width: f32, viewport_height: f32) -> Camera {
        let (page_w, page_h) = {
            let world = self.world.read().unwrap();
            let dims = world
                .iter_entities()
                .find_map(|e| e.get::<Page>().map(|p| (p.width, p.height)))
                .unwrap_or((800.0, 600.0));
            dims
        };

        if let Ok(mut cam) = self.camera.write() {
            cam.fit_page(page_w, page_h, viewport_width, viewport_height);
            *cam
        } else {
            Camera::default()
        }
    }

    pub fn reset_camera(&self) -> Camera {
        if let Ok(mut cam) = self.camera.write() {
            cam.reset();
            *cam
        } else {
            Camera::default()
        }
    }

    /// Raycasts screen pixel coordinates to select an entity in document space
    pub fn raycast_select_entity(&self, screen_x: f32, screen_y: f32) -> Option<u32> {
        let (doc_x, doc_y) = {
            let cam = self.camera.read().ok()?;
            cam.screen_to_document(screen_x, screen_y)
        };

        let hits = self.hit_test(doc_x, doc_y).ok()?;
        hits.first().map(|h| h.entity_id)
    }

    /// Spawns a new Frame inside a Layer and registers it in the Undo stack
    pub fn spawn_frame(
        &self,
        parent_layer: Option<Entity>,
        name: String,
        frame_type: FrameType,
        transform: Transform,
        size: Size,
        style: Style,
        text: Option<String>,
    ) -> Result<u32, String> {
        self.spawn_frame_with_path(parent_layer, name, frame_type, transform, size, style, text, None)
    }

    /// Spawns a frame, optionally carrying a bezier outline.
    ///
    /// Bounds come from the frame's real geometry, so an ellipse or path is not
    /// treated as its rectangular box.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_frame_with_path(
        &self,
        parent_layer: Option<Entity>,
        name: String,
        frame_type: FrameType,
        transform: Transform,
        size: Size,
        style: Style,
        text: Option<String>,
        path: Option<PathData>,
    ) -> Result<u32, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;

        // 1. Resolve target layer before spawning
        let resolved_parent = if let Some(parent) = parent_layer {
            Some(parent)
        } else {
            // Find first layer entity in the world
            world
                .iter_entities()
                .find(|e| e.contains::<Layer>())
                .map(|e| e.id())
        };

        let bb = crate::geometry::frame_bounds(frame_type, &transform, &size, path.as_ref());
        let z_idx = ZIndex(0);

        let frame_comp = Frame {
            name: name.clone(),
            frame_type,
        };

        let mut entity_cmd = world.spawn((
            frame_comp.clone(),
            transform,
            size,
            z_idx,
            bb,
            style.clone(),
        ));

        let parent_id = if let Some(parent) = resolved_parent {
            entity_cmd.insert(BelongsTo(parent));
            Some(parent.index())
        } else {
            None
        };

        if let Some(path_data) = path.clone() {
            entity_cmd.insert(path_data);
        }

        let text_comp = text.map(|t| TextContent {
            text: t,
            ..Default::default()
        });

        if let Some(t_comp) = &text_comp {
            entity_cmd.insert(t_comp.clone());
        }

        let entity_id = entity_cmd.id().index();

        // Push to History
        if let Ok(mut hist) = self.history.lock() {
            hist.push(HistoryAction::SpawnFrame(EntitySnapshotData {
                entity_index: entity_id,
                frame: frame_comp,
                transform,
                size,
                z_index: z_idx,
                bounding_box: bb,
                style,
                parent: parent_id,
                text_content: text_comp,
            }));
        }

        self.increment_scene_revision();

        Ok(entity_id)
    }

    /// Hit-tests a point (x, y) against all Frame bounding boxes in the ECS World
    pub fn hit_test(&self, px: f32, py: f32) -> Result<Vec<HitTestResult>, String> {
        let world = self.world.read().map_err(|e| e.to_string())?;

        let mut hits: Vec<HitTestResult> = world
            .iter_entities()
            .filter_map(|e| {
                let frame = e.get::<Frame>()?;
                let z = e.get::<ZIndex>().copied().unwrap_or(ZIndex(0));
                let bb = e.get::<BoundingBox>()?;

                // Broad phase on the AABB, then an exact test against the real
                // outline so a click in an ellipse's corner does not select it.
                let hit = bb.contains_point(px, py)
                    && match (e.get::<Transform>(), e.get::<Size>()) {
                        (Some(transform), Some(size)) => crate::geometry::frame_contains_point(
                            frame.frame_type,
                            transform,
                            size,
                            e.get::<PathData>(),
                            px,
                            py,
                        ),
                        // Without geometry components the AABB is all we have.
                        _ => true,
                    };

                if hit {
                    Some(HitTestResult {
                        entity_id: e.id().index(),
                        name: frame.name.clone(),
                        frame_type: frame.frame_type,
                        z_index: z.0,
                        bounding_box: *bb,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by ZIndex descending (topmost element first)
        hits.sort_by(|a, b| b.z_index.cmp(&a.z_index));
        Ok(hits)
    }

    /// The document's settings, or defaults when no document entity exists.
    pub fn get_document_settings(&self) -> Document {
        self.world
            .read()
            .ok()
            .and_then(|world| {
                world
                    .iter_entities()
                    .find_map(|e| e.get::<Document>().cloned())
            })
            .unwrap_or_default()
    }

    /// Replaces the document's settings and re-places every page.
    ///
    /// Changing page size or the facing-pages flag moves pages on the
    /// pasteboard, so placements are recomputed rather than left stale.
    pub fn set_document_settings(&self, settings: Document) -> Result<(), String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;

        let doc_entity = world
            .iter_entities()
            .find(|e| e.contains::<Document>())
            .map(|e| e.id());
        match doc_entity {
            Some(entity) => {
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut slot) = entity_mut.get_mut::<Document>() {
                        *slot = settings.clone();
                    }
                }
            }
            None => {
                world.spawn(settings.clone());
            }
        }

        Self::replace_pages(&mut world, &settings);
        drop(world);
        self.increment_scene_revision();
        Ok(())
    }

    /// How many pages the document currently has.
    pub fn page_count(&self) -> u32 {
        self.world
            .read()
            .map(|world| world.iter_entities().filter(|e| e.contains::<Page>()).count() as u32)
            .unwrap_or(0)
    }

    /// Every page's placement on the pasteboard, ordered by page number.
    pub fn page_placements(&self) -> Vec<PagePlacement> {
        let settings = self.get_document_settings();
        settings.spread_layout().place_all(self.page_count())
    }

    /// Appends a page to the end of the document.
    ///
    /// Returns the new page count. Adding a page can re-pair every later
    /// spread, so all pages are re-placed.
    pub fn add_page(&self) -> Result<u32, String> {
        let settings = self.get_document_settings();
        let mut world = self.world.write().map_err(|e| e.to_string())?;

        let count = world.iter_entities().filter(|e| e.contains::<Page>()).count() as u32;
        let doc_entity = world
            .iter_entities()
            .find(|e| e.contains::<Document>())
            .map(|e| e.id());

        let placement = settings.spread_layout().place(count + 1);
        let mut page = world.spawn((
            Page {
                page_number: placement.page_number,
                width: placement.width,
                height: placement.height,
                spread_index: placement.spread_index,
            },
            settings.guides,
        ));
        if let Some(doc) = doc_entity {
            page.insert(BelongsTo(doc));
        }

        Self::renumber_pages(&mut world, &settings);
        drop(world);
        self.increment_scene_revision();
        Ok(count + 1)
    }

    /// Removes a page by its one-based number, renumbering those after it.
    ///
    /// The last page cannot be removed: a document always has at least one.
    pub fn remove_page(&self, page_number: u32) -> Result<u32, String> {
        let settings = self.get_document_settings();
        let mut world = self.world.write().map_err(|e| e.to_string())?;

        let pages: Vec<_> = world
            .iter_entities()
            .filter_map(|e| e.get::<Page>().map(|p| (e.id(), p.page_number)))
            .collect();
        if pages.len() <= 1 {
            return Err("a document must keep at least one page".to_string());
        }

        let target = pages
            .iter()
            .find(|(_, number)| *number == page_number)
            .map(|(entity, _)| *entity)
            .ok_or_else(|| format!("page {page_number} not found"))?;

        if let Ok(entity) = world.get_entity_mut(target) {
            entity.despawn();
        }

        Self::renumber_pages(&mut world, &settings);
        let count = world.iter_entities().filter(|e| e.contains::<Page>()).count() as u32;
        drop(world);
        self.increment_scene_revision();
        Ok(count)
    }

    /// Re-places every page after the page list changes.
    ///
    /// Page numbers follow document order, and each page's size and spread
    /// index are derived from the layout rather than stored independently.
    fn renumber_pages(world: &mut World, settings: &Document) {
        let layout = settings.spread_layout();
        let mut pages: Vec<_> = world
            .iter_entities()
            .filter_map(|e| e.get::<Page>().map(|p| (e.id(), p.page_number)))
            .collect();
        pages.sort_by_key(|(_, number)| *number);

        for (index, (entity, _)) in pages.into_iter().enumerate() {
            let placement = layout.place(index as u32 + 1);
            if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                if let Some(mut page) = entity_mut.get_mut::<Page>() {
                    page.page_number = placement.page_number;
                    page.width = placement.width;
                    page.height = placement.height;
                    page.spread_index = placement.spread_index;
                }
            }
        }
    }

    /// Re-places pages against new document settings.
    fn replace_pages(world: &mut World, settings: &Document) {
        Self::renumber_pages(world, settings);
    }

    /// Links `from` so its overflow continues in `to`.
    ///
    /// Both frames must be text frames. A frame cannot be threaded to itself,
    /// and the link is rejected if it would create a cycle — a story that
    /// looped would reflow forever.
    pub fn thread_text_frames(&self, from: u32, to: u32) -> Result<(), String> {
        if from == to {
            return Err("a frame cannot be threaded to itself".to_string());
        }

        let mut world = self.world.write().map_err(|e| e.to_string())?;
        for index in [from, to] {
            let is_text = world
                .get_entity(Entity::from_raw(index))
                .map(|e| {
                    e.get::<Frame>()
                        .is_some_and(|f| f.frame_type == FrameType::Text)
                })
                .unwrap_or(false);
            if !is_text {
                return Err(format!("entity {index} is not a text frame"));
            }
        }

        let from_entity = Entity::from_raw(from);
        let to_entity = Entity::from_raw(to);

        // Walk forward from `to`; reaching `from` would close a loop.
        let mut cursor = Some(to_entity);
        let mut visited = 0usize;
        while let Some(current) = cursor {
            if current == from_entity {
                return Err("that link would create a threading cycle".to_string());
            }
            visited += 1;
            if visited > 10_000 {
                return Err("threading chain is unexpectedly long".to_string());
            }
            cursor = world
                .get_entity(current)
                .ok()
                .and_then(|e| e.get::<TextThread>())
                .and_then(|t| t.next);
        }

        // Detach whatever each end was previously linked to, so no frame is
        // left pointing at a frame that no longer points back.
        let old_next = Self::thread_of(&world, from_entity).and_then(|t| t.next);
        if let Some(old) = old_next {
            Self::update_thread(&mut world, old, |t| t.previous = None);
        }
        let old_previous = Self::thread_of(&world, to_entity).and_then(|t| t.previous);
        if let Some(old) = old_previous {
            Self::update_thread(&mut world, old, |t| t.next = None);
        }

        Self::update_thread(&mut world, from_entity, |t| t.next = Some(to_entity));
        Self::update_thread(&mut world, to_entity, |t| t.previous = Some(from_entity));

        drop(world);
        self.increment_scene_revision();
        Ok(())
    }

    /// Breaks the link after `from`, ending its story there.
    pub fn unthread_text_frame(&self, from: u32) -> Result<(), String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let entity = Entity::from_raw(from);

        let next = Self::thread_of(&world, entity)
            .and_then(|t| t.next)
            .ok_or_else(|| format!("frame {from} has nothing threaded after it"))?;

        Self::update_thread(&mut world, entity, |t| t.next = None);
        Self::update_thread(&mut world, next, |t| t.previous = None);

        drop(world);
        self.increment_scene_revision();
        Ok(())
    }

    /// The full chain of frames in the story containing `entity_index`.
    ///
    /// Walks back to the head first, so any frame in a story returns the same
    /// ordered chain.
    pub fn text_story_chain(&self, entity_index: u32) -> Vec<u32> {
        let Ok(world) = self.world.read() else {
            return Vec::new();
        };

        let mut head = Entity::from_raw(entity_index);
        let mut guard = 0;
        while let Some(previous) = Self::thread_of(&world, head).and_then(|t| t.previous) {
            head = previous;
            guard += 1;
            if guard > 10_000 {
                break;
            }
        }

        let mut chain = Vec::new();
        let mut cursor = Some(head);
        while let Some(current) = cursor {
            chain.push(current.index());
            if chain.len() > 10_000 {
                break;
            }
            cursor = Self::thread_of(&world, current).and_then(|t| t.next);
        }
        chain
    }

    fn thread_of(world: &World, entity: Entity) -> Option<TextThread> {
        world
            .get_entity(entity)
            .ok()
            .and_then(|e| e.get::<TextThread>())
            .copied()
    }

    /// Applies a change to an entity's thread links, inserting the component
    /// if the frame was not previously threaded.
    fn update_thread(world: &mut World, entity: Entity, edit: impl FnOnce(&mut TextThread)) {
        let mut thread = Self::thread_of(world, entity).unwrap_or_default();
        edit(&mut thread);
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert(thread);
        }
    }

    /// Creates a master page and returns its entity index.
    pub fn create_master_page(&self, name: String, prefix: String) -> Result<u32, String> {
        let settings = self.get_document_settings();
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let id = world
            .spawn(MasterPage {
                name,
                prefix,
                width: settings.width,
                height: settings.height,
            })
            .id()
            .index();
        drop(world);
        self.increment_scene_revision();
        Ok(id)
    }

    /// Spawns a frame onto a master page, in page-local coordinates.
    ///
    /// Master items are parented to the master rather than a layer, which is
    /// what distinguishes them from ordinary page content.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_master_frame(
        &self,
        master_index: u32,
        name: String,
        frame_type: FrameType,
        transform: Transform,
        size: Size,
        style: Style,
        text: Option<String>,
    ) -> Result<u32, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let master = Entity::from_raw(master_index);
        if !world
            .get_entity(master)
            .map(|e| e.contains::<MasterPage>())
            .unwrap_or(false)
        {
            return Err(format!("entity {master_index} is not a master page"));
        }

        let bounds = crate::geometry::frame_bounds(frame_type, &transform, &size, None);
        let mut spawned = world.spawn((
            Frame {
                name,
                frame_type,
            },
            transform,
            size,
            ZIndex(0),
            bounds,
            style,
            BelongsTo(master),
        ));
        if let Some(text) = text {
            spawned.insert(TextContent {
                text,
                ..Default::default()
            });
        }
        let id = spawned.id().index();

        drop(world);
        self.increment_scene_revision();
        Ok(id)
    }

    /// Every master page, as (entity index, master).
    pub fn master_pages(&self) -> Vec<(u32, MasterPage)> {
        self.world
            .read()
            .map(|world| {
                world
                    .iter_entities()
                    .filter_map(|e| e.get::<MasterPage>().map(|m| (e.id().index(), m.clone())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Applies a master to a page, replacing any master already applied.
    pub fn apply_master_to_page(
        &self,
        page_number: u32,
        master_index: u32,
    ) -> Result<(), String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;

        let master = Entity::from_raw(master_index);
        if !world
            .get_entity(master)
            .map(|e| e.contains::<MasterPage>())
            .unwrap_or(false)
        {
            return Err(format!("entity {master_index} is not a master page"));
        }

        let page = world
            .iter_entities()
            .find(|e| e.get::<Page>().is_some_and(|p| p.page_number == page_number))
            .map(|e| e.id())
            .ok_or_else(|| format!("page {page_number} not found"))?;

        world
            .get_entity_mut(page)
            .map_err(|_| "page vanished".to_string())?
            .insert(AppliedMaster(master));
        drop(world);
        self.increment_scene_revision();
        Ok(())
    }

    /// Removes the master from a page, leaving only its own content.
    pub fn detach_master_from_page(&self, page_number: u32) -> Result<(), String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let page = world
            .iter_entities()
            .find(|e| e.get::<Page>().is_some_and(|p| p.page_number == page_number))
            .map(|e| e.id())
            .ok_or_else(|| format!("page {page_number} not found"))?;

        world
            .get_entity_mut(page)
            .map_err(|_| "page vanished".to_string())?
            .remove::<AppliedMaster>();
        drop(world);
        self.increment_scene_revision();
        Ok(())
    }

    /// The master applied to a page, if any.
    pub fn master_of_page(&self, page_number: u32) -> Option<u32> {
        let world = self.world.read().ok()?;
        // Bound to a local so the read guard outlives the borrow chain.
        let master = world
            .iter_entities()
            .find(|e| e.get::<Page>().is_some_and(|p| p.page_number == page_number))
            .and_then(|e| e.get::<AppliedMaster>())
            .map(|applied| applied.0.index());
        master
    }

    /// Promotes an inherited master item into an editable frame on a page.
    ///
    /// The copy is placed in document space at the page's position, so it lands
    /// exactly where the inherited item appeared. Overriding the same item
    /// twice is rejected rather than silently duplicating it.
    pub fn override_master_item(
        &self,
        page_number: u32,
        master_frame_index: u32,
    ) -> Result<u32, String> {
        let settings = self.get_document_settings();
        let placement = settings.spread_layout().place(page_number);

        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let page_entity = world
            .iter_entities()
            .find(|e| e.get::<Page>().is_some_and(|p| p.page_number == page_number))
            .map(|e| e.id())
            .ok_or_else(|| format!("page {page_number} not found"))?;

        let source = Entity::from_raw(master_frame_index);
        let already_overridden = world.iter_entities().any(|e| {
            e.get::<MasterOverride>()
                .is_some_and(|o| o.source == source && o.page == page_entity)
        });
        if already_overridden {
            return Err("that master item is already overridden on this page".to_string());
        }

        let source_entity = world
            .get_entity(source)
            .map_err(|_| format!("master item {master_frame_index} not found"))?;
        let frame = source_entity
            .get::<Frame>()
            .cloned()
            .ok_or_else(|| format!("entity {master_frame_index} is not a frame"))?;
        let local_transform = source_entity.get::<Transform>().copied().unwrap_or_default();
        let size = source_entity.get::<Size>().copied().unwrap_or_default();
        let z_index = source_entity.get::<ZIndex>().copied().unwrap_or_default();
        let style = source_entity.get::<Style>().cloned().unwrap_or_default();
        let text = source_entity.get::<TextContent>().cloned();
        let path = source_entity.get::<PathData>().cloned();

        // Master frames are page-local, so shift into document space.
        let transform = Transform {
            position: Position {
                x: local_transform.position.x + placement.x,
                y: local_transform.position.y + placement.y,
            },
            ..local_transform
        };
        let bounds =
            crate::geometry::frame_bounds(frame.frame_type, &transform, &size, path.as_ref());

        let mut spawned = world.spawn((
            frame,
            transform,
            size,
            z_index,
            bounds,
            style,
            MasterOverride {
                source,
                page: page_entity,
            },
        ));
        if let Some(text) = text {
            spawned.insert(text);
        }
        if let Some(path) = path {
            spawned.insert(path);
        }
        let id = spawned.id().index();

        drop(world);
        self.increment_scene_revision();
        Ok(id)
    }

    /// Collects every snap target in the document except the frame being moved.
    ///
    /// The moving frame is excluded so it cannot snap to its own edges.
    pub fn snap_targets(&self, exclude_entity: Option<u32>) -> Result<SnapTargets, String> {
        let settings = self.get_document_settings();
        let layout = settings.spread_layout();
        let world = self.world.read().map_err(|e| e.to_string())?;

        let mut targets = SnapTargets::default();

        let mut pages: Vec<_> = world
            .iter_entities()
            .filter_map(|e| {
                e.get::<Page>()
                    .map(|p| (p.page_number, e.get::<PageGuides>().copied()))
            })
            .collect();
        pages.sort_by_key(|(number, _)| *number);
        for (page_number, guides) in pages {
            targets.from_page(
                &layout.place(page_number),
                &guides.unwrap_or(settings.guides),
            );
        }

        for entity in world.iter_entities() {
            if let Some(guide) = entity.get::<RulerGuide>() {
                targets.from_ruler_guide(guide);
            }
            if entity.contains::<Frame>() {
                if Some(entity.id().index()) == exclude_entity {
                    continue;
                }
                if let Some(bounds) = entity.get::<BoundingBox>() {
                    targets.from_object(bounds);
                }
            }
        }

        Ok(targets)
    }

    /// Applies snapping to a proposed geometry without writing it to the world.
    ///
    /// The caller feeds this the geometry a drag would produce, and gets back
    /// the corrected transform plus which lines were hit, so the UI can draw
    /// snap feedback.
    pub fn snap_frame_geometry(
        &self,
        entity_index: u32,
        transform: Transform,
        size: Size,
        zoom: f32,
        threshold_px: f32,
    ) -> Result<(Transform, SnapResult), String> {
        let frame_type = {
            let world = self.world.read().map_err(|e| e.to_string())?;
            Self::frame_type_of(&world, entity_index)?
        };
        let path = {
            let world = self.world.read().map_err(|e| e.to_string())?;
            world
                .get_entity(Entity::from_raw(entity_index))
                .ok()
                .and_then(|e| e.get::<PathData>().cloned())
        };

        let bounds = crate::geometry::frame_bounds(frame_type, &transform, &size, path.as_ref());
        let targets = self.snap_targets(Some(entity_index))?;
        let result = crate::snapping::snap_bounds(&bounds, &targets, zoom, threshold_px);

        if let Ok(mut slot) = self.active_snap.lock() {
            *slot = result.is_snapped().then_some(result);
        }

        let snapped = Transform {
            position: Position {
                x: transform.position.x + result.delta_x,
                y: transform.position.y + result.delta_y,
            },
            ..transform
        };
        Ok((snapped, result))
    }

    /// Clears snap feedback when a gesture ends.
    pub fn clear_active_snap(&self) {
        if let Ok(mut slot) = self.active_snap.lock() {
            *slot = None;
        }
    }

    /// The snap lines currently being highlighted, if any.
    pub fn get_active_snap(&self) -> Option<SnapResult> {
        self.active_snap.lock().ok().and_then(|slot| *slot)
    }

    /// Adds a ruler guide and returns its entity index.
    pub fn add_ruler_guide(&self, axis: GuideAxis, position: f32) -> Result<u32, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let id = world.spawn(RulerGuide { axis, position }).id().index();
        drop(world);
        self.increment_scene_revision();
        Ok(id)
    }

    /// Removes a ruler guide by entity index.
    pub fn remove_ruler_guide(&self, entity_index: u32) -> Result<(), String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let entity = world
            .get_entity_mut(Entity::from_raw(entity_index))
            .map_err(|_| format!("guide {entity_index} not found"))?;
        if entity.contains::<RulerGuide>() {
            entity.despawn();
        } else {
            return Err(format!("entity {entity_index} is not a guide"));
        }
        drop(world);
        self.increment_scene_revision();
        Ok(())
    }

    /// Every ruler guide in the document.
    pub fn ruler_guides(&self) -> Vec<(u32, RulerGuide)> {
        self.world
            .read()
            .map(|world| {
                world
                    .iter_entities()
                    .filter_map(|e| e.get::<RulerGuide>().map(|g| (e.id().index(), *g)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Reads a frame's current transform and size.
    ///
    /// The frontend captures this at the start of a drag so the completed
    /// gesture can be committed as a single undoable delta.
    pub fn get_frame_geometry(&self, entity_index: u32) -> Result<(Transform, Size), String> {
        let world = self.world.read().map_err(|e| e.to_string())?;
        let entity = world
            .get_entity(Entity::from_raw(entity_index))
            .map_err(|_| format!("entity {entity_index} not found"))?;

        let transform = entity
            .get::<Transform>()
            .copied()
            .ok_or_else(|| format!("entity {entity_index} has no transform"))?;
        let size = entity
            .get::<Size>()
            .copied()
            .ok_or_else(|| format!("entity {entity_index} has no size"))?;
        Ok((transform, size))
    }

    /// Applies a transform and size directly, without recording history.
    ///
    /// This is the live path used while a drag is in flight: it must not push
    /// an undo entry per mouse move, or one gesture would fill the stack.
    pub fn set_frame_geometry(
        &self,
        entity_index: u32,
        transform: Transform,
        size: Size,
    ) -> Result<BoundingBox, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let bounds = Self::apply_geometry(&mut world, entity_index, transform, size)?;
        drop(world);
        self.increment_scene_revision();
        Ok(bounds)
    }

    /// Records a finished drag as one undoable action.
    ///
    /// `old_*` are the values captured when the gesture began. A gesture that
    /// changed nothing is dropped rather than pushed, so a stray click does not
    /// leave a no-op entry on the undo stack.
    pub fn commit_frame_geometry(
        &self,
        entity_index: u32,
        old_transform: Transform,
        old_size: Size,
        new_transform: Transform,
        new_size: Size,
    ) -> Result<HistoryStatus, String> {
        let transform_changed = old_transform != new_transform;
        let size_changed = old_size != new_size;

        if !transform_changed && !size_changed {
            return self.get_history_status();
        }

        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let old_bounds = crate::geometry::frame_bounds(
            Self::frame_type_of(&world, entity_index)?,
            &old_transform,
            &old_size,
            None,
        );
        let new_bounds = Self::apply_geometry(&mut world, entity_index, new_transform, new_size)?;
        drop(world);

        let mut history = self.history.lock().map_err(|e| e.to_string())?;
        if transform_changed {
            history.push(HistoryAction::UpdateTransform {
                entity_index,
                old_transform,
                new_transform,
                old_bounding_box: old_bounds,
                new_bounding_box: new_bounds,
            });
        }
        if size_changed {
            history.push(HistoryAction::UpdateSize {
                entity_index,
                old_size,
                new_size,
                old_bounding_box: old_bounds,
                new_bounding_box: new_bounds,
            });
        }
        drop(history);

        self.increment_scene_revision();
        self.get_history_status()
    }

    /// Writes geometry onto an entity and refreshes its bounds from real shape.
    fn apply_geometry(
        world: &mut World,
        entity_index: u32,
        transform: Transform,
        size: Size,
    ) -> Result<BoundingBox, String> {
        let frame_type = Self::frame_type_of(world, entity_index)?;
        let path = world
            .get_entity(Entity::from_raw(entity_index))
            .ok()
            .and_then(|e| e.get::<PathData>().cloned());
        let bounds = crate::geometry::frame_bounds(frame_type, &transform, &size, path.as_ref());

        let mut entity = world
            .get_entity_mut(Entity::from_raw(entity_index))
            .map_err(|_| format!("entity {entity_index} not found"))?;
        if let Some(mut slot) = entity.get_mut::<Transform>() {
            *slot = transform;
        }
        if let Some(mut slot) = entity.get_mut::<Size>() {
            *slot = size;
        }
        if let Some(mut slot) = entity.get_mut::<BoundingBox>() {
            *slot = bounds;
        }
        Ok(bounds)
    }

    fn frame_type_of(world: &World, entity_index: u32) -> Result<FrameType, String> {
        world
            .get_entity(Entity::from_raw(entity_index))
            .map_err(|_| format!("entity {entity_index} not found"))?
            .get::<Frame>()
            .map(|frame| frame.frame_type)
            .ok_or_else(|| format!("entity {entity_index} is not a frame"))
    }

    /// Replaces a path frame's bezier outline.
    pub fn set_frame_path(&self, entity_index: u32, svg: String) -> Result<BoundingBox, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let (transform, size) = {
            let entity = world
                .get_entity(Entity::from_raw(entity_index))
                .map_err(|_| format!("entity {entity_index} not found"))?;
            (
                entity.get::<Transform>().copied().unwrap_or_default(),
                entity.get::<Size>().copied().unwrap_or_default(),
            )
        };
        let frame_type = Self::frame_type_of(&world, entity_index)?;
        let path = PathData { svg };
        let bounds = crate::geometry::frame_bounds(frame_type, &transform, &size, Some(&path));

        let mut entity = world
            .get_entity_mut(Entity::from_raw(entity_index))
            .map_err(|_| format!("entity {entity_index} not found"))?;
        entity.insert(path);
        if let Some(mut slot) = entity.get_mut::<BoundingBox>() {
            *slot = bounds;
        }
        drop(world);

        self.increment_scene_revision();
        Ok(bounds)
    }

    /// Performs an Undo operation restoring the ECS World to its prior state
    pub fn undo(&self) -> Result<HistoryStatus, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let mut hist = self.history.lock().map_err(|e| e.to_string())?;
        hist.undo(&mut world);
        self.increment_scene_revision();
        let (u, r) = hist.depth();
        Ok(HistoryStatus {
            undo_count: u,
            redo_count: r,
            can_undo: hist.can_undo(),
            can_redo: hist.can_redo(),
        })
    }

    /// Performs a Redo operation
    pub fn redo(&self) -> Result<HistoryStatus, String> {
        let mut world = self.world.write().map_err(|e| e.to_string())?;
        let mut hist = self.history.lock().map_err(|e| e.to_string())?;
        hist.redo(&mut world);
        self.increment_scene_revision();
        let (u, r) = hist.depth();
        Ok(HistoryStatus {
            undo_count: u,
            redo_count: r,
            can_undo: hist.can_undo(),
            can_redo: hist.can_redo(),
        })
    }

    /// Returns the current Undo/Redo depth and state
    pub fn get_history_status(&self) -> Result<HistoryStatus, String> {
        let hist = self.history.lock().map_err(|e| e.to_string())?;
        let (u, r) = hist.depth();
        Ok(HistoryStatus {
            undo_count: u,
            redo_count: r,
            can_undo: hist.can_undo(),
            can_redo: hist.can_redo(),
        })
    }

    /// Compiles and queries the complete hierarchical Document Tree
    pub fn get_document_tree(&self) -> Result<DocumentTreeSnapshot, String> {
        let world = self.world.read().map_err(|e| e.to_string())?;

        // 1. Get Document
        let (doc_entity, doc) = world
            .iter_entities()
            .find_map(|e| e.get::<Document>().map(|d| (e.id(), d.clone())))
            .ok_or_else(|| "No document found in ECS world".to_string())?;

        // 2. Get Pages belonging to this document
        let page_entities: Vec<(Entity, Page)> = world
            .iter_entities()
            .filter_map(|e| {
                let page = e.get::<Page>()?;
                let parent_link = e.get::<BelongsTo>();
                let is_match = match parent_link {
                    Some(link) => link.0 == doc_entity,
                    None => true,
                };
                if is_match {
                    Some((e.id(), page.clone()))
                } else {
                    None
                }
            })
            .collect();

        let mut pages = Vec::new();

        for (page_ent, page) in page_entities {
            // 3. Get Layers belonging to this Page
            let layer_entities: Vec<(Entity, Layer)> = world
                .iter_entities()
                .filter_map(|e| {
                    let layer = e.get::<Layer>()?;
                    let link = e.get::<BelongsTo>();
                    let is_match = match link {
                        Some(l) => l.0 == page_ent,
                        None => true,
                    };
                    if is_match {
                        Some((e.id(), layer.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            let mut layers = Vec::new();

            for (layer_ent, layer) in layer_entities {
                // 4. Get Frames belonging to this Layer
                let mut frames: Vec<FrameNode> = world
                    .iter_entities()
                    .filter_map(|e| {
                        let frame = e.get::<Frame>()?;
                        let transform = e.get::<Transform>()?;
                        let size = e.get::<Size>()?;
                        let z_idx = e.get::<ZIndex>().copied().unwrap_or(ZIndex(0));
                        let bb = e.get::<BoundingBox>()?;
                        let style = e.get::<Style>()?;
                        let text = e.get::<TextContent>();
                        let link = e.get::<BelongsTo>();

                        let is_match = match link {
                            Some(l) => l.0 == layer_ent,
                            None => true,
                        };

                        if is_match {
                            Some(FrameNode {
                                id: e.id().index(),
                                name: frame.name.clone(),
                                frame_type: frame.frame_type,
                                transform: *transform,
                                size: *size,
                                z_index: z_idx.0,
                                bounding_box: *bb,
                                style: style.clone(),
                                text: text.map(|t| t.text.clone()),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                frames.sort_by(|a, b| a.z_index.cmp(&b.z_index));

                layers.push(LayerNode {
                    id: layer_ent.index(),
                    name: layer.name,
                    z_index: layer.z_index,
                    is_visible: layer.is_visible,
                    is_locked: layer.is_locked,
                    frames,
                });
            }

            layers.sort_by(|a, b| a.z_index.cmp(&b.z_index));

            pages.push(PageNode {
                id: page_ent.index(),
                page_number: page.page_number,
                width: page.width,
                height: page.height,
                layers,
            });
        }

        pages.sort_by(|a, b| a.page_number.cmp(&b.page_number));

        Ok(DocumentTreeSnapshot {
            document_id: doc_entity.index(),
            title: doc.title,
            width: doc.width,
            height: doc.height,
            dpi: doc.dpi,
            bleed: doc.bleed,
            pages,
            total_entities: world.entities().len() as usize,
        })
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_hierarchy_and_tree_query() {
        let app_state = AppState::new();
        let tree = app_state
            .get_document_tree()
            .expect("Should query document tree");

        assert_eq!(tree.title, "Tessera Document 1");
        assert_eq!(tree.pages.len(), 1);
        assert_eq!(tree.pages[0].layers.len(), 1);
    }

    #[test]
    fn test_frame_spawning_and_bounding_box_hit_test() {
        let app_state = AppState::new();

        let entity_id = app_state
            .spawn_frame(
                None,
                "Hero Rectangle".to_string(),
                FrameType::Rectangle,
                Transform {
                    position: Position { x: 100.0, y: 150.0 },
                    rotation: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                },
                Size {
                    width: 200.0,
                    height: 100.0,
                },
                Style::default(),
                None,
            )
            .expect("Should spawn frame");

        // Point inside (150, 180) -> Hit!
        let hits = app_state
            .hit_test(150.0, 180.0)
            .expect("Hit test should run");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity_id, entity_id);

        // Point outside (50, 50) -> Miss
        let misses = app_state
            .hit_test(50.0, 50.0)
            .expect("Hit test should run");
        assert_eq!(misses.len(), 0);
    }

    #[test]
    fn test_undo_redo_stack_reversibility() {
        let app_state = AppState::new();

        // 1. Spawn frame
        let _frame_id = app_state
            .spawn_frame(
                None,
                "Test Frame".to_string(),
                FrameType::Text,
                Transform::default(),
                Size {
                    width: 100.0,
                    height: 50.0,
                },
                Style::default(),
                Some("Headline".to_string()),
            )
            .expect("Should spawn frame");

        let status = app_state.get_history_status().unwrap();
        assert_eq!(status.undo_count, 1);
        assert_eq!(status.redo_count, 0);

        // 2. Undo spawn
        let undo_res = app_state.undo().expect("Undo should succeed");
        assert_eq!(undo_res.undo_count, 0);
        assert_eq!(undo_res.redo_count, 1);

        // Frame should not be present in hit test
        let hits = app_state.hit_test(10.0, 10.0).unwrap();
        assert_eq!(hits.len(), 0);

        // 3. Redo spawn
        let redo_res = app_state.redo().expect("Redo should succeed");
        assert_eq!(redo_res.undo_count, 1);
        assert_eq!(redo_res.redo_count, 0);

        // Frame is restored
        let tree = app_state.get_document_tree().unwrap();
        assert_eq!(tree.pages[0].layers[0].frames.len(), 1);
    }

    /// Spawns one rectangle and returns its entity index.
    fn spawn_test_rect(app_state: &AppState) -> u32 {
        app_state
            .spawn_frame(
                None,
                "Drag Target".to_string(),
                FrameType::Rectangle,
                Transform {
                    position: Position { x: 10.0, y: 10.0 },
                    rotation: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                },
                Size { width: 100.0, height: 50.0 },
                Style::default(),
                None,
            )
            .expect("spawn should succeed")
    }

    #[test]
    fn a_new_document_starts_with_one_page() {
        let app_state = AppState::new();
        assert_eq!(app_state.page_count(), 1);
        assert_eq!(app_state.page_placements().len(), 1);
    }

    #[test]
    fn adding_pages_repairs_the_spreads() {
        // The pairing changes as pages are added: page 1 is a lone recto, then
        // 2 and 3 pair up. Placements must reflect that, not just append.
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        app_state.add_page().unwrap();

        let placements = app_state.page_placements();
        assert_eq!(placements.len(), 3);
        assert_eq!(placements[0].spread_index, 0);
        assert_eq!(placements[1].spread_index, 1);
        assert_eq!(placements[2].spread_index, 1, "pages 2 and 3 share a spread");
    }

    #[test]
    fn removing_a_page_renumbers_the_rest() {
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        app_state.add_page().unwrap();

        app_state.remove_page(2).unwrap();

        assert_eq!(app_state.page_count(), 2);
        let world = app_state.world.read().unwrap();
        let mut numbers: Vec<u32> = world
            .iter_entities()
            .filter_map(|e| e.get::<Page>().map(|p| p.page_number))
            .collect();
        numbers.sort();
        assert_eq!(numbers, vec![1, 2], "numbering must stay contiguous");
    }

    #[test]
    fn the_last_page_cannot_be_removed() {
        let app_state = AppState::new();
        assert!(app_state.remove_page(1).is_err());
        assert_eq!(app_state.page_count(), 1);
    }

    #[test]
    fn removing_a_missing_page_is_an_error() {
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        assert!(app_state.remove_page(99).is_err());
    }

    #[test]
    fn changing_page_size_re_places_existing_pages() {
        let app_state = AppState::new();
        app_state.add_page().unwrap();

        let mut settings = app_state.get_document_settings();
        settings.width = 300.0;
        settings.height = 400.0;
        app_state.set_document_settings(settings).unwrap();

        let placements = app_state.page_placements();
        assert_eq!(placements[0].width, 300.0);
        // The spine moves with the page width.
        assert_eq!(placements[0].x, 300.0, "page 1 sits right of the new spine");
    }

    #[test]
    fn turning_off_facing_pages_gives_each_page_its_own_spread() {
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        app_state.add_page().unwrap();

        let mut settings = app_state.get_document_settings();
        settings.facing_pages = false;
        app_state.set_document_settings(settings).unwrap();

        let placements = app_state.page_placements();
        assert_eq!(placements[1].spread_index, 1);
        assert_eq!(placements[2].spread_index, 2);
        assert!(placements.iter().all(|p| p.x == 0.0));
    }

    /// Spawns a text frame and returns its entity index.
    fn spawn_text_frame(app_state: &AppState, name: &str) -> u32 {
        app_state
            .spawn_frame(
                None,
                name.to_string(),
                FrameType::Text,
                Transform::default(),
                Size { width: 200.0, height: 100.0 },
                Style::default(),
                Some("Story text".to_string()),
            )
            .expect("spawn should succeed")
    }

    #[test]
    fn threading_links_frames_in_both_directions() {
        let app_state = AppState::new();
        let first = spawn_text_frame(&app_state, "A");
        let second = spawn_text_frame(&app_state, "B");

        app_state.thread_text_frames(first, second).unwrap();

        assert_eq!(app_state.text_story_chain(first), vec![first, second]);
        // Asking from the tail must return the same chain, walking back first.
        assert_eq!(app_state.text_story_chain(second), vec![first, second]);
    }

    #[test]
    fn a_story_can_span_three_frames() {
        let app_state = AppState::new();
        let a = spawn_text_frame(&app_state, "A");
        let b = spawn_text_frame(&app_state, "B");
        let c = spawn_text_frame(&app_state, "C");

        app_state.thread_text_frames(a, b).unwrap();
        app_state.thread_text_frames(b, c).unwrap();

        assert_eq!(app_state.text_story_chain(b), vec![a, b, c]);
    }

    #[test]
    fn a_frame_cannot_be_threaded_to_itself() {
        let app_state = AppState::new();
        let frame = spawn_text_frame(&app_state, "A");
        assert!(app_state.thread_text_frames(frame, frame).is_err());
    }

    #[test]
    fn threading_rejects_cycles() {
        // A loop would make reflow run forever, so it must be refused.
        let app_state = AppState::new();
        let a = spawn_text_frame(&app_state, "A");
        let b = spawn_text_frame(&app_state, "B");

        app_state.thread_text_frames(a, b).unwrap();
        assert!(app_state.thread_text_frames(b, a).is_err());
    }

    #[test]
    fn only_text_frames_can_be_threaded() {
        let app_state = AppState::new();
        let text = spawn_text_frame(&app_state, "A");
        let rect = spawn_test_rect(&app_state);

        assert!(app_state.thread_text_frames(text, rect).is_err());
    }

    #[test]
    fn rethreading_detaches_the_previous_link() {
        // Pointing A at C must leave B with no incoming link, rather than two
        // frames both claiming to follow A.
        let app_state = AppState::new();
        let a = spawn_text_frame(&app_state, "A");
        let b = spawn_text_frame(&app_state, "B");
        let c = spawn_text_frame(&app_state, "C");

        app_state.thread_text_frames(a, b).unwrap();
        app_state.thread_text_frames(a, c).unwrap();

        assert_eq!(app_state.text_story_chain(a), vec![a, c]);
        assert_eq!(app_state.text_story_chain(b), vec![b], "B is its own story now");
    }

    #[test]
    fn unthreading_splits_a_story_in_two() {
        let app_state = AppState::new();
        let a = spawn_text_frame(&app_state, "A");
        let b = spawn_text_frame(&app_state, "B");
        app_state.thread_text_frames(a, b).unwrap();

        app_state.unthread_text_frame(a).unwrap();

        assert_eq!(app_state.text_story_chain(a), vec![a]);
        assert_eq!(app_state.text_story_chain(b), vec![b]);
    }

    #[test]
    fn unthreading_an_unlinked_frame_is_an_error() {
        let app_state = AppState::new();
        let frame = spawn_text_frame(&app_state, "A");
        assert!(app_state.unthread_text_frame(frame).is_err());
    }

    #[test]
    fn masters_can_be_applied_and_detached() {
        let app_state = AppState::new();
        let master = app_state
            .create_master_page("A-Master".to_string(), "A".to_string())
            .unwrap();

        app_state.apply_master_to_page(1, master).unwrap();
        assert_eq!(app_state.master_of_page(1), Some(master));

        app_state.detach_master_from_page(1).unwrap();
        assert_eq!(app_state.master_of_page(1), None);
    }

    #[test]
    fn applying_a_non_master_is_rejected() {
        let app_state = AppState::new();
        let frame = spawn_test_rect(&app_state);
        assert!(app_state.apply_master_to_page(1, frame).is_err());
    }

    #[test]
    fn overriding_a_master_item_places_it_in_document_space() {
        // A master item is stored page-local, so the override must land where
        // the inherited item appeared on that page, not at the origin.
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        let master = app_state
            .create_master_page("A-Master".to_string(), "A".to_string())
            .unwrap();
        let master_item = app_state
            .spawn_frame(
                None,
                "Folio".to_string(),
                FrameType::Rectangle,
                Transform {
                    position: Position { x: 20.0, y: 30.0 },
                    ..Default::default()
                },
                Size { width: 50.0, height: 20.0 },
                Style::default(),
                None,
            )
            .unwrap();
        app_state.apply_master_to_page(2, master).unwrap();

        let override_id = app_state.override_master_item(2, master_item).unwrap();
        let (transform, _) = app_state.get_frame_geometry(override_id).unwrap();
        let placement = app_state.get_document_settings().spread_layout().place(2);

        assert!((transform.position.x - (placement.x + 20.0)).abs() < 1e-3);
        assert!((transform.position.y - (placement.y + 30.0)).abs() < 1e-3);
    }

    #[test]
    fn a_master_item_cannot_be_overridden_twice_on_one_page() {
        let app_state = AppState::new();
        let master = app_state
            .create_master_page("A-Master".to_string(), "A".to_string())
            .unwrap();
        let item = spawn_test_rect(&app_state);
        app_state.apply_master_to_page(1, master).unwrap();

        app_state.override_master_item(1, item).unwrap();
        assert!(app_state.override_master_item(1, item).is_err());
    }

    #[test]
    fn a_dragged_frame_snaps_to_the_page_margin() {
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        let settings = app_state.get_document_settings();
        let placement = settings.spread_layout().place(1);
        let (margin_left, _, _, _) = settings.guides.content_rect(&placement);

        // Drop the frame three units short of the left margin.
        let transform = Transform {
            position: Position { x: margin_left - 3.0, y: 400.0 },
            ..Default::default()
        };
        let (snapped, result) = app_state
            .snap_frame_geometry(id, transform, Size { width: 100.0, height: 50.0 }, 1.0, 5.0)
            .unwrap();

        assert!(result.is_snapped());
        assert!((snapped.position.x - margin_left).abs() < 1e-3);
    }

    #[test]
    fn a_frame_never_snaps_to_itself() {
        // Without excluding the moving frame, its own edges would pin it in
        // place and it could never be dragged at all.
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        let (transform, size) = app_state.get_frame_geometry(id).unwrap();

        let targets = app_state.snap_targets(Some(id)).unwrap();
        let bounds = crate::geometry::frame_bounds(FrameType::Rectangle, &transform, &size, None);
        assert!(!targets
            .vertical
            .iter()
            .any(|line| (line.position - bounds.min_x).abs() < 1e-6
                && line.source == SnapSource::Object));
    }

    #[test]
    fn a_frame_snaps_to_another_frames_edge() {
        let app_state = AppState::new();
        let anchor = spawn_test_rect(&app_state);
        let (anchor_transform, anchor_size) = app_state.get_frame_geometry(anchor).unwrap();
        let anchor_right = anchor_transform.position.x + anchor_size.width;

        let mover = spawn_test_rect(&app_state);
        let transform = Transform {
            position: Position { x: anchor_right + 2.0, y: 10.0 },
            ..Default::default()
        };
        let (snapped, result) = app_state
            .snap_frame_geometry(mover, transform, Size { width: 50.0, height: 20.0 }, 1.0, 5.0)
            .unwrap();

        assert!(result.is_snapped());
        assert!((snapped.position.x - anchor_right).abs() < 1e-3);
    }

    #[test]
    fn ruler_guides_can_be_added_listed_and_removed() {
        let app_state = AppState::new();
        let id = app_state.add_ruler_guide(GuideAxis::Vertical, 123.0).unwrap();

        let guides = app_state.ruler_guides();
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].1.position, 123.0);

        app_state.remove_ruler_guide(id).unwrap();
        assert!(app_state.ruler_guides().is_empty());
    }

    #[test]
    fn removing_a_non_guide_entity_is_rejected() {
        let app_state = AppState::new();
        let frame = spawn_test_rect(&app_state);
        assert!(app_state.remove_ruler_guide(frame).is_err());
    }

    #[test]
    fn a_ruler_guide_becomes_a_snap_target() {
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        app_state.add_ruler_guide(GuideAxis::Vertical, 500.0).unwrap();

        let transform = Transform {
            position: Position { x: 498.0, y: 1000.0 },
            ..Default::default()
        };
        let (snapped, result) = app_state
            .snap_frame_geometry(id, transform, Size { width: 10.0, height: 10.0 }, 1.0, 5.0)
            .unwrap();

        assert_eq!(result.snapped_vertical.map(|l| l.source), Some(SnapSource::Guide));
        assert!((snapped.position.x - 500.0).abs() < 1e-3);
    }

    #[test]
    fn live_drag_updates_geometry_without_touching_history() {
        // A drag in flight must not push an entry per mouse move.
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        // Spawning is itself undoable, so compare against the post-spawn depth.
        let baseline = app_state.get_history_status().unwrap().undo_count;
        let (mut transform, size) = app_state.get_frame_geometry(id).unwrap();

        for step in 1..=10 {
            transform.position.x = 10.0 + step as f32;
            app_state.set_frame_geometry(id, transform, size).unwrap();
        }

        let status = app_state.get_history_status().unwrap();
        assert_eq!(status.undo_count, baseline, "live drag should not record history");

        let (moved, _) = app_state.get_frame_geometry(id).unwrap();
        assert_eq!(moved.position.x, 20.0);
    }

    #[test]
    fn committing_a_drag_records_one_undoable_action() {
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        let baseline = app_state.get_history_status().unwrap().undo_count;
        let (old_transform, old_size) = app_state.get_frame_geometry(id).unwrap();

        let new_transform = Transform {
            position: Position { x: 200.0, y: 120.0 },
            ..old_transform
        };
        app_state
            .commit_frame_geometry(id, old_transform, old_size, new_transform, old_size)
            .unwrap();

        assert_eq!(
            app_state.get_history_status().unwrap().undo_count,
            baseline + 1,
            "a completed drag should add exactly one entry"
        );

        app_state.undo().unwrap();
        let (restored, _) = app_state.get_frame_geometry(id).unwrap();
        assert_eq!(restored.position.x, 10.0, "undo should restore the origin");

        app_state.redo().unwrap();
        let (redone, _) = app_state.get_frame_geometry(id).unwrap();
        assert_eq!(redone.position.x, 200.0, "redo should reapply the move");
    }

    #[test]
    fn a_gesture_that_changed_nothing_records_nothing() {
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        let baseline = app_state.get_history_status().unwrap().undo_count;
        let (transform, size) = app_state.get_frame_geometry(id).unwrap();

        app_state
            .commit_frame_geometry(id, transform, size, transform, size)
            .unwrap();

        assert_eq!(app_state.get_history_status().unwrap().undo_count, baseline);
    }

    #[test]
    fn resizing_updates_the_bounding_box() {
        let app_state = AppState::new();
        let id = spawn_test_rect(&app_state);
        let (transform, _) = app_state.get_frame_geometry(id).unwrap();

        let bounds = app_state
            .set_frame_geometry(id, transform, Size { width: 400.0, height: 200.0 })
            .unwrap();

        assert!((bounds.width() - 400.0).abs() < 1e-3);
        assert!((bounds.height() - 200.0).abs() < 1e-3);
    }

    #[test]
    fn hit_testing_an_ellipse_rejects_its_bounding_box_corners() {
        // The point of precise hit testing: the AABB corner is not the shape.
        let app_state = AppState::new();
        app_state
            .spawn_frame(
                None,
                "Circle".to_string(),
                FrameType::Ellipse,
                Transform {
                    position: Position { x: 0.0, y: 0.0 },
                    rotation: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                },
                Size { width: 100.0, height: 100.0 },
                Style::default(),
                None,
            )
            .unwrap();

        assert_eq!(app_state.hit_test(50.0, 50.0).unwrap().len(), 1, "centre should hit");
        assert!(app_state.hit_test(2.0, 2.0).unwrap().is_empty(), "corner should miss");
    }

    #[test]
    fn path_frames_hit_test_against_their_outline() {
        let app_state = AppState::new();
        app_state
            .spawn_frame_with_path(
                None,
                "Triangle".to_string(),
                FrameType::Path,
                Transform::default(),
                Size { width: 100.0, height: 100.0 },
                Style::default(),
                None,
                Some(PathData { svg: "M 0 0 L 0 100 L 100 100 Z".to_string() }),
            )
            .unwrap();

        assert_eq!(app_state.hit_test(20.0, 80.0).unwrap().len(), 1);
        assert!(app_state.hit_test(90.0, 10.0).unwrap().is_empty());
    }

    #[test]
    fn replacing_a_path_outline_refreshes_bounds() {
        let app_state = AppState::new();
        let id = app_state
            .spawn_frame_with_path(
                None,
                "Shape".to_string(),
                FrameType::Path,
                Transform::default(),
                Size { width: 100.0, height: 100.0 },
                Style::default(),
                None,
                Some(PathData { svg: "M 0 0 L 10 0 L 10 10 Z".to_string() }),
            )
            .unwrap();

        let bounds = app_state
            .set_frame_path(id, "M 0 0 L 80 0 L 80 40 Z".to_string())
            .unwrap();

        assert!((bounds.width() - 80.0).abs() < 1e-3);
        assert!((bounds.height() - 40.0).abs() < 1e-3);
    }

    #[test]
    fn test_concurrent_rwlock_reads() {
        let app_state = std::sync::Arc::new(AppState::new());

        let mut handles = vec![];
        for _ in 0..8 {
            let state_clone = app_state.clone();
            handles.push(std::thread::spawn(move || {
                let tree = state_clone.get_document_tree().unwrap();
                assert_eq!(tree.title, "Tessera Document 1");
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_camera_affine_transform_and_cursor_zoom() {
        let mut camera = Camera::new(1200.0, 800.0);
        camera.pan_x = 100.0;
        camera.pan_y = 50.0;
        camera.zoom = 2.0;

        // Document (50, 50) -> Screen (50*2 + 100, 50*2 + 50) = (200, 150)
        let (screen_x, screen_y) = camera.document_to_screen(50.0, 50.0);
        assert_eq!(screen_x, 200.0);
        assert_eq!(screen_y, 150.0);

        // Screen (200, 150) -> Document (50, 50)
        let (doc_x, doc_y) = camera.screen_to_document(200.0, 150.0);
        assert_eq!(doc_x, 50.0);
        assert_eq!(doc_y, 50.0);

        // Cursor-centered zoom invariance test:
        // Before zooming at screen point (400, 300), document point is D
        let (doc_before_x, doc_before_y) = camera.screen_to_document(400.0, 300.0);
        camera.zoom_at(400.0, 300.0, 1.5);
        let (doc_after_x, doc_after_y) = camera.screen_to_document(400.0, 300.0);

        assert!((doc_before_x - doc_after_x).abs() < 1e-4);
        assert!((doc_before_y - doc_after_y).abs() < 1e-4);
    }

    #[test]
    fn test_raycast_select_entity_with_camera() {
        let app_state = AppState::new();

        let entity_id = app_state
            .spawn_frame(
                None,
                "Selectable Box".to_string(),
                FrameType::Rectangle,
                Transform {
                    position: Position { x: 100.0, y: 100.0 },
                    rotation: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                },
                Size {
                    width: 200.0,
                    height: 100.0,
                },
                Style::default(),
                None,
            )
            .expect("Should spawn frame");

        // Camera default: pan_x=60, pan_y=60, zoom=1.0
        // Document rect is [100..300, 100..200]
        // Screen rect is [160..360, 160..260]
        // Clicking at Screen (200, 200) -> Doc (140, 140) -> Inside!
        let selected = app_state.raycast_select_entity(200.0, 200.0);
        assert_eq!(selected, Some(entity_id));

        // Clicking outside (Screen 50, 50) -> Doc (-10, -10) -> None
        let missed = app_state.raycast_select_entity(50.0, 50.0);
        assert_eq!(missed, None);
    }
}

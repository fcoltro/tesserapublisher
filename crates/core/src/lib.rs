pub mod camera;
pub mod components;
pub mod history;

pub use camera::*;
pub use components::*;
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
}

impl AppState {
    /// Initializes a new AppState containing an initialized Document hierarchy
    pub fn new() -> Self {
        let mut world = World::new();
        let history = HistoryStack::new(100);
        let camera = Camera::default();

        // Scaffold initial Document -> Page -> Layer structure
        let doc_entity = world
            .spawn(Document {
                title: "Tessera Document 1".to_string(),
                width: 1200.0,
                height: 800.0,
                dpi: 300.0,
                bleed: 3.0,
            })
            .id();

        let page_entity = world
            .spawn((
                Page {
                    page_number: 1,
                    width: 1200.0,
                    height: 800.0,
                    spread_index: 0,
                },
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

        let bb = BoundingBox::from_transform_and_size(&transform, &size);
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

        let text_comp = text.map(|t| TextContent {
            text: t,
            font_size: 16.0,
            line_height: 1.4,
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

                if bb.contains_point(px, py) {
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

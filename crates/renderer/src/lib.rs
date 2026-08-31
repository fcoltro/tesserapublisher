pub mod gpu;
pub mod paint;
pub mod scene;
pub mod text;

pub use gpu::*;
pub use paint::*;
pub use scene::*;
pub use text::*;

use serde::{Deserialize, Serialize};

/// Renderer engine status reported to the UI.
///
/// Unlike the placeholder this replaces, every field here is observed from the
/// live pipeline: `backend` and `is_ready` are only populated once wgpu has
/// actually handed back an adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendererInfo {
    pub engine: String,
    pub backend: String,
    pub is_ready: bool,
    pub supports_webgpu: bool,
}

impl Default for RendererInfo {
    fn default() -> Self {
        Self {
            engine: "Vello 0.10".to_string(),
            backend: "not initialized".to_string(),
            is_ready: false,
            supports_webgpu: false,
        }
    }
}

impl RendererInfo {
    /// Builds the status report for a live surface, naming the real adapter.
    pub fn active(adapter_name: &str) -> Self {
        Self {
            engine: "Vello 0.10".to_string(),
            backend: adapter_name.to_string(),
            is_ready: true,
            supports_webgpu: true,
        }
    }

    /// Builds the status report for a pipeline that failed to come up.
    pub fn failed(reason: &str) -> Self {
        Self {
            engine: "Vello 0.10".to_string(),
            backend: reason.to_string(),
            is_ready: false,
            supports_webgpu: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_core::{AppState, FrameType, Position, Size, Style, Transform};

    #[test]
    fn test_scene_compilation_from_ecs() {
        let app_state = AppState::new();

        // 1. Spawn a rectangle frame
        let frame_id = app_state
            .spawn_frame(
                None,
                "Render Test Frame".to_string(),
                FrameType::Rectangle,
                Transform {
                    position: Position { x: 50.0, y: 50.0 },
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

        // 2. Compile scene with selection
        let world = app_state.world.read().unwrap();
        let camera = app_state.get_camera();
        let scene = SceneCompiler::compile(&world, Some(frame_id), 1, &camera);

        assert_eq!(scene.revision, 1);
        assert_eq!(scene.total_frames, 1);

        // Verify PageSurface is first
        assert!(matches!(scene.elements[0], RenderElement::PageSurface { .. }));

        // Verify RectShape exists
        let has_rect = scene
            .elements
            .iter()
            .any(|el| matches!(el, RenderElement::RectShape { id, .. } if *id == frame_id));
        assert!(has_rect);

        // Verify SelectionOverlay exists
        let has_selection = scene
            .elements
            .iter()
            .any(|el| matches!(el, RenderElement::SelectionOverlay { entity_id, .. } if *entity_id == frame_id));
        assert!(has_selection);
    }

    /// Spawns a text frame at a point and returns its entity id.
    fn text_frame_at(app_state: &AppState, x: f32, y: f32) -> u32 {
        app_state
            .spawn_frame(
                None,
                "Body".to_string(),
                FrameType::Text,
                Transform {
                    position: Position { x, y },
                    rotation: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                },
                Size { width: 200.0, height: 300.0 },
                Style::default(),
                Some("Body copy".to_string()),
            )
            .expect("should spawn a text frame")
    }

    fn compile(app_state: &AppState) -> RenderScene {
        let world = app_state.world.read().unwrap();
        let camera = app_state.get_camera();
        SceneCompiler::compile(&world, None, 1, &camera)
    }

    fn baseline_of(scene: &RenderScene, frame_id: u32) -> Option<[f32; 2]> {
        scene.elements.iter().find_map(|el| match el {
            RenderElement::TextBlock { id, baseline, .. } if *id == frame_id => Some(*baseline),
            _ => None,
        })?
    }

    #[test]
    fn a_snapping_frame_takes_the_grid_of_the_page_it_sits_on() {
        // Page 1 is a recto, so it starts at x = page_width. Its grid origin is
        // the top margin, which is what makes text on facing pages line up.
        let app_state = AppState::new();
        let settings = app_state.get_document_settings();
        let page_x = settings.width;
        let expected_origin = settings.guides.margin_top;
        let increment = settings.baseline_grid.increment;

        let id = text_frame_at(&app_state, page_x + 60.0, 100.0);
        app_state.set_frame_baseline_snap(id, true).unwrap();

        assert_eq!(
            baseline_of(&compile(&app_state), id),
            Some([increment, expected_origin])
        );
    }

    #[test]
    fn a_frame_that_does_not_snap_carries_no_grid() {
        let app_state = AppState::new();
        let page_x = app_state.get_document_settings().width;
        let id = text_frame_at(&app_state, page_x + 60.0, 100.0);

        assert_eq!(baseline_of(&compile(&app_state), id), None);
    }

    #[test]
    fn a_frame_on_the_pasteboard_has_no_grid_to_snap_to() {
        // Deliberate: a frame with no page beneath it keeps its natural leading
        // rather than borrowing the nearest page's rhythm, which would make it
        // reflow the moment it were dragged across the spine.
        let app_state = AppState::new();
        let id = text_frame_at(&app_state, -400.0, 100.0);
        app_state.set_frame_baseline_snap(id, true).unwrap();

        assert_eq!(baseline_of(&compile(&app_state), id), None);
    }

    #[test]
    fn a_disabled_grid_leaves_snapping_frames_alone() {
        // A zero increment is a value a user can type into the panel; it must
        // not reach the painter as a live grid.
        let app_state = AppState::new();
        let mut settings = app_state.get_document_settings();
        let page_x = settings.width;
        settings.baseline_grid.increment = 0.0;
        app_state.set_document_settings(settings).unwrap();

        let id = text_frame_at(&app_state, page_x + 60.0, 100.0);
        app_state.set_frame_baseline_snap(id, true).unwrap();

        assert_eq!(baseline_of(&compile(&app_state), id), None);
    }

    #[test]
    fn the_grid_is_drawn_only_when_it_is_visible() {
        // Snapping and drawing are independent, so a designer can work against
        // an invisible grid.
        let app_state = AppState::new();

        let chrome_grid = |scene: &RenderScene| {
            scene.elements.iter().find_map(|el| match el {
                RenderElement::PageChrome { baseline_grid, .. } => Some(*baseline_grid),
                _ => None,
            })
        };

        assert_eq!(chrome_grid(&compile(&app_state)), Some(None), "hidden by default");

        let mut settings = app_state.get_document_settings();
        settings.baseline_grid.visible = true;
        let expected = [settings.guides.margin_top, settings.baseline_grid.increment];
        app_state.set_document_settings(settings).unwrap();

        assert_eq!(chrome_grid(&compile(&app_state)), Some(Some(expected)));
    }

    #[test]
    fn compiled_scenes_paint_without_a_gpu() {
        // Guards the compiler/painter contract: every RenderElement the compiler
        // can emit must be paintable, and this runs on machines with no adapter.
        let app_state = AppState::new();
        for (name, frame_type) in [
            ("Rect", FrameType::Rectangle),
            ("Ellipse", FrameType::Ellipse),
            ("Text", FrameType::Text),
            ("Image", FrameType::Image),
        ] {
            app_state
                .spawn_frame(
                    None,
                    name.to_string(),
                    frame_type,
                    Transform::default(),
                    Size {
                        width: 120.0,
                        height: 60.0,
                    },
                    Style::default(),
                    None,
                )
                .expect("Should spawn frame");
        }

        let world = app_state.world.read().unwrap();
        let camera = app_state.get_camera();
        let scene = SceneCompiler::compile(&world, None, 1, &camera);
        let painted = paint::Painter::new().paint(&scene);

        assert_eq!(scene.total_frames, 4);
        assert!(painted.encoding().n_paths > 0);
    }

    #[test]
    fn every_page_gets_a_surface_and_chrome() {
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        app_state.add_page().unwrap();

        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        let surfaces = scene
            .elements
            .iter()
            .filter(|e| matches!(e, RenderElement::PageSurface { .. }))
            .count();
        let chrome = scene
            .elements
            .iter()
            .filter(|e| matches!(e, RenderElement::PageChrome { .. }))
            .count();

        assert_eq!(surfaces, 3);
        assert_eq!(chrome, 3, "each page carries its own guides");
    }

    #[test]
    fn pages_are_placed_where_the_spread_layout_puts_them() {
        // Page 1 is a lone recto right of the spine; page 2 is the verso of the
        // next spread, so it sits at x=0 and one spread lower.
        let app_state = AppState::new();
        app_state.add_page().unwrap();

        let settings = app_state.get_document_settings();
        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        let mut surfaces = scene.elements.iter().filter_map(|e| match e {
            RenderElement::PageSurface { page_number, x, y, .. } => Some((*page_number, *x, *y)),
            _ => None,
        });

        let (first_number, first_x, first_y) = surfaces.next().unwrap();
        let (second_number, second_x, second_y) = surfaces.next().unwrap();

        assert_eq!(first_number, 1);
        assert_eq!(first_x, settings.width, "page 1 sits right of the spine");
        assert_eq!(first_y, 0.0);

        assert_eq!(second_number, 2);
        assert_eq!(second_x, 0.0, "page 2 is a verso");
        assert!(second_y > first_y, "later spreads sit lower");
    }

    #[test]
    fn page_chrome_nests_margins_inside_trim_inside_bleed() {
        let app_state = AppState::new();
        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        let (surface, chrome) = scene
            .elements
            .iter()
            .find_map(|e| match e {
                RenderElement::PageChrome { bleed, margins, .. } => Some((bleed, margins)),
                _ => None,
            })
            .map(|(bleed, margins)| (*bleed, *margins))
            .unwrap();

        let trim = scene
            .elements
            .iter()
            .find_map(|e| match e {
                RenderElement::PageSurface { x, y, width, height, .. } => {
                    Some([*x, *y, *x + *width, *y + *height])
                }
                _ => None,
            })
            .unwrap();

        // Bleed is outside the trim on every side.
        assert!(surface[0] < trim[0] && surface[1] < trim[1]);
        assert!(surface[2] > trim[2] && surface[3] > trim[3]);
        // Margins are inside it.
        assert!(chrome[0] > trim[0] && chrome[1] > trim[1]);
        assert!(chrome[2] < trim[2] && chrome[3] < trim[3]);
    }

    #[test]
    fn master_items_are_inherited_onto_pages_that_apply_them() {
        // A folio placed once on a master must appear on every page using it,
        // offset to each page's position on the pasteboard.
        let app_state = AppState::new();
        app_state.add_page().unwrap();
        let master = app_state
            .create_master_page("A-Master".to_string(), "A".to_string())
            .unwrap();
        app_state
            .spawn_master_frame(
                master,
                "Folio".to_string(),
                FrameType::Rectangle,
                Transform {
                    position: Position { x: 10.0, y: 20.0 },
                    ..Default::default()
                },
                Size { width: 40.0, height: 15.0 },
                Style::default(),
                None,
            )
            .unwrap();
        app_state.apply_master_to_page(1, master).unwrap();
        app_state.apply_master_to_page(2, master).unwrap();

        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        let folios: Vec<_> = scene
            .elements
            .iter()
            .filter_map(|e| match e {
                RenderElement::RectShape { x, y, width, .. } if *width == 40.0 => Some((*x, *y)),
                _ => None,
            })
            .collect();

        assert_eq!(folios.len(), 2, "one inherited item per page");
        assert_ne!(folios[0], folios[1], "each lands on its own page");
    }

    #[test]
    fn a_page_without_a_master_inherits_nothing() {
        let app_state = AppState::new();
        let master = app_state
            .create_master_page("A-Master".to_string(), "A".to_string())
            .unwrap();
        app_state
            .spawn_master_frame(
                master,
                "Folio".to_string(),
                FrameType::Rectangle,
                Transform::default(),
                Size { width: 40.0, height: 15.0 },
                Style::default(),
                None,
            )
            .unwrap();

        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        assert!(
            !scene
                .elements
                .iter()
                .any(|e| matches!(e, RenderElement::RectShape { width, .. } if *width == 40.0)),
            "an unapplied master must not render"
        );
    }

    #[test]
    fn a_rectangle_is_square_unless_its_style_asks_for_rounding() {
        let app_state = AppState::new();
        let square = app_state
            .spawn_frame(
                None,
                "Square".to_string(),
                FrameType::Rectangle,
                Transform::default(),
                Size { width: 50.0, height: 50.0 },
                Style::default(),
                None,
            )
            .unwrap();

        let rounded = app_state
            .spawn_frame(
                None,
                "Rounded".to_string(),
                FrameType::Rectangle,
                Transform::default(),
                Size { width: 60.0, height: 60.0 },
                Style {
                    corner_radius: 12.0,
                    ..Default::default()
                },
                None,
            )
            .unwrap();

        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        let radius_of = |wanted: u32| {
            scene
                .elements
                .iter()
                .find_map(|e| match e {
                    RenderElement::RectShape { id, corner_radius, .. } if *id == wanted => {
                        Some(*corner_radius)
                    }
                    _ => None,
                })
                .expect("frame should be in the scene")
        };

        assert_eq!(radius_of(square), 0.0, "a default rectangle must be square");
        assert_eq!(radius_of(rounded), 12.0, "rounding must come from the style");
    }

    #[test]
    fn an_overridden_master_item_is_not_drawn_twice() {
        let app_state = AppState::new();
        let master = app_state
            .create_master_page("A-Master".to_string(), "A".to_string())
            .unwrap();
        let item = app_state
            .spawn_master_frame(
                master,
                "Folio".to_string(),
                FrameType::Rectangle,
                Transform::default(),
                Size { width: 40.0, height: 15.0 },
                Style::default(),
                None,
            )
            .unwrap();
        app_state.apply_master_to_page(1, master).unwrap();
        app_state.override_master_item(1, item).unwrap();

        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        let count = scene
            .elements
            .iter()
            .filter(|e| matches!(e, RenderElement::RectShape { width, .. } if *width == 40.0))
            .count();
        assert_eq!(count, 1, "the override replaces the inherited item");
    }

    #[test]
    fn threaded_frames_share_one_story() {
        let app_state = AppState::new();
        let head = app_state
            .spawn_frame(
                None,
                "Head".to_string(),
                FrameType::Text,
                Transform::default(),
                Size { width: 100.0, height: 40.0 },
                Style::default(),
                Some("a long story that will not fit in one frame".to_string()),
            )
            .unwrap();
        let tail = app_state
            .spawn_frame(
                None,
                "Tail".to_string(),
                FrameType::Text,
                Transform::default(),
                Size { width: 100.0, height: 40.0 },
                Style::default(),
                None,
            )
            .unwrap();
        app_state.thread_text_frames(head, tail).unwrap();

        let world = app_state.world.read().unwrap();
        let scene = SceneCompiler::compile(&world, None, 1, &app_state.get_camera());

        assert_eq!(scene.stories.len(), 1);
        assert_eq!(scene.stories[0].id, head);
        assert_eq!(scene.stories[0].frames.len(), 2);

        let refs: Vec<_> = scene
            .elements
            .iter()
            .filter_map(|e| match e {
                RenderElement::TextBlock { id, story, .. } => Some((*id, *story)),
                _ => None,
            })
            .collect();
        assert!(refs.contains(&(head, Some([head, 0]))));
        assert!(refs.contains(&(tail, Some([head, 1]))));
    }

    #[test]
    fn renderer_info_defaults_to_not_ready() {
        // The old implementation hard-coded is_ready: true regardless of state.
        let info = RendererInfo::default();
        assert!(!info.is_ready);
        assert!(!info.supports_webgpu);
    }

    #[test]
    fn renderer_info_reports_the_real_adapter() {
        let info = RendererInfo::active("NVIDIA GeForce RTX 4070");
        assert!(info.is_ready);
        assert_eq!(info.backend, "NVIDIA GeForce RTX 4070");
    }
}

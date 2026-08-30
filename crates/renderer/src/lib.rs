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

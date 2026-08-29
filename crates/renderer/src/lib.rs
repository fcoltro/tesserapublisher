pub mod scene;

pub use scene::*;

use serde::{Deserialize, Serialize};

/// Renderer engine status and capability info
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
            engine: "Vello + WebGPU Scene Pipeline".to_string(),
            backend: "wgpu/vello + Canvas Context".to_string(),
            is_ready: true,
            supports_webgpu: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_core::{
        AppState, FrameType, Position, Size, Style, Transform,
    };

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
}

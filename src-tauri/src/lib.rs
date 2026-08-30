pub mod render_host;
pub mod state;

use serde::{Deserialize, Serialize};
use state::{
    AppState, DocumentTreeSnapshot, FrameType, HitTestResult, Position, Size, Style, Transform,
};
use render_host::RenderHost;
use tauri::{Manager, State};
use tessera_core::HistoryStatus;
use tessera_renderer::{RendererInfo, Viewport};

#[derive(Debug, Serialize, Deserialize)]
pub struct EcsStatus {
    pub entity_count: usize,
    pub is_initialized: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub version: String,
    pub crates: Vec<String>,
    pub renderer: RendererInfo,
    pub is_ipc_ready: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonBridgeTestPayload {
    pub message: String,
    pub timestamp_ms: u64,
    pub items: Vec<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonBridgeTestResponse {
    pub received_payload: JsonBridgeTestPayload,
    pub server_timestamp_ms: u64,
    pub success: bool,
    pub echo_summary: String,
}

/// IPC string test command
#[tauri::command]
fn echo_string(input: String) -> String {
    format!("Rust backend received: \"{}\"", input)
}

/// IPC JSON test command
#[tauri::command]
fn test_json_bridge(payload: JsonBridgeTestPayload) -> JsonBridgeTestResponse {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let summary = format!(
        "Payload '{}' processed with {} items",
        payload.message,
        payload.items.len()
    );

    JsonBridgeTestResponse {
        received_payload: payload,
        server_timestamp_ms: now,
        success: true,
        echo_summary: summary,
    }
}

/// IPC workspace info command
#[tauri::command]
fn get_workspace_info() -> WorkspaceInfo {
    WorkspaceInfo {
        version: "0.1.0".to_string(),
        crates: vec![
            "tessera-publish (app)".to_string(),
            "tessera-core".to_string(),
            "tessera-renderer".to_string(),
        ],
        renderer: RendererInfo::default(),
        is_ipc_ready: true,
    }
}

#[tauri::command]
fn get_ecs_status(state: State<AppState>) -> Result<EcsStatus, String> {
    let world = state.world.read().map_err(|e| e.to_string())?;
    Ok(EcsStatus {
        entity_count: world.entities().len() as usize,
        is_initialized: true,
        message: "Bevy ECS World is active with RwLock thread safety".into(),
    })
}

#[tauri::command]
fn query_document_tree(state: State<AppState>) -> Result<DocumentTreeSnapshot, String> {
    state.get_document_tree()
}

#[tauri::command]
fn spawn_frame(
    name: String,
    frame_type: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill_color: [f32; 4],
    text: Option<String>,
    state: State<AppState>,
) -> Result<u32, String> {
    let f_type = match frame_type.to_lowercase().as_str() {
        "ellipse" => FrameType::Ellipse,
        "text" => FrameType::Text,
        "image" => FrameType::Image,
        "line" => FrameType::Line,
        "path" => FrameType::Path,
        _ => FrameType::Rectangle,
    };

    let transform = Transform {
        position: Position { x, y },
        rotation: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
    };

    let size = Size { width, height };

    let style = Style {
        fill_color,
        stroke_color: Some([0.4, 0.6, 1.0, 1.0]),
        stroke_width: 1.5,
        opacity: 1.0,
    };

    state.spawn_frame(None, name, f_type, transform, size, style, text)
}

#[tauri::command]
fn hit_test_point(x: f32, y: f32, state: State<AppState>) -> Result<Vec<HitTestResult>, String> {
    state.hit_test(x, y)
}

#[tauri::command]
fn undo_action(state: State<AppState>) -> Result<HistoryStatus, String> {
    state.undo()
}

#[tauri::command]
fn redo_action(state: State<AppState>) -> Result<HistoryStatus, String> {
    state.redo()
}

#[tauri::command]
fn get_history_status(state: State<AppState>) -> Result<HistoryStatus, String> {
    state.get_history_status()
}

#[tauri::command]
fn get_camera_state(state: State<AppState>) -> tessera_core::Camera {
    state.get_camera()
}

#[tauri::command]
fn pan_camera(dx: f32, dy: f32, state: State<AppState>) -> tessera_core::Camera {
    state.pan_camera(dx, dy)
}

#[tauri::command]
fn zoom_camera(
    screen_x: f32,
    screen_y: f32,
    factor: f32,
    state: State<AppState>,
) -> tessera_core::Camera {
    state.zoom_camera(screen_x, screen_y, factor)
}

#[tauri::command]
fn fit_page_view(
    viewport_width: f32,
    viewport_height: f32,
    state: State<AppState>,
) -> tessera_core::Camera {
    state.fit_page_view(viewport_width, viewport_height)
}

#[tauri::command]
fn reset_camera(state: State<AppState>) -> tessera_core::Camera {
    state.reset_camera()
}

#[tauri::command]
fn raycast_select_entity(
    screen_x: f32,
    screen_y: f32,
    state: State<AppState>,
) -> Option<u32> {
    state.raycast_select_entity(screen_x, screen_y)
}

#[tauri::command]
fn compile_render_scene(
    selected_id: Option<u32>,
    state: State<AppState>,
) -> Result<tessera_renderer::RenderScene, String> {
    let world = state.world.read().map_err(|e| e.to_string())?;
    let rev = state.get_scene_revision();
    let camera = state.get_camera();
    Ok(tessera_renderer::SceneCompiler::compile(
        &world,
        selected_id,
        rev,
        &camera,
    ))
}

#[tauri::command]
fn get_scene_revision(state: State<AppState>) -> u64 {
    state.get_scene_revision()
}

/// Brings up the vello pipeline against the application window.
///
/// The frontend calls this once the canvas element has been laid out, so the
/// surface is created at the correct size on the first try.
#[tauri::command]
fn init_renderer(window: tauri::WebviewWindow, host: State<RenderHost>) -> RendererInfo {
    let size = match window.inner_size() {
        Ok(size) => size,
        Err(e) => return tessera_renderer::RendererInfo::failed(&e.to_string()),
    };

    // DisplayAndWindow (rather than Window) is required on X11/Wayland, where a
    // surface cannot be created from the window handle alone.
    let target =
        vello::wgpu::SurfaceTarget::DisplayAndWindow(Box::new(window.clone()));
    host.initialize(target, size.width, size.height)
}

/// Reports the current renderer status without attempting initialization.
#[tauri::command]
fn get_renderer_info(host: State<RenderHost>) -> RendererInfo {
    host.info()
}

/// Records the canvas rectangle, in physical pixels, that the document is drawn into.
#[tauri::command]
fn set_viewport_rect(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    host: State<RenderHost>,
) {
    host.set_viewport(Viewport {
        x,
        y,
        width,
        height,
    });
}

/// Compiles the current ECS state and presents one frame.
///
/// This is the only path that paints. It is called on demand after a state or
/// camera change rather than on a timer, so an idle document costs no GPU work.
#[tauri::command]
fn render_frame(
    selected_id: Option<u32>,
    state: State<AppState>,
    host: State<RenderHost>,
) -> Result<bool, String> {
    let scene = {
        let world = state.world.read().map_err(|e| e.to_string())?;
        let rev = state.get_scene_revision();
        let camera = state.get_camera();
        tessera_renderer::SceneCompiler::compile(&world, selected_id, rev, &camera)
    };
    host.render(&scene)
}

/// A frame's geometry, flattened for the IPC bridge.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FrameGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
}

impl FrameGeometry {
    fn split(self) -> (Transform, Size) {
        (
            Transform {
                position: Position { x: self.x, y: self.y },
                rotation: self.rotation,
                scale_x: self.scale_x,
                scale_y: self.scale_y,
            },
            Size {
                width: self.width,
                height: self.height,
            },
        )
    }

    fn join(transform: Transform, size: Size) -> Self {
        Self {
            x: transform.position.x,
            y: transform.position.y,
            width: size.width,
            height: size.height,
            rotation: transform.rotation,
            scale_x: transform.scale_x,
            scale_y: transform.scale_y,
        }
    }
}

/// Reads a frame's geometry, for capturing the start of a drag.
#[tauri::command]
fn get_frame_geometry(entity_id: u32, state: State<AppState>) -> Result<FrameGeometry, String> {
    let (transform, size) = state.get_frame_geometry(entity_id)?;
    Ok(FrameGeometry::join(transform, size))
}

/// Applies geometry live during a drag, without recording history.
#[tauri::command]
fn set_frame_geometry(
    entity_id: u32,
    geometry: FrameGeometry,
    state: State<AppState>,
) -> Result<(), String> {
    let (transform, size) = geometry.split();
    state.set_frame_geometry(entity_id, transform, size)?;
    Ok(())
}

/// Records a finished drag as a single undoable action.
#[tauri::command]
fn commit_frame_geometry(
    entity_id: u32,
    before: FrameGeometry,
    after: FrameGeometry,
    state: State<AppState>,
) -> Result<HistoryStatus, String> {
    let (old_transform, old_size) = before.split();
    let (new_transform, new_size) = after.split();
    state.commit_frame_geometry(entity_id, old_transform, old_size, new_transform, new_size)
}

/// Replaces a path frame's bezier outline.
#[tauri::command]
fn set_frame_path(entity_id: u32, svg: String, state: State<AppState>) -> Result<(), String> {
    state.set_frame_path(entity_id, svg)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Register Bevy ECS World wrapped in RwLock as Tauri managed state
        .manage(AppState::new())
        .manage(RenderHost::new())
        .invoke_handler(tauri::generate_handler![
            echo_string,
            test_json_bridge,
            get_workspace_info,
            get_ecs_status,
            query_document_tree,
            spawn_frame,
            hit_test_point,
            undo_action,
            redo_action,
            get_history_status,
            compile_render_scene,
            get_scene_revision,
            get_camera_state,
            pan_camera,
            zoom_camera,
            fit_page_view,
            reset_camera,
            raycast_select_entity,
            init_renderer,
            get_renderer_info,
            set_viewport_rect,
            render_frame,
            get_frame_geometry,
            set_frame_geometry,
            commit_frame_geometry,
            set_frame_path
        ])
        .on_window_event(|window, event| {
            // Keeping the swapchain in step with the window avoids a stretched
            // frame between the resize and the next redraw request.
            if let tauri::WindowEvent::Resized(size) = event {
                if let Some(host) = window.try_state::<RenderHost>() {
                    host.resize(size.width, size.height);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

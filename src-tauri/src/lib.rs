pub mod render_host;
pub mod state;

use serde::{Deserialize, Serialize};
use state::{
    AppState, DocumentTreeSnapshot, FrameType, HitTestResult, Position, Size, Style, TextContent,
    Transform,
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
        tessera_renderer::SceneCompiler::compile_with_snap(
            &world,
            selected_id,
            rev,
            &camera,
            state.get_active_snap(),
        )
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

/// Reads a text frame's content and type settings for the inspector.
#[tauri::command]
fn get_frame_text(entity_id: u32, state: State<AppState>) -> Result<TextContent, String> {
    state.get_frame_text(entity_id)
}

/// Live path for typography edits; does not record history.
#[tauri::command]
fn set_frame_text(
    entity_id: u32,
    text: TextContent,
    state: State<AppState>,
) -> Result<(), String> {
    state.set_frame_text(entity_id, text)
}

/// Records a finished typography edit as a single undoable action.
#[tauri::command]
fn commit_frame_text(
    entity_id: u32,
    before: TextContent,
    after: TextContent,
    state: State<AppState>,
) -> Result<HistoryStatus, String> {
    state.commit_frame_text(entity_id, before, after)
}

/// Reads a frame's paint settings for the inspector.
#[tauri::command]
fn get_frame_style(entity_id: u32, state: State<AppState>) -> Result<Style, String> {
    state.get_frame_style(entity_id)
}

/// Live path for inspector edits; does not record history.
#[tauri::command]
fn set_frame_style(entity_id: u32, style: Style, state: State<AppState>) -> Result<(), String> {
    state.set_frame_style(entity_id, style)
}

/// Records a finished style edit as a single undoable action.
#[tauri::command]
fn commit_frame_style(
    entity_id: u32,
    before: Style,
    after: Style,
    state: State<AppState>,
) -> Result<HistoryStatus, String> {
    state.commit_frame_style(entity_id, before, after)
}

/// Replaces a path frame's bezier outline.
#[tauri::command]
fn set_frame_path(entity_id: u32, svg: String, state: State<AppState>) -> Result<(), String> {
    state.set_frame_path(entity_id, svg)?;
    Ok(())
}

// --- Phase 3: document architecture -------------------------------------

#[tauri::command]
fn get_document_settings(state: State<AppState>) -> tessera_core::Document {
    state.get_document_settings()
}

#[tauri::command]
fn set_document_settings(
    settings: tessera_core::Document,
    state: State<AppState>,
) -> Result<(), String> {
    state.set_document_settings(settings)
}

#[tauri::command]
fn get_page_placements(state: State<AppState>) -> Vec<tessera_core::PagePlacement> {
    state.page_placements()
}

#[tauri::command]
fn add_page(state: State<AppState>) -> Result<u32, String> {
    state.add_page()
}

#[tauri::command]
fn remove_page(page_number: u32, state: State<AppState>) -> Result<u32, String> {
    state.remove_page(page_number)
}

// --- Master pages --------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct MasterPageSummary {
    pub id: u32,
    pub name: String,
    pub prefix: String,
}

#[tauri::command]
fn create_master_page(
    name: String,
    prefix: String,
    state: State<AppState>,
) -> Result<u32, String> {
    state.create_master_page(name, prefix)
}

#[tauri::command]
fn list_master_pages(state: State<AppState>) -> Vec<MasterPageSummary> {
    state
        .master_pages()
        .into_iter()
        .map(|(id, master)| MasterPageSummary {
            id,
            name: master.name,
            prefix: master.prefix,
        })
        .collect()
}

#[tauri::command]
fn apply_master_to_page(
    page_number: u32,
    master_id: u32,
    state: State<AppState>,
) -> Result<(), String> {
    state.apply_master_to_page(page_number, master_id)
}

#[tauri::command]
fn detach_master_from_page(page_number: u32, state: State<AppState>) -> Result<(), String> {
    state.detach_master_from_page(page_number)
}

#[tauri::command]
fn override_master_item(
    page_number: u32,
    master_frame_id: u32,
    state: State<AppState>,
) -> Result<u32, String> {
    state.override_master_item(page_number, master_frame_id)
}

// --- Text threading ------------------------------------------------------

#[tauri::command]
fn thread_text_frames(from: u32, to: u32, state: State<AppState>) -> Result<(), String> {
    state.thread_text_frames(from, to)
}

#[tauri::command]
fn unthread_text_frame(from: u32, state: State<AppState>) -> Result<(), String> {
    state.unthread_text_frame(from)
}

#[tauri::command]
fn get_text_story_chain(entity_id: u32, state: State<AppState>) -> Vec<u32> {
    state.text_story_chain(entity_id)
}

// --- Guides and snapping -------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct RulerGuideSummary {
    pub id: u32,
    pub is_vertical: bool,
    pub position: f32,
}

#[tauri::command]
fn add_ruler_guide(
    is_vertical: bool,
    position: f32,
    state: State<AppState>,
) -> Result<u32, String> {
    let axis = if is_vertical {
        tessera_core::GuideAxis::Vertical
    } else {
        tessera_core::GuideAxis::Horizontal
    };
    state.add_ruler_guide(axis, position)
}

#[tauri::command]
fn remove_ruler_guide(entity_id: u32, state: State<AppState>) -> Result<(), String> {
    state.remove_ruler_guide(entity_id)
}

#[tauri::command]
fn list_ruler_guides(state: State<AppState>) -> Vec<RulerGuideSummary> {
    state
        .ruler_guides()
        .into_iter()
        .map(|(id, guide)| RulerGuideSummary {
            id,
            is_vertical: guide.axis == tessera_core::GuideAxis::Vertical,
            position: guide.position,
        })
        .collect()
}

/// Geometry corrected by snapping, plus which lines caught it.
#[derive(Debug, Serialize, Deserialize)]
pub struct SnappedGeometry {
    pub geometry: FrameGeometry,
    pub snapped: bool,
}

/// Snaps proposed drag geometry without writing it to the document.
#[tauri::command]
fn snap_frame_geometry(
    entity_id: u32,
    geometry: FrameGeometry,
    threshold_px: Option<f32>,
    state: State<AppState>,
) -> Result<SnappedGeometry, String> {
    let (transform, size) = geometry.split();
    let zoom = state.get_camera().zoom;
    let threshold = threshold_px.unwrap_or(tessera_core::DEFAULT_SNAP_THRESHOLD_PX);

    let (snapped_transform, result) =
        state.snap_frame_geometry(entity_id, transform, size, zoom, threshold)?;

    Ok(SnappedGeometry {
        geometry: FrameGeometry::join(snapped_transform, size),
        snapped: result.is_snapped(),
    })
}

/// Clears snap feedback when a gesture ends.
/// Turns baseline-grid locking on or off for one text frame.
///
/// The grid itself rides on `Document`, so it is configured through
/// `set_document_settings`; only the per-frame opt-in needs a command.
#[tauri::command]
fn set_frame_baseline_snap(
    entity_id: u32,
    enabled: bool,
    state: State<AppState>,
) -> Result<(), String> {
    state.set_frame_baseline_snap(entity_id, enabled)
}

#[tauri::command]
fn get_frame_baseline_snap(entity_id: u32, state: State<AppState>) -> Result<bool, String> {
    state.frame_baseline_snap(entity_id)
}

#[tauri::command]
fn clear_active_snap(state: State<AppState>) {
    state.clear_active_snap();
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
            set_frame_path,
            get_frame_style,
            set_frame_style,
            commit_frame_style,
            get_frame_text,
            set_frame_text,
            commit_frame_text,
            get_document_settings,
            set_document_settings,
            get_page_placements,
            add_page,
            remove_page,
            create_master_page,
            list_master_pages,
            apply_master_to_page,
            detach_master_from_page,
            override_master_item,
            thread_text_frames,
            unthread_text_frame,
            get_text_story_chain,
            add_ruler_guide,
            remove_ruler_guide,
            list_ruler_guides,
            snap_frame_geometry,
            clear_active_snap,
            set_frame_baseline_snap,
            get_frame_baseline_snap
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

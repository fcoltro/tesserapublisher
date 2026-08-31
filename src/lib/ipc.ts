/**
 * The typed boundary between Svelte and Rust.
 *
 * Every `invoke` in the application goes through this module. Keeping the
 * command names in one place means a renamed Rust command breaks the build
 * here rather than failing silently at runtime in whichever panel happened to
 * call it.
 *
 * Types mirror the serde representation of the ECS components in
 * `crates/core/src/components.rs`. Rust unit enums serialize as their variant
 * name, and `Option<T>` as `T | null`.
 */
import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Editing surface
// ---------------------------------------------------------------------------

/** The active editing tool. Creation tools drag out a new frame; Select picks and moves existing ones. */
export type Tool = "Select" | "Rectangle" | "Ellipse" | "Line" | "Text";

/** What the current pointer gesture is doing. */
export type DragMode = "none" | "pan" | "create" | "move" | "resize";

export type FrameType = "Rectangle" | "Ellipse" | "Text" | "Image" | "Line" | "Path";

export type TextAlignment = "Start" | "Center" | "End" | "Justify";

/** How a linked image maps into the frame box that holds it. */
export type ImageFit = "Fill" | "Fit" | "Stretch";

// ---------------------------------------------------------------------------
// Component mirrors
// ---------------------------------------------------------------------------

export type Rgba = [number, number, number, number];

export interface Position {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface Transform {
  position: Position;
  rotation: number;
  scale_x: number;
  scale_y: number;
}

export interface BoundingBox {
  min_x: number;
  min_y: number;
  max_x: number;
  max_y: number;
}

/** Paint settings for a frame. */
export interface Style {
  fill_color: Rgba;
  stroke_color: Rgba | null;
  stroke_width: number;
  opacity: number;
  /** Corner rounding in document units. Zero draws true square corners. */
  corner_radius: number;
}

/** Text content and type settings for a Text frame. */
export interface TextContent {
  text: string;
  font_size: number;
  /** Leading, as a multiple of font size. */
  line_height: number;
  align: TextAlignment;
  /** Preferred family name; null uses the system default. */
  font_family: string | null;
  /** CSS-style numeric weight, where 400 is regular and 700 is bold. */
  font_weight: number;
  /** Letter spacing in thousandths of an em, the unit layout tools use. */
  tracking: number;
  /** Lock every line onto the document's baseline grid. */
  snap_to_baseline: boolean;
}

/** A linked raster image. Only the path is stored, never the pixels. */
export interface ImageSource {
  path: string;
  natural_width: number;
  natural_height: number;
  fit: ImageFit;
}

/** One linked image, as the assets panel shows it. */
export interface AssetSummary {
  entity_id: number;
  path: string;
  file_name: string;
  natural_width: number;
  natural_height: number;
  /** Width the image occupies on the page, in points. */
  placed_width: number;
  /** Resolution the image actually prints at, given that placed width. */
  effective_ppi: number;
  fit: ImageFit;
  status: "Ok" | "Missing";
}

/** A frame's geometry as carried over the IPC bridge. */
export interface FrameGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
  scale_x: number;
  scale_y: number;
}

/** Geometry returned by the snapping pass. */
export interface SnappedGeometry {
  geometry: FrameGeometry;
  snapped: boolean;
}

// ---------------------------------------------------------------------------
// Document tree
// ---------------------------------------------------------------------------

export interface FrameNode {
  id: number;
  name: string;
  frame_type: FrameType;
  transform: Transform;
  size: Size;
  z_index: number;
  bounding_box: BoundingBox;
  style: Style;
  text: string | null;
}

export interface LayerNode {
  id: number;
  name: string;
  z_index: number;
  is_visible: boolean;
  is_locked: boolean;
  frames: FrameNode[];
}

export interface PageNode {
  id: number;
  page_number: number;
  width: number;
  height: number;
  layers: LayerNode[];
}

export interface DocumentTreeSnapshot {
  document_id: number;
  title: string;
  width: number;
  height: number;
  dpi: number;
  bleed: number;
  pages: PageNode[];
  total_entities: number;
}

/** A page's placement on the pasteboard. */
export interface PagePlacement {
  page_number: number;
  spread_index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  is_left: boolean;
}

/** The document's vertical rhythm for typography. */
export interface BaselineGrid {
  increment: number;
  start: number;
  visible: boolean;
}

/**
 * Document settings, mirrored from the Rust `Document` component.
 *
 * Only the fields the UI reads are named; the rest ride along untouched so a
 * round trip through `setDocumentSettings` cannot drop them.
 */
export interface DocumentSettings {
  baseline_grid: BaselineGrid;
  [key: string]: unknown;
}

export interface MasterPageSummary {
  id: number;
  name: string;
  prefix: string;
}

export interface RulerGuideSummary {
  id: number;
  axis: string;
  position: number;
}

// ---------------------------------------------------------------------------
// Camera, history, renderer
// ---------------------------------------------------------------------------

export interface Camera {
  pan_x: number;
  pan_y: number;
  zoom: number;
  viewport_width: number;
  viewport_height: number;
}

export interface HistoryStatus {
  undo_count: number;
  redo_count: number;
  can_undo: boolean;
  can_redo: boolean;
}

/** Live status of the Rust-side vello pipeline. */
export interface RendererInfo {
  engine: string;
  backend: string;
  is_ready: boolean;
  supports_webgpu: boolean;
}

// ---------------------------------------------------------------------------
// Commands — renderer
// ---------------------------------------------------------------------------

export const initRenderer = () => invoke<RendererInfo>("init_renderer");

export const setViewportRect = (x: number, y: number, width: number, height: number) =>
  invoke("set_viewport_rect", { x, y, width, height });

export const renderFrame = (selectedId: number | null) =>
  invoke<boolean>("render_frame", { selectedId });

export const getSceneRevision = () => invoke<number>("get_scene_revision");

// ---------------------------------------------------------------------------
// Commands — camera
// ---------------------------------------------------------------------------

export const getCameraState = () => invoke<Camera>("get_camera_state");

export const panCamera = (dx: number, dy: number) => invoke<Camera>("pan_camera", { dx, dy });

export const zoomCamera = (screenX: number, screenY: number, factor: number) =>
  invoke<Camera>("zoom_camera", { screenX, screenY, factor });

export const fitPageView = (viewportWidth: number, viewportHeight: number) =>
  invoke<Camera>("fit_page_view", { viewportWidth, viewportHeight });

export const resetCamera = () => invoke<Camera>("reset_camera");

// ---------------------------------------------------------------------------
// Commands — selection and history
// ---------------------------------------------------------------------------

export const raycastSelectEntity = (screenX: number, screenY: number) =>
  invoke<number | null>("raycast_select_entity", { screenX, screenY });

export const undoAction = () => invoke<HistoryStatus>("undo_action");

export const redoAction = () => invoke<HistoryStatus>("redo_action");

export const getHistoryStatus = () => invoke<HistoryStatus>("get_history_status");

// ---------------------------------------------------------------------------
// Commands — frames
// ---------------------------------------------------------------------------

export interface SpawnFrameArgs {
  name: string;
  frameType: string;
  x: number;
  y: number;
  width: number;
  height: number;
  fillColor: Rgba;
  text?: string | null;
  /** Tauri requires argument records to be indexable. */
  [key: string]: unknown;
}

export const spawnFrame = (args: SpawnFrameArgs) => invoke<number>("spawn_frame", args);

export const queryDocumentTree = () => invoke<DocumentTreeSnapshot>("query_document_tree");

export const getFrameGeometry = (entityId: number) =>
  invoke<FrameGeometry>("get_frame_geometry", { entityId });

export const setFrameGeometry = (entityId: number, geometry: FrameGeometry) =>
  invoke("set_frame_geometry", { entityId, geometry });

export const commitFrameGeometry = (
  entityId: number,
  before: FrameGeometry,
  after: FrameGeometry,
) => invoke<HistoryStatus>("commit_frame_geometry", { entityId, before, after });

export const setFramePath = (entityId: number, svg: string) =>
  invoke("set_frame_path", { entityId, svg });

// Style — the live/commit split keeps one scrub gesture as one undo entry.
export const getFrameStyle = (entityId: number) => invoke<Style>("get_frame_style", { entityId });

export const setFrameStyle = (entityId: number, style: Style) =>
  invoke("set_frame_style", { entityId, style });

export const commitFrameStyle = (entityId: number, before: Style, after: Style) =>
  invoke<HistoryStatus>("commit_frame_style", { entityId, before, after });

// Text — same split as style.
export const getFrameText = (entityId: number) =>
  invoke<TextContent>("get_frame_text", { entityId });

export const setFrameText = (entityId: number, text: TextContent) =>
  invoke("set_frame_text", { entityId, text });

export const commitFrameText = (entityId: number, before: TextContent, after: TextContent) =>
  invoke<HistoryStatus>("commit_frame_text", { entityId, before, after });

// ---------------------------------------------------------------------------
// Commands — linked images
// ---------------------------------------------------------------------------

export const placeImage = (path: string, x: number, y: number, maxEdge?: number) =>
  invoke<number>("place_image", { path, x, y, maxEdge });

export const getImageSource = (entityId: number) =>
  invoke<ImageSource>("get_image_source", { entityId });

export const setImageFit = (entityId: number, fit: ImageFit) =>
  invoke("set_image_fit", { entityId, fit });

export const relinkImage = (entityId: number, path: string) =>
  invoke("relink_image", { entityId, path });

export const listLinkedAssets = () => invoke<AssetSummary[]>("list_linked_assets");

// ---------------------------------------------------------------------------
// Commands — structure
// ---------------------------------------------------------------------------

export const setLayerVisibility = (layerId: number, visible: boolean) =>
  invoke("set_layer_visibility", { layerId, visible });

export const setLayerLocked = (layerId: number, locked: boolean) =>
  invoke("set_layer_locked", { layerId, locked });

export const setFrameZIndex = (entityId: number, z: number) =>
  invoke("set_frame_z_index", { entityId, z });

export const renameFrame = (entityId: number, name: string) =>
  invoke("rename_frame", { entityId, name });

export const deleteFrame = (entityId: number) =>
  invoke<HistoryStatus>("delete_frame", { entityId });

// ---------------------------------------------------------------------------
// Commands — document, pages, masters
// ---------------------------------------------------------------------------

export const getDocumentSettings = () => invoke<DocumentSettings>("get_document_settings");

export const setDocumentSettings = (settings: DocumentSettings) =>
  invoke("set_document_settings", { settings });

export const getPagePlacements = () => invoke<PagePlacement[]>("get_page_placements");

export const addPage = () => invoke<number>("add_page");

export const removePage = (pageNumber: number) => invoke<number>("remove_page", { pageNumber });

export const listMasterPages = () => invoke<MasterPageSummary[]>("list_master_pages");

export const applyMasterToPage = (pageNumber: number, masterId: number) =>
  invoke("apply_master_to_page", { pageNumber, masterId });

export const detachMasterFromPage = (pageNumber: number) =>
  invoke("detach_master_from_page", { pageNumber });

// ---------------------------------------------------------------------------
// Commands — text threading
// ---------------------------------------------------------------------------

export const threadTextFrames = (from: number, to: number) =>
  invoke("thread_text_frames", { from, to });

export const unthreadTextFrame = (from: number) => invoke("unthread_text_frame", { from });

export const getTextStoryChain = (entityId: number) =>
  invoke<number[]>("get_text_story_chain", { entityId });

// ---------------------------------------------------------------------------
// Commands — guides, snapping, baseline
// ---------------------------------------------------------------------------

export const addRulerGuide = (isVertical: boolean, position: number) =>
  invoke<number>("add_ruler_guide", { isVertical, position });

export const removeRulerGuide = (entityId: number) => invoke("remove_ruler_guide", { entityId });

export const listRulerGuides = () => invoke<RulerGuideSummary[]>("list_ruler_guides");

export const snapFrameGeometry = (entityId: number, geometry: FrameGeometry) =>
  invoke<SnappedGeometry>("snap_frame_geometry", { entityId, geometry });

export const clearActiveSnap = () => invoke("clear_active_snap");

export const setFrameBaselineSnap = (entityId: number, enabled: boolean) =>
  invoke("set_frame_baseline_snap", { entityId, enabled });

export const getFrameBaselineSnap = (entityId: number) =>
  invoke<boolean>("get_frame_baseline_snap", { entityId });

<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  /// The active editing tool. Creation tools drag out a new frame; Select
  /// picks and moves existing ones.
  type Tool = "Select" | "Rectangle" | "Ellipse" | "Line" | "Text";

  /// What the current pointer gesture is doing.
  type DragMode = "none" | "pan" | "create" | "move" | "resize";

  /// A page's placement on the pasteboard.
  type PagePlacement = {
    page_number: number;
    spread_index: number;
    x: number;
    y: number;
    width: number;
    height: number;
    is_left: boolean;
  };

  /// The document's vertical rhythm for typography.
  type BaselineGrid = { increment: number; start: number; visible: boolean };

  /// Document settings, mirrored from the Rust `Document` component.
  ///
  /// Only the fields the UI reads are named; the rest ride along untouched so
  /// a round trip through `set_document_settings` cannot drop them.
  type DocumentSettings = {
    baseline_grid: BaselineGrid;
    [key: string]: unknown;
  };

  /// Geometry returned by the snapping pass.
  type SnappedGeometry = { geometry: FrameGeometry; snapped: boolean };

  /// A frame's geometry as carried over the IPC bridge.
  type FrameGeometry = {
    x: number;
    y: number;
    width: number;
    height: number;
    rotation: number;
    scale_x: number;
    scale_y: number;
  };

  // Live status of the Rust-side vello pipeline.
  type RendererInfo = {
    engine: string;
    backend: string;
    is_ready: boolean;
    supports_webgpu: boolean;
  };

  // Types for compiled render elements
  type RenderElement =
    | {
        type: "PageSurface";
        page_number: number;
        x: number;
        y: number;
        width: number;
        height: number;
        bleed: number;
        shadow_blur: number;
      }
    | {
        type: "RectShape";
        id: number;
        name: string;
        x: number;
        y: number;
        width: number;
        height: number;
        rotation: number;
        fill_color: [number, number, number, number];
        stroke_color?: [number, number, number, number];
        stroke_width: number;
        corner_radius: number;
        is_selected: boolean;
      }
    | {
        type: "EllipseShape";
        id: number;
        name: string;
        cx: number;
        cy: number;
        rx: number;
        ry: number;
        rotation: number;
        fill_color: [number, number, number, number];
        stroke_color?: [number, number, number, number];
        stroke_width: number;
        is_selected: boolean;
      }
    | {
        type: "TextBlock";
        id: number;
        name: string;
        x: number;
        y: number;
        width: number;
        height: number;
        text: string;
        font_size: number;
        line_height: number;
        fill_color: [number, number, number, number];
        is_selected: boolean;
      }
    | {
        type: "SelectionOverlay";
        entity_id: number;
        min_x: number;
        min_y: number;
        max_x: number;
        max_y: number;
        corner_nodes: [number, number][];
      };

  interface Camera {
    pan_x: number;
    pan_y: number;
    zoom: number;
    viewport_width: number;
    viewport_height: number;
  }

  interface RenderScene {
    revision: number;
    pasteboard_color: [number, number, number, number];
    page_width: number;
    page_height: number;
    pan_x: number;
    pan_y: number;
    zoom: number;
    elements: RenderElement[];
    total_frames: number;
  }

  interface HistoryStatus {
    undo_count: number;
    redo_count: number;
    can_undo: boolean;
    can_redo: boolean;
  }

  // Svelte 5 Runes for reactive state
  // The document is painted by vello on a native GPU surface behind the
  // webview; this element only reserves the layout box for it.
  let viewportEl = $state<HTMLDivElement | null>(null);
  let renderEngineMode = $state("Starting Vello...");
  let isWebGpuActive = $state(false);
  let camera = $state<Camera>({
    pan_x: 60,
    pan_y: 60,
    zoom: 1.0,
    viewport_width: 1200,
    viewport_height: 800,
  });

  let selectedEntityId = $state<number | null>(null);

  let historyStatus = $state<HistoryStatus>({
    undo_count: 0,
    redo_count: 0,
    can_undo: false,
    can_redo: false,
  });

  // Mouse & Navigation runes
  let mouseScreenX = $state(0);
  let mouseScreenY = $state(0);
  let isMiddlePanning = $state(false);
  let isSpacePressed = $state(false);

  // Tool and gesture state
  let activeTool = $state<Tool>("Select");
  let dragMode = $state<DragMode>("none");
  let dragEntityId = $state<number | null>(null);
  let dragBefore = $state<FrameGeometry | null>(null);
  let dragStartDoc = { x: 0, y: 0 };
  /// Geometry of the selected frame, kept so resize handles can be located
  /// without an IPC round trip on every mouse move.
  let selectedGeometry = $state<FrameGeometry | null>(null);
  /// Which corner is being dragged, and the document-space point it pivots about.
  let resizeAnchor = { x: 0, y: 0 };

  // Phase 3 document state
  let pages = $state<PagePlacement[]>([]);
  let snapEnabled = $state(true);
  let isSnapped = $state(false);
  /// When set, the next click threads the selected frame into the one clicked.
  let isThreading = $state(false);
  let documentSettings = $state<DocumentSettings | null>(null);
  let baselineGrid = $state<BaselineGrid>({ increment: 12, start: 0, visible: false });
  /// Whether the selected text frame locks to the grid. False for anything
  /// that is not a text frame.
  let selectionSnapsToBaseline = $state(false);

  const TOOLS: { id: Tool; label: string; key: string }[] = [
    { id: "Select", label: "Select", key: "V" },
    { id: "Rectangle", label: "Rectangle", key: "R" },
    { id: "Ellipse", label: "Ellipse", key: "E" },
    { id: "Line", label: "Line", key: "L" },
    { id: "Text", label: "Text", key: "T" },
  ];
  let panStartScreenX = $state(0);
  let panStartScreenY = $state(0);

  // Form input runes
  let frameName = $state("Hero Heading");
  let frameType = $state<"Rectangle" | "Ellipse" | "Text">("Rectangle");
  let posX = $state(80);
  let posY = $state(80);
  let sizeW = $state(220);
  let sizeH = $state(110);
  let textContent = $state("Tessera Typography");
  let selectedColorPreset = $state<"cyan" | "purple" | "emerald" | "amber">("cyan");

  // Derived runes
  let mouseDocX = $derived(Math.round((mouseScreenX - camera.pan_x) / camera.zoom));
  let mouseDocY = $derived(Math.round((mouseScreenY - camera.pan_y) / camera.zoom));
  let zoomPercentage = $derived(Math.round(camera.zoom * 100));

  const COLOR_MAP: Record<string, [number, number, number, number]> = {
    cyan: [0.22, 0.74, 0.97, 0.95],
    purple: [0.65, 0.33, 0.97, 0.95],
    emerald: [0.13, 0.77, 0.36, 0.95],
    amber: [0.96, 0.62, 0.14, 0.95],
  };

  /// Reports the canvas rectangle to Rust in physical pixels.
  ///
  /// The GPU surface spans the whole window, so Rust needs the DOM rect to know
  /// where to place and clip the document.
  async function syncViewport() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    await invoke("set_viewport_rect", {
      x: rect.left * dpr,
      y: rect.top * dpr,
      width: rect.width * dpr,
      height: rect.height * dpr,
    });
  }

  /// Paints one frame. Called after state or camera changes, never on a timer.
  async function requestRender() {
    try {
      await invoke<boolean>("render_frame", { selectedId: selectedEntityId });
    } catch (err) {
      console.warn("render_frame failed:", err);
    }
  }

  async function initRenderer() {
    try {
      const info = await invoke<RendererInfo>("init_renderer");
      isWebGpuActive = info.is_ready;
      renderEngineMode = info.is_ready
        ? `${info.engine} on ${info.backend}`
        : `Renderer unavailable: ${info.backend}`;
    } catch (err) {
      isWebGpuActive = false;
      renderEngineMode = `Renderer unavailable: ${err}`;
    }
  }

  async function fetchScene() {
    try {
      const [hist, cam, placements, settings] = await Promise.all([
        invoke<HistoryStatus>("get_history_status"),
        invoke<Camera>("get_camera_state"),
        invoke<PagePlacement[]>("get_page_placements"),
        invoke<DocumentSettings>("get_document_settings"),
      ]);
      historyStatus = hist;
      camera = cam;
      pages = placements;
      documentSettings = settings;
      baselineGrid = settings.baseline_grid;
      await syncViewport();
      await requestRender();
    } catch (err) {
      // Backend not reachable yet.
    }
  }

  // Camera & Mouse Interactions

  /// Converts a pointer event into document coordinates.
  ///
  /// This mirrors Camera::screen_to_document in Rust; keeping the arithmetic
  /// here avoids an IPC round trip on every mouse move.
  function toDocumentSpace(e: MouseEvent): { x: number; y: number } | null {
    const rect = viewportEl?.getBoundingClientRect();
    if (!rect) return null;
    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;
    return {
      x: (screenX - camera.pan_x) / camera.zoom,
      y: (screenY - camera.pan_y) / camera.zoom,
    };
  }

  async function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = viewportEl?.getBoundingClientRect();
    if (!rect) return;

    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;

    if (e.ctrlKey || e.metaKey) {
      const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
      camera = await invoke<Camera>("zoom_camera", {
        screenX,
        screenY,
        factor: zoomFactor,
      });
    } else {
      camera = await invoke<Camera>("pan_camera", {
        dx: -e.deltaX,
        dy: -e.deltaY,
      });
    }
    await requestRender();
  }

  /// Default size for a shape created by a click rather than a drag.
  const CLICK_CREATE_SIZE = 120;
  /// Half-extent of a selection handle in screen pixels, matching the renderer.
  const HANDLE_PX = 6;

  /// The four corners of a frame in document space.
  function cornersOf(g: FrameGeometry): { x: number; y: number }[] {
    const w = g.width * g.scale_x;
    const h = g.height * g.scale_y;
    return [
      { x: g.x, y: g.y },
      { x: g.x + w, y: g.y },
      { x: g.x + w, y: g.y + h },
      { x: g.x, y: g.y + h },
    ];
  }

  /// Finds which selection handle a document-space point is grabbing, if any.
  ///
  /// Rotated frames are excluded: resizing them along screen axes would shear
  /// the result, so they fall through to a move instead.
  function handleUnderCursor(
    g: FrameGeometry,
    doc: { x: number; y: number },
  ): number | null {
    if (g.rotation !== 0) return null;

    const tolerance = HANDLE_PX / camera.zoom;
    const corners = cornersOf(g);
    for (let i = 0; i < corners.length; i++) {
      if (
        Math.abs(corners[i].x - doc.x) <= tolerance &&
        Math.abs(corners[i].y - doc.y) <= tolerance
      ) {
        return i;
      }
    }
    return null;
  }

  async function handleMouseDown(e: MouseEvent) {
    // Space or middle button always pans, whatever tool is active.
    if (e.button === 1 || isSpacePressed) {
      dragMode = "pan";
      isMiddlePanning = true;
      panStartScreenX = e.clientX;
      panStartScreenY = e.clientY;
      e.preventDefault();
      return;
    }
    if (e.button !== 0) return;

    const doc = toDocumentSpace(e);
    if (!doc) return;
    dragStartDoc = doc;

    if (activeTool === "Select") {
      await beginMoveGesture(e, doc);
      return;
    }
    await beginCreateGesture(doc);
  }

  /// Starts dragging out a new shape of the active tool's type.
  async function beginCreateGesture(doc: { x: number; y: number }) {
    try {
      const id = await invoke<number>("spawn_frame", {
        name: `${activeTool} ${Date.now() % 10000}`,
        frameType: activeTool,
        x: doc.x,
        y: doc.y,
        width: 1,
        height: 1,
        fillColor: COLOR_MAP[selectedColorPreset],
        text: activeTool === "Text" ? textContent : null,
      });

      dragMode = "create";
      dragEntityId = id;
      selectedEntityId = id;
      dragBefore = await invoke<FrameGeometry>("get_frame_geometry", { entityId: id });
      await fetchScene();
    } catch (err) {
      console.warn("could not start shape:", err);
      dragMode = "none";
    }
  }

  /// Selects whatever is under the cursor and prepares to move it.
  async function beginMoveGesture(e: MouseEvent, doc: { x: number; y: number }) {
    const rect = viewportEl?.getBoundingClientRect();
    if (!rect) return;

    try {
      // Grabbing a handle of the current selection takes priority over
      // selecting whatever sits underneath it.
      if (selectedEntityId !== null && selectedGeometry) {
        const corner = handleUnderCursor(selectedGeometry, doc);
        if (corner !== null) {
          const opposite = cornersOf(selectedGeometry)[(corner + 2) % 4];
          resizeAnchor = opposite;
          dragMode = "resize";
          dragEntityId = selectedEntityId;
          dragBefore = selectedGeometry;
          return;
        }
      }

      const hit = await invoke<number | null>("raycast_select_entity", {
        screenX: e.clientX - rect.left,
        screenY: e.clientY - rect.top,
      });

      // While linking, a click picks the frame the story continues into
      // rather than changing the selection.
      if (isThreading && hit !== null) {
        await threadSelectionInto(hit);
        dragMode = "none";
        return;
      }

      selectedEntityId = hit;

      if (hit === null) {
        selectedGeometry = null;
        dragMode = "none";
        await requestRender();
        return;
      }

      dragMode = "move";
      dragEntityId = hit;
      dragBefore = await invoke<FrameGeometry>("get_frame_geometry", { entityId: hit });
      selectedGeometry = dragBefore;
      await requestRender();
    } catch (err) {
      dragMode = "none";
    }
  }

  async function handleMouseMove(e: MouseEvent) {
    const rect = viewportEl?.getBoundingClientRect();
    if (!rect) return;
    mouseScreenX = Math.round(e.clientX - rect.left);
    mouseScreenY = Math.round(e.clientY - rect.top);

    if (dragMode === "pan") {
      const dx = e.clientX - panStartScreenX;
      const dy = e.clientY - panStartScreenY;
      panStartScreenX = e.clientX;
      panStartScreenY = e.clientY;
      camera = await invoke<Camera>("pan_camera", { dx, dy });
      await requestRender();
      return;
    }

    if (dragMode === "none" || dragEntityId === null || !dragBefore) return;

    const doc = toDocumentSpace(e);
    if (!doc) return;

    // Live geometry updates deliberately skip the history stack; the whole
    // gesture is committed as one entry on mouse up.
    let next: FrameGeometry;
    if (dragMode === "create") {
      next = {
        ...dragBefore,
        x: Math.min(dragStartDoc.x, doc.x),
        y: Math.min(dragStartDoc.y, doc.y),
        width: Math.max(Math.abs(doc.x - dragStartDoc.x), 1),
        height: Math.max(Math.abs(doc.y - dragStartDoc.y), 1),
      };
    } else if (dragMode === "resize") {
      // The opposite corner stays pinned while the grabbed one follows the
      // cursor, so the frame rescales about the anchor.
      next = {
        ...dragBefore,
        x: Math.min(resizeAnchor.x, doc.x),
        y: Math.min(resizeAnchor.y, doc.y),
        width: Math.max(Math.abs(doc.x - resizeAnchor.x) / dragBefore.scale_x, 1),
        height: Math.max(Math.abs(doc.y - resizeAnchor.y) / dragBefore.scale_y, 1),
      };
    } else {
      next = {
        ...dragBefore,
        x: dragBefore.x + (doc.x - dragStartDoc.x),
        y: dragBefore.y + (doc.y - dragStartDoc.y),
      };
    }

    try {
      // Snapping runs on the proposed geometry before it is written, so the
      // frame lands on the guide rather than being corrected afterwards.
      let target = next;
      if (snapEnabled && dragMode !== "create") {
        const result = await invoke<SnappedGeometry>("snap_frame_geometry", {
          entityId: dragEntityId,
          geometry: next,
        });
        target = result.geometry;
        isSnapped = result.snapped;
      }

      await invoke("set_frame_geometry", { entityId: dragEntityId, geometry: target });
      await requestRender();
    } catch (err) {
      // The entity may have been removed mid-drag.
    }
  }

  async function handleMouseUp(e: MouseEvent) {
    if (dragMode === "pan") {
      dragMode = "none";
      isMiddlePanning = false;
      return;
    }

    if (dragMode === "none" || dragEntityId === null || !dragBefore) {
      dragMode = "none";
      return;
    }

    try {
      let after = await invoke<FrameGeometry>("get_frame_geometry", {
        entityId: dragEntityId,
      });

      // A click with no drag would leave a 1x1 sliver, so give it a usable size.
      if (dragMode === "create" && after.width <= 2 && after.height <= 2) {
        after = { ...after, width: CLICK_CREATE_SIZE, height: CLICK_CREATE_SIZE * 0.6 };
        await invoke("set_frame_geometry", { entityId: dragEntityId, geometry: after });
      }

      historyStatus = await invoke<HistoryStatus>("commit_frame_geometry", {
        entityId: dragEntityId,
        before: dragBefore,
        after,
      });
    } catch (err) {
      console.warn("could not commit gesture:", err);
    }

    dragMode = "none";
    dragEntityId = null;
    dragBefore = null;
    isSnapped = false;
    await invoke("clear_active_snap");
    if (selectedEntityId !== null) {
      try {
        selectedGeometry = await invoke<FrameGeometry>("get_frame_geometry", {
          entityId: selectedEntityId,
        });
      } catch (err) {
        selectedGeometry = null;
      }
    }

    // Creation tools revert to Select so the next click does not stack shapes.
    if (activeTool !== "Select") activeTool = "Select";
    await fetchScene();
  }

  /// Selection is handled on mouse down so a drag can begin immediately; this
  /// exists to swallow the trailing click event.
  async function handleCanvasClick(_e: MouseEvent) {}

  /// Keyboard navigation for the viewport.
  ///
  /// Arrow keys nudge the selected frame when there is one, and pan the view
  /// otherwise — the convention layout tools use. Shift takes a coarser step.
  async function handleViewportKeyDown(e: KeyboardEvent) {
    const step = e.shiftKey ? 10 : 1;
    let dx = 0;
    let dy = 0;

    switch (e.key) {
      case "ArrowLeft":
        dx = -step;
        break;
      case "ArrowRight":
        dx = step;
        break;
      case "ArrowUp":
        dy = -step;
        break;
      case "ArrowDown":
        dy = step;
        break;
      case "+":
      case "=":
        await zoomIn();
        e.preventDefault();
        return;
      case "-":
      case "_":
        await zoomOut();
        e.preventDefault();
        return;
      default:
        return;
    }

    e.preventDefault();

    if (selectedEntityId === null) {
      // Nothing selected: pan the view instead, in screen pixels.
      camera = await invoke<Camera>("pan_camera", { dx: -dx * 20, dy: -dy * 20 });
      await requestRender();
      return;
    }

    try {
      const before = await invoke<FrameGeometry>("get_frame_geometry", {
        entityId: selectedEntityId,
      });
      const after = { ...before, x: before.x + dx, y: before.y + dy };
      // Each nudge is its own undoable step, matching how layout tools behave.
      historyStatus = await invoke<HistoryStatus>("commit_frame_geometry", {
        entityId: selectedEntityId,
        before,
        after,
      });
      await requestRender();
    } catch (err) {
      console.warn("nudge failed:", err);
    }
  }


  async function addPage() {
    try {
      await invoke("add_page");
      await fetchScene();
    } catch (err) {
      console.warn("could not add page:", err);
    }
  }

  async function removeLastPage() {
    if (pages.length <= 1) return;
    try {
      await invoke("remove_page", { pageNumber: pages.length });
      await fetchScene();
    } catch (err) {
      console.warn("could not remove page:", err);
    }
  }

  /// Adds a ruler guide through the current cursor position.
  async function addGuideAtCursor(isVertical: boolean) {
    const position = isVertical ? mouseDocX : mouseDocY;
    try {
      await invoke("add_ruler_guide", { isVertical, position });
      await requestRender();
    } catch (err) {
      console.warn("could not add guide:", err);
    }
  }

  /// Writes a change to the document's baseline grid.
  ///
  /// The grid rides on the whole `Document`, so the stored settings are spread
  /// back in rather than rebuilt — otherwise a round trip here would silently
  /// reset margins, bleed and page size to their defaults.
  async function applyBaselineGrid(patch: Partial<BaselineGrid>) {
    if (!documentSettings) return;
    const grid = { ...baselineGrid, ...patch };
    baselineGrid = grid;
    try {
      await invoke("set_document_settings", {
        settings: { ...documentSettings, baseline_grid: grid },
      });
      documentSettings = { ...documentSettings, baseline_grid: grid };
      await requestRender();
    } catch (err) {
      console.warn("could not update the baseline grid:", err);
    }
  }

  /// Locks the selected text frame to the baseline grid, or releases it.
  async function toggleSelectionBaselineSnap() {
    if (selectedEntityId === null) return;
    const enabled = !selectionSnapsToBaseline;
    try {
      await invoke("set_frame_baseline_snap", { entityId: selectedEntityId, enabled });
      selectionSnapsToBaseline = enabled;
      await requestRender();
    } catch (err) {
      // Not a text frame; the toggle stays off rather than lying about state.
      console.warn("could not toggle baseline snapping:", err);
      selectionSnapsToBaseline = false;
    }
  }

  // Reading the toggle back from the backend keeps it honest when the
  // selection changes, including for frames that cannot snap at all.
  $effect(() => {
    const id = selectedEntityId;
    if (id === null) {
      selectionSnapsToBaseline = false;
      return;
    }
    invoke<boolean>("get_frame_baseline_snap", { entityId: id })
      .then((snaps) => {
        if (selectedEntityId === id) selectionSnapsToBaseline = snaps;
      })
      .catch(() => {
        if (selectedEntityId === id) selectionSnapsToBaseline = false;
      });
  });

  /// Threads the selected frame into `target`, continuing its story there.
  async function threadSelectionInto(target: number) {
    if (selectedEntityId === null || selectedEntityId === target) return;
    try {
      await invoke("thread_text_frames", { from: selectedEntityId, to: target });
      isThreading = false;
      await fetchScene();
    } catch (err) {
      console.warn("could not thread frames:", err);
      isThreading = false;
    }
  }

  async function zoomIn() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    camera = await invoke<Camera>("zoom_camera", {
      screenX: rect.width / 2,
      screenY: rect.height / 2,
      factor: 1.25,
    });
    await requestRender();
  }

  async function zoomOut() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    camera = await invoke<Camera>("zoom_camera", {
      screenX: rect.width / 2,
      screenY: rect.height / 2,
      factor: 0.8,
    });
    await requestRender();
  }

  async function fitPageView() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    camera = await invoke<Camera>("fit_page_view", {
      viewportWidth: rect.width,
      viewportHeight: rect.height,
    });
    await requestRender();
  }

  async function resetCamera() {
    camera = await invoke<Camera>("reset_camera");
    await requestRender();
  }

  async function spawnNewFrame() {
    try {
      const fill = COLOR_MAP[selectedColorPreset] ?? [0.2, 0.7, 1.0, 1.0];
      const newId = await invoke<number>("spawn_frame", {
        name: frameName || "New Frame",
        frameType,
        x: posX,
        y: posY,
        width: sizeW,
        height: sizeH,
        fillColor: fill,
        text: frameType === "Text" ? textContent : null,
      });
      selectedEntityId = newId;
      await fetchScene();
    } catch (err) {
      console.error("Failed to spawn frame:", err);
    }
  }

  async function spawnQuick(type: "Rectangle" | "Ellipse" | "Text") {
    frameType = type;
    posX = Math.floor(Math.random() * 260) + 60;
    posY = Math.floor(Math.random() * 180) + 60;
    sizeW = Math.floor(Math.random() * 120) + 90;
    sizeH = Math.floor(Math.random() * 80) + 60;
    frameName = `${type} #${Math.floor(Math.random() * 900) + 100}`;
    await spawnNewFrame();
  }

  async function handleUndo() {
    try {
      await invoke("undo_action");
      await fetchScene();
    } catch (err) {
      console.error("Undo failed:", err);
    }
  }

  async function handleRedo() {
    try {
      await invoke("redo_action");
      await fetchScene();
    } catch (err) {
      console.error("Redo failed:", err);
    }
  }

  onMount(() => {
    // The surface is sized to the window, so it must be created after the
    // canvas element has been laid out.
    (async () => {
      await initRenderer();
      await fetchScene();
    })();

    // The viewport box moves whenever the window or panels resize.
    const observer = new ResizeObserver(() => {
      syncViewport().then(requestRender);
    });
    if (viewportEl) observer.observe(viewportEl);

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space") isSpacePressed = true;

      // Tool shortcuts are ignored while typing into a field.
      const target = e.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.isContentEditable;
      if (typing || e.ctrlKey || e.metaKey || e.altKey) return;

      const tool = TOOLS.find((t) => t.key === e.key.toUpperCase());
      if (tool) {
        activeTool = tool.id;
        e.preventDefault();
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") isSpacePressed = false;
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("resize", fetchScene);

    return () => {
      observer.disconnect();
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("resize", fetchScene);
    };
  });
</script>

<main class="app-container">
  <!-- Header -->
  <header class="header">
    <div class="brand">
      <div class="logo-gem">
        <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="2">
          <polygon points="12 2 2 7 12 12 22 7 12 2" stroke="url(#gem-gradient)" />
          <polyline points="2 17 12 22 22 17" stroke="url(#gem-gradient)" />
          <polyline points="2 12 12 17 22 12" stroke="url(#gem-gradient)" />
          <defs>
            <linearGradient id="gem-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
              <stop offset="0%" stop-color="#38bdf8" />
              <stop offset="100%" stop-color="#818cf8" />
            </linearGradient>
          </defs>
        </svg>
      </div>
      <div>
        <h1 class="title">Tessera Publish</h1>
        <p class="subtitle">Interactive Viewport • Phase 1.4: Affine Camera & Selection Engine</p>
      </div>
    </div>

    <!-- Camera Navigation Toolbar -->
    <div class="header-controls">
      <div class="camera-toolbar">
        <button class="btn-cam" onclick={zoomOut} title="Zoom Out (-)">−</button>
        <span class="zoom-indicator">{zoomPercentage}%</span>
        <button class="btn-cam" onclick={zoomIn} title="Zoom In (+)">+</button>
        <button class="btn-cam-text" onclick={fitPageView} title="Fit document page in view">Fit Page</button>
        <button class="btn-cam-text" onclick={resetCamera} title="Reset to 100%">100%</button>
      </div>

      <div class="history-group">
        <button class="btn-icon" onclick={handleUndo} disabled={!historyStatus.can_undo}>
          ↶ Undo <span class="counter">({historyStatus.undo_count})</span>
        </button>
        <button class="btn-icon" onclick={handleRedo} disabled={!historyStatus.can_redo}>
          ↷ Redo <span class="counter">({historyStatus.redo_count})</span>
        </button>
      </div>

      <span class="badge {isWebGpuActive ? 'webgpu-active' : 'engine-badge'}">
        ⚡ {renderEngineMode}
      </span>
    </div>
  </header>

  <!-- Viewport Grid -->
  <div class="viewport-layout">
    <!-- Center: Interactive Pan & Zoom Canvas -->
    <div class="canvas-panel card">
      <div class="canvas-header">
        <div class="hud-group">
          <h2>Document Viewport</h2>
          <div class="hud-coords">
            <span class="hud-pill">Screen: {mouseScreenX}, {mouseScreenY} px</span>
            <span class="hud-pill highlight">Doc Space: {mouseDocX}, {mouseDocY} pt</span>
          </div>
        </div>

        <div class="selection-status">
          {#if selectedEntityId !== null}
            <span class="selection-tag">Selected Frame #{selectedEntityId}</span>
          {:else}
            <span class="status-idle">
              {activeTool === "Select"
                ? "Click to select • Drag to move • Wheel to navigate"
                : `Drag on the canvas to draw a ${activeTool.toLowerCase()}`}
            </span>
          {/if}
        </div>
      </div>

      <div class="doc-bar">
        <span class="doc-group">
          <strong>{pages.length}</strong>
          {pages.length === 1 ? "page" : "pages"}
          <button class="chip" onclick={addPage} title="Add a page">+ Page</button>
          <button
            class="chip"
            onclick={removeLastPage}
            disabled={pages.length <= 1}
            title="Remove the last page"
          >
            − Page
          </button>
        </span>

        <span class="doc-group">
          <label class="chip-toggle">
            <input type="checkbox" bind:checked={snapEnabled} />
            Snap
          </label>
          <button class="chip" onclick={() => addGuideAtCursor(true)} title="Vertical guide at cursor">
            + V Guide
          </button>
          <button class="chip" onclick={() => addGuideAtCursor(false)} title="Horizontal guide at cursor">
            + H Guide
          </button>
        </span>

        <span class="doc-group">
          <label class="chip-toggle">
            <input
              type="checkbox"
              checked={baselineGrid.visible}
              onchange={(e) =>
                applyBaselineGrid({ visible: e.currentTarget.checked })}
            />
            Baseline
          </label>
          <label class="chip-field" title="Distance between baselines">
            <input
              type="number"
              min="1"
              step="0.5"
              value={baselineGrid.increment}
              onchange={(e) =>
                applyBaselineGrid({ increment: Number(e.currentTarget.value) })}
            />
          </label>
          <button
            class="chip"
            class:active={selectionSnapsToBaseline}
            disabled={selectedEntityId === null}
            onclick={toggleSelectionBaselineSnap}
            title="Lock the selected text frame's lines to the baseline grid"
          >
            {selectionSnapsToBaseline ? "On Grid" : "Lock to Grid"}
          </button>
        </span>

        <span class="doc-group">
          <button
            class="chip"
            class:active={isThreading}
            disabled={selectedEntityId === null}
            onclick={() => (isThreading = !isThreading)}
            title="Link the selected text frame into another"
          >
            {isThreading ? "Click target frame…" : "Link Text"}
          </button>
          {#if isSnapped}
            <span class="snap-flag">snapped</span>
          {/if}
        </span>
      </div>

      <div class="tool-palette" role="toolbar" aria-label="Editing tools">
        {#each TOOLS as tool (tool.id)}
          <button
            class="tool-button"
            class:active={activeTool === tool.id}
            aria-pressed={activeTool === tool.id}
            title="{tool.label} ({tool.key})"
            onclick={() => (activeTool = tool.id)}
          >
            {tool.label}<span class="tool-key">{tool.key}</span>
          </button>
        {/each}
      </div>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="canvas-container {isSpacePressed || isMiddlePanning ? 'panning' : ''}">
        <!-- role="application" is correct here: this is a custom widget that
             handles its own pointer and keyboard input. Svelte's a11y rules
             classify the role as non-interactive, hence the suppressions. -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="gpu-viewport"
          role="application"
          tabindex="0"
          aria-label="Document viewport. Arrow keys pan the view, plus and minus zoom."
          bind:this={viewportEl}
          onwheel={handleWheel}
          onmousedown={handleMouseDown}
          onmousemove={handleMouseMove}
          onmouseup={handleMouseUp}
          onclick={handleCanvasClick}
          onkeydown={handleViewportKeyDown}
        ></div>
      </div>

      <div class="canvas-footer">
        <span>💡 <strong>Tools:</strong> <code>V</code> Select • <code>R</code> Rectangle • <code>E</code> Ellipse • <code>L</code> Line • <code>T</code> Text — <strong>Navigation:</strong> <code>Ctrl + Wheel</code> to Zoom centered on cursor • <code>Trackpad Swipe</code> / <code>Wheel</code> to Pan • <code>Space + Drag</code> for Pan tool.</span>
      </div>
    </div>

    <!-- Right Sidebar: Vector Spawner & Transforms -->
    <aside class="sidebar card">
      <h2>Vector Tools & Frame Spawner</h2>
      <p class="sidebar-desc">Spawn elements into document coordinates and observe live affine camera mapping.</p>

      <div class="quick-tools">
        <button class="btn-quick" onclick={() => spawnQuick("Rectangle")}>+ Rectangle</button>
        <button class="btn-quick" onclick={() => spawnQuick("Ellipse")}>+ Ellipse</button>
        <button class="btn-quick" onclick={() => spawnQuick("Text")}>+ Text Frame</button>
      </div>

      <form class="tool-form" onsubmit={(e) => { e.preventDefault(); spawnNewFrame(); }}>
        <div class="input-group">
          <label for="f-name">Frame Name</label>
          <input id="f-name" type="text" bind:value={frameName} />
        </div>

        <div class="form-row">
          <div class="input-group">
            <label for="f-type">Shape Type</label>
            <select id="f-type" bind:value={frameType}>
              <option value="Rectangle">Rectangle (Kurbo)</option>
              <option value="Ellipse">Ellipse (Kurbo)</option>
              <option value="Text">Text Frame</option>
            </select>
          </div>

          <div class="input-group">
            <label for="f-color">Fill Palette</label>
            <select id="f-color" bind:value={selectedColorPreset}>
              <option value="cyan">Cyan Glow</option>
              <option value="purple">Purple Mist</option>
              <option value="emerald">Emerald</option>
              <option value="amber">Amber Warm</option>
            </select>
          </div>
        </div>

        {#if frameType === 'Text'}
          <div class="input-group">
            <label for="f-text">Text String</label>
            <input id="f-text" type="text" bind:value={textContent} />
          </div>
        {/if}

        <div class="form-row">
          <div class="input-group">
            <label for="f-x">Doc Position X</label>
            <input id="f-x" type="number" bind:value={posX} step="5" />
          </div>
          <div class="input-group">
            <label for="f-y">Doc Position Y</label>
            <input id="f-y" type="number" bind:value={posY} step="5" />
          </div>
        </div>

        <div class="form-row">
          <div class="input-group">
            <label for="f-w">Width</label>
            <input id="f-w" type="number" bind:value={sizeW} step="5" />
          </div>
          <div class="input-group">
            <label for="f-h">Height</label>
            <input id="f-h" type="number" bind:value={sizeH} step="5" />
          </div>
        </div>

        <button type="submit" class="btn-primary">
          <span>+</span> Spawn Entity into World
        </button>
      </form>

      <!-- Camera Transform Inspector -->
      <div class="scene-stats">
        <div class="stat-item">
          <span class="stat-k">Camera Pan Offset</span>
          <span class="stat-v">({Math.round(camera.pan_x)}, {Math.round(camera.pan_y)}) px</span>
        </div>
        <div class="stat-item">
          <span class="stat-k">Active Zoom</span>
          <span class="stat-v highlight">{(camera.zoom * 100).toFixed(1)}%</span>
        </div>
        <div class="stat-item">
          <span class="stat-k">Raycast Selection</span>
          <span class="stat-v success">Affine Document Mapping</span>
        </div>
      </div>
    </aside>
  </div>
</main>

<style>
  /* The window is transparent so the native vello surface behind the webview
     shows through. Vello clears the full surface to the pasteboard colour, so
     that — not a CSS background — is what fills the window. */
  :global(html) {
    background: transparent;
  }

  :global(body) {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    background: transparent;
    color: #f1f5f9;
    overflow-x: hidden;
  }

  .app-container {
    max-width: 1440px;
    margin: 0 auto;
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
  }

  /* Header */
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 1rem;
    padding-bottom: 1.25rem;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .logo-gem {
    width: 44px;
    height: 44px;
    border-radius: 12px;
    background: rgba(56, 189, 248, 0.1);
    border: 1px solid rgba(56, 189, 248, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 0 20px rgba(56, 189, 248, 0.2);
  }

  .title {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 700;
    letter-spacing: -0.02em;
    background: linear-gradient(135deg, #f8fafc, #94a3b8);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
  }

  .subtitle {
    margin: 0.2rem 0 0;
    font-size: 0.8rem;
    color: #94a3b8;
  }

  .header-controls {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    flex-wrap: wrap;
  }

  /* Camera Toolbar */
  .camera-toolbar {
    display: flex;
    align-items: center;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    padding: 2px 4px;
    gap: 2px;
  }

  .btn-cam {
    background: transparent;
    border: none;
    color: #f1f5f9;
    font-size: 1.1rem;
    font-weight: 700;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .btn-cam:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #38bdf8;
  }

  .zoom-indicator {
    font-size: 0.78rem;
    font-weight: 700;
    color: #38bdf8;
    font-family: ui-monospace, monospace;
    min-width: 48px;
    text-align: center;
  }

  .btn-cam-text {
    background: transparent;
    border: none;
    color: #cbd5e1;
    font-size: 0.75rem;
    font-weight: 600;
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .btn-cam-text:hover {
    background: rgba(56, 189, 248, 0.15);
    color: #38bdf8;
  }

  .history-group {
    display: flex;
    gap: 0.4rem;
  }

  .btn-icon {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    color: #f8fafc;
    padding: 0.45rem 0.85rem;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
  }

  .btn-icon:hover:not(:disabled) {
    background: rgba(56, 189, 248, 0.15);
    border-color: #38bdf8;
    color: #38bdf8;
  }

  .btn-icon:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .counter {
    font-size: 0.75rem;
    color: #94a3b8;
  }

  .badge {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 0.35rem 0.7rem;
    border-radius: 6px;
    border: 1px solid transparent;
  }

  .engine-badge {
    background: rgba(56, 189, 248, 0.12);
    color: #38bdf8;
    border-color: rgba(56, 189, 248, 0.3);
  }

  .webgpu-active {
    background: rgba(34, 197, 94, 0.15);
    color: #4ade80;
    border-color: rgba(34, 197, 94, 0.4);
    box-shadow: 0 0 12px rgba(34, 197, 94, 0.25);
  }

  /* Viewport Layout */
  .viewport-layout {
    display: grid;
    grid-template-columns: 1fr 340px;
    gap: 1.25rem;
  }

  @media (max-width: 1080px) {
    .viewport-layout {
      grid-template-columns: 1fr;
    }
  }

  /* Card base */
  .card {
    background: rgba(15, 23, 42, 0.75);
    backdrop-filter: blur(14px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 14px;
    padding: 1.25rem;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
  }

  /* Canvas Panel */
  /* Transparent all the way down to .gpu-viewport, otherwise the card would
     paint over the GPU surface that is layered behind the webview. */
  .canvas-panel {
    min-height: 540px;
    background: transparent;
    backdrop-filter: none;
  }

  .canvas-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.85rem;
  }

  .hud-group {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  h2 {
    font-size: 1.05rem;
    font-weight: 600;
    margin: 0;
    color: #f8fafc;
  }

  .hud-coords {
    display: flex;
    gap: 0.4rem;
  }

  .hud-pill {
    font-size: 0.72rem;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
    color: #94a3b8;
    font-family: ui-monospace, monospace;
  }

  .hud-pill.highlight {
    color: #38bdf8;
    background: rgba(56, 189, 248, 0.1);
    border-color: rgba(56, 189, 248, 0.25);
  }

  .selection-tag {
    font-size: 0.75rem;
    color: #38bdf8;
    background: rgba(56, 189, 248, 0.15);
    border: 1px solid #38bdf8;
    padding: 0.2rem 0.6rem;
    border-radius: 6px;
    font-weight: 600;
  }

  .status-idle {
    font-size: 0.75rem;
    color: #64748b;
  }

  .canvas-container {
    position: relative;
    flex: 1;
    min-height: 480px;
    border-radius: 10px;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: transparent;
    cursor: default;
  }

  .canvas-container.panning {
    cursor: grab;
  }

  .gpu-viewport {
    width: 100%;
    height: 100%;
    display: block;
    background: transparent;
  }

  .doc-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 1rem;
    margin-bottom: 0.6rem;
    font-size: 0.78rem;
    color: #94a3b8;
  }

  .doc-group {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }

  .chip,
  .chip-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.55rem;
    font-size: 0.75rem;
    color: #cbd5e1;
    background: rgba(15, 23, 42, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 6px;
    cursor: pointer;
  }

  /* A chip that holds a numeric field, sized so a two- or three-digit value
     fits without the toolbar reflowing as the number changes. */
  .chip-field {
    display: inline-flex;
    align-items: center;
    padding: 0.15rem 0.35rem;
    background: rgba(15, 23, 42, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 6px;
  }

  .chip-field input {
    width: 3.4rem;
    padding: 0.1rem 0.2rem;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
    color: #cbd5e1;
    background: transparent;
    border: none;
    outline: none;
  }

  .chip:hover:not(:disabled) {
    border-color: rgba(56, 189, 248, 0.5);
  }

  .chip:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .chip.active {
    background: rgba(56, 189, 248, 0.2);
    border-color: #38bdf8;
    color: #e0f2fe;
  }

  .snap-flag {
    color: #34d399;
    font-weight: 600;
  }

  .tool-palette {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.6rem;
    flex-wrap: wrap;
  }

  .tool-button {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.7rem;
    font-size: 0.8rem;
    color: #cbd5e1;
    background: rgba(15, 23, 42, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    cursor: pointer;
  }

  .tool-button:hover {
    border-color: rgba(56, 189, 248, 0.5);
  }

  .tool-button.active {
    background: rgba(56, 189, 248, 0.18);
    border-color: #38bdf8;
    color: #e0f2fe;
  }

  .tool-key {
    font-size: 0.68rem;
    opacity: 0.6;
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 0.25rem;
  }

  .canvas-footer {
    margin-top: 0.75rem;
    font-size: 0.78rem;
    color: #94a3b8;
  }

  /* Sidebar Controls */
  .sidebar-desc {
    font-size: 0.8rem;
    color: #94a3b8;
    margin: 0 0 1rem;
  }

  .quick-tools {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.4rem;
    margin-bottom: 1rem;
  }

  .btn-quick {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 6px;
    color: #cbd5e1;
    padding: 0.45rem 0.2rem;
    font-size: 0.72rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
  }

  .btn-quick:hover {
    background: rgba(56, 189, 248, 0.12);
    border-color: #38bdf8;
    color: #38bdf8;
  }

  .tool-form {
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
  }

  .form-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
  }

  .input-group {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  label {
    font-size: 0.72rem;
    color: #94a3b8;
    font-weight: 500;
  }

  input,
  select {
    background: rgba(10, 15, 30, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    padding: 0.5rem 0.65rem;
    color: #f8fafc;
    font-size: 0.85rem;
    outline: none;
    transition: border-color 0.2s;
  }

  input:focus,
  select:focus {
    border-color: #38bdf8;
  }

  .btn-primary {
    background: linear-gradient(135deg, #0284c7, #2563eb);
    color: white;
    padding: 0.65rem 1rem;
    border-radius: 8px;
    border: none;
    font-weight: 600;
    font-size: 0.85rem;
    cursor: pointer;
    margin-top: 0.25rem;
    transition: all 0.2s;
  }

  .btn-primary:hover {
    background: linear-gradient(135deg, #0369a1, #1d4ed8);
    transform: translateY(-1px);
  }

  /* Scene Stats */
  .scene-stats {
    margin-top: 1.25rem;
    padding-top: 1rem;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .stat-item {
    display: flex;
    justify-content: space-between;
    font-size: 0.8rem;
  }

  .stat-k {
    color: #64748b;
  }

  .stat-v {
    font-weight: 600;
    color: #f8fafc;
  }

  .stat-v.highlight {
    color: #38bdf8;
    font-family: ui-monospace, monospace;
  }

  .stat-v.success {
    color: #22c55e;
  }
</style>

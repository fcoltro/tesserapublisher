/**
 * Shared editor state.
 *
 * Everything more than one panel needs to agree on lives here: the selection,
 * the camera, the document tree. Panels read it directly and call `invalidate`
 * after a mutation rather than each keeping a private copy of the document.
 *
 * Gesture-local state — which corner is being dragged, where a pan started —
 * deliberately does *not* live here. It belongs to the viewport that owns the
 * gesture and would only be noise to every other panel.
 */
import * as ipc from "./ipc";
import type {
  BaselineGrid,
  Camera,
  DocumentSettings,
  DocumentTreeSnapshot,
  FrameNode,
  HistoryStatus,
  PagePlacement,
  Rgba,
  Tool,
} from "./ipc";

export type ColorPreset = "cyan" | "purple" | "emerald" | "amber";

/** Fill presets offered when creating a frame. */
export const COLOR_MAP: Record<ColorPreset, Rgba> = {
  cyan: [0.22, 0.74, 0.97, 0.95],
  purple: [0.65, 0.33, 0.97, 0.95],
  emerald: [0.13, 0.77, 0.36, 0.95],
  amber: [0.96, 0.62, 0.14, 0.95],
};

/**
 * Painting hooks owned by the viewport.
 *
 * The store knows *when* a repaint is needed but not *how* to do one — that
 * requires the canvas element, which belongs to the viewport component. The
 * viewport registers these on mount so state changes anywhere in the UI can
 * trigger a repaint without the store reaching into the DOM.
 */
export interface RenderHooks {
  syncViewport: () => Promise<void>;
  requestRender: () => Promise<void>;
}

class StudioState {
  // --- Renderer status -----------------------------------------------------
  renderEngineMode = $state("Starting Vello...");
  isWebGpuActive = $state(false);

  // --- Camera --------------------------------------------------------------
  camera = $state<Camera>({
    pan_x: 60,
    pan_y: 60,
    zoom: 1.0,
    viewport_width: 1200,
    viewport_height: 800,
  });

  // --- Selection and tool --------------------------------------------------
  selectedEntityId = $state<number | null>(null);
  activeTool = $state<Tool>("Select");

  // --- History -------------------------------------------------------------
  history = $state<HistoryStatus>({
    undo_count: 0,
    redo_count: 0,
    can_undo: false,
    can_redo: false,
  });

  // --- Document ------------------------------------------------------------
  tree = $state<DocumentTreeSnapshot | null>(null);
  pages = $state<PagePlacement[]>([]);
  documentSettings = $state<DocumentSettings | null>(null);
  baselineGrid = $state<BaselineGrid>({ increment: 12, start: 0, visible: false });

  // --- Editing modes -------------------------------------------------------
  snapEnabled = $state(true);
  isSnapped = $state(false);
  /** When set, the next click threads the selected frame into the one clicked. */
  isThreading = $state(false);

  // --- Defaults for newly drawn frames -------------------------------------
  // Shared because a frame can be created two ways — dragged out on the canvas
  // or spawned from a panel — and both must agree on how it looks.
  colorPreset = $state<ColorPreset>("cyan");
  defaultText = $state("Tessera Typography");

  // --- Derived -------------------------------------------------------------
  zoomPercentage = $derived(Math.round(this.camera.zoom * 100));

  /** Every frame in the document, flattened out of the page/layer tree. */
  allFrames = $derived<FrameNode[]>(
    this.tree?.pages.flatMap((page) => page.layers.flatMap((layer) => layer.frames)) ?? [],
  );

  /** The selected frame's node, or null when nothing is selected. */
  selectedFrame = $derived<FrameNode | null>(
    this.selectedEntityId === null
      ? null
      : (this.allFrames.find((f) => f.id === this.selectedEntityId) ?? null),
  );

  // --- Painting hooks ------------------------------------------------------
  #hooks: RenderHooks | null = null;

  registerRenderHooks(hooks: RenderHooks) {
    this.#hooks = hooks;
  }

  clearRenderHooks() {
    this.#hooks = null;
  }

  /** Repaints without re-reading the document. Cheap; safe to call often. */
  async repaint() {
    if (!this.#hooks) return;
    await this.#hooks.syncViewport();
    await this.#hooks.requestRender();
  }

  select(id: number | null) {
    this.selectedEntityId = id;
  }

  /**
   * Re-reads document state from Rust and repaints.
   *
   * Call after any mutation. Pulls the whole document rather than patching,
   * which is correct but not cheap — Phase 4.7 replaces the blanket pull with
   * a scene-revision check.
   */
  async invalidate() {
    try {
      const [history, camera, pages, settings, tree] = await Promise.all([
        ipc.getHistoryStatus(),
        ipc.getCameraState(),
        ipc.getPagePlacements(),
        ipc.getDocumentSettings(),
        ipc.queryDocumentTree(),
      ]);
      this.history = history;
      this.camera = camera;
      this.pages = pages;
      this.documentSettings = settings;
      this.baselineGrid = settings.baseline_grid;
      this.tree = tree;
      await this.repaint();
    } catch {
      // Backend not reachable yet. The next invalidate will pick it up.
    }
  }

  async initRenderer() {
    try {
      const info = await ipc.initRenderer();
      this.isWebGpuActive = info.is_ready;
      this.renderEngineMode = info.is_ready
        ? `${info.engine} on ${info.backend}`
        : `Renderer unavailable: ${info.backend}`;
    } catch (err) {
      this.isWebGpuActive = false;
      this.renderEngineMode = `Renderer unavailable: ${err}`;
    }
  }

  async undo() {
    this.history = await ipc.undoAction();
    await this.invalidate();
  }

  async redo() {
    this.history = await ipc.redoAction();
    await this.invalidate();
  }
}

/** The single editor state instance the whole UI shares. */
export const studio = new StudioState();

/** Tool palette definition, shared by the tool rail and the shortcut handler. */
export const TOOLS: { id: Tool; label: string; key: string }[] = [
  { id: "Select", label: "Select", key: "V" },
  { id: "Rectangle", label: "Rectangle", key: "R" },
  { id: "Ellipse", label: "Ellipse", key: "E" },
  { id: "Line", label: "Line", key: "L" },
  { id: "Text", label: "Text", key: "T" },
];

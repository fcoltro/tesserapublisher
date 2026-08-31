<script lang="ts">
  /**
   * The document canvas and every pointer gesture that acts on it.
   *
   * The vello surface is painted natively behind a transparent webview, so the
   * `.gpu-viewport` element here holds no pixels of its own — it reserves the
   * layout box and reports it to Rust. All the drawing happens on the other
   * side of the IPC bridge.
   *
   * Gesture state (which corner is being dragged, where a pan began) is local
   * on purpose: it is meaningless to every other panel and would only add
   * noise to the shared store.
   */
  import { onMount } from "svelte";
  import * as ipc from "$lib/ipc";
  import type { BaselineGrid, FrameGeometry } from "$lib/ipc";
  import { COLOR_MAP, studio, TOOLS } from "$lib/state.svelte";

  // The document is painted by vello on a native GPU surface behind the
  // webview; this element only reserves the layout box for it.
  let viewportEl = $state<HTMLDivElement | null>(null);

  // Mouse & navigation
  let mouseScreenX = $state(0);
  let mouseScreenY = $state(0);
  let isMiddlePanning = $state(false);
  let isSpacePressed = $state(false);
  let panStartScreenX = $state(0);
  let panStartScreenY = $state(0);

  // Gesture state
  let dragMode = $state<ipc.DragMode>("none");
  let dragEntityId = $state<number | null>(null);
  let dragBefore = $state<FrameGeometry | null>(null);
  let dragStartDoc = { x: 0, y: 0 };
  /// Geometry of the selected frame, kept so resize handles can be located
  /// without an IPC round trip on every mouse move.
  let selectedGeometry = $state<FrameGeometry | null>(null);
  /// Which corner is being dragged, and the document-space point it pivots about.
  let resizeAnchor = { x: 0, y: 0 };
  /// Whether the selected text frame locks to the grid. False for anything
  /// that is not a text frame.
  let selectionSnapsToBaseline = $state(false);

  // Derived runes
  let mouseDocX = $derived(Math.round((mouseScreenX - studio.camera.pan_x) / studio.camera.zoom));
  let mouseDocY = $derived(Math.round((mouseScreenY - studio.camera.pan_y) / studio.camera.zoom));

  /// Reports the canvas rectangle to Rust in physical pixels.
  ///
  /// The GPU surface spans the whole window, so Rust needs the DOM rect to know
  /// where to place and clip the document.
  async function syncViewport() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    await ipc.setViewportRect(
      rect.left * dpr,
      rect.top * dpr,
      rect.width * dpr,
      rect.height * dpr,
    );
  }

  /// Paints one frame. Called after state or camera changes, never on a timer.
  async function requestRender() {
    try {
      await ipc.renderFrame(studio.selectedEntityId);
    } catch (err) {
      console.warn("render_frame failed:", err);
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
      x: (screenX - studio.camera.pan_x) / studio.camera.zoom,
      y: (screenY - studio.camera.pan_y) / studio.camera.zoom,
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
      studio.camera = await ipc.zoomCamera(screenX, screenY, zoomFactor);
    } else {
      studio.camera = await ipc.panCamera(-e.deltaX, -e.deltaY);
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

    const tolerance = HANDLE_PX / studio.camera.zoom;
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

    if (studio.activeTool === "Select") {
      await beginMoveGesture(e, doc);
      return;
    }
    await beginCreateGesture(doc);
  }

  /// Starts dragging out a new shape of the active tool's type.
  async function beginCreateGesture(doc: { x: number; y: number }) {
    try {
      const id = await ipc.spawnFrame({
        name: `${studio.activeTool} ${Date.now() % 10000}`,
        frameType: studio.activeTool,
        x: doc.x,
        y: doc.y,
        width: 1,
        height: 1,
        fillColor: COLOR_MAP[studio.colorPreset],
        text: studio.activeTool === "Text" ? studio.defaultText : null,
      });

      dragMode = "create";
      dragEntityId = id;
      studio.selectedEntityId = id;
      dragBefore = await ipc.getFrameGeometry(id);
      await studio.invalidate();
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
      if (studio.selectedEntityId !== null && selectedGeometry) {
        const corner = handleUnderCursor(selectedGeometry, doc);
        if (corner !== null) {
          const opposite = cornersOf(selectedGeometry)[(corner + 2) % 4];
          resizeAnchor = opposite;
          dragMode = "resize";
          dragEntityId = studio.selectedEntityId;
          dragBefore = selectedGeometry;
          return;
        }
      }

      const hit = await ipc.raycastSelectEntity(e.clientX - rect.left, e.clientY - rect.top);

      // While linking, a click picks the frame the story continues into
      // rather than changing the selection.
      if (studio.isThreading && hit !== null) {
        await threadSelectionInto(hit);
        dragMode = "none";
        return;
      }

      studio.selectedEntityId = hit;

      if (hit === null) {
        selectedGeometry = null;
        dragMode = "none";
        await requestRender();
        return;
      }

      dragMode = "move";
      dragEntityId = hit;
      dragBefore = await ipc.getFrameGeometry(hit);
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
      studio.camera = await ipc.panCamera(dx, dy);
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
      if (studio.snapEnabled && dragMode !== "create") {
        const result = await ipc.snapFrameGeometry(dragEntityId, next);
        target = result.geometry;
        studio.isSnapped = result.snapped;
      }

      await ipc.setFrameGeometry(dragEntityId, target);
      await requestRender();
    } catch (err) {
      // The entity may have been removed mid-drag.
    }
  }

  async function handleMouseUp(_e: MouseEvent) {
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
      let after = await ipc.getFrameGeometry(dragEntityId);

      // A click with no drag would leave a 1x1 sliver, so give it a usable size.
      if (dragMode === "create" && after.width <= 2 && after.height <= 2) {
        after = { ...after, width: CLICK_CREATE_SIZE, height: CLICK_CREATE_SIZE * 0.6 };
        await ipc.setFrameGeometry(dragEntityId, after);
      }

      studio.history = await ipc.commitFrameGeometry(dragEntityId, dragBefore, after);
    } catch (err) {
      console.warn("could not commit gesture:", err);
    }

    dragMode = "none";
    dragEntityId = null;
    dragBefore = null;
    studio.isSnapped = false;
    await ipc.clearActiveSnap();
    if (studio.selectedEntityId !== null) {
      try {
        selectedGeometry = await ipc.getFrameGeometry(studio.selectedEntityId);
      } catch (err) {
        selectedGeometry = null;
      }
    }

    // Creation tools revert to Select so the next click does not stack shapes.
    if (studio.activeTool !== "Select") studio.activeTool = "Select";
    await studio.invalidate();
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

    if (studio.selectedEntityId === null) {
      // Nothing selected: pan the view instead, in screen pixels.
      studio.camera = await ipc.panCamera(-dx * 20, -dy * 20);
      await requestRender();
      return;
    }

    try {
      const before = await ipc.getFrameGeometry(studio.selectedEntityId);
      const after = { ...before, x: before.x + dx, y: before.y + dy };
      // Each nudge is its own undoable step, matching how layout tools behave.
      studio.history = await ipc.commitFrameGeometry(studio.selectedEntityId, before, after);
      await requestRender();
    } catch (err) {
      console.warn("nudge failed:", err);
    }
  }

  async function addPage() {
    try {
      await ipc.addPage();
      await studio.invalidate();
    } catch (err) {
      console.warn("could not add page:", err);
    }
  }

  async function removeLastPage() {
    if (studio.pages.length <= 1) return;
    try {
      await ipc.removePage(studio.pages.length);
      await studio.invalidate();
    } catch (err) {
      console.warn("could not remove page:", err);
    }
  }

  /// Adds a ruler guide through the current cursor position.
  async function addGuideAtCursor(isVertical: boolean) {
    const position = isVertical ? mouseDocX : mouseDocY;
    try {
      await ipc.addRulerGuide(isVertical, position);
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
    if (!studio.documentSettings) return;
    const grid = { ...studio.baselineGrid, ...patch };
    studio.baselineGrid = grid;
    try {
      await ipc.setDocumentSettings({ ...studio.documentSettings, baseline_grid: grid });
      studio.documentSettings = { ...studio.documentSettings, baseline_grid: grid };
      await requestRender();
    } catch (err) {
      console.warn("could not update the baseline grid:", err);
    }
  }

  /// Locks the selected text frame to the baseline grid, or releases it.
  async function toggleSelectionBaselineSnap() {
    if (studio.selectedEntityId === null) return;
    const enabled = !selectionSnapsToBaseline;
    try {
      await ipc.setFrameBaselineSnap(studio.selectedEntityId, enabled);
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
    const id = studio.selectedEntityId;
    if (id === null) {
      selectionSnapsToBaseline = false;
      return;
    }
    ipc
      .getFrameBaselineSnap(id)
      .then((snaps) => {
        if (studio.selectedEntityId === id) selectionSnapsToBaseline = snaps;
      })
      .catch(() => {
        if (studio.selectedEntityId === id) selectionSnapsToBaseline = false;
      });
  });

  /// Threads the selected frame into `target`, continuing its story there.
  async function threadSelectionInto(target: number) {
    if (studio.selectedEntityId === null || studio.selectedEntityId === target) return;
    try {
      await ipc.threadTextFrames(studio.selectedEntityId, target);
      studio.isThreading = false;
      await studio.invalidate();
    } catch (err) {
      console.warn("could not thread frames:", err);
      studio.isThreading = false;
    }
  }

  export async function zoomIn() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    studio.camera = await ipc.zoomCamera(rect.width / 2, rect.height / 2, 1.25);
    await requestRender();
  }

  export async function zoomOut() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    studio.camera = await ipc.zoomCamera(rect.width / 2, rect.height / 2, 0.8);
    await requestRender();
  }

  export async function fitPage() {
    if (!viewportEl) return;
    const rect = viewportEl.getBoundingClientRect();
    studio.camera = await ipc.fitPageView(rect.width, rect.height);
    await requestRender();
  }

  export async function resetView() {
    studio.camera = await ipc.resetCamera();
    await requestRender();
  }

  onMount(() => {
    // Painting needs the canvas element, which only this component has, so the
    // store borrows these rather than reaching into the DOM itself.
    studio.registerRenderHooks({ syncViewport, requestRender });

    // The surface is sized to the window, so it must be created after the
    // canvas element has been laid out.
    (async () => {
      await studio.initRenderer();
      await studio.invalidate();
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
        studio.activeTool = tool.id;
        e.preventDefault();
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") isSpacePressed = false;
    };

    const onResize = () => {
      studio.invalidate();
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("resize", onResize);

    return () => {
      observer.disconnect();
      studio.clearRenderHooks();
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("resize", onResize);
    };
  });
</script>

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
      {#if studio.selectedEntityId !== null}
        <span class="selection-tag">Selected Frame #{studio.selectedEntityId}</span>
      {:else}
        <span class="status-idle">
          {studio.activeTool === "Select"
            ? "Click to select • Drag to move • Wheel to navigate"
            : `Drag on the canvas to draw a ${studio.activeTool.toLowerCase()}`}
        </span>
      {/if}
    </div>
  </div>

  <div class="doc-bar">
    <span class="doc-group">
      <strong>{studio.pages.length}</strong>
      {studio.pages.length === 1 ? "page" : "pages"}
      <button class="chip" onclick={addPage} title="Add a page">+ Page</button>
      <button
        class="chip"
        onclick={removeLastPage}
        disabled={studio.pages.length <= 1}
        title="Remove the last page"
      >
        − Page
      </button>
    </span>

    <span class="doc-group">
      <label class="chip-toggle">
        <input type="checkbox" bind:checked={studio.snapEnabled} />
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
          checked={studio.baselineGrid.visible}
          onchange={(e) => applyBaselineGrid({ visible: e.currentTarget.checked })}
        />
        Baseline
      </label>
      <label class="chip-field" title="Distance between baselines">
        <input
          type="number"
          min="1"
          step="0.5"
          value={studio.baselineGrid.increment}
          onchange={(e) => applyBaselineGrid({ increment: Number(e.currentTarget.value) })}
        />
      </label>
      <button
        class="chip"
        class:active={selectionSnapsToBaseline}
        disabled={studio.selectedEntityId === null}
        onclick={toggleSelectionBaselineSnap}
        title="Lock the selected text frame's lines to the baseline grid"
      >
        {selectionSnapsToBaseline ? "On Grid" : "Lock to Grid"}
      </button>
    </span>

    <span class="doc-group">
      <button
        class="chip"
        class:active={studio.isThreading}
        disabled={studio.selectedEntityId === null}
        onclick={() => (studio.isThreading = !studio.isThreading)}
        title="Link the selected text frame into another"
      >
        {studio.isThreading ? "Click target frame…" : "Link Text"}
      </button>
      {#if studio.isSnapped}
        <span class="snap-flag">snapped</span>
      {/if}
    </span>
  </div>

  <div class="tool-palette" role="toolbar" aria-label="Editing tools">
    {#each TOOLS as tool (tool.id)}
      <button
        class="tool-button"
        class:active={studio.activeTool === tool.id}
        aria-pressed={studio.activeTool === tool.id}
        title="{tool.label} ({tool.key})"
        onclick={() => (studio.activeTool = tool.id)}
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

<style>
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

</style>

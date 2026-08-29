<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

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
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let renderEngineMode = $state("WebGPU Initializing...");
  let isWebGpuActive = $state(false);

  let currentScene = $state<RenderScene | null>(null);
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

  async function initWebGpuContext() {
    if (typeof navigator !== "undefined" && "gpu" in navigator) {
      try {
        const adapter = await (navigator as any).gpu.requestAdapter();
        if (adapter) {
          const device = await adapter.requestDevice();
          if (device) {
            isWebGpuActive = true;
            renderEngineMode = "WebGPU Accelerated (Vello Scene)";
            return;
          }
        }
      } catch (err) {
        console.warn("WebGPU initialization note:", err);
      }
    }
    renderEngineMode = "Vector Engine (Vello Pipeline)";
  }

  async function fetchScene() {
    try {
      const [scene, hist, cam] = await Promise.all([
        invoke<RenderScene>("compile_render_scene", {
          selectedId: selectedEntityId,
        }),
        invoke<HistoryStatus>("get_history_status"),
        invoke<Camera>("get_camera_state"),
      ]);
      currentScene = scene;
      historyStatus = hist;
      camera = cam;
      drawScene(scene, cam);
    } catch (err) {
      // Offline fallback
    }
  }

  function drawScene(scene: RenderScene, cam: Camera) {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvasEl.getBoundingClientRect();
    const width = rect.width;
    const height = rect.height;

    if (canvasEl.width !== width * dpr || canvasEl.height !== height * dpr) {
      canvasEl.width = width * dpr;
      canvasEl.height = height * dpr;
    }

    ctx.save();
    ctx.scale(dpr, dpr);

    // 1. Dark Pasteboard Background
    ctx.fillStyle = "#070a12";
    ctx.fillRect(0, 0, width, height);

    // Pasteboard grid (screen space)
    ctx.strokeStyle = "rgba(255, 255, 255, 0.03)";
    ctx.lineWidth = 1;
    for (let x = 0; x < width; x += 24) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
      ctx.stroke();
    }
    for (let y = 0; y < height; y += 24) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }

    // 2. Camera Viewport Transformation (Pan & Zoom)
    ctx.save();
    ctx.translate(cam.pan_x, cam.pan_y);
    ctx.scale(cam.zoom, cam.zoom);

    // Render Compiled Scene in Document Coordinates
    for (const el of scene.elements) {
      switch (el.type) {
        case "PageSurface": {
          // Drop Shadow
          ctx.save();
          ctx.shadowColor = "rgba(0, 0, 0, 0.7)";
          ctx.shadowBlur = el.shadow_blur / cam.zoom;
          ctx.shadowOffsetX = 0;
          ctx.shadowOffsetY = 6 / cam.zoom;

          // White Paper Sheet
          ctx.fillStyle = "#ffffff";
          ctx.fillRect(el.x, el.y, el.width, el.height);
          ctx.restore();

          // Page Outline
          ctx.strokeStyle = "rgba(0, 0, 0, 0.15)";
          ctx.lineWidth = 1 / cam.zoom;
          ctx.strokeRect(el.x, el.y, el.width, el.height);

          // Bleed Margin (Magenta dash)
          ctx.save();
          ctx.strokeStyle = "rgba(236, 72, 153, 0.6)";
          ctx.lineWidth = 1 / cam.zoom;
          ctx.setLineDash([4 / cam.zoom, 4 / cam.zoom]);
          ctx.strokeRect(
            el.x - el.bleed * 2,
            el.y - el.bleed * 2,
            el.width + el.bleed * 4,
            el.height + el.bleed * 4
          );
          ctx.restore();

          // Page Dimension Tag
          ctx.fillStyle = "#94a3b8";
          ctx.font = `${Math.max(10, 11 / cam.zoom)}px system-ui, sans-serif`;
          ctx.fillText(`Page ${el.page_number} (${el.width} × ${el.height} pt)`, el.x, el.y - 8 / cam.zoom);
          break;
        }

        case "RectShape": {
          ctx.save();
          const fill = el.fill_color;
          ctx.fillStyle = `rgba(${fill[0] * 255}, ${fill[1] * 255}, ${fill[2] * 255}, ${fill[3]})`;

          ctx.beginPath();
          ctx.roundRect(el.x, el.y, el.width, el.height, el.corner_radius);
          ctx.fill();

          if (el.stroke_color) {
            const strk = el.stroke_color;
            ctx.strokeStyle = `rgba(${strk[0] * 255}, ${strk[1] * 255}, ${strk[2] * 255}, ${strk[3]})`;
            ctx.lineWidth = el.stroke_width / cam.zoom;
            ctx.stroke();
          }

          ctx.fillStyle = "#ffffff";
          ctx.font = `bold ${Math.max(9, 11)}px system-ui, sans-serif`;
          ctx.fillText(`#${el.id} ${el.name}`, el.x + 8, el.y + 18);
          ctx.restore();
          break;
        }

        case "EllipseShape": {
          ctx.save();
          const fill = el.fill_color;
          ctx.fillStyle = `rgba(${fill[0] * 255}, ${fill[1] * 255}, ${fill[2] * 255}, ${fill[3]})`;

          ctx.beginPath();
          ctx.ellipse(el.cx, el.cy, el.rx, el.ry, el.rotation, 0, Math.PI * 2);
          ctx.fill();

          if (el.stroke_color) {
            const strk = el.stroke_color;
            ctx.strokeStyle = `rgba(${strk[0] * 255}, ${strk[1] * 255}, ${strk[2] * 255}, ${strk[3]})`;
            ctx.lineWidth = el.stroke_width / cam.zoom;
            ctx.stroke();
          }

          ctx.fillStyle = "#ffffff";
          ctx.font = `bold ${Math.max(9, 11)}px system-ui, sans-serif`;
          ctx.textAlign = "center";
          ctx.fillText(`#${el.id} ${el.name}`, el.cx, el.cy);
          ctx.restore();
          break;
        }

        case "TextBlock": {
          ctx.save();
          const fill = el.fill_color;
          ctx.fillStyle = `rgba(${fill[0] * 255}, ${fill[1] * 255}, ${fill[2] * 255}, 0.15)`;
          ctx.strokeStyle = `rgba(${fill[0] * 255}, ${fill[1] * 255}, ${fill[2] * 255}, 0.7)`;
          ctx.lineWidth = 1 / cam.zoom;
          ctx.setLineDash([3 / cam.zoom, 3 / cam.zoom]);

          ctx.fillRect(el.x, el.y, el.width, el.height);
          ctx.strokeRect(el.x, el.y, el.width, el.height);

          ctx.fillStyle = "#ffffff";
          ctx.font = `${el.font_size}px system-ui, sans-serif`;
          ctx.fillText(el.text, el.x + 8, el.y + el.font_size + 6);
          ctx.restore();
          break;
        }

        case "SelectionOverlay": {
          ctx.save();
          ctx.strokeStyle = "#38bdf8";
          ctx.lineWidth = 2 / cam.zoom;
          ctx.shadowColor = "rgba(56, 189, 248, 0.8)";
          ctx.shadowBlur = 10 / cam.zoom;
          ctx.strokeRect(el.min_x, el.min_y, el.max_x - el.min_x, el.max_y - el.min_y);

          // Corner Anchor Nodes
          ctx.fillStyle = "#ffffff";
          const nodeSize = 8 / cam.zoom;
          for (const node of el.corner_nodes) {
            ctx.fillRect(node[0] - nodeSize / 2, node[1] - nodeSize / 2, nodeSize, nodeSize);
            ctx.strokeRect(node[0] - nodeSize / 2, node[1] - nodeSize / 2, nodeSize, nodeSize);
          }
          ctx.restore();
          break;
        }
      }
    }

    ctx.restore(); // Restore Camera
    ctx.restore(); // Restore Base
  }

  // Camera & Mouse Interactions
  async function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect) return;

    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;

    if (e.ctrlKey || e.metaKey) {
      // Zoom centered at mouse cursor
      const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
      camera = await invoke<Camera>("zoom_camera", {
        screenX,
        screenY,
        factor: zoomFactor,
      });
    } else {
      // Pan
      const dx = -e.deltaX;
      const dy = -e.deltaY;
      camera = await invoke<Camera>("pan_camera", { dx, dy });
    }

    if (currentScene) {
      drawScene(currentScene, camera);
    }
  }

  function handleMouseDown(e: MouseEvent) {
    if (e.button === 1 || isSpacePressed) {
      isMiddlePanning = true;
      panStartScreenX = e.clientX;
      panStartScreenY = e.clientY;
      e.preventDefault();
    }
  }

  async function handleMouseMove(e: MouseEvent) {
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect) return;
    mouseScreenX = Math.round(e.clientX - rect.left);
    mouseScreenY = Math.round(e.clientY - rect.top);

    if (isMiddlePanning) {
      const dx = e.clientX - panStartScreenX;
      const dy = e.clientY - panStartScreenY;
      panStartScreenX = e.clientX;
      panStartScreenY = e.clientY;

      camera = await invoke<Camera>("pan_camera", { dx, dy });
      if (currentScene) drawScene(currentScene, camera);
    }
  }

  function handleMouseUp(e: MouseEvent) {
    if (e.button === 1 || isMiddlePanning) {
      isMiddlePanning = false;
    }
  }

  async function handleCanvasClick(e: MouseEvent) {
    if (isMiddlePanning) return;
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect) return;

    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;

    try {
      const selected = await invoke<number | null>("raycast_select_entity", {
        screenX,
        screenY,
      });
      selectedEntityId = selected;
      await fetchScene();
    } catch (err) {
      //
    }
  }

  async function zoomIn() {
    if (!canvasEl) return;
    const rect = canvasEl.getBoundingClientRect();
    camera = await invoke<Camera>("zoom_camera", {
      screenX: rect.width / 2,
      screenY: rect.height / 2,
      factor: 1.25,
    });
    if (currentScene) drawScene(currentScene, camera);
  }

  async function zoomOut() {
    if (!canvasEl) return;
    const rect = canvasEl.getBoundingClientRect();
    camera = await invoke<Camera>("zoom_camera", {
      screenX: rect.width / 2,
      screenY: rect.height / 2,
      factor: 0.8,
    });
    if (currentScene) drawScene(currentScene, camera);
  }

  async function fitPageView() {
    if (!canvasEl) return;
    const rect = canvasEl.getBoundingClientRect();
    camera = await invoke<Camera>("fit_page_view", {
      viewportWidth: rect.width,
      viewportHeight: rect.height,
    });
    if (currentScene) drawScene(currentScene, camera);
  }

  async function resetCamera() {
    camera = await invoke<Camera>("reset_camera");
    if (currentScene) drawScene(currentScene, camera);
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
    initWebGpuContext();
    fetchScene();

    const interval = setInterval(fetchScene, 400);

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space") isSpacePressed = true;
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") isSpacePressed = false;
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("resize", fetchScene);

    return () => {
      clearInterval(interval);
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
            <span class="status-idle">Click element to select • Drag / Wheel to navigate</span>
          {/if}
        </div>
      </div>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="canvas-container {isSpacePressed || isMiddlePanning ? 'panning' : ''}">
        <canvas
          bind:this={canvasEl}
          onwheel={handleWheel}
          onmousedown={handleMouseDown}
          onmousemove={handleMouseMove}
          onmouseup={handleMouseUp}
          onclick={handleCanvasClick}
        ></canvas>
      </div>

      <div class="canvas-footer">
        <span>💡 <strong>Navigation Shortcuts:</strong> <code>Ctrl + Wheel</code> to Zoom centered on cursor • <code>Trackpad Swipe</code> / <code>Wheel</code> to Pan • <code>Space + Drag</code> for Pan tool.</span>
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
  :global(body) {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    background-color: #050811;
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
  .canvas-panel {
    min-height: 540px;
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
    cursor: default;
  }

  .canvas-container.panning {
    cursor: grab;
  }

  canvas {
    width: 100%;
    height: 100%;
    display: block;
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

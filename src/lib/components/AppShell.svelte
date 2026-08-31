<script lang="ts">
  /**
   * The application frame: title bar, camera controls, and the dock layout.
   *
   * Dock slots are fixed rather than floating. Panels live in named grid areas
   * and resize with the window; making them draggable is deferred until the
   * editing surface itself is proven.
   */
  import { studio } from "$lib/state.svelte";
  import Viewport from "./Viewport.svelte";
  import Inspector from "./Inspector.svelte";
  import LayersPanel from "./LayersPanel.svelte";
  import AssetsPanel from "./AssetsPanel.svelte";
  import PagesPanel from "./PagesPanel.svelte";

  // The camera controls live in the header but act on the canvas, which owns
  // the element they measure against. Binding the instance is how the header
  // reaches them without duplicating the viewport-rect arithmetic.
  let viewport = $state<ReturnType<typeof Viewport> | null>(null);
</script>

<main class="app-container">
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
        <p class="subtitle">Layout Studio • Phase 4: Editing Surface</p>
      </div>
    </div>

    <div class="header-controls">
      <div class="camera-toolbar">
        <button class="btn-cam" onclick={() => viewport?.zoomOut()} title="Zoom Out (-)">−</button>
        <span class="zoom-indicator">{studio.zoomPercentage}%</span>
        <button class="btn-cam" onclick={() => viewport?.zoomIn()} title="Zoom In (+)">+</button>
        <button class="btn-cam-text" onclick={() => viewport?.fitPage()} title="Fit document page in view">Fit Page</button>
        <button class="btn-cam-text" onclick={() => viewport?.resetView()} title="Reset to 100%">100%</button>
      </div>

      <div class="history-group">
        <button class="btn-icon" onclick={() => studio.undo()} disabled={!studio.history.can_undo}>
          ↶ Undo <span class="counter">({studio.history.undo_count})</span>
        </button>
        <button class="btn-icon" onclick={() => studio.redo()} disabled={!studio.history.can_redo}>
          ↷ Redo <span class="counter">({studio.history.redo_count})</span>
        </button>
      </div>

      <span class="badge {studio.isWebGpuActive ? 'webgpu-active' : 'engine-badge'}">
        ⚡ {studio.renderEngineMode}
      </span>
    </div>
  </header>

  <div class="viewport-layout">
    <div class="stage">
      <Viewport bind:this={viewport} />
      <PagesPanel />
    </div>
    <div class="rail">
      <Inspector />
      <AssetsPanel />
      <LayersPanel />
    </div>
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
  /* Fixed dock slots: the canvas and its pages strip on the left, the
     inspector and layers rail on the right. */
  .viewport-layout {
    display: grid;
    grid-template-columns: 1fr 340px;
    gap: 1.25rem;
    align-items: start;
  }

  .stage,
  .rail {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    min-width: 0;
  }

  @media (max-width: 1080px) {
    .viewport-layout {
      grid-template-columns: 1fr;
    }
  }


</style>

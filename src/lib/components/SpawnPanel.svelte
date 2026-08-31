<script lang="ts">
  /**
   * The original frame-spawning sidebar, moved here unchanged.
   *
   * Temporary. The next step replaces this with the context-aware property
   * inspector; it is kept for now so the decomposition changes no behavior.
   */
  import * as ipc from "$lib/ipc";
  import { COLOR_MAP, studio } from "$lib/state.svelte";
  import type { ColorPreset } from "$lib/state.svelte";

  let frameName = $state("Hero Heading");
  let frameType = $state<"Rectangle" | "Ellipse" | "Text">("Rectangle");
  let posX = $state(80);
  let posY = $state(80);
  let sizeW = $state(220);
  let sizeH = $state(110);

  async function spawnNewFrame() {
    try {
      const fill = COLOR_MAP[studio.colorPreset] ?? [0.2, 0.7, 1.0, 1.0];
      const newId = await ipc.spawnFrame({
        name: frameName || "New Frame",
        frameType,
        x: posX,
        y: posY,
        width: sizeW,
        height: sizeH,
        fillColor: fill,
        text: frameType === "Text" ? studio.defaultText : null,
      });
      studio.selectedEntityId = newId;
      await studio.invalidate();
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
</script>

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
        <select id="f-color" bind:value={studio.colorPreset as ColorPreset}>
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
        <input id="f-text" type="text" bind:value={studio.defaultText} />
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
      <span class="stat-v">({Math.round(studio.camera.pan_x)}, {Math.round(studio.camera.pan_y)}) px</span>
    </div>
    <div class="stat-item">
      <span class="stat-k">Active Zoom</span>
      <span class="stat-v highlight">{(studio.camera.zoom * 100).toFixed(1)}%</span>
    </div>
    <div class="stat-item">
      <span class="stat-k">Raycast Selection</span>
      <span class="stat-v success">Affine Document Mapping</span>
    </div>
  </div>
</aside>

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

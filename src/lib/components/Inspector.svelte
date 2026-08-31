<script lang="ts">
  /**
   * The context-aware property panel.
   *
   * Which sections appear is derived from the selected frame's type, so a
   * rectangle never shows type controls and a text frame never hides them.
   * With nothing selected it falls back to the quick-create row, because an
   * empty panel wastes the most valuable column on screen.
   */
  import * as ipc from "$lib/ipc";
  import { COLOR_MAP, studio } from "$lib/state.svelte";
  import type { ColorPreset } from "$lib/state.svelte";
  import TransformSection from "./inspector/TransformSection.svelte";
  import VectorSection from "./inspector/VectorSection.svelte";
  import TypographySection from "./inspector/TypographySection.svelte";

  let frame = $derived(studio.selectedFrame);
  let isText = $derived(frame?.frame_type === "Text");

  const PRESETS: ColorPreset[] = ["cyan", "purple", "emerald", "amber"];

  let renaming = $state(false);
  let draftName = $state("");

  async function quickSpawn(type: "Rectangle" | "Ellipse" | "Text") {
    try {
      const id = await ipc.spawnFrame({
        name: `${type} ${Math.floor(Math.random() * 900) + 100}`,
        frameType: type,
        x: Math.floor(Math.random() * 260) + 60,
        y: Math.floor(Math.random() * 180) + 60,
        width: type === "Text" ? 220 : 160,
        height: type === "Text" ? 110 : 120,
        fillColor: COLOR_MAP[studio.colorPreset],
        text: type === "Text" ? studio.defaultText : null,
      });
      studio.select(id);
      await studio.invalidate();
    } catch (err) {
      console.error("could not create frame:", err);
    }
  }

  function startRename() {
    if (!frame) return;
    draftName = frame.name;
    renaming = true;
  }

  async function commitRename() {
    if (!frame || !renaming) return;
    renaming = false;
    const name = draftName.trim();
    if (!name || name === frame.name) return;
    await ipc.renameFrame(frame.id, name);
    await studio.invalidate();
  }

  async function removeFrame() {
    if (!frame) return;
    studio.history = await ipc.deleteFrame(frame.id);
    studio.select(null);
    await studio.invalidate();
  }
</script>

<aside class="inspector card">
  {#if frame}
    <header class="head">
      {#if renaming}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="rename"
          autofocus
          bind:value={draftName}
          onblur={commitRename}
          onkeydown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") renaming = false;
          }}
        />
      {:else}
        <button class="name" onclick={startRename} title="Rename this frame">
          {frame.name}
        </button>
      {/if}
      <div class="head-meta">
        <span class="kind">{frame.frame_type}</span>
        <button class="danger" onclick={removeFrame} title="Delete this frame">Delete</button>
      </div>
    </header>

    <div class="sections">
      <TransformSection entityId={frame.id} />
      <VectorSection entityId={frame.id} frameType={frame.frame_type} />
      {#if isText}
        <TypographySection entityId={frame.id} />
      {/if}
    </div>
  {:else}
    <header class="head">
      <span class="name idle">Nothing selected</span>
    </header>
    <p class="hint">Select a frame on the canvas to edit it, or create one:</p>
    <div class="quick">
      <button onclick={() => quickSpawn("Rectangle")}>Rectangle</button>
      <button onclick={() => quickSpawn("Ellipse")}>Ellipse</button>
      <button onclick={() => quickSpawn("Text")}>Text</button>
    </div>

    <div class="stack">
      <span class="label">Fill for new frames</span>
      <div class="presets">
        {#each PRESETS as preset (preset)}
          <button
            class="preset"
            class:active={studio.colorPreset === preset}
            aria-label={preset}
            title={preset}
            style="background: rgba({COLOR_MAP[preset][0] * 255}, {COLOR_MAP[preset][1] *
              255}, {COLOR_MAP[preset][2] * 255}, 1)"
            onclick={() => (studio.colorPreset = preset)}
          ></button>
        {/each}
      </div>
    </div>
  {/if}
</aside>

<style>
  .card {
    background: rgba(15, 23, 42, 0.75);
    backdrop-filter: blur(14px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 14px;
    padding: 1.15rem;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
  }

  .inspector {
    gap: 1rem;
    min-width: 0;
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-bottom: 0.75rem;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }

  .name {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    font-size: 1rem;
    font-weight: 600;
    color: #f8fafc;
    cursor: text;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name:hover {
    color: #38bdf8;
  }

  .name.idle {
    color: #64748b;
    font-weight: 500;
    cursor: default;
  }

  .rename {
    background: rgba(10, 15, 30, 0.9);
    border: 1px solid #38bdf8;
    border-radius: 6px;
    padding: 0.35rem 0.5rem;
    color: #f8fafc;
    font-size: 0.95rem;
    font-weight: 600;
    outline: none;
  }

  .head-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .kind {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: #38bdf8;
    background: rgba(56, 189, 248, 0.12);
    padding: 0.15rem 0.45rem;
    border-radius: 4px;
  }

  .danger {
    background: transparent;
    border: 1px solid rgba(248, 113, 113, 0.3);
    border-radius: 5px;
    color: #f87171;
    font-size: 0.68rem;
    font-weight: 600;
    padding: 0.2rem 0.5rem;
    cursor: pointer;
    transition: all 0.15s;
  }

  .danger:hover {
    background: rgba(248, 113, 113, 0.12);
    border-color: #f87171;
  }

  .sections {
    display: flex;
    flex-direction: column;
    gap: 1.1rem;
  }

  .hint {
    margin: 0;
    font-size: 0.8rem;
    color: #94a3b8;
  }

  .quick {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.4rem;
  }

  .quick button {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 6px;
    color: #cbd5e1;
    padding: 0.45rem 0.2rem;
    font-size: 0.72rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s;
  }

  .quick button:hover {
    background: rgba(56, 189, 248, 0.12);
    border-color: #38bdf8;
    color: #38bdf8;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .label {
    font-size: 0.7rem;
    color: #94a3b8;
    font-weight: 500;
  }

  .presets {
    display: flex;
    gap: 0.4rem;
  }

  .preset {
    width: 1.7rem;
    height: 1.7rem;
    border-radius: 6px;
    border: 2px solid transparent;
    cursor: pointer;
    transition: all 0.15s;
  }

  .preset:hover {
    transform: translateY(-1px);
  }

  .preset.active {
    border-color: #f8fafc;
    box-shadow: 0 0 0 2px rgba(56, 189, 248, 0.4);
  }
</style>

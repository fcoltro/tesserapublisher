<script lang="ts">
  /**
   * The document tree, with the controls that act on its structure.
   *
   * Frames are listed top-of-stack first, matching what the canvas shows —
   * the topmost row is the frame you would click. Dropping one row on another
   * restacks it to that neighbour's depth.
   */
  import * as ipc from "$lib/ipc";
  import type { FrameNode, LayerNode } from "$lib/ipc";
  import { studio } from "$lib/state.svelte";

  let pages = $derived(studio.tree?.pages ?? []);

  /** The frame being dragged, or null. */
  let draggingId = $state<number | null>(null);
  let dropTargetId = $state<number | null>(null);

  /** Topmost first, so the list reads the way the canvas stacks. */
  function stacked(layer: LayerNode): FrameNode[] {
    return [...layer.frames].sort((a, b) => b.z_index - a.z_index);
  }

  async function toggleVisible(layer: LayerNode) {
    await ipc.setLayerVisibility(layer.id, !layer.is_visible);
    await studio.invalidate();
  }

  async function toggleLocked(layer: LayerNode) {
    await ipc.setLayerLocked(layer.id, !layer.is_locked);
    await studio.invalidate();
  }

  async function removeFrame(id: number) {
    studio.history = await ipc.deleteFrame(id);
    if (studio.selectedEntityId === id) studio.select(null);
    await studio.invalidate();
  }

  function onDragStart(id: number) {
    draggingId = id;
  }

  function onDragOver(event: DragEvent, id: number) {
    if (draggingId === null || draggingId === id) return;
    event.preventDefault();
    dropTargetId = id;
  }

  async function onDrop(target: FrameNode) {
    const moved = draggingId;
    draggingId = null;
    dropTargetId = null;
    if (moved === null || moved === target.id) return;

    // Taking the target's depth puts the dragged frame where the drop landed.
    await ipc.setFrameZIndex(moved, target.z_index);
    await studio.invalidate();
  }
</script>

<section class="panel card">
  <header class="head">
    <h2>Layers</h2>
    <span class="tally">{studio.allFrames.length} frames</span>
  </header>

  <div class="scroll">
    {#each pages as page (page.id)}
      {#each page.layers as layer (layer.id)}
        <div class="layer">
          <div class="layer-head">
            <button
              class="icon"
              class:off={!layer.is_visible}
              title={layer.is_visible ? "Hide this layer" : "Show this layer"}
              aria-label={layer.is_visible ? "Hide this layer" : "Show this layer"}
              onclick={() => toggleVisible(layer)}
            >
              {layer.is_visible ? "◉" : "○"}
            </button>
            <button
              class="icon"
              class:on={layer.is_locked}
              title={layer.is_locked ? "Unlock this layer" : "Lock this layer"}
              aria-label={layer.is_locked ? "Unlock this layer" : "Lock this layer"}
              onclick={() => toggleLocked(layer)}
            >
              {layer.is_locked ? "🔒" : "🔓"}
            </button>
            <span class="layer-name">{layer.name}</span>
            <span class="page-tag">p{page.page_number}</span>
          </div>

          {#if layer.frames.length === 0}
            <p class="empty">No frames on this layer.</p>
          {:else}
            <ul class="frames">
              {#each stacked(layer) as frame (frame.id)}
                <li>
                  <div
                    class="row"
                    class:selected={studio.selectedEntityId === frame.id}
                    class:dragging={draggingId === frame.id}
                    class:drop={dropTargetId === frame.id}
                    draggable="true"
                    role="option"
                    tabindex="0"
                    aria-selected={studio.selectedEntityId === frame.id}
                    ondragstart={() => onDragStart(frame.id)}
                    ondragover={(e) => onDragOver(e, frame.id)}
                    ondragleave={() => (dropTargetId = null)}
                    ondrop={() => onDrop(frame)}
                    ondragend={() => {
                      draggingId = null;
                      dropTargetId = null;
                    }}
                    onclick={() => studio.select(frame.id)}
                    onkeydown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        studio.select(frame.id);
                      }
                      if (e.key === "Delete" || e.key === "Backspace") {
                        e.preventDefault();
                        removeFrame(frame.id);
                      }
                    }}
                  >
                    <span class="glyph" aria-hidden="true">
                      {#if frame.frame_type === "Text"}T
                      {:else if frame.frame_type === "Ellipse"}○
                      {:else if frame.frame_type === "Line"}╱
                      {:else if frame.frame_type === "Path"}✎
                      {:else if frame.frame_type === "Image"}▤
                      {:else}▭{/if}
                    </span>
                    <span class="row-name">{frame.name}</span>
                    <span class="depth">{frame.z_index}</span>
                    <button
                      class="del"
                      title="Delete {frame.name}"
                      aria-label="Delete {frame.name}"
                      onclick={(e) => {
                        e.stopPropagation();
                        removeFrame(frame.id);
                      }}
                    >
                      ×
                    </button>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/each}
    {/each}
  </div>
</section>

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

  .panel {
    gap: 0.75rem;
    min-height: 0;
  }

  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }

  h2 {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
  }

  .tally {
    font-size: 0.7rem;
    color: #64748b;
    font-variant-numeric: tabular-nums;
  }

  .scroll {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    overflow-y: auto;
    max-height: 20rem;
  }

  .layer {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .layer-head {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .icon {
    background: transparent;
    border: none;
    color: #94a3b8;
    font-size: 0.8rem;
    line-height: 1;
    padding: 0.15rem;
    cursor: pointer;
    border-radius: 4px;
    transition: all 0.15s;
  }

  .icon:hover {
    color: #38bdf8;
    background: rgba(255, 255, 255, 0.06);
  }

  .icon.off {
    color: #475569;
  }

  .icon.on {
    color: #fbbf24;
  }

  .layer-name {
    font-size: 0.78rem;
    font-weight: 600;
    color: #cbd5e1;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .page-tag {
    font-size: 0.64rem;
    color: #64748b;
    background: rgba(255, 255, 255, 0.05);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .empty {
    margin: 0 0 0 1.4rem;
    font-size: 0.72rem;
    color: #475569;
  }

  .frames {
    list-style: none;
    margin: 0;
    padding: 0 0 0 0.55rem;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.32rem 0.4rem;
    border-radius: 5px;
    border: 1px solid transparent;
    cursor: pointer;
    transition: background 0.12s;
  }

  .row:hover {
    background: rgba(255, 255, 255, 0.05);
  }

  .row:focus-visible {
    outline: 2px solid #38bdf8;
    outline-offset: -1px;
  }

  .row.selected {
    background: rgba(56, 189, 248, 0.14);
    border-color: rgba(56, 189, 248, 0.35);
  }

  .row.dragging {
    opacity: 0.4;
  }

  .row.drop {
    border-color: #38bdf8;
    border-style: dashed;
  }

  .glyph {
    width: 1rem;
    text-align: center;
    font-size: 0.72rem;
    color: #64748b;
    flex-shrink: 0;
  }

  .row.selected .glyph {
    color: #38bdf8;
  }

  .row-name {
    flex: 1;
    font-size: 0.76rem;
    color: #e2e8f0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .depth {
    font-size: 0.65rem;
    color: #475569;
    font-variant-numeric: tabular-nums;
  }

  .del {
    background: transparent;
    border: none;
    color: #475569;
    font-size: 0.95rem;
    line-height: 1;
    padding: 0 0.15rem;
    cursor: pointer;
    border-radius: 3px;
    opacity: 0;
    transition: all 0.15s;
  }

  .row:hover .del {
    opacity: 1;
  }

  .del:hover {
    color: #f87171;
    background: rgba(248, 113, 113, 0.12);
  }
</style>

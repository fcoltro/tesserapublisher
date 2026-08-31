<script lang="ts">
  /**
   * Position and size for the selected frame.
   *
   * Reads geometry straight from the backend rather than from the document
   * tree, because a drag on the canvas moves the frame without the tree being
   * re-pulled — the tree would show a stale box mid-gesture.
   */
  import * as ipc from "$lib/ipc";
  import type { FrameGeometry } from "$lib/ipc";
  import { studio } from "$lib/state.svelte";
  import NumberField from "../NumberField.svelte";

  interface Props {
    entityId: number;
  }

  let { entityId }: Props = $props();

  let geometry = $state<FrameGeometry | null>(null);
  /** Geometry when the current gesture opened, for the undo entry. */
  let before: FrameGeometry | null = null;

  $effect(() => {
    const id = entityId;
    ipc
      .getFrameGeometry(id)
      .then((g) => {
        if (entityId === id) geometry = g;
      })
      .catch(() => {
        if (entityId === id) geometry = null;
      });
  });

  function start() {
    before = geometry ? { ...geometry } : null;
  }

  async function apply(patch: Partial<FrameGeometry>) {
    if (!geometry) return;
    geometry = { ...geometry, ...patch };
    await ipc.setFrameGeometry(entityId, geometry);
    await studio.repaint();
  }

  async function commit() {
    if (!before || !geometry) return;
    studio.history = await ipc.commitFrameGeometry(entityId, before, geometry);
    before = null;
    await studio.invalidate();
  }
</script>

{#if geometry}
  <section class="section">
    <h3>Transform</h3>
    <div class="grid">
      <NumberField
        label="X"
        value={geometry.x}
        suffix="pt"
        onstart={start}
        oninput={(v) => apply({ x: v })}
        onend={commit}
      />
      <NumberField
        label="Y"
        value={geometry.y}
        suffix="pt"
        onstart={start}
        oninput={(v) => apply({ y: v })}
        onend={commit}
      />
      <NumberField
        label="Width"
        value={geometry.width}
        min={1}
        suffix="pt"
        onstart={start}
        oninput={(v) => apply({ width: v })}
        onend={commit}
      />
      <NumberField
        label="Height"
        value={geometry.height}
        min={1}
        suffix="pt"
        onstart={start}
        oninput={(v) => apply({ height: v })}
        onend={commit}
      />
    </div>
  </section>
{/if}

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  h3 {
    margin: 0;
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: #64748b;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.55rem;
  }
</style>

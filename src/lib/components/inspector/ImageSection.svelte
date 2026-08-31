<script lang="ts">
  /**
   * Fit mode and link state for a placed image.
   *
   * Fit is applied immediately rather than through a gesture: it is a discrete
   * choice, so one click is one change.
   */
  import * as ipc from "$lib/ipc";
  import type { ImageFit, ImageSource } from "$lib/ipc";
  import { studio } from "$lib/state.svelte";

  interface Props {
    entityId: number;
    /** Placed width in points, for the resolution figure. */
    placedWidth: number;
  }

  let { entityId, placedWidth }: Props = $props();

  let source = $state<ImageSource | null>(null);

  const FITS: { id: ImageFit; label: string; hint: string }[] = [
    { id: "Fill", label: "Fill", hint: "Cover the frame, cropping the overflow" },
    { id: "Fit", label: "Fit", hint: "Show the whole image inside the frame" },
    { id: "Stretch", label: "Stretch", hint: "Ignore the aspect ratio" },
  ];

  /** Points per inch, matching the backend's calculation. */
  let effectivePpi = $derived(
    source && placedWidth > 0 ? source.natural_width / (placedWidth / 72) : 0,
  );

  $effect(() => {
    const id = entityId;
    ipc
      .getImageSource(id)
      .then((s) => {
        if (entityId === id) source = s;
      })
      .catch(() => {
        if (entityId === id) source = null;
      });
  });

  async function chooseFit(fit: ImageFit) {
    if (!source) return;
    source = { ...source, fit };
    await ipc.setImageFit(entityId, fit);
    await studio.repaint();
  }
</script>

{#if source}
  <section class="section">
    <h3>Image</h3>

    <div class="meta">
      <span class="path" title={source.path}>{source.path}</span>
      <span class="dims">
        {source.natural_width} × {source.natural_height} px ·
        <strong class:low={effectivePpi < 300}>{Math.round(effectivePpi)} PPI</strong>
      </span>
    </div>

    <div class="stack">
      <span class="label">Fit</span>
      <div class="segmented" role="group" aria-label="Image fit">
        {#each FITS as option (option.id)}
          <button
            class="seg"
            class:active={source.fit === option.id}
            aria-pressed={source.fit === option.id}
            title={option.hint}
            onclick={() => chooseFit(option.id)}
          >
            {option.label}
          </button>
        {/each}
      </div>
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

  .meta {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }

  .path {
    font-size: 0.72rem;
    color: #94a3b8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .dims {
    font-size: 0.7rem;
    color: #64748b;
    font-variant-numeric: tabular-nums;
  }

  .dims strong {
    color: #4ade80;
  }

  .dims strong.low {
    color: #fbbf24;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .label {
    font-size: 0.7rem;
    color: #94a3b8;
    font-weight: 500;
  }

  .segmented {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
    background: rgba(10, 15, 30, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    padding: 2px;
  }

  .seg {
    background: transparent;
    border: none;
    border-radius: 4px;
    color: #94a3b8;
    font-size: 0.7rem;
    font-weight: 600;
    padding: 0.35rem 0;
    cursor: pointer;
    transition: all 0.15s;
  }

  .seg:hover {
    background: rgba(255, 255, 255, 0.06);
    color: #e2e8f0;
  }

  .seg.active {
    background: rgba(56, 189, 248, 0.18);
    color: #38bdf8;
  }
</style>

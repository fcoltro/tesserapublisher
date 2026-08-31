<script lang="ts">
  /**
   * Fill, stroke, opacity and corner rounding for the selected frame.
   *
   * Corner radius applies only to rectangles — an ellipse has no corners to
   * round, and showing a dead control is worse than showing none.
   */
  import * as ipc from "$lib/ipc";
  import type { FrameType, Style } from "$lib/ipc";
  import { hexToRgba, rgbaToHex } from "$lib/color";
  import { studio } from "$lib/state.svelte";
  import NumberField from "../NumberField.svelte";

  interface Props {
    entityId: number;
    frameType: FrameType;
  }

  let { entityId, frameType }: Props = $props();

  let style = $state<Style | null>(null);
  /** Style when the current gesture opened, for the undo entry. */
  let before: Style | null = null;

  const DEFAULT_STROKE: ipc.Rgba = [0.4, 0.6, 1.0, 1.0];

  $effect(() => {
    const id = entityId;
    ipc
      .getFrameStyle(id)
      .then((s) => {
        if (entityId === id) style = s;
      })
      .catch(() => {
        if (entityId === id) style = null;
      });
  });

  function start() {
    before = style ? { ...style } : null;
  }

  async function apply(patch: Partial<Style>) {
    if (!style) return;
    style = { ...style, ...patch };
    await ipc.setFrameStyle(entityId, style);
    await studio.repaint();
  }

  async function commit() {
    if (!before || !style) return;
    studio.history = await ipc.commitFrameStyle(entityId, before, style);
    before = null;
    await studio.invalidate();
  }

  /** A colour swatch has no gesture of its own; each pick is one edit. */
  async function pickColor(which: "fill" | "stroke", hex: string) {
    if (!style) return;
    start();
    if (which === "fill") {
      await apply({ fill_color: hexToRgba(hex, style.fill_color[3]) });
    } else {
      await apply({ stroke_color: hexToRgba(hex, style.stroke_color?.[3] ?? 1) });
    }
    await commit();
  }

  async function toggleStroke() {
    if (!style) return;
    start();
    await apply({ stroke_color: style.stroke_color ? null : DEFAULT_STROKE });
    await commit();
  }
</script>

{#if style}
  <section class="section">
    <h3>Appearance</h3>

    <div class="swatch-row">
      <label class="swatch">
        <span class="swatch-label">Fill</span>
        <input
          type="color"
          value={rgbaToHex(style.fill_color)}
          oninput={(e) => pickColor("fill", e.currentTarget.value)}
        />
      </label>

      <label class="swatch">
        <span class="swatch-label">Stroke</span>
        <input
          type="color"
          disabled={!style.stroke_color}
          value={rgbaToHex(style.stroke_color ?? DEFAULT_STROKE)}
          oninput={(e) => pickColor("stroke", e.currentTarget.value)}
        />
      </label>

      <button
        class="ghost"
        onclick={toggleStroke}
        title={style.stroke_color ? "Remove the stroke" : "Add a stroke"}
      >
        {style.stroke_color ? "No stroke" : "Add stroke"}
      </button>
    </div>

    <div class="grid">
      <NumberField
        label="Stroke width"
        value={style.stroke_width}
        min={0}
        step={0.25}
        suffix="pt"
        disabled={!style.stroke_color}
        onstart={start}
        oninput={(v) => apply({ stroke_width: v })}
        onend={commit}
      />
      <NumberField
        label="Opacity"
        value={style.opacity * 100}
        min={0}
        max={100}
        precision={0}
        suffix="%"
        onstart={start}
        oninput={(v) => apply({ opacity: v / 100 })}
        onend={commit}
      />
      {#if frameType === "Rectangle"}
        <NumberField
          label="Corner radius"
          value={style.corner_radius}
          min={0}
          step={0.5}
          suffix="pt"
          onstart={start}
          oninput={(v) => apply({ corner_radius: v })}
          onend={commit}
        />
      {/if}
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

  .swatch-row {
    display: flex;
    align-items: flex-end;
    gap: 0.6rem;
    flex-wrap: wrap;
  }

  .swatch {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .swatch-label {
    font-size: 0.7rem;
    color: #94a3b8;
    font-weight: 500;
  }

  input[type="color"] {
    width: 3.1rem;
    height: 1.85rem;
    padding: 2px;
    background: rgba(10, 15, 30, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    cursor: pointer;
  }

  input[type="color"]:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .ghost {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 6px;
    color: #cbd5e1;
    font-size: 0.72rem;
    font-weight: 600;
    padding: 0.42rem 0.6rem;
    cursor: pointer;
    transition: all 0.15s;
  }

  .ghost:hover {
    border-color: #38bdf8;
    color: #38bdf8;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.55rem;
  }
</style>

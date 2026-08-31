<script lang="ts">
  /**
   * Type settings for a text frame.
   *
   * Leading is expressed as a multiple of font size and tracking in
   * thousandths of an em, matching what the backend stores and what
   * typesetters expect — both stay proportional when the size changes.
   */
  import * as ipc from "$lib/ipc";
  import type { TextAlignment, TextContent } from "$lib/ipc";
  import { studio } from "$lib/state.svelte";
  import NumberField from "../NumberField.svelte";

  interface Props {
    entityId: number;
  }

  let { entityId }: Props = $props();

  let text = $state<TextContent | null>(null);
  /** Content when the current gesture opened, for the undo entry. */
  let before: TextContent | null = null;

  const ALIGNMENTS: { id: TextAlignment; label: string; glyph: string }[] = [
    { id: "Start", label: "Align left", glyph: "◀" },
    { id: "Center", label: "Align centre", glyph: "◆" },
    { id: "End", label: "Align right", glyph: "▶" },
    { id: "Justify", label: "Justify", glyph: "▬" },
  ];

  const WEIGHTS = [300, 400, 500, 600, 700, 800];

  $effect(() => {
    const id = entityId;
    ipc
      .getFrameText(id)
      .then((t) => {
        if (entityId === id) text = t;
      })
      .catch(() => {
        if (entityId === id) text = null;
      });
  });

  function start() {
    before = text ? { ...text } : null;
  }

  async function apply(patch: Partial<TextContent>) {
    if (!text) return;
    text = { ...text, ...patch };
    await ipc.setFrameText(entityId, text);
    await studio.repaint();
  }

  async function commit() {
    if (!before || !text) return;
    studio.history = await ipc.commitFrameText(entityId, before, text);
    before = null;
    await studio.invalidate();
  }

  /** Discrete controls open and close their own gesture around one change. */
  async function applyOnce(patch: Partial<TextContent>) {
    start();
    await apply(patch);
    await commit();
  }
</script>

{#if text}
  <section class="section">
    <h3>Typography</h3>

    <label class="stack">
      <span class="label">Text</span>
      <textarea
        rows="3"
        value={text.text}
        oninput={(e) => apply({ text: e.currentTarget.value })}
        onfocus={start}
        onblur={commit}
      ></textarea>
    </label>

    <label class="stack">
      <span class="label">Font family</span>
      <input
        type="text"
        placeholder="System default"
        value={text.font_family ?? ""}
        oninput={(e) => apply({ font_family: e.currentTarget.value || null })}
        onfocus={start}
        onblur={commit}
      />
    </label>

    <div class="grid">
      <NumberField
        label="Size"
        value={text.font_size}
        min={1}
        step={0.5}
        suffix="pt"
        onstart={start}
        oninput={(v) => apply({ font_size: v })}
        onend={commit}
      />
      <NumberField
        label="Leading"
        value={text.line_height}
        min={0.1}
        step={0.05}
        suffix="×"
        onstart={start}
        oninput={(v) => apply({ line_height: v })}
        onend={commit}
      />
      <NumberField
        label="Tracking"
        value={text.tracking}
        step={5}
        precision={0}
        suffix="/1000 em"
        onstart={start}
        oninput={(v) => apply({ tracking: v })}
        onend={commit}
      />
      <label class="stack">
        <span class="label">Weight</span>
        <select
          value={text.font_weight}
          onchange={(e) => applyOnce({ font_weight: Number(e.currentTarget.value) })}
        >
          {#each WEIGHTS as weight (weight)}
            <option value={weight}>{weight}</option>
          {/each}
        </select>
      </label>
    </div>

    <div class="stack">
      <span class="label">Alignment</span>
      <div class="segmented" role="group" aria-label="Paragraph alignment">
        {#each ALIGNMENTS as option (option.id)}
          <button
            class="seg"
            class:active={text.align === option.id}
            aria-pressed={text.align === option.id}
            title={option.label}
            onclick={() => applyOnce({ align: option.id })}
          >
            {option.glyph}
          </button>
        {/each}
      </div>
    </div>

    <label class="check">
      <input
        type="checkbox"
        checked={text.snap_to_baseline}
        onchange={(e) => applyOnce({ snap_to_baseline: e.currentTarget.checked })}
      />
      Lock lines to the baseline grid
    </label>
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

  .stack {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .label {
    font-size: 0.7rem;
    color: #94a3b8;
    font-weight: 500;
  }

  textarea,
  input[type="text"],
  select {
    width: 100%;
    background: rgba(10, 15, 30, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    padding: 0.42rem 0.5rem;
    color: #f8fafc;
    font-size: 0.82rem;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s;
    resize: vertical;
  }

  textarea:focus,
  input[type="text"]:focus,
  select:focus {
    border-color: #38bdf8;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.55rem;
  }

  .segmented {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
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
    font-size: 0.72rem;
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

  .check {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.76rem;
    color: #cbd5e1;
    cursor: pointer;
  }
</style>

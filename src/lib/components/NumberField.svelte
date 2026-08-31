<script lang="ts">
  /**
   * A numeric field whose label is also a scrub handle.
   *
   * Reports the three phases of an edit rather than a single change event:
   * the parent snapshots on `onstart`, writes live on `oninput`, and records
   * one undo entry on `onend`. Typing into the box is the same shape — focus
   * opens the gesture, blur closes it — so a typed value and a dragged value
   * cost exactly one history entry each.
   */
  import { scrub } from "$lib/actions/scrub";

  interface Props {
    label: string;
    value: number;
    step?: number;
    min?: number;
    max?: number;
    /** Digits shown in the box; scrubbing often lands on long decimals. */
    precision?: number;
    suffix?: string;
    disabled?: boolean;
    onstart?: () => void;
    oninput?: (value: number) => void;
    onend?: () => void;
  }

  let {
    label,
    value,
    step = 1,
    min = -Infinity,
    max = Infinity,
    precision = 2,
    suffix = "",
    disabled = false,
    onstart,
    oninput,
    onend,
  }: Props = $props();

  /** Value when the current gesture began, so scrub deltas stay absolute. */
  let gestureBase = 0;
  /** Open only between a start and its matching end. */
  let inGesture = false;

  const clamp = (n: number) => Math.min(max, Math.max(min, n));

  /** Trims float noise without showing a trailing `.00` on round numbers. */
  const display = (n: number) => String(Number(n.toFixed(precision)));

  function beginScrub() {
    gestureBase = value;
    inGesture = true;
    onstart?.();
  }

  function onScrub(delta: number) {
    oninput?.(clamp(gestureBase + delta));
  }

  function endScrub() {
    inGesture = false;
    onend?.();
  }

  function onTyped(event: Event) {
    const raw = Number((event.currentTarget as HTMLInputElement).value);
    if (Number.isNaN(raw)) return;
    // Typing outside a scrub still needs a gesture around it, or the commit
    // on blur would have no snapshot to compare against.
    if (!inGesture) {
      inGesture = true;
      onstart?.();
    }
    oninput?.(clamp(raw));
  }

  function onBlur() {
    if (!inGesture) return;
    inGesture = false;
    onend?.();
  }
</script>

<div class="field" class:disabled>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <span
    class="label"
    class:scrubbable={!disabled}
    title="Drag sideways to change. Hold shift for finer steps."
    use:scrub={{
      step,
      enabled: !disabled,
      onstart: beginScrub,
      onscrub: onScrub,
      onend: endScrub,
    }}
  >
    {label}
  </span>
  <div class="box">
    <input
      type="number"
      {step}
      {disabled}
      value={display(value)}
      oninput={onTyped}
      onblur={onBlur}
      onkeydown={(e) => {
        if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
      }}
    />
    {#if suffix}<span class="suffix">{suffix}</span>{/if}
  </div>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .label {
    font-size: 0.7rem;
    color: #94a3b8;
    font-weight: 500;
    user-select: none;
    width: fit-content;
  }

  .scrubbable {
    cursor: ew-resize;
    border-bottom: 1px dotted rgba(148, 163, 184, 0.5);
  }

  .scrubbable:hover {
    color: #38bdf8;
    border-bottom-color: #38bdf8;
  }

  .box {
    display: flex;
    align-items: center;
    background: rgba(10, 15, 30, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    padding-right: 0.4rem;
    transition: border-color 0.15s;
  }

  .box:focus-within {
    border-color: #38bdf8;
  }

  input {
    width: 100%;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    color: #f8fafc;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
    padding: 0.42rem 0.5rem;
  }

  /* The spinners steal width that the numbers need. */
  input::-webkit-outer-spin-button,
  input::-webkit-inner-spin-button {
    appearance: none;
    margin: 0;
  }

  input[type="number"] {
    appearance: textfield;
    -moz-appearance: textfield;
  }

  .suffix {
    font-size: 0.68rem;
    color: #64748b;
    user-select: none;
  }

  .disabled {
    opacity: 0.45;
  }
</style>

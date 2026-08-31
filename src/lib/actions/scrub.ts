/**
 * Drag a label sideways to change its number.
 *
 * The interaction layout tools use for every numeric field: the label itself
 * is a horizontal slider, so a value can be dialled in without selecting text
 * or reaching for arrow keys.
 *
 * The action reports the gesture's phases separately rather than just emitting
 * values, because the caller needs to snapshot state when the drag begins and
 * record one undo entry when it ends.
 */

export interface ScrubOptions {
  /** Document units added per pixel of horizontal travel. */
  step?: number;
  /** Called once when the drag begins, before any value changes. */
  onstart?: () => void;
  /** Called on every movement, with the running total offset from the start. */
  onscrub?: (delta: number) => void;
  /** Called once when the drag ends. */
  onend?: () => void;
  /** When false the element behaves as a plain label. */
  enabled?: boolean;
}

export function scrub(node: HTMLElement, options: ScrubOptions = {}) {
  let opts = options;
  let startX = 0;
  let dragging = false;
  /** Distinguishes a click from a drag, so a stray click commits nothing. */
  let moved = false;

  function onPointerDown(event: PointerEvent) {
    if (opts.enabled === false || event.button !== 0) return;
    dragging = true;
    moved = false;
    startX = event.clientX;
    node.setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function onPointerMove(event: PointerEvent) {
    if (!dragging) return;
    const travel = event.clientX - startX;
    if (!moved) {
      // A pointer that has not really moved is a click; do not open a gesture
      // for it, or every click would leave an undo entry.
      if (Math.abs(travel) < 2) return;
      moved = true;
      opts.onstart?.();
    }
    // Fine control on shift, the convention these fields follow elsewhere.
    const step = (opts.step ?? 1) * (event.shiftKey ? 0.1 : 1);
    opts.onscrub?.(travel * step);
  }

  function endGesture(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    if (node.hasPointerCapture(event.pointerId)) {
      node.releasePointerCapture(event.pointerId);
    }
    if (moved) opts.onend?.();
    moved = false;
  }

  node.addEventListener("pointerdown", onPointerDown);
  node.addEventListener("pointermove", onPointerMove);
  node.addEventListener("pointerup", endGesture);
  node.addEventListener("pointercancel", endGesture);

  return {
    update(next: ScrubOptions) {
      opts = next;
    },
    destroy() {
      node.removeEventListener("pointerdown", onPointerDown);
      node.removeEventListener("pointermove", onPointerMove);
      node.removeEventListener("pointerup", endGesture);
      node.removeEventListener("pointercancel", endGesture);
    },
  };
}

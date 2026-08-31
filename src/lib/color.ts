/**
 * Conversions between the renderer's colour form and the browser's.
 *
 * Vello works in normalised floats; `<input type="color">` speaks `#rrggbb`.
 * Alpha is carried separately because the colour input has no alpha channel —
 * opacity is its own control in the inspector.
 */
import type { Rgba } from "./ipc";

const clamp01 = (n: number) => Math.min(1, Math.max(0, n));

/** `[0..1]` floats to `#rrggbb`, dropping alpha. */
export function rgbaToHex(rgba: Rgba): string {
  const part = (n: number) =>
    Math.round(clamp01(n) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${part(rgba[0])}${part(rgba[1])}${part(rgba[2])}`;
}

/** `#rrggbb` to `[0..1]` floats, keeping the alpha already in use. */
export function hexToRgba(hex: string, alpha = 1): Rgba {
  const clean = hex.replace("#", "");
  const value = parseInt(
    clean.length === 3
      ? clean
          .split("")
          .map((c) => c + c)
          .join("")
      : clean,
    16,
  );
  if (Number.isNaN(value)) return [0, 0, 0, alpha];
  return [((value >> 16) & 255) / 255, ((value >> 8) & 255) / 255, (value & 255) / 255, alpha];
}

import { clamp } from "../../lib/format";
import type { CropRect } from "../../lib/tauri/types";

/** Even, in-bounds, at least 64 px — matches the Rust validator. */
export function normalizeCrop(c: CropRect, vw: number, vh: number): CropRect {
  const width = clamp(Math.round(c.width / 2) * 2, 64, vw);
  const height = clamp(Math.round(c.height / 2) * 2, 64, vh);
  const x = clamp(Math.round(c.x / 2) * 2, 0, vw - width);
  const y = clamp(Math.round(c.y / 2) * 2, 0, vh - height);
  return { x, y, width, height };
}

/** Largest centered rect with aspect `ar` (w/h) inside the frame. */
export function aspectCrop(ar: number, vw: number, vh: number): CropRect {
  let w = vw;
  let h = Math.round(vw / ar);
  if (h > vh) {
    h = vh;
    w = Math.round(vh * ar);
  }
  return normalizeCrop({ x: (vw - w) / 2, y: (vh - h) / 2, width: w, height: h }, vw, vh);
}

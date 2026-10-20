import { useEffect, useRef } from "react";

/** Draws into a canvas sized to its CSS box; `css` resolves theme tokens. */
export type Paint = (
  g: CanvasRenderingContext2D,
  w: number,
  h: number,
  css: CSSStyleDeclaration,
) => void;

/**
 * Ref for a canvas kept painted at device resolution with `paint` (memoize it):
 * repaints when `paint` changes, the canvas is resized or Windows switches
 * light/dark.
 */
export function useCanvasPaint(paint: Paint) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = c.clientWidth;
      const h = c.clientHeight;
      c.width = Math.round(w * dpr);
      c.height = Math.round(h * dpr);
      const g = c.getContext("2d");
      if (!g || w <= 0 || h <= 0) return;
      g.scale(dpr, dpr);
      g.clearRect(0, 0, w, h);
      paint(g, w, h, getComputedStyle(c));
    };
    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(c);
    const scheme = window.matchMedia("(prefers-color-scheme: light)");
    scheme.addEventListener("change", draw);
    return () => {
      ro.disconnect();
      scheme.removeEventListener("change", draw);
    };
  }, [paint]);
  return canvas;
}

/** A theme token's value, or `fallback` while styles are not resolved. */
export function token(css: CSSStyleDeclaration, name: string, fallback: string): string {
  return css.getPropertyValue(name).trim() || fallback;
}

/**
 * Mirrored peak waveform, one bar per pixel column. `peaks` are linear 0..1 per
 * bucket across the whole clip, scaled by `gain`; columns that would exceed full
 * scale are drawn in `clipColor` (when given).
 */
export function paintWaveform(
  g: CanvasRenderingContext2D,
  w: number,
  h: number,
  peaks: number[],
  gain: number,
  color: string,
  clipColor?: string,
) {
  const n = peaks.length;
  if (n === 0) return;
  const mid = h / 2;
  const half = h / 2 - 3;
  const cols = Math.max(1, Math.floor(w));
  let fill = "";
  for (let x = 0; x < cols; x++) {
    // Loudest bucket under this column.
    const a = Math.floor((x / cols) * n);
    const b = Math.max(a + 1, Math.floor(((x + 1) / cols) * n));
    let peak = 0;
    for (let i = a; i < b && i < n; i++) peak = Math.max(peak, peaks[i] ?? 0);
    const v = peak * gain;
    // sqrt keeps quiet passages visible.
    const amp = Math.max(0.5, Math.sqrt(Math.min(1, v)) * half);
    const want = clipColor && v > 1 ? clipColor : color;
    if (want !== fill) {
      fill = want;
      g.fillStyle = fill;
    }
    g.fillRect(x, mid - amp, 1, amp * 2);
  }
}

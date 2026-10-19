import { useEffect, useRef } from "react";

import { clamp, formatTime } from "../../lib/format";

type Drag = "start" | "end" | "seek" | null;

/**
 * Timeline with waveform, keyframe ticks, draggable trim handles and a playhead.
 * Times snap to whole frames.
 */
export function Timeline({
  duration,
  fps,
  current,
  trimStart,
  trimEnd,
  keyframes,
  waveform,
  onSeek,
  onTrim,
  onTrimCommit,
}: {
  duration: number;
  fps: number;
  current: number;
  trimStart: number;
  trimEnd: number;
  keyframes: number[];
  waveform: number[] | null;
  onSeek: (t: number) => void;
  onTrim: (start: number, end: number) => void;
  onTrimCommit: () => void;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<Drag>(null);

  const snap = (t: number) => (fps > 0 ? Math.round(t * fps) / fps : t);
  const timeAt = (clientX: number) => {
    const r = wrap.current?.getBoundingClientRect();
    if (!r || r.width <= 0) return 0;
    return snap(clamp(((clientX - r.left) / r.width) * duration, 0, duration));
  };
  const pct = (t: number) => `${duration > 0 ? (t / duration) * 100 : 0}%`;

  // Waveform + keyframe ticks.
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
      if (!g) return;
      g.scale(dpr, dpr);
      g.clearRect(0, 0, w, h);
      // Theme colors (light/dark follows Windows).
      const css = getComputedStyle(c);
      if (waveform && waveform.length > 0) {
        const mid = h / 2;
        g.fillStyle = css.getPropertyValue("--muted").trim() || "rgba(170, 180, 200, 0.55)";
        const step = w / waveform.length;
        waveform.forEach((v, i) => {
          const a = Math.max(1, Math.sqrt(Math.min(1, v)) * (h / 2 - 6));
          g.fillRect(i * step, mid - a, Math.max(1, step - 0.5), a * 2);
        });
      }
      g.fillStyle = css.getPropertyValue("--info").trim() || "rgba(90, 169, 255, 0.9)";
      for (const k of keyframes) {
        const x = duration > 0 ? (k / duration) * w : 0;
        g.fillRect(Math.round(x) - 1, h - 9, 2, 9);
      }
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
  }, [waveform, keyframes, duration]);

  const onPointerDown = (e: React.PointerEvent, kind: Drag) => {
    e.preventDefault();
    e.stopPropagation();
    drag.current = kind;
    (e.target as Element).setPointerCapture(e.pointerId);
    if (kind === "seek") onSeek(timeAt(e.clientX));
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const kind = drag.current;
    if (!kind) return;
    const t = timeAt(e.clientX);
    const minLen = Math.max(0.1, fps > 0 ? 1 / fps : 0.05);
    if (kind === "seek") onSeek(t);
    else if (kind === "start") {
      const s = Math.min(t, trimEnd - minLen);
      onTrim(Math.max(0, s), trimEnd);
      onSeek(Math.max(0, s));
    } else {
      const en = Math.max(t, trimStart + minLen);
      onTrim(trimStart, Math.min(duration, en));
      onSeek(Math.min(duration, en));
    }
  };
  const onPointerUp = () => {
    if (drag.current === "start" || drag.current === "end") onTrimCommit();
    drag.current = null;
  };

  return (
    <div className="timeline">
      <div
        ref={wrap}
        className="tl-track"
        onPointerDown={(e) => {
          onPointerDown(e, "seek");
        }}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      >
        <canvas ref={canvas} className="tl-wave" />
        <div className="tl-dim" style={{ left: 0, width: pct(trimStart) }} />
        <div className="tl-dim" style={{ left: pct(trimEnd), right: 0 }} />
        <div
          className="tl-sel"
          style={{ left: pct(trimStart), width: `calc(${pct(trimEnd)} - ${pct(trimStart)})` }}
        />
        <div
          className="tl-handle start"
          style={{ left: pct(trimStart) }}
          title="Drag to set the start"
          onPointerDown={(e) => {
            onPointerDown(e, "start");
          }}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        />
        <div
          className="tl-handle end"
          style={{ left: pct(trimEnd) }}
          title="Drag to set the end"
          onPointerDown={(e) => {
            onPointerDown(e, "end");
          }}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        />
        <div className="tl-playhead" style={{ left: pct(current) }} />
      </div>
      <div className="tl-scale">
        <span>{formatTime(0, 0)}</span>
        <span>{formatTime(duration / 2, 1)}</span>
        <span>{formatTime(duration, 1)}</span>
      </div>
    </div>
  );
}

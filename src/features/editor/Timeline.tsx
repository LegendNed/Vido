import { useCallback, useRef } from "react";

import { Icon } from "../../components/Icon";
import { clamp, formatTime } from "../../lib/format";
import { AudioLane, type LaneView } from "./AudioLane";
import { token, useCanvasPaint, type Paint } from "./canvas";
import type { Lane } from "./model";

type Drag = "start" | "end" | "seek" | null;

/** Label spacing candidates for the ruler, in seconds. */
const STEPS = [0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600];
const MIN_LABEL_PX = 64;

/** "4.00 s", or "2:05.50" from a minute up. */
function formatLength(sec: number): string {
  return sec < 60 ? `${sec.toFixed(2)} s` : formatTime(sec);
}

/**
 * The editor's timeline: a time ruler, the video row (selection, trim handles,
 * keyframe ticks) and one lane per audio track, all on one time axis. Clicking or
 * dragging anywhere on the rows scrubs; the trim region and playhead span all
 * rows. Times snap to whole frames.
 */
export function Timeline({
  duration,
  fps,
  current,
  trimStart,
  trimEnd,
  keyframes,
  lanes,
  onSeek,
  onTrim,
  onTrimCommit,
  onLane,
  onLaneCommit,
}: {
  duration: number;
  fps: number;
  current: number;
  trimStart: number;
  trimEnd: number;
  keyframes: number[];
  lanes: LaneView[];
  onSeek: (t: number) => void;
  onTrim: (start: number, end: number) => void;
  onTrimCommit: () => void;
  onLane: (index: number, patch: Partial<Lane>, record: boolean) => void;
  onLaneCommit: () => void;
}) {
  // Every row's `.tl-track` spans the same x range; the ruler's maps pointer → time.
  const axis = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag>(null);

  const snap = (t: number) => (fps > 0 ? Math.round(t * fps) / fps : t);
  const timeAt = (clientX: number) => {
    const r = axis.current?.getBoundingClientRect();
    if (!r || r.width <= 0) return 0;
    return snap(clamp(((clientX - r.left) / r.width) * duration, 0, duration));
  };

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const el = e.target as HTMLElement;
    // Lane controls and scrollbars keep their own behavior.
    if (!el.closest(".tl-cell")) return;
    const edge = el.closest<HTMLElement>(".tl-handle")?.dataset.edge;
    const kind: Drag = edge === "start" || edge === "end" ? edge : "seek";
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = kind;
    if (kind === "seek") onSeek(timeAt(e.clientX));
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const kind = drag.current;
    if (!kind) return;
    const t = timeAt(e.clientX);
    const minLen = Math.max(0.1, fps > 0 ? 1 / fps : 0.05);
    if (kind === "seek") onSeek(t);
    else if (kind === "start") {
      const s = Math.max(0, Math.min(t, trimEnd - minLen));
      onTrim(s, trimEnd);
      onSeek(s);
    } else {
      const en = Math.min(duration, Math.max(t, trimStart + minLen));
      onTrim(trimStart, en);
      onSeek(en);
    }
  };
  const onPointerUp = () => {
    if (drag.current === "start" || drag.current === "end") onTrimCommit();
    drag.current = null;
  };

  const pct = (t: number) => `${duration > 0 ? (clamp(t, 0, duration) / duration) * 100 : 0}%`;
  const overlay = (
    <div className="tl-overlay" aria-hidden="true">
      <div className="tl-dim" style={{ left: 0, width: pct(trimStart) }} />
      <div className="tl-dim" style={{ left: pct(trimEnd), right: 0 }} />
      <div className="tl-edge" style={{ left: pct(trimStart) }} />
      <div className="tl-edge" style={{ left: pct(trimEnd) }} />
      <div className="tl-playhead" style={{ left: pct(current) }} />
    </div>
  );

  return (
    <div
      className="tl"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <div className="tl-pane">
        <div className="tl-body">
          <div className="tl-row ruler">
            <div className="tl-head" />
            <div className="tl-cell">
              <div ref={axis} className="tl-track">
                <Ruler duration={duration} />
              </div>
            </div>
          </div>
          <div className="tl-row video">
            <div className="tl-head video-head">
              <span className="lane-icon" aria-hidden="true">
                <Icon name="film" size={14} />
              </span>
              <div className="lane-meta" title="Selection: in – out and its length">
                <span className="lane-title video-title">
                  Video
                  <span className="tabular">{formatLength(trimEnd - trimStart)}</span>
                </span>
                <span className="lane-note ellipsis tabular">
                  {formatTime(trimStart)} – {formatTime(trimEnd)}
                </span>
              </div>
            </div>
            <div className="tl-cell">
              <div className="tl-track">
                <div
                  className="tl-sel"
                  style={{
                    left: pct(trimStart),
                    width: `calc(${pct(trimEnd)} - ${pct(trimStart)})`,
                  }}
                />
                <Keyframes duration={duration} keyframes={keyframes} />
                <div
                  className="tl-handle"
                  data-edge="start"
                  style={{ left: pct(trimStart) }}
                  title="Drag to set the start (I)"
                />
                <div
                  className="tl-handle"
                  data-edge="end"
                  style={{ left: pct(trimEnd) }}
                  title="Drag to set the end (O)"
                />
              </div>
            </div>
          </div>
          {overlay}
        </div>
      </div>
      {lanes.length > 0 && (
        <div className="tl-pane tl-lanes">
          <div className="tl-body">
            {lanes.map((l) => (
              <AudioLane key={l.index} view={l} onChange={onLane} onCommit={onLaneCommit} />
            ))}
            {overlay}
          </div>
        </div>
      )}
    </div>
  );
}

/** Time labels with major and minor ticks. */
function Ruler({ duration }: { duration: number }) {
  const paint = useCallback<Paint>(
    (g, w, h, css) => {
      if (duration <= 0) return;
      const pxPerSec = w / duration;
      const step = STEPS.find((s) => s * pxPerSec >= MIN_LABEL_PX) ?? 3600;
      const sub = step === 15 || step === 900 ? 3 : step >= 60 ? 4 : 5;
      g.fillStyle = token(css, "--faint", "rgba(255, 255, 255, 0.45)");
      for (let i = 0; (i * step) / sub <= duration + 1e-9; i++) {
        const x = Math.min(w - 1, Math.round(((i * step) / sub) * pxPerSec));
        const major = i % sub === 0;
        g.fillRect(x, major ? h - 7 : h - 4, 1, major ? 7 : 4);
      }
      g.fillStyle = token(css, "--text-muted", "rgba(255, 255, 255, 0.62)");
      g.font = `11px ${token(css, "--font-small", "system-ui")}`;
      g.textBaseline = "top";
      const digits = step < 1 ? 1 : 0;
      for (let i = 0; i * step <= duration + 1e-9; i++) {
        const t = i * step;
        const label = formatTime(t, digits);
        const x = Math.round(t * pxPerSec) + 4;
        // Keep the last label inside the ruler.
        if (x + g.measureText(label).width > w) break;
        g.fillText(label, x, 1);
      }
    },
    [duration],
  );
  const canvas = useCanvasPaint(paint);
  return <canvas ref={canvas} className="tl-canvas" />;
}

/** Keyframe ticks along the bottom of the video row (stream copy starts on these). */
function Keyframes({ duration, keyframes }: { duration: number; keyframes: number[] }) {
  const paint = useCallback<Paint>(
    (g, w, h, css) => {
      if (duration <= 0) return;
      g.fillStyle = token(css, "--info", "rgba(90, 169, 255, 0.9)");
      for (const k of keyframes) {
        const x = Math.round((k / duration) * w);
        g.fillRect(x - 1, h - 7, 2, 7);
      }
    },
    [duration, keyframes],
  );
  const canvas = useCanvasPaint(paint);
  return <canvas ref={canvas} className="tl-canvas" />;
}

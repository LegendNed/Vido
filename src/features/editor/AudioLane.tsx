import { memo, useCallback } from "react";

import { Icon } from "../../components/Icon";
import { Button, Slider } from "../../components/ui";
import { paintWaveform, token, useCanvasPaint, type Paint } from "./canvas";
import type { Lane } from "./model";

export interface LaneView {
  /** Audio track index in the clip. */
  index: number;
  title: string;
  lane: Lane;
  /** Peak waveform of the whole clip (null while loading). */
  wave: number[] | null;
  /**
   * Set for the recorded mix: it is built from the other lanes, so it has no
   * controls of its own; the text says how it relates to them.
   */
  derived?: string;
}

/**
 * One audio lane of the timeline: name, mute and volume on the left, the track's
 * waveform (scaled by its volume) on the right, on the timeline's time axis.
 */
export const AudioLane = memo(function AudioLane({
  view,
  onChange,
  onCommit,
}: {
  view: LaneView;
  /** `record` adds an undo step; otherwise the change is a preview (slider drag). */
  onChange: (index: number, patch: Partial<Lane>, record: boolean) => void;
  onCommit: () => void;
}) {
  const { index, title, lane, wave, derived } = view;
  const silent = lane.muted || lane.volume <= 0;
  const pct = Math.round(lane.volume * 100);

  const paint = useCallback<Paint>(
    (g, w, h, css) => {
      if (!wave) return;
      const color = token(css, "--muted", "rgba(170, 180, 200, 0.6)");
      // A derived mix is an estimate: never flag it as clipping.
      const clip = derived === undefined ? token(css, "--warn", "#fce100") : undefined;
      paintWaveform(g, w, h, wave, derived === undefined ? lane.volume : 1, color, clip);
    },
    [wave, lane.volume, derived],
  );
  const canvas = useCanvasPaint(paint);

  return (
    <div className={`tl-row lane${silent ? " silent" : ""}${derived ? " derived" : ""}`}>
      <div className="tl-head lane-head">
        {derived !== undefined ? (
          <span className="lane-icon" aria-hidden="true">
            <Icon name="sliders" size={14} />
          </span>
        ) : (
          <Button
            small
            variant={lane.muted ? "danger" : "ghost"}
            icon={lane.muted ? "mute" : "speaker"}
            title={lane.muted ? `Unmute ${title}` : `Mute ${title}`}
            onClick={() => {
              onChange(index, { muted: !lane.muted }, true);
            }}
          />
        )}
        <div className="lane-meta">
          <span className="lane-title ellipsis" title={title}>
            {title}
          </span>
          {derived !== undefined ? (
            <span className="lane-note ellipsis" title={derived}>
              {derived}
            </span>
          ) : (
            <div className="lane-volume">
              <Slider
                value={pct}
                min={0}
                max={200}
                onChange={(v) => {
                  onChange(index, { volume: v / 100 }, false);
                }}
                onCommit={onCommit}
              />
              <span
                className="lane-pct"
                title="Volume (double-click for 100 %)"
                onDoubleClick={() => {
                  onChange(index, { volume: 1 }, true);
                }}
              >
                {pct}%
              </span>
            </div>
          )}
        </div>
      </div>
      <div className="tl-cell">
        <div className="tl-track">
          <canvas ref={canvas} className="tl-canvas" />
          {!wave && <span className="lane-loading">Loading waveform…</span>}
        </div>
      </div>
    </div>
  );
});

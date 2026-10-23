import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { Banner, Button, Segmented, Select } from "../../components/controls";
import { formatBitrate, formatBytes, formatDuration, formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import {
  errorMessage,
  isAppError,
  type AppError,
  type ExportEstimate,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import {
  buildRequest,
  quickFormat,
  type EditState,
  type ExportOpts,
  type QuickFormat,
  type Vertical,
} from "../editor/model";
import { EncodeOptions } from "./EncodeOptions";

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

const FORMATS: { value: QuickFormat; label: string }[] = [
  { value: "original", label: "Original" },
  { value: "phone", label: "Phone (9:16)" },
  { value: "gif", label: "GIF" },
];
const PHONE: { value: Exclude<Vertical, "off">; label: string }[] = [
  { value: "crop", label: "Crop" },
  { value: "fit", label: "Fit" },
];
const SPEEDS = [0.25, 0.5, 1, 1.5, 2, 4].map((v) => ({ value: v, label: `${v}×` }));
const FADES = [0, 0.5, 1, 2].map((v) => ({ value: v, label: v === 0 ? "None" : `${v} s` }));

/**
 * The editor's export pane: preset chips, every encode option under "Advanced"
 * (shared with the settings pages via `EncodeOptions`) and where Save writes. The
 * estimate and `actions` (Save / Save & override) stay pinned at the bottom.
 */
export function ExportPanel({
  path,
  state,
  setOpts,
  onVertical,
  onFormat,
  onTiming,
  actions,
}: {
  path: string;
  state: EditState;
  setOpts: (fn: (x: ExportOpts) => ExportOpts) => void;
  onVertical: (v: Vertical) => void;
  onFormat: (f: QuickFormat) => void;
  onTiming: (patch: Partial<Pick<EditState, "speed" | "fadeIn" | "fadeOut">>) => void;
  actions: ReactNode;
}) {
  const [estimate, setEstimate] = useState<ExportEstimate | null>(null);
  const [problem, setProblem] = useState<AppError | null>(null);
  const x = state.x;

  // A custom output path has the old extension: back to the default name when
  // the container changes (also through a preset).
  const update = useCallback(
    (fn: (o: ExportOpts) => ExportOpts) => {
      setOpts((o) => {
        const n = fn(o);
        return n.container !== o.container && n.outputPath ? { ...n, outputPath: null } : n;
      });
    },
    [setOpts],
  );

  // Debounced estimate (Rust validates the whole request).
  const reqKey = JSON.stringify(buildRequest(path, state));
  const seq = useRef(0);
  useEffect(() => {
    const my = ++seq.current;
    const t = setTimeout(() => {
      commands.estimateExport(buildRequest(path, state)).then(
        (e) => {
          if (my === seq.current) {
            setEstimate(e);
            setProblem(null);
          }
        },
        (e: unknown) => {
          if (my === seq.current) {
            setEstimate(null);
            setProblem(isAppError(e) ? e : { code: "INTERNAL", message: errorMessage(e) });
          }
        },
      );
    }, 250);
    return () => {
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the serialized request
  }, [reqKey]);

  const stem = path.replace(/\.[^.\\/]+$/, "");
  const ext = state.gif ? "gif" : x.container;
  const format = quickFormat(state);
  const browse = async () => {
    const p = await commands.pickExportPath(`${stem}_edit.${ext}`, x.container, state.gif);
    if (p) setOpts((o) => ({ ...o, outputPath: p }));
  };

  const willCopy = estimate?.willCopyVideo ?? false;

  const notify = useApp((s) => s.notify);
  const [exportingAudio, setExportingAudio] = useState(false);
  const exportAudio = async () => {
    setExportingAudio(true);
    try {
      await commands.exportAudioTracks(path, true);
      notify("info", "Audio tracks exported", "One WAV file per track.");
    } catch (e) {
      notify("error", "Couldn't export the audio", errorMessage(e));
    } finally {
      setExportingAudio(false);
    }
  };

  return (
    <div className="export-panel">
      <div className="export-scroll">
        <div className="side-title">Export</div>
        <div className="export-shape">
          <span className="muted small">Quick formats</span>
          <Segmented<QuickFormat> value={format} options={FORMATS} onChange={onFormat} />
          {format === "phone" && (
            <Segmented<Exclude<Vertical, "off">>
              value={state.vertical === "fit" ? "fit" : "crop"}
              options={PHONE}
              onChange={onVertical}
            />
          )}
          <span className="muted small">
            {format === "gif"
              ? "Animated GIF: 15 fps, up to 480 px, no sound, at most 30 s. Crop and blur apply."
              : state.vertical === "crop"
                ? "For Shorts, TikTok and Reels: drag the 9:16 window over the part to keep."
                : state.vertical === "fit"
                  ? "For Shorts, TikTok and Reels: the whole frame on a blurred 9:16 background."
                  : "As recorded."}
          </span>
        </div>
        <div className="export-timing">
          <label className="export-timing-field">
            <span className="muted small">Speed</span>
            <Select<number>
              value={state.speed}
              options={SPEEDS}
              onChange={(speed) => {
                onTiming({ speed });
              }}
            />
          </label>
          <label className="export-timing-field">
            <span className="muted small">Fade in</span>
            <Select<number>
              value={state.fadeIn}
              options={FADES}
              onChange={(fadeIn) => {
                onTiming({ fadeIn });
              }}
            />
          </label>
          <label className="export-timing-field">
            <span className="muted small">Fade out</span>
            <Select<number>
              value={state.fadeOut}
              options={FADES}
              onChange={(fadeOut) => {
                onTiming({ fadeOut });
              }}
            />
          </label>
        </div>
        {!state.gif && (
          <EncodeOptions
            value={x}
            onChange={update}
            scope="editor"
            copy={{
              available: estimate?.canStreamCopy ?? false,
              blocker: estimate?.streamCopyBlocker ?? null,
              active: willCopy,
            }}
          />
        )}

        {problem && (
          <Banner tone="error">
            {problem.message}
            {problem.field ? ` (${problem.field})` : ""}
          </Banner>
        )}
        {estimate?.warnings
          .filter((w) => !w.startsWith("Without re-encoding"))
          .map((w) => (
            <Banner key={w}>{w}</Banner>
          ))}

        <div className="export-dest">
          <div className="export-dest-text">
            <span className="muted small">Save writes</span>
            <span className="ellipsis" title={x.outputPath ?? undefined}>
              {x.outputPath ? fileName(x.outputPath) : `${fileName(stem)}_edit.${ext}`}
            </span>
            <span className="muted small ellipsis" title={x.outputPath ?? undefined}>
              {x.outputPath
                ? x.outputPath.slice(0, -fileName(x.outputPath).length)
                : "Next to the clip"}
            </span>
          </div>
          {x.outputPath && (
            <Button
              small
              variant="ghost"
              icon="x"
              title="Save next to the clip"
              onClick={() => {
                setOpts((o) => ({ ...o, outputPath: null }));
              }}
            />
          )}
          <Button small icon="folder" onClick={() => void browse()}>
            Save as…
          </Button>
        </div>
        {state.lanes.length > 0 && !state.gif && (
          <Button
            small
            variant="ghost"
            icon="audio"
            className="self-start"
            disabled={exportingAudio}
            title="Every audio track as its own WAV file, in a folder next to the clip"
            onClick={() => void exportAudio()}
          >
            {exportingAudio ? "Exporting audio…" : "Export audio tracks (WAV)"}
          </Button>
        )}
      </div>

      <div className="export-footer">
        {estimate && (
          <div className="estimate">
            <div>
              <div className="big">
                {estimate.estimateIsRough ? "~" : ""}
                {formatBytes(estimate.estimatedBytes)}
              </div>
              <div className="muted small">estimated size</div>
            </div>
            <div className="small col" style={{ gap: 2 }}>
              <span>
                {formatDuration(estimate.outputDuration)} · {estimate.outputWidth}×
                {estimate.outputHeight}
              </span>
              <span className="muted">
                {willCopy
                  ? "Video copied (no re-encode)"
                  : `Video ${formatBitrate(estimate.videoBitrateKbps)}`}{" "}
                · audio {formatBitrate(estimate.audioBitrateKbps)}
              </span>
              {willCopy && Math.abs(estimate.copyStart - state.trimStart) > 0.01 && (
                <span className="muted">Starts at keyframe {formatTime(estimate.copyStart)}</span>
              )}
            </div>
          </div>
        )}
        {actions}
      </div>
    </div>
  );
}

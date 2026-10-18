import { useEffect, useRef, useState } from "react";

import {
  Banner,
  Button,
  Field,
  NumberInput,
  Progress,
  Segmented,
  Select,
  Toggle,
} from "../../components/ui";
import {
  formatBitrate,
  formatBytes,
  formatDuration,
  formatTime,
  parseSizeMb,
} from "../../lib/format";
import {
  AUDIO_CODEC_OPTIONS,
  codecOptions,
  CONTAINER_OPTIONS,
  encoderOptions,
  FPS_OPTIONS,
  PRESET_OPTIONS,
  RATE_MODE_OPTIONS,
  SCALE_OPTIONS,
} from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import {
  errorMessage,
  isAppError,
  type AppError,
  type ExportEstimate,
  type ExportProgress,
  type ExportResult,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { buildRequest, PRESETS, type EditState, type ExportOpts } from "../editor/model";

type Job =
  | { kind: "idle" }
  | { kind: "running"; id: string; progress: ExportProgress | null }
  | { kind: "done"; result: ExportResult }
  | { kind: "failed"; error: string };

export function ExportPanel({
  path,
  state,
  setOpts,
}: {
  path: string;
  state: EditState;
  setOpts: (fn: (x: ExportOpts) => ExportOpts) => void;
}) {
  const caps = useApp((s) => s.caps);
  const notify = useApp((s) => s.notify);
  const [estimate, setEstimate] = useState<ExportEstimate | null>(null);
  const [problem, setProblem] = useState<AppError | null>(null);
  const [job, setJob] = useState<Job>({ kind: "idle" });
  // Only set while the user is typing a size; otherwise the stored value is shown.
  const [sizeDraft, setSizeDraft] = useState<string | null>(null);
  const sizeText = sizeDraft ?? `${state.x.targetSizeMb} MB`;
  const jobId = job.kind === "running" ? job.id : null;
  const x = state.x;

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

  useEvent("export-progress", (p) => {
    if (p.jobId === jobId) setJob({ kind: "running", id: p.jobId, progress: p });
  });
  useEvent("export-completed", (r) => {
    if (r.jobId === jobId) setJob({ kind: "done", result: r });
  });
  useEvent("export-failed", (f) => {
    if (f.jobId === jobId)
      setJob(
        f.error.code === "CANCELLED"
          ? { kind: "idle" }
          : { kind: "failed", error: errorMessage(f.error) },
      );
  });

  const start = async () => {
    try {
      const id = await commands.exportClip(buildRequest(path, state));
      setJob({ kind: "running", id, progress: null });
    } catch (e) {
      setJob({ kind: "failed", error: errorMessage(e) });
    }
  };

  const browse = async () => {
    const stem = path.replace(/\.[^.\\/]+$/, "");
    const p = await commands.pickExportPath(`${stem}_export.${x.container}`, x.container);
    if (p) setOpts((o) => ({ ...o, outputPath: p }));
  };

  const willCopy = estimate?.willCopyVideo ?? false;
  const encodeControlsDisabled = willCopy;

  return (
    <div className="col" style={{ gap: 10 }}>
      <div className="preset-row">
        {PRESETS.map((p) => (
          <button
            type="button"
            key={p.id}
            className="preset"
            title={p.hint}
            onClick={() => {
              setOpts(p.apply);
            }}
          >
            {p.label}
          </button>
        ))}
      </div>

      <div className="card" style={{ padding: "0 12px" }}>
        <Field
          label="Re-encode"
          description={
            estimate?.canStreamCopy
              ? "Off = lossless & instant"
              : (estimate?.streamCopyBlocker ?? undefined)
          }
        >
          <Toggle
            value={!x.streamCopy || !(estimate?.canStreamCopy ?? true)}
            disabled={!(estimate?.canStreamCopy ?? false)}
            onChange={(v) => {
              setOpts((o) => ({ ...o, streamCopy: !v }));
            }}
          />
        </Field>
        <Field label="Container">
          <Select
            value={x.container}
            options={CONTAINER_OPTIONS}
            onChange={(v) => {
              setOpts((o) => ({
                ...o,
                container: v,
                ...(v === "webm"
                  ? { videoCodec: "av1" as const, audioCodec: "opus" as const }
                  : {}),
                outputPath: null,
              }));
            }}
          />
        </Field>
        <Field label="Codec">
          <Select
            value={x.videoCodec}
            disabled={encodeControlsDisabled}
            options={codecOptions(caps)}
            onChange={(v) => {
              setOpts((o) => ({ ...o, videoCodec: v }));
            }}
          />
        </Field>
        <Field label="Encoder">
          <Select
            value={x.encoder}
            disabled={encodeControlsDisabled}
            options={encoderOptions(caps, x.videoCodec)}
            onChange={(v) => {
              setOpts((o) => ({ ...o, encoder: v }));
            }}
          />
        </Field>
        <Field label="Mode">
          <Segmented
            value={x.rateMode}
            options={RATE_MODE_OPTIONS}
            onChange={(v) => {
              setOpts((o) => ({
                ...o,
                rateMode: v,
                ...(v === "targetSize" ? { streamCopy: false } : {}),
              }));
            }}
          />
        </Field>
        {x.rateMode === "quality" && (
          <Field label="Quality" description="Lower = better (18–28 typical)">
            <NumberInput
              value={x.quality}
              min={0}
              max={51}
              onChange={(v) => {
                setOpts((o) => ({ ...o, quality: v }));
              }}
            />
          </Field>
        )}
        {x.rateMode === "bitrate" && (
          <Field label="Bitrate">
            <NumberInput
              value={x.bitrateKbps}
              min={100}
              max={200000}
              step={100}
              suffix="kbps"
              onChange={(v) => {
                setOpts((o) => ({ ...o, bitrateKbps: v }));
              }}
            />
          </Field>
        )}
        {x.rateMode === "targetSize" && (
          <Field label="Target size" description='e.g. "8 MB", "25", "1.5 GB"'>
            <input
              className={`input${parseSizeMb(sizeText) === null ? " invalid" : ""}`}
              style={{ width: 110 }}
              value={sizeText}
              onChange={(e) => {
                setSizeDraft(e.target.value);
              }}
              onBlur={() => {
                const mb = parseSizeMb(sizeText);
                if (mb !== null) {
                  setOpts((o) => ({ ...o, targetSizeMb: Math.min(10000, Math.max(1, mb)) }));
                  setSizeDraft(null);
                }
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              }}
            />
          </Field>
        )}
        <Field label="Speed / quality">
          <Select
            value={x.preset}
            disabled={encodeControlsDisabled}
            options={PRESET_OPTIONS}
            onChange={(v) => {
              setOpts((o) => ({ ...o, preset: v }));
            }}
          />
        </Field>
        <Field label="Resolution">
          <Select
            value={x.scale}
            options={SCALE_OPTIONS}
            onChange={(v) => {
              setOpts((o) => ({ ...o, scale: v }));
            }}
          />
        </Field>
        <Field label="Frame rate">
          <Select
            value={x.fps}
            options={FPS_OPTIONS.map((o) =>
              o.value === 0 ? { ...o, label: "Same as source" } : o,
            )}
            onChange={(v) => {
              setOpts((o) => ({ ...o, fps: v }));
            }}
          />
        </Field>
        <Field label="Audio">
          <Select
            value={x.audioCodec}
            options={AUDIO_CODEC_OPTIONS}
            onChange={(v) => {
              setOpts((o) => ({ ...o, audioCodec: v }));
            }}
          />
          <Select
            value={x.audioBitrateKbps}
            options={[64, 96, 128, 160, 192, 256, 320].map((k) => ({ value: k, label: `${k}k` }))}
            onChange={(v) => {
              setOpts((o) => ({ ...o, audioBitrateKbps: v }));
            }}
          />
        </Field>
        <Field
          label="Save as"
          description={
            x.outputPath ? (
              <span className="mono ellipsis">{x.outputPath}</span>
            ) : (
              "Next to the clip (…_export)"
            )
          }
        >
          {x.outputPath && (
            <Button
              small
              variant="ghost"
              icon="x"
              title="Use default location"
              onClick={() => {
                setOpts((o) => ({ ...o, outputPath: null }));
              }}
            />
          )}
          <Button small icon="folder" onClick={() => void browse()}>
            Browse…
          </Button>
        </Field>
      </div>

      {problem && (
        <Banner tone="error">
          {problem.message}
          {problem.field ? ` (${problem.field})` : ""}
        </Banner>
      )}
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
      {estimate?.warnings
        .filter((w) => !w.startsWith("Without re-encoding"))
        .map((w) => (
          <Banner key={w}>{w}</Banner>
        ))}

      {job.kind === "running" && (
        <div className="col" style={{ gap: 6 }}>
          <Progress value={job.progress?.percent ?? 0} />
          <div className="row small muted">
            <span>
              {job.progress?.stage === "retrying" ? "Retrying at a lower bitrate… " : "Exporting… "}
              {Math.round(job.progress?.percent ?? 0)}%
            </span>
            {job.progress && job.progress.fps > 0 && (
              <span>· {Math.round(job.progress.fps)} fps</span>
            )}
            {job.progress && job.progress.etaSeconds > 0 && (
              <span>· {Math.ceil(job.progress.etaSeconds)} s left</span>
            )}
            <span className="spacer" />
            <Button small onClick={() => void commands.cancelExport(job.id)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
      {job.kind === "done" && (
        <Banner tone="info">
          <div>
            Exported {formatBytes(job.result.sizeBytes)}
            {job.result.videoCopied
              ? " without re-encoding"
              : job.result.encoder
                ? ` with ${job.result.encoder}`
                : ""}
            {job.result.attempts > 1 ? ` (${job.result.attempts} passes)` : ""}.
            {job.result.overTarget &&
              " The target size could not be reached — try a lower resolution."}
          </div>
          <div className="row" style={{ marginTop: 6 }}>
            <Button
              small
              icon="folder"
              onClick={() => void commands.revealClip(job.result.outputPath)}
            >
              Show
            </Button>
            <Button
              small
              icon="external"
              onClick={() => void commands.openClipExternal(job.result.outputPath)}
            >
              Play
            </Button>
            <Button
              small
              icon="copy"
              onClick={() => {
                void commands.copyClipPath(job.result.outputPath).then(() => {
                  notify("info", "Path copied");
                });
              }}
            >
              Copy path
            </Button>
          </div>
        </Banner>
      )}
      {job.kind === "failed" && <Banner tone="error">Export failed: {job.error}</Banner>}

      <Button
        variant="primary"
        icon="download"
        disabled={job.kind === "running" || problem !== null}
        onClick={() => void start()}
      >
        {willCopy ? "Export (fast)" : "Export"}
      </Button>
    </div>
  );
}

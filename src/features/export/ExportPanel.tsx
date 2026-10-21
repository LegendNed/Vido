import { useCallback, useEffect, useRef, useState } from "react";

import { Banner, Button, Progress } from "../../components/ui";
import { formatBitrate, formatBytes, formatDuration, formatTime } from "../../lib/format";
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
import { buildRequest, type EditState, type ExportOpts } from "../editor/model";
import { EncodeOptions } from "./EncodeOptions";

type Job =
  | { kind: "idle" }
  | { kind: "running"; id: string; progress: ExportProgress | null }
  | { kind: "done"; result: ExportResult }
  | { kind: "failed"; error: string };

type Outcome = Extract<Job, { kind: "idle" | "done" | "failed" }>;

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/**
 * Editor export: preset chips and the estimate up front, every encode option
 * under "Advanced" (shared with the settings pages via `EncodeOptions`).
 */
export function ExportPanel({
  path,
  state,
  setOpts,
}: {
  path: string;
  state: EditState;
  setOpts: (fn: (x: ExportOpts) => ExportOpts) => void;
}) {
  const notify = useApp((s) => s.notify);
  const [estimate, setEstimate] = useState<ExportEstimate | null>(null);
  const [problem, setProblem] = useState<AppError | null>(null);
  const [job, setJob] = useState<Job>({ kind: "idle" });
  // The running job. Its id arrives with the command's reply and a quick stream
  // copy may finish before that, so outcomes of unknown jobs are kept briefly.
  const active = useRef<string | null>(null);
  const early = useRef(new Map<string, Outcome>());
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

  const settle = (id: string, o: Outcome) => {
    if (id === active.current) {
      active.current = null;
      setJob(o);
      return;
    }
    early.current.set(id, o);
    // Only the latest few matter (other ids belong to Save or re-encode-on-save).
    for (const k of [...early.current.keys()].slice(0, -8)) early.current.delete(k);
  };
  useEvent("export-progress", (p) => {
    if (p.jobId === active.current) setJob({ kind: "running", id: p.jobId, progress: p });
  });
  useEvent("export-completed", (r) => {
    settle(r.jobId, { kind: "done", result: r });
  });
  useEvent("export-failed", (f) => {
    settle(
      f.jobId,
      f.error.code === "CANCELLED"
        ? { kind: "idle" }
        : { kind: "failed", error: errorMessage(f.error) },
    );
  });

  const start = async () => {
    let id: string;
    try {
      id = await commands.exportClip(buildRequest(path, state));
    } catch (e) {
      setJob({ kind: "failed", error: errorMessage(e) });
      return;
    }
    const done = early.current.get(id);
    if (done) {
      early.current.delete(id);
      setJob(done);
      return;
    }
    active.current = id;
    setJob({ kind: "running", id, progress: null });
  };

  const stem = path.replace(/\.[^.\\/]+$/, "");
  const browse = async () => {
    const p = await commands.pickExportPath(`${stem}_export.${x.container}`, x.container);
    if (p) setOpts((o) => ({ ...o, outputPath: p }));
  };

  const willCopy = estimate?.willCopyVideo ?? false;

  return (
    <div className="col export-panel" style={{ gap: 10 }}>
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

      <div className="export-dest">
        <div className="export-dest-text">
          <span className="muted small">Saves as</span>
          <span className="ellipsis" title={x.outputPath ?? undefined}>
            {x.outputPath ? fileName(x.outputPath) : `${fileName(stem)}_export.${x.container}`}
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
              " The target size could not be reached - try a lower resolution."}
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

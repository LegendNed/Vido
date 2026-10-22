import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { Banner, Button } from "../../components/controls";
import { formatBitrate, formatBytes, formatDuration, formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import {
  errorMessage,
  isAppError,
  type AppError,
  type ExportEstimate,
} from "../../lib/tauri/types";
import { buildRequest, type EditState, type ExportOpts } from "../editor/model";
import { EncodeOptions } from "./EncodeOptions";

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/**
 * The editor's export pane: preset chips, every encode option under "Advanced"
 * (shared with the settings pages via `EncodeOptions`) and where Save writes. The
 * estimate and `actions` (Save / Save & override) stay pinned at the bottom.
 */
export function ExportPanel({
  path,
  state,
  setOpts,
  actions,
}: {
  path: string;
  state: EditState;
  setOpts: (fn: (x: ExportOpts) => ExportOpts) => void;
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
  const browse = async () => {
    const p = await commands.pickExportPath(`${stem}_edit.${x.container}`, x.container);
    if (p) setOpts((o) => ({ ...o, outputPath: p }));
  };

  const willCopy = estimate?.willCopyVideo ?? false;

  return (
    <div className="export-panel">
      <div className="export-scroll">
        <div className="side-title">Export</div>
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
        {estimate?.warnings
          .filter((w) => !w.startsWith("Without re-encoding"))
          .map((w) => (
            <Banner key={w}>{w}</Banner>
          ))}

        <div className="export-dest">
          <div className="export-dest-text">
            <span className="muted small">Save writes</span>
            <span className="ellipsis" title={x.outputPath ?? undefined}>
              {x.outputPath ? fileName(x.outputPath) : `${fileName(stem)}_edit.${x.container}`}
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

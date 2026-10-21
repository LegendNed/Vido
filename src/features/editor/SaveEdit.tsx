import { useRef, useState } from "react";

import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Button, Progress } from "../../components/controls";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import {
  errorMessage,
  type AppError,
  type ExportProgress,
  type ExportResult,
  type SaveEditMode,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { buildSaveRequest, type EditState } from "./model";

interface SaveJob {
  id: string;
  mode: SaveEditMode;
  progress: ExportProgress | null;
}

type Outcome = { ok: true; result: ExportResult } | { ok: false; error: AppError };

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/**
 * Editor header actions: "Save" writes `<name>_edit.<ext>` next to the clip,
 * "Save & override" replaces the clip (the original goes to the Recycle Bin).
 * Both run as export jobs; progress and cancel are shown in place of the buttons.
 */
export function SaveActions({
  path,
  state,
  dirty,
  release,
  restore,
  onReplaced,
}: {
  path: string;
  state: EditState;
  /** Whether there is anything to save. */
  dirty: boolean;
  /** Lets go of the clip file (the preview may hold it) before it is replaced. */
  release: () => void;
  /** Reloads the preview after an override did not happen. */
  restore: () => void;
  /** The clip at `path` was replaced: reload it. */
  onReplaced: () => void;
}) {
  const notify = useApp((s) => s.notify);
  const [job, setJob] = useState<SaveJob | null>(null);
  const [confirming, setConfirming] = useState(false);
  // The job id arrives with the command's reply; a quick stream copy may finish
  // before that, so outcomes for unknown jobs are kept briefly.
  const active = useRef<{ id: string; mode: SaveEditMode } | null>(null);
  const early = useRef(new Map<string, Outcome>());

  const finish = (mode: SaveEditMode, o: Outcome) => {
    active.current = null;
    setJob(null);
    if (o.ok) {
      if (mode === "override") {
        notify("info", `Replaced ${fileName(path)}`, "The original is in the Recycle Bin.");
        onReplaced();
      } else {
        const out = o.result.outputPath;
        notify(
          "info",
          `Saved ${fileName(out)}`,
          o.result.videoCopied ? "Saved without re-encoding." : undefined,
          [
            { label: "Show in folder", run: () => void commands.revealClip(out) },
            { label: "Play", run: () => void commands.openClipExternal(out) },
          ],
        );
      }
      return;
    }
    if (mode === "override") restore();
    if (o.error.code !== "CANCELLED")
      notify(
        "error",
        mode === "override" ? "Save & override failed" : "Save failed",
        errorMessage(o.error),
      );
  };

  const settle = (jobId: string, o: Outcome) => {
    const a = active.current;
    if (a?.id === jobId) {
      finish(a.mode, o);
      return;
    }
    early.current.set(jobId, o);
    // Only the latest few matter (other ids belong to the Export panel).
    for (const k of [...early.current.keys()].slice(0, -8)) early.current.delete(k);
  };

  useEvent("export-progress", (p) => {
    const a = active.current;
    if (a?.id === p.jobId) setJob({ id: a.id, mode: a.mode, progress: p });
  });
  useEvent("export-completed", (r) => {
    settle(r.jobId, { ok: true, result: r });
  });
  useEvent("export-failed", (f) => {
    settle(f.jobId, { ok: false, error: f.error });
  });

  const start = async (mode: SaveEditMode) => {
    setConfirming(false);
    // WebView2 may keep the file open while the preview has it loaded.
    if (mode === "override") release();
    let id: string;
    try {
      id = await commands.saveEdit(buildSaveRequest(path, state), mode);
    } catch (e) {
      if (mode === "override") restore();
      notify(
        "error",
        mode === "override" ? "Save & override failed" : "Save failed",
        errorMessage(e),
      );
      return;
    }
    const done = early.current.get(id);
    if (done) {
      early.current.delete(id);
      finish(mode, done);
      return;
    }
    active.current = { id, mode };
    setJob({ id, mode, progress: null });
  };

  if (job) {
    const pct = Math.round(job.progress?.percent ?? 0);
    const finalizing = job.progress?.stage === "finalizing";
    return (
      <div className="save-progress" role="status">
        <span className="small muted">
          {finalizing ? "Replacing original…" : `Saving… ${pct}%`}
        </span>
        <Progress value={pct} />
        {!finalizing && (
          <Button small onClick={() => void commands.cancelExport(job.id)}>
            Cancel
          </Button>
        )}
      </div>
    );
  }

  const why = dirty ? undefined : "No changes to save";
  return (
    <>
      <Button
        small
        variant="primary"
        icon="save"
        disabled={!dirty}
        title={why ?? "Save the edit as a new clip next to this one (…_edit)"}
        onClick={() => void start("new")}
      >
        Save
      </Button>
      <Button
        small
        icon="refresh"
        disabled={!dirty}
        title={why ?? "Replace this clip with the edit (the original goes to the Recycle Bin)"}
        onClick={() => {
          setConfirming(true);
        }}
      >
        Save &amp; override
      </Button>
      {confirming && (
        <ConfirmDialog
          title={`Replace ${fileName(path)}?`}
          confirmLabel="Replace"
          onConfirm={() => void start("override")}
          onCancel={() => {
            setConfirming(false);
          }}
        >
          The original will be moved to the Recycle Bin.
        </ConfirmDialog>
      )}
    </>
  );
}

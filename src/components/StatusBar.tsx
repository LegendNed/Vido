import { useState } from "react";

import { formatBitrate, formatBytes, formatTime } from "../lib/format";
import { commands } from "../lib/tauri/commands";
import { errorMessage, type BufferStatus } from "../lib/tauri/types";
import { useApp } from "../stores/app";
import { Button } from "./ui";

const LABELS: Record<BufferStatus["state"], string> = {
  stopped: "Stopped",
  starting: "Starting",
  buffering: "Buffering",
  paused: "Paused",
  waiting: "Waiting for source",
  error: "Error",
};

export function StatusBar() {
  const status = useApp((s) => s.status);
  const notify = useApp((s) => s.notify);
  const [busy, setBusy] = useState(false);
  if (!status) return <div className="statusbar" />;

  const run = async (fn: () => Promise<unknown>, okMessage?: string) => {
    setBusy(true);
    try {
      await fn();
      if (okMessage) notify("info", okMessage);
    } catch (e) {
      notify("error", "Action failed", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const live = status.state === "buffering";
  const dotClass = live ? "live" : status.state === "stopped" ? "" : "warn";
  const canSave =
    status.state === "buffering" || status.state === "paused" || status.bufferedSeconds > 0;

  return (
    <div className="statusbar">
      <span className="status-pill">
        <span className={`dot ${dotClass}`} />
        {status.recording
          ? `REC ${formatTime(status.recording.elapsedSeconds, 0)}`
          : LABELS[status.state]}
      </span>
      <div className="status-meta">
        {status.source && (
          <span className="ellipsis" style={{ maxWidth: 220 }}>
            {status.source}
          </span>
        )}
        {status.width > 0 && (
          <span>
            {status.width}×{status.height} @ {Math.round(status.fps)}
          </span>
        )}
        {status.encoder && (
          <span title={status.zeroCopy ? "GPU zero-copy path" : "CPU copy path"}>
            {status.encoder}
            {status.zeroCopy ? " ⚡" : ""}
          </span>
        )}
        {status.videoBitrateKbps > 0 && <span>{formatBitrate(status.videoBitrateKbps)}</span>}
        {status.state !== "stopped" && (
          <span>
            {Math.floor(status.bufferedSeconds)}/{status.targetSeconds} s ·{" "}
            {formatBytes(status.sizeBytes)} in {status.storage}
          </span>
        )}
        {status.droppedFrames > 0 && (
          <span style={{ color: "var(--warn)" }}>{status.droppedFrames} dropped</span>
        )}
      </div>
      <div className="grow" />
      {status.state === "stopped" || status.state === "error" ? (
        <Button icon="record" disabled={busy} onClick={() => void run(commands.startBuffer)}>
          Start buffer
        </Button>
      ) : (
        <>
          <Button
            icon={status.state === "paused" ? "play" : "pause"}
            variant="ghost"
            disabled={busy || status.state === "starting"}
            title={status.state === "paused" ? "Resume buffer" : "Pause buffer"}
            onClick={() =>
              void run(status.state === "paused" ? commands.resumeBuffer : commands.pauseBuffer)
            }
          />
          <Button
            icon="stop"
            variant="ghost"
            disabled={busy}
            title="Stop buffer"
            onClick={() => void run(commands.stopBuffer)}
          />
          <Button
            icon="record"
            variant="ghost"
            disabled={busy || !live}
            title={status.recording ? "Stop recording" : "Start manual recording"}
            onClick={() => void run(commands.toggleRecording)}
          >
            {status.recording ? "Stop" : "Rec"}
          </Button>
        </>
      )}
      <Button
        variant="primary"
        icon="save"
        disabled={busy || !canSave}
        title="Save everything in the buffer"
        onClick={() =>
          void run(async () => {
            const clip = await commands.saveClip();
            notify("info", `Clip saved (${formatTime(Math.round(clip.durationSec), 0)})`);
          })
        }
      >
        Save clip
      </Button>
    </div>
  );
}

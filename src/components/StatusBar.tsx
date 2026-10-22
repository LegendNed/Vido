import { useState } from "react";

import { formatTime } from "../lib/format";
import { commands } from "../lib/tauri/commands";
import { errorMessage, type BufferStatus } from "../lib/tauri/types";
import { cn } from "../lib/utils";
import { useApp } from "../stores/app";
import { Button } from "./controls";

const LABELS: Record<BufferStatus["state"], string> = {
  stopped: "Stopped",
  starting: "Starting",
  buffering: "Buffering",
  paused: "Paused",
  waiting: "Waiting for source",
  error: "Error",
};

/**
 * Right side of the page header: buffer state and its buttons, then Save clip.
 * The state badge is left out while the full sidebar shows it (with the details).
 */
export function StatusBar() {
  const status = useApp((s) => s.status);
  const notify = useApp((s) => s.notify);
  const sidebar = useApp((s) => s.settings?.general.sidebar ?? "full");
  const pendingUpdate = useApp((s) => s.pendingUpdate);
  const setUpdateOpen = useApp((s) => s.setUpdateOpen);
  const [busy, setBusy] = useState(false);
  if (!status) return null;

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
  const canSave =
    status.state === "buffering" || status.state === "paused" || status.bufferedSeconds > 0;
  return (
    <div className="ml-auto flex min-w-0 items-center gap-1.5">
      {pendingUpdate && (
        <Button
          small
          icon="download"
          title={`Vido ${pendingUpdate.version} is available`}
          onClick={() => {
            setUpdateOpen(true);
          }}
        >
          Update
        </Button>
      )}
      {sidebar !== "full" && (
        <span className="mr-1 inline-flex items-center gap-2 whitespace-nowrap rounded-md border px-2.5 py-1 text-xs font-medium">
          <span
            className={cn(
              "size-2 rounded-full",
              live || status.recording
                ? "bg-rec"
                : status.state === "stopped"
                  ? "bg-muted-foreground/50"
                  : "bg-warning",
            )}
          />
          {status.recording
            ? `REC ${formatTime(status.recording.elapsedSeconds, 0)}`
            : LABELS[status.state]}
          {!status.recording && live && (
            <span className="font-normal text-muted-foreground">
              {Math.floor(status.bufferedSeconds)} s
            </span>
          )}
        </span>
      )}
      {status.state === "stopped" || status.state === "error" ? (
        <Button small icon="record" disabled={busy} onClick={() => void run(commands.startBuffer)}>
          Start buffer
        </Button>
      ) : (
        <>
          <Button
            small
            icon={status.state === "paused" ? "play" : "pause"}
            variant="ghost"
            disabled={busy || status.state === "starting"}
            title={status.state === "paused" ? "Resume buffer" : "Pause buffer"}
            onClick={() =>
              void run(status.state === "paused" ? commands.resumeBuffer : commands.pauseBuffer)
            }
          />
          <Button
            small
            icon="stop"
            variant="ghost"
            disabled={busy}
            title="Stop buffer"
            onClick={() => void run(commands.stopBuffer)}
          />
          <Button
            small
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
        small
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

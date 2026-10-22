import { useState } from "react";

import { Button, Progress } from "../../components/controls";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import { formatBytes } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage, type UpdateProgress } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { ReleaseNotes } from "./ReleaseNotes";

/**
 * The update offered by the top bar's Update button: release notes, then
 * "Download and install" (verified by the backend; Vido restarts afterwards).
 */
export function UpdateDialog() {
  const update = useApp((s) => s.pendingUpdate);
  const open = useApp((s) => s.updateOpen);
  const setOpen = useApp((s) => s.setUpdateOpen);
  const status = useApp((s) => s.status);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEvent("update-progress", setProgress);
  if (!update) return null;

  const install = async () => {
    setError(null);
    setInstalling(true);
    try {
      // Vido exits here when it works; the installer or the restart brings it back.
      await commands.installUpdate();
    } catch (e) {
      setError(errorMessage(e));
      setInstalling(false);
      setProgress(null);
    }
  };

  const pct =
    progress?.total && progress.total > 0 ? (progress.downloaded / progress.total) * 100 : 0;
  const busy = status?.recording ? "A recording is running; it will stop." : null;

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!installing) setOpen(o);
      }}
    >
      <DialogContent className="sm:max-w-lg" showCloseButton={!installing}>
        <DialogHeader>
          <DialogTitle>Vido {update.version} is available</DialogTitle>
          <DialogDescription>
            You have {update.currentVersion}.{" "}
            {update.staged
              ? "It's downloaded and installs the next time Vido starts, or now."
              : "Vido restarts to finish the update; the buffer starts again by itself."}
          </DialogDescription>
        </DialogHeader>
        {update.notes && (
          <div className="max-h-64 overflow-y-auto rounded-md border bg-muted/30 p-3 text-sm">
            <ReleaseNotes markdown={update.notes} />
          </div>
        )}
        {installing && (
          <div className="flex flex-col gap-1.5">
            <Progress value={pct} />
            <span className="text-xs text-muted-foreground">
              {progress
                ? `Downloading… ${formatBytes(progress.downloaded)}${progress.total ? ` of ${formatBytes(progress.total)}` : ""}`
                : "Preparing…"}
            </span>
          </div>
        )}
        {error && <p className="text-sm text-destructive">{error}</p>}
        {busy && !installing && <p className="text-sm text-muted-foreground">{busy}</p>}
        <DialogFooter>
          <Button
            variant="ghost"
            icon="external"
            className="sm:mr-auto"
            onClick={() => void commands.openReleasePage()}
          >
            View on GitHub
          </Button>
          <Button
            variant="ghost"
            disabled={installing}
            onClick={() => {
              setOpen(false);
            }}
          >
            Later
          </Button>
          <Button
            variant="primary"
            icon="download"
            disabled={installing}
            onClick={() => void install()}
          >
            {update.staged ? "Install now" : "Download and install"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

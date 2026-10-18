import { useEffect, useRef, useState } from "react";

import { formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import type { BufferStatus, HudToast } from "../../lib/tauri/types";

/** Tiny always-on-top indicator. Excluded from capture by Rust (WDA_EXCLUDEFROMCAPTURE). */
export function Hud() {
  const [status, setStatus] = useState<BufferStatus | null>(null);
  const [details, setDetails] = useState(true);
  const [toast, setToast] = useState<HudToast | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    void commands.getBufferStatus().then(setStatus);
    void commands.getSettings().then((s) => {
      setDetails(s.general.hud.showDetails);
    });
  }, []);
  useEvent("buffer-status", setStatus);
  useEvent("settings-changed", (s) => {
    setDetails(s.general.hud.showDetails);
  });
  useEvent("hud-toast", (t) => {
    setToast(t);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      setToast(null);
    }, 2600);
  });

  if (!status) return null;
  const rec = status.recording;
  const state = status.state;
  const dot = rec || state === "buffering" ? "live" : state === "stopped" ? "" : "warn";
  const LABEL: Record<BufferStatus["state"], string> = {
    buffering: `${Math.floor(status.bufferedSeconds)} s`,
    paused: "Paused",
    waiting: "Waiting",
    error: "Error",
    starting: "Starting",
    stopped: "Off",
  };
  const text = rec ? `REC ${formatTime(rec.elapsedSeconds, 0)}` : LABEL[state];

  return (
    <div className="hud">
      {toast ? (
        <div className={`hud-pill toast ${toast.kind}`}>
          <span className={`dot ${toast.kind === "error" ? "warn" : "ok"}`} />
          {toast.text}
        </div>
      ) : (
        <div className="hud-pill">
          <span className={`dot ${dot}`} />
          {(details || rec) && <span>{text}</span>}
        </div>
      )}
    </div>
  );
}

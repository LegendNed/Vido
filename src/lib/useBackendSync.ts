import { useApp } from "../stores/app";
import { useEvent } from "./tauri/events";

/** Keeps the store in sync with backend events (used by the main and editor windows). */
export function useBackendSync() {
  const setStatus = useApp((s) => s.setStatus);
  const setSettings = useApp((s) => s.setSettingsFromBackend);
  const notify = useApp((s) => s.notify);
  useEvent("buffer-status", setStatus);
  useEvent("settings-changed", setSettings);
  useEvent("capture-error", (i) => {
    notify(i.severity, i.message, i.hint);
  });
}

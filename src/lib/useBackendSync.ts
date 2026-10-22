import { useApp } from "../stores/app";
import { useEvent } from "./tauri/events";

/** Keeps the store in sync with backend events (used by the main and editor windows). */
export function useBackendSync() {
  const setStatus = useApp((s) => s.setStatus);
  const setSettings = useApp((s) => s.setSettingsFromBackend);
  const notify = useApp((s) => s.notify);
  const setPendingUpdate = useApp((s) => s.setPendingUpdate);
  useEvent("buffer-status", setStatus);
  useEvent("settings-changed", setSettings);
  useEvent("update-available", setPendingUpdate);
  useEvent("capture-error", (i) => {
    notify(i.severity, i.message, i.hint);
  });
}

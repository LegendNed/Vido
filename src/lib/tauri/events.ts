// Typed subscriptions to events emitted by the Rust backend (see src-tauri/src/events.rs).

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

import type {
  AccentColors,
  AudioDeviceInfo,
  BufferStatus,
  CaptureIssue,
  ClipInfo,
  EditorOpen,
  ExportFailed,
  ExportProgress,
  ExportResult,
  HotkeyTriggered,
  HudToast,
  Navigate,
  Settings,
  TrackLevel,
} from "./types";

export interface EventMap {
  "buffer-status": BufferStatus;
  "capture-error": CaptureIssue;
  "audio-devices-changed": AudioDeviceInfo[];
  "audio-levels": TrackLevel[];
  "export-progress": ExportProgress;
  "export-completed": ExportResult;
  "export-failed": ExportFailed;
  "hotkey-triggered": HotkeyTriggered;
  "clip-saved": ClipInfo;
  "clips-changed": null;
  "settings-changed": Settings;
  "editor-open": EditorOpen;
  "hud-toast": HudToast;
  navigate: Navigate;
  "accent-changed": AccentColors;
}

export type EventName = keyof EventMap;

export function on<K extends EventName>(
  name: K,
  handler: (payload: EventMap[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventMap[K]>(name, (e) => {
    handler(e.payload);
  });
}

/** Subscribes for the lifetime of the component; the latest handler is always used. */
export function useEvent<K extends EventName>(
  name: K,
  handler: (payload: EventMap[K]) => void,
): void {
  const ref = useRef(handler);
  useEffect(() => {
    ref.current = handler;
  });
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void on(name, (p) => {
      ref.current(p);
    }).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [name]);
}

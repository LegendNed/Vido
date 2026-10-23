import { convertFileSrc } from "@tauri-apps/api/core";
import type { DragEvent } from "react";

import { commands } from "../../lib/tauri/commands";
import type { ClipInfo } from "../../lib/tauri/types";

/**
 * Props that let a clip be dragged out of Vido as a file (into Discord, Explorer,
 * a browser upload box…). The web drag is replaced by the system's file drag.
 */
export function dragOut(path: string) {
  return {
    draggable: true,
    onDragStart: (e: DragEvent) => {
      e.preventDefault();
      commands.dragClip(path).catch(() => undefined);
    },
  };
}

/**
 * `<video>` URL for a clip, keyed on its size and date so a file replaced in
 * place ("Save & override") is never served from the webview's cache.
 */
export function clipSrc(c: ClipInfo): string {
  return `${convertFileSrc(c.path)}?v=${c.modifiedMs}-${c.sizeBytes}`;
}

/** Stops playback and lets go of the file (so it can be deleted or replaced). */
export function unloadVideo(v: HTMLVideoElement): void {
  v.pause();
  v.removeAttribute("src");
  v.load();
}

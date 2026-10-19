import { convertFileSrc } from "@tauri-apps/api/core";

import type { ClipInfo } from "../../lib/tauri/types";

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

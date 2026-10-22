// Recording presets (resolution, frame rate, bitrate) for the replay buffer.

import presets from "../../../config/recording-presets.json";
import type { CaptureSettings, ScalePreset } from "../../lib/tauri/types";

export type RecordingPresetId = "low" | "standard" | "high" | "native";

export interface RecordingPreset {
  id: RecordingPresetId;
  label: string;
  hint: string;
  scale: ScalePreset;
  fps: 30 | 60;
  /** kbps, or `null` = automatic by resolution (`quality: high`). */
  kbps: number | null;
}

/** Recording presets: each sets resolution, frame rate and bitrate (`config/recording-presets.json`). */
export const RECORDING_PRESETS = presets as RecordingPreset[];

export const SCALE_LABEL: Record<ScalePreset, string> = {
  "720p": "720p",
  "1080p": "1080p",
  "1440p": "1440p",
  native: "Native",
};

export function applyPreset(c: CaptureSettings, p: RecordingPreset) {
  c.scale = p.scale;
  c.fps = p.fps;
  if (p.kbps === null) c.quality = "high";
  else {
    c.quality = "custom";
    c.customBitrateKbps = p.kbps;
  }
}

/** The preset `c` matches, or `null` (custom settings). */
export function activePreset(c: CaptureSettings): RecordingPresetId | null {
  const p = RECORDING_PRESETS.find(
    (x) =>
      x.scale === c.scale &&
      x.fps === c.fps &&
      (x.kbps === null
        ? c.quality === "high"
        : c.quality === "custom" && c.customBitrateKbps === x.kbps),
  );
  return p?.id ?? null;
}

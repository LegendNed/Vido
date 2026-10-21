// Medal-style recording presets (resolution, frame rate, bitrate) for the replay buffer.

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

/** Medal-style recording presets: each sets resolution, frame rate and bitrate. */
export const RECORDING_PRESETS: RecordingPreset[] = [
  { id: "low", label: "Low", hint: "Older PCs, small files", scale: "720p", fps: 30, kbps: 5000 },
  {
    id: "standard",
    label: "Standard",
    hint: "Light and easy to share",
    scale: "1080p",
    fps: 60,
    kbps: 15000,
  },
  {
    id: "high",
    label: "High",
    hint: "Sharper, larger files",
    scale: "1440p",
    fps: 60,
    kbps: 25000,
  },
  {
    id: "native",
    label: "Native",
    hint: "Full screen resolution",
    scale: "native",
    fps: 60,
    kbps: null,
  },
];

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

import type { ExportDefaults } from "../../lib/tauri/types";

/**
 * Encode options as edited by `EncodeOptions`: the export-defaults shape, plus
 * `streamCopy` where copying the video is possible (the editor's export).
 */
export type EncodeValues = Omit<ExportDefaults, "streamCopy"> & { streamCopy?: boolean };

export interface Preset {
  id: string;
  label: string;
  /** One line shown under the chips while the preset is selected. */
  hint: string;
  /** Only offered where the video can be copied (`streamCopy`). */
  needsCopy?: boolean;
  apply: <T extends EncodeValues>(v: T) => T;
}

/** Sets `values` (and `streamCopy`, where the value has it). */
function set<T extends EncodeValues>(
  v: T,
  values: Partial<ExportDefaults>,
  streamCopy: boolean,
): T {
  return { ...v, ...values, ...(v.streamCopy === undefined ? {} : { streamCopy }) };
}

/** Everything but `encoder` and `mixAudio`, which presets leave alone. */
const H264: Partial<ExportDefaults> = {
  container: "mp4",
  videoCodec: "h264",
  preset: "balanced",
  audioCodec: "aac",
  fps: 0,
};

/**
 * Basic choices for people who don't want to think about codecs. Each preset sets
 * every quality-related option, so changing one of them under Advanced shows
 * "Custom". "High quality" matches `ExportDefaults::high_quality()` in Rust (the
 * re-encode-on-save default).
 */
export const PRESETS: Preset[] = [
  {
    id: "original",
    label: "Original",
    hint: "No re-encode when only trimming: lossless and instant.",
    needsCopy: true,
    apply: (v) =>
      set(
        v,
        {
          scale: "native",
          fps: 0,
          rateMode: v.rateMode === "targetSize" ? "quality" : v.rateMode,
          container: v.container === "webm" ? "mp4" : v.container,
        },
        true,
      ),
  },
  {
    id: "discord10",
    label: "10 MB",
    hint: "Fits 10 MB upload limits · H.264, 720p.",
    apply: (v) =>
      set(
        v,
        { ...H264, rateMode: "targetSize", targetSizeMb: 10, audioBitrateKbps: 96, scale: "720p" },
        false,
      ),
  },
  {
    id: "discord25",
    label: "25 MB",
    hint: "Fits 25 MB upload limits · H.264, 1080p.",
    apply: (v) =>
      set(
        v,
        {
          ...H264,
          rateMode: "targetSize",
          targetSizeMb: 25,
          audioBitrateKbps: 128,
          scale: "1080p",
        },
        false,
      ),
  },
  {
    id: "discord50",
    label: "50 MB",
    hint: "Fits 50 MB upload limits · H.264, source resolution.",
    apply: (v) =>
      set(
        v,
        {
          ...H264,
          rateMode: "targetSize",
          targetSizeMb: 50,
          audioBitrateKbps: 160,
          scale: "native",
        },
        false,
      ),
  },
  {
    id: "hq",
    label: "High quality",
    hint: "Near-lossless H.264 at the source resolution.",
    apply: (v) =>
      set(
        v,
        { ...H264, rateMode: "quality", quality: 18, audioBitrateKbps: 192, scale: "native" },
        false,
      ),
  },
  {
    id: "small",
    label: "Small (HEVC)",
    hint: "Small HEVC files at 1080p (some apps can't play HEVC).",
    apply: (v) =>
      set(
        v,
        {
          ...H264,
          videoCodec: "hevc",
          rateMode: "quality",
          quality: 28,
          audioBitrateKbps: 128,
          scale: "1080p",
        },
        false,
      ),
  },
];

/** Presets offered for `v` (no "Original" where nothing can be copied). */
export function presetsFor(v: EncodeValues): Preset[] {
  return v.streamCopy === undefined ? PRESETS.filter((p) => !p.needsCopy) : PRESETS;
}

/** The preset `v` is set to, or `null` for custom settings. */
export function activePreset(v: EncodeValues): Preset | null {
  const same = (a: EncodeValues, b: EncodeValues) =>
    (Object.keys(a) as (keyof EncodeValues)[]).every((k) => a[k] === b[k]);
  return presetsFor(v).find((p) => same(p.apply(v), v)) ?? null;
}

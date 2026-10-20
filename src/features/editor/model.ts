import type {
  ClipDetails,
  CropRect,
  ExportAudioTrack,
  ExportDefaults,
  ExportRequest,
} from "../../lib/tauri/types";

/**
 * Export options: the encode settings (same shape as the export defaults, edited
 * with `EncodeOptions`; `mixAudio` = one mixed audio track) plus per-export bits.
 */
export interface ExportOpts extends ExportDefaults {
  /** Copy the video instead of re-encoding when possible. */
  streamCopy: boolean;
  /** `null` = `<name>_export.<ext>` next to the clip. */
  outputPath: string | null;
}

/**
 * - `plain`: no separate mix track; tracks are edited directly.
 * - `mix`: use the recorded "Mix" track as-is (stream-copyable).
 * - `separate`: rebuild the mix from the per-source tracks (per-track volume/mute).
 */
export type AudioMode = "plain" | "mix" | "separate";

export interface EditState {
  trimStart: number;
  trimEnd: number;
  crop: CropRect | null;
  speed: number;
  fadeIn: number;
  fadeOut: number;
  audioMode: AudioMode;
  tracks: ExportAudioTrack[];
  /**
   * No longer read: whether exports mix the audio is `x.mixAudio` (Export → Advanced →
   * Keep separate audio tracks). Remove together with the editor's old Audio section.
   */
  mixAudio: boolean;
  x: ExportOpts;
}

export function hasMixStructure(d: ClipDetails): boolean {
  const t = d.info.audioTracks;
  return t.length > 1 && t[0]?.title === "Mix";
}

export function initialState(d: ClipDetails, defaults: ExportDefaults): EditState {
  return {
    trimStart: 0,
    trimEnd: d.info.durationSec,
    crop: null,
    speed: 1,
    fadeIn: 0,
    fadeOut: 0,
    audioMode: hasMixStructure(d) ? "mix" : "plain",
    tracks: d.info.audioTracks.map((t) => ({
      index: t.index,
      include: true,
      muted: false,
      volume: 1,
    })),
    mixAudio: defaults.mixAudio,
    x: {
      ...defaults,
      // Trim-only edits default to a lossless, instant export.
      streamCopy: true,
      outputPath: null,
    },
  };
}

export function requestTracks(s: EditState): { tracks: ExportAudioTrack[]; mix: boolean } {
  switch (s.audioMode) {
    case "plain":
      return { tracks: s.tracks, mix: s.x.mixAudio };
    case "mix":
      return {
        tracks: s.tracks.map((t, i) => (i === 0 ? t : { ...t, include: false })),
        mix: false,
      };
    case "separate":
      return {
        tracks: s.tracks.map((t, i) => (i === 0 ? { ...t, include: false } : t)),
        mix: s.x.mixAudio,
      };
  }
}

export function buildRequest(path: string, s: EditState): ExportRequest {
  const { tracks, mix } = requestTracks(s);
  return {
    inputPath: path,
    ...(s.x.outputPath ? { outputPath: s.x.outputPath } : {}),
    trimStart: s.trimStart,
    trimEnd: s.trimEnd,
    ...(s.crop ? { crop: s.crop } : {}),
    speed: s.speed,
    scale: s.x.scale,
    fps: s.x.fps,
    videoCodec: s.x.videoCodec,
    encoder: s.x.encoder,
    preset: s.x.preset,
    rateMode: s.x.rateMode,
    quality: s.x.quality,
    bitrateKbps: s.x.bitrateKbps,
    maxBitrateKbps: s.x.maxBitrateKbps,
    targetSizeMb: s.x.targetSizeMb,
    audioTracks: tracks,
    mixAudio: mix,
    fadeIn: s.fadeIn,
    fadeOut: s.fadeOut,
    audioCodec: s.x.audioCodec,
    audioBitrateKbps: s.x.audioBitrateKbps,
    container: s.x.container,
    streamCopy: s.x.streamCopy,
  };
}

/**
 * Request for the editor's Save / Save & override. Same edits as an export (Rust
 * then matches the output format to the original), except that in "Recorded mix"
 * mode the per-source tracks are carried along untouched, so the clip keeps its
 * track layout.
 */
export function buildSaveRequest(path: string, s: EditState): ExportRequest {
  const req = buildRequest(path, s);
  if (s.audioMode !== "mix") return req;
  return {
    ...req,
    audioTracks: s.tracks.map((t, i) =>
      i === 0 ? t : { index: t.index, include: true, muted: false, volume: 1 },
    ),
    mixAudio: false,
  };
}

/** Whether `s` changes the clip itself (export-only options don't count). */
export function hasEdits(s: EditState, initial: EditState): boolean {
  const key = (e: EditState) =>
    JSON.stringify([
      e.trimStart,
      e.trimEnd,
      e.crop,
      e.speed,
      e.fadeIn,
      e.fadeOut,
      e.audioMode,
      e.tracks,
    ]);
  return key(s) !== key(initial);
}

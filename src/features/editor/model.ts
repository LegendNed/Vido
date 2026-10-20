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
  /**
   * Exports carry one audio track (the lanes mixed). `false` keeps separate tracks:
   * the (re)built mix first when the clip has one, then every audible lane.
   */
  mixAudio: boolean;
}

/** Mute and volume of one audio lane (one audio track of the clip). */
export interface Lane {
  muted: boolean;
  /** Linear 0–2. */
  volume: number;
}

export interface EditState {
  trimStart: number;
  trimEnd: number;
  crop: CropRect | null;
  /** One per audio track of the clip, by track index. */
  lanes: Lane[];
  /**
   * Fixed per clip: track 0 is the recorded mix of the other tracks (Vido's
   * layout). Its lane is derived from the source lanes and never edited.
   */
  recordedMix: boolean;
  x: ExportOpts;
}

/** The audio part of the edit state. */
export type LaneSet = Pick<EditState, "lanes" | "recordedMix">;

export const UNITY: Lane = { muted: false, volume: 1 };

export function hasMixStructure(d: ClipDetails): boolean {
  const t = d.info.audioTracks;
  return t.length > 1 && t[0]?.title === "Mix";
}

export function initialState(d: ClipDetails, defaults: ExportDefaults): EditState {
  return {
    trimStart: 0,
    trimEnd: d.info.durationSec,
    crop: null,
    lanes: d.info.audioTracks.map(() => UNITY),
    recordedMix: hasMixStructure(d),
    x: {
      ...defaults,
      // "Original" (the default): trim-only edits export lossless and instant.
      streamCopy: defaults.streamCopy,
      outputPath: null,
      mixAudio: defaults.mixAudio,
    },
  };
}

const isUnity = (l: Lane) => !l.muted && Math.abs(l.volume - 1) < 1e-4;

/** Heard in the output: not muted and above 0 %. */
export const isAudible = (l: Lane) => !l.muted && l.volume > 0;

/** Track indices of the lanes the user edits (all but a recorded mix). */
export function editableLanes(s: LaneSet): number[] {
  return s.lanes.map((_, i) => i).filter((i) => !(s.recordedMix && i === 0));
}

/** Whether any lane's mute or volume differs from the recording. */
export function lanesChanged(s: LaneSet): boolean {
  return editableLanes(s).some((i) => !isUnity(s.lanes[i] ?? UNITY));
}

/** The audio part of an `ExportRequest`. */
interface AudioOut {
  audioTracks: ExportAudioTrack[];
  mixAudio: boolean;
  keepMixedTracks: boolean;
}

const keep = (index: number, l: Lane): ExportAudioTrack => ({
  index,
  include: true,
  muted: l.muted,
  volume: l.volume,
});
const drop = (index: number): ExportAudioTrack => ({
  index,
  include: false,
  muted: false,
  volume: 1,
});

/**
 * Export audio. While the lanes are untouched a recorded mix is kept as it is
 * (stream-copyable); otherwise the lanes are mixed with their volumes, leaving the
 * recorded mix out so nothing is counted twice. `x.mixAudio` off keeps separate
 * tracks: the mix (rebuilt from the lanes when the clip has one) followed by every
 * audible lane.
 */
export function exportAudio(s: EditState): AudioOut {
  if (s.lanes.length === 0) return { audioTracks: [], mixAudio: false, keepMixedTracks: false };
  const lanes = editableLanes(s);
  const tracks = s.lanes.map((_, i) => drop(i));
  if (s.recordedMix && !lanesChanged(s)) {
    s.lanes.forEach((_, i) => {
      if (i === 0 || !s.x.mixAudio) tracks[i] = keep(i, UNITY);
    });
    return { audioTracks: tracks, mixAudio: false, keepMixedTracks: false };
  }
  const audible = lanes.filter((i) => isAudible(s.lanes[i] ?? UNITY));
  if (s.x.mixAudio || audible.length === 0) {
    // One track: a lone lane is just re-leveled; all muted gives silence.
    for (const i of lanes) tracks[i] = keep(i, s.lanes[i] ?? UNITY);
    return { audioTracks: tracks, mixAudio: true, keepMixedTracks: false };
  }
  for (const i of audible) tracks[i] = keep(i, s.lanes[i] ?? UNITY);
  // A single audible lane already is the mix.
  const remix = s.recordedMix && audible.length > 1;
  return { audioTracks: tracks, mixAudio: remix, keepMixedTracks: remix };
}

/**
 * Save / Save & override audio: the clip keeps its track layout. A recorded mix is
 * copied while the lanes are untouched, else rebuilt from them; every other track
 * keeps its place with its lane applied (muted = silent).
 */
export function saveAudio(s: EditState): AudioOut {
  if (s.recordedMix && lanesChanged(s)) {
    return {
      audioTracks: s.lanes.map((l, i) => (i === 0 ? drop(0) : keep(i, l))),
      mixAudio: true,
      keepMixedTracks: true,
    };
  }
  return {
    audioTracks: s.lanes.map((l, i) => keep(i, s.recordedMix ? UNITY : l)),
    mixAudio: false,
    keepMixedTracks: false,
  };
}

function request(path: string, s: EditState, audio: AudioOut): ExportRequest {
  return {
    inputPath: path,
    ...(s.x.outputPath ? { outputPath: s.x.outputPath } : {}),
    trimStart: s.trimStart,
    trimEnd: s.trimEnd,
    ...(s.crop ? { crop: s.crop } : {}),
    // The editor offers no speed change or fades (the backend still supports them).
    speed: 1,
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
    ...audio,
    fadeIn: 0,
    fadeOut: 0,
    audioCodec: s.x.audioCodec,
    audioBitrateKbps: s.x.audioBitrateKbps,
    container: s.x.container,
    streamCopy: s.x.streamCopy,
  };
}

export function buildRequest(path: string, s: EditState): ExportRequest {
  return request(path, s, exportAudio(s));
}

/**
 * Request for the editor's Save / Save & override. Same edits as an export (Rust
 * then matches the output format to the original), but the audio keeps the clip's
 * track layout.
 */
export function buildSaveRequest(path: string, s: EditState): ExportRequest {
  return request(path, s, saveAudio(s));
}

/** Whether `s` changes the clip itself (export-only options don't count). */
export function hasEdits(s: EditState, initial: EditState): boolean {
  const key = (e: EditState) => JSON.stringify([e.trimStart, e.trimEnd, e.crop, e.lanes]);
  return key(s) !== key(initial);
}

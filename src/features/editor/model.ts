import type {
  AudioCodec,
  ClipDetails,
  Container,
  CropRect,
  EncoderPreference,
  ExportAudioTrack,
  ExportDefaults,
  ExportRequest,
  QualityPreset,
  RateMode,
  ScalePreset,
  VideoCodec,
} from "../../lib/tauri/types";

export interface ExportOpts {
  container: Container;
  videoCodec: VideoCodec;
  encoder: EncoderPreference;
  preset: QualityPreset;
  rateMode: RateMode;
  quality: number;
  bitrateKbps: number;
  maxBitrateKbps: number;
  targetSizeMb: number;
  audioCodec: AudioCodec;
  audioBitrateKbps: number;
  scale: ScalePreset;
  fps: number;
  streamCopy: boolean;
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
      container: defaults.container,
      videoCodec: defaults.videoCodec,
      encoder: defaults.encoder,
      preset: defaults.preset,
      rateMode: defaults.rateMode,
      quality: defaults.quality,
      bitrateKbps: defaults.bitrateKbps,
      maxBitrateKbps: defaults.maxBitrateKbps,
      targetSizeMb: defaults.targetSizeMb,
      audioCodec: defaults.audioCodec,
      audioBitrateKbps: defaults.audioBitrateKbps,
      scale: defaults.scale,
      fps: defaults.fps,
      // Trim-only edits default to a lossless, instant export.
      streamCopy: true,
      outputPath: null,
    },
  };
}

export function requestTracks(s: EditState): { tracks: ExportAudioTrack[]; mix: boolean } {
  switch (s.audioMode) {
    case "plain":
      return { tracks: s.tracks, mix: s.mixAudio };
    case "mix":
      return {
        tracks: s.tracks.map((t, i) => (i === 0 ? t : { ...t, include: false })),
        mix: false,
      };
    case "separate":
      return {
        tracks: s.tracks.map((t, i) => (i === 0 ? { ...t, include: false } : t)),
        mix: s.mixAudio,
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

export interface Preset {
  id: string;
  label: string;
  hint: string;
  apply: (x: ExportOpts) => ExportOpts;
}

export const PRESETS: Preset[] = [
  {
    id: "original",
    label: "Original",
    hint: "Trim only — no re-encode when possible",
    apply: (x) => ({
      ...x,
      streamCopy: true,
      rateMode: "quality",
      quality: 20,
      scale: "native",
      fps: 0,
    }),
  },
  {
    id: "discord10",
    label: "10 MB",
    hint: "Fits small upload limits",
    apply: (x) => ({
      ...x,
      streamCopy: false,
      rateMode: "targetSize",
      targetSizeMb: 10,
      videoCodec: "h264",
      container: "mp4",
      audioCodec: "aac",
      audioBitrateKbps: 96,
      scale: "720p",
    }),
  },
  {
    id: "discord25",
    label: "25 MB",
    hint: "H.264, 1080p",
    apply: (x) => ({
      ...x,
      streamCopy: false,
      rateMode: "targetSize",
      targetSizeMb: 25,
      videoCodec: "h264",
      container: "mp4",
      audioCodec: "aac",
      audioBitrateKbps: 128,
      scale: "1080p",
    }),
  },
  {
    id: "discord50",
    label: "50 MB",
    hint: "H.264, source resolution",
    apply: (x) => ({
      ...x,
      streamCopy: false,
      rateMode: "targetSize",
      targetSizeMb: 50,
      videoCodec: "h264",
      container: "mp4",
      audioCodec: "aac",
      audioBitrateKbps: 160,
      scale: "native",
    }),
  },
  {
    id: "hq",
    label: "High quality",
    hint: "H.264 CRF 18",
    apply: (x) => ({
      ...x,
      streamCopy: false,
      rateMode: "quality",
      quality: 18,
      videoCodec: "h264",
      scale: "native",
    }),
  },
  {
    id: "small",
    label: "Small (HEVC)",
    hint: "HEVC quality 28, 1080p",
    apply: (x) => ({
      ...x,
      streamCopy: false,
      rateMode: "quality",
      quality: 28,
      videoCodec: "hevc",
      container: x.container === "webm" ? "mp4" : x.container,
      scale: "1080p",
    }),
  },
];

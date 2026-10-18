// Device checklist model: every ticked device is one audio track (see `AudioSettings`
// in src-tauri/src/settings). Rust validates the result: at most one track per
// (direction, device) and at most MAX_AUDIO_TRACKS tracks.

import type {
  AudioDeviceInfo,
  AudioSettings,
  AudioTrackSettings,
  DeviceKind,
} from "../../lib/tauri/types";

/** Mirrors `settings::MAX_AUDIO_TRACKS`. */
export const MAX_AUDIO_TRACKS = 16;
const NAME_MAX = 64;

/** Which endpoints a track records; legacy `input` tracks are inputs. */
export function directionOf(t: AudioTrackSettings): DeviceKind {
  return t.kind === "desktop" ? "output" : "input";
}

export interface DeviceRow {
  key: string;
  direction: DeviceKind;
  /** `null` = the "Windows default" entry (follows the default device). */
  deviceId: string | null;
  label: string;
  /** Present right now; for the default entry: any device of this direction. */
  connected: boolean;
  /** This device is the current Windows default. */
  isDefault: boolean;
  /** The track for this device (ticked, or unticked but remembered). */
  track: AudioTrackSettings | undefined;
  /** Position of `track` in `audio.tracks` (-1 if none). */
  index: number;
  checked: boolean;
}

export function findTrack(
  audio: AudioSettings,
  direction: DeviceKind,
  deviceId: string | null,
): number {
  return audio.tracks.findIndex((t) => directionOf(t) === direction && t.deviceId === deviceId);
}

export function enabledCount(audio: AudioSettings): number {
  return audio.tracks.filter((t) => t.enabled).length;
}

function fallbackName(direction: DeviceKind): string {
  return direction === "output" ? "Output device" : "Input device";
}

/**
 * One checklist group: the Windows-default entry, every present device, then ticked
 * devices that are not connected (so they can be unticked). `devices` is `null`
 * while the list is loading; nothing is marked "not connected" until it arrives.
 */
export function deviceRows(
  audio: AudioSettings,
  devices: AudioDeviceInfo[] | null,
  direction: DeviceKind,
): DeviceRow[] {
  const present = (devices ?? []).filter((d) => d.kind === direction);
  const row = (
    deviceId: string | null,
    label: string,
    connected: boolean,
    isDefault: boolean,
  ): DeviceRow => {
    const index = findTrack(audio, direction, deviceId);
    const track = audio.tracks[index];
    return {
      key: `${direction}:${deviceId ?? "default"}`,
      direction,
      deviceId,
      label,
      connected,
      isDefault,
      track,
      index,
      checked: track?.enabled ?? false,
    };
  };
  const rows = [
    row(
      null,
      direction === "output" ? "Windows default output" : "Windows default input",
      devices === null || present.length > 0,
      false,
    ),
  ];
  for (const d of present) {
    rows.push(row(d.id, d.name.trim() || fallbackName(direction), true, d.isDefault));
  }
  for (const t of audio.tracks) {
    if (directionOf(t) !== direction || !t.enabled || t.deviceId === null) continue;
    if (present.some((d) => d.id === t.deviceId)) continue;
    rows.push(row(t.deviceId, t.name, devices === null, false));
  }
  return rows;
}

/** Track title for a newly ticked device: the device name (max. 64 characters). */
export function trackNameFor(row: Pick<DeviceRow, "direction" | "deviceId" | "label">): string {
  if (row.deviceId === null) return row.direction === "output" ? "Desktop audio" : "Microphone";
  const chars = Array.from(row.label.trim() || fallbackName(row.direction));
  return chars.length > NAME_MAX ? `${chars.slice(0, NAME_MAX - 1).join("")}…` : chars.join("");
}

function newTrackId(audio: AudioSettings, direction: DeviceKind): string {
  const base = `${direction === "output" ? "out" : "in"}-${Date.now().toString(36)}`;
  let id = base;
  for (let n = 2; audio.tracks.some((t) => t.id === id); n++) id = `${base}-${n}`;
  return id;
}

/**
 * Ticks or unticks a device (mutates `audio`). Unticking keeps the track, disabled, so
 * ticking again restores its name/volume/gain — except for devices that are not
 * connected, which would otherwise linger invisibly. When the list is full, the
 * oldest remembered (unticked) track makes room for a new one.
 */
export function setDeviceChecked(
  audio: AudioSettings,
  row: Pick<DeviceRow, "direction" | "deviceId" | "label" | "connected">,
  checked: boolean,
): void {
  const i = findTrack(audio, row.direction, row.deviceId);
  const existing = audio.tracks[i];
  if (!checked) {
    if (!existing) return;
    if (row.connected) existing.enabled = false;
    else audio.tracks.splice(i, 1);
    return;
  }
  if (existing) {
    existing.enabled = true;
    return;
  }
  if (enabledCount(audio) >= MAX_AUDIO_TRACKS) return;
  while (audio.tracks.length >= MAX_AUDIO_TRACKS) {
    const j = audio.tracks.findIndex((t) => !t.enabled);
    if (j < 0) return;
    audio.tracks.splice(j, 1);
  }
  audio.tracks.push({
    id: newTrackId(audio, row.direction),
    kind: row.direction === "output" ? "desktop" : "microphone",
    name: trackNameFor(row),
    deviceId: row.deviceId,
    enabled: true,
    muted: false,
    volume: 1,
    gainDb: 0,
    mono: false,
    noiseSuppression: false,
  });
}

/**
 * The ticked device that is also the current Windows default while the default entry
 * is ticked too: the same sound would be recorded twice.
 */
export function recordedTwice(rows: DeviceRow[]): DeviceRow | undefined {
  if (!rows.some((r) => r.deviceId === null && r.checked)) return undefined;
  return rows.find((r) => r.deviceId !== null && r.checked && r.isDefault);
}

/** "4 tracks: mix + 3 sources" — mirrors the stream layout of the audio engine. */
export function trackSummary(audio: AudioSettings): string {
  const on = audio.tracks.filter((t) => t.enabled);
  const n = on.length;
  if (n === 0) return "No audio";
  const mix = audio.mixedTrack;
  const separate = audio.separateTracks && !(mix && n === 1);
  const total = (mix ? 1 : 0) + (separate ? n : 0);
  const tracks = `${total} track${total === 1 ? "" : "s"}`;
  const sources = `${n} source${n === 1 ? "" : "s"}`;
  if (n === 1) return `${tracks}: ${on[0]?.name ?? ""}`;
  if (mix && separate) return `${tracks}: mix + ${sources}`;
  if (mix) return `${tracks}: mix of ${sources}`;
  return `${tracks}, one per source`;
}

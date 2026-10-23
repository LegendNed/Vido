// Typed wrappers around every Rust command. The frontend never calls `invoke`
// directly; argument names are camelCase and converted to snake_case by Tauri.

import { invoke } from "@tauri-apps/api/core";

import type {
  AccentColors,
  AppInfo,
  AudioDeviceInfo,
  BufferStatus,
  ClipDetails,
  ClipInfo,
  Container,
  CropRect,
  DisplayInfo,
  EncoderCapabilities,
  QualityTier,
  ExportEstimate,
  ExportRequest,
  FolderKind,
  HotkeyStatus,
  RegionRect,
  SaveEditMode,
  SaveSound,
  UpdateInfo,
  Settings,
  WindowInfo,
} from "./types";

export const commands = {
  // App & settings
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  /** Windows accent palette; also pushed as `accent-changed`. */
  getAccentColors: () => invoke<AccentColors>("get_accent_colors"),
  getSettings: () => invoke<Settings>("get_settings"),
  getDefaultSettings: () => invoke<Settings>("get_default_settings"),
  updateSettings: (settings: Settings) => invoke<Settings>("update_settings", { settings }),
  resetSettings: () => invoke<Settings>("reset_settings"),
  completeFirstRun: (settings: Settings) =>
    invoke<BufferStatus>("complete_first_run", { settings }),

  // Devices & encoders
  listDisplays: () => invoke<DisplayInfo[]>("list_displays"),
  listWindows: () => invoke<WindowInfo[]>("list_windows"),
  listAudioDevices: () => invoke<AudioDeviceInfo[]>("list_audio_devices"),
  getEncoderCapabilities: (refresh = false) =>
    invoke<EncoderCapabilities>("get_encoder_capabilities", { refresh }),
  /** A recording quality suited to this PC (hardware encoder, GPU memory, display). */
  recommendQuality: () => invoke<QualityTier>("recommend_quality"),

  // Replay buffer
  startBuffer: () => invoke<BufferStatus>("start_buffer"),
  stopBuffer: () => invoke<BufferStatus>("stop_buffer"),
  pauseBuffer: () => invoke<BufferStatus>("pause_buffer"),
  resumeBuffer: () => invoke<BufferStatus>("resume_buffer"),
  getBufferStatus: () => invoke<BufferStatus>("get_buffer_status"),
  /** Saves everything in the buffer (up to the buffer length set under Recording). */
  saveClip: () => invoke<ClipInfo>("save_clip"),
  toggleRecording: () => invoke<BufferStatus>("toggle_recording"),
  setMetersEnabled: (enabled: boolean) => invoke<undefined>("set_meters_enabled", { enabled }),

  // Clips
  listClips: () => invoke<ClipInfo[]>("list_clips"),
  getClipDetails: (path: string) => invoke<ClipDetails>("get_clip_details", { path }),
  deleteClip: (path: string) => invoke<undefined>("delete_clip", { path }),
  revealClip: (path: string) => invoke<undefined>("reveal_clip", { path }),
  openClipExternal: (path: string) => invoke<undefined>("open_clip_external", { path }),
  copyClipPath: (path: string) => invoke<undefined>("copy_clip_path", { path }),
  getWaveform: (path: string, track: number, buckets: number) =>
    invoke<number[]>("get_waveform", { path, track, buckets }),
  /** One playable copy per audio track (`null` if the webview can't play it). */
  prepareAudioPreview: (path: string) =>
    invoke<(string | null)[]>("prepare_audio_preview", { path }),
  takeScreenshot: (path: string, timeSec: number, crop?: CropRect) =>
    invoke<string>("take_screenshot", { path, timeSec, crop }),

  // Export
  estimateExport: (request: ExportRequest) =>
    invoke<ExportEstimate>("estimate_export", { request }),
  /** Returns the job id; progress arrives via `export-progress` events. */
  exportClip: (request: ExportRequest) => invoke<string>("export_clip", { request }),
  cancelExport: (jobId: string) => invoke<boolean>("cancel_export", { jobId }),
  /**
   * Editor Save (`new`: `<name>_edit.<ext>`) or Save & override (`override`: replaces
   * the clip, the original goes to the Recycle Bin). Returns the export job id.
   */
  saveEdit: (request: ExportRequest, mode: SaveEditMode) =>
    invoke<string>("save_edit", { request, mode }),

  // Windows & dialogs
  openEditor: (path?: string) => invoke<undefined>("open_editor", { path }),
  showMainWindow: (page?: string) => invoke<undefined>("show_main_window", { page }),
  pickFolder: (initial?: string) => invoke<string | null>("pick_folder", { initial }),
  getCurrentGame: () => invoke<string | null>("get_current_game"),
  setFavorite: (path: string, favorite: boolean) =>
    invoke<null>("set_favorite", { path, favorite }),
  /** Renames a clip in its folder; returns the new path. */
  renameClip: (path: string, name: string) => invoke<string>("rename_clip", { path, name }),
  /** One WAV per audio track into "<clip> audio" next to it; returns that folder. */
  exportAudioTracks: (path: string, reveal: boolean) =>
    invoke<string>("export_audio_tracks", { path, reveal }),

  // Updates
  checkForUpdates: () => invoke<UpdateInfo | null>("check_for_updates"),
  getPendingUpdate: () => invoke<UpdateInfo | null>("get_pending_update"),
  /** Downloads and installs the pending update; Vido exits and comes back. */
  installUpdate: () => invoke<null>("install_update"),
  /** Opens Vido's releases on GitHub in the browser. */
  openReleasePage: () => invoke<null>("open_release_page"),
  pickSoundFile: (initial?: string) => invoke<string | null>("pick_sound_file", { initial }),
  previewSaveSound: (sound: SaveSound, path: string | null, volume: number) =>
    invoke<null>("preview_save_sound", { sound, path, volume }),
  pickExportPath: (suggested: string, container: Container) =>
    invoke<string | null>("pick_export_path", { suggested, container }),
  openFolder: (which: FolderKind) => invoke<undefined>("open_folder", { which }),

  // Hotkeys & region
  getHotkeyStatus: () => invoke<HotkeyStatus[]>("get_hotkey_status"),
  setHotkeysSuspended: (suspended: boolean) =>
    invoke<undefined>("set_hotkeys_suspended", { suspended }),

  // Overlay
  /** Shows a sample "Clip saved" popup in the overlay's position (whatever the toggles). */
  previewHud: () => invoke<undefined>("preview_hud"),
  openRegionPicker: (monitorId?: string) => invoke<undefined>("open_region_picker", { monitorId }),
  closeRegionPicker: () => invoke<undefined>("close_region_picker"),
  setCaptureRegion: (region: RegionRect) => invoke<Settings>("set_capture_region", { region }),
} as const;

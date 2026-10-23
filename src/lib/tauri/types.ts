// Shared IPC types. Everything here mirrors a Rust type; the definitions in
// ./bindings are generated from Rust with ts-rs (`pnpm bindings`) - do not edit them.

export type { AccentColors } from "./bindings/AccentColors";
export type { AdvancedSettings } from "./bindings/AdvancedSettings";
export type { AppError } from "./bindings/AppError";
export type { AppInfo } from "./bindings/AppInfo";
export type { AudioCodec } from "./bindings/AudioCodec";
export type { AudioDeviceInfo } from "./bindings/AudioDeviceInfo";
export type { AudioSettings } from "./bindings/AudioSettings";
export type { AudioTrackSettings } from "./bindings/AudioTrackSettings";
export type { AudioTrackStatus } from "./bindings/AudioTrackStatus";
export type { BufferQuality } from "./bindings/BufferQuality";
export type { BufferState } from "./bindings/BufferState";
export type { BufferStatus } from "./bindings/BufferStatus";
export type { BufferStorage } from "./bindings/BufferStorage";
export type { CaptureIssue } from "./bindings/CaptureIssue";
export type { CaptureMode } from "./bindings/CaptureMode";
export type { CaptureSettings } from "./bindings/CaptureSettings";
export type { ClipAudioTrack } from "./bindings/ClipAudioTrack";
export type { ClipDetails } from "./bindings/ClipDetails";
export type { ClipInfo } from "./bindings/ClipInfo";
export type { Container } from "./bindings/Container";
export type { CropRect } from "./bindings/CropRect";
export type { DeviceKind } from "./bindings/DeviceKind";
export type { DisplayInfo } from "./bindings/DisplayInfo";
export type { EditorOpen } from "./bindings/EditorOpen";
export type { EncoderCapabilities } from "./bindings/EncoderCapabilities";
export type { EncoderInfo } from "./bindings/EncoderInfo";
export type { EncoderPreference } from "./bindings/EncoderPreference";
export type { EncoderSpeed } from "./bindings/EncoderSpeed";
export type { QualityTier } from "./bindings/QualityTier";
export type { ErrorCode } from "./bindings/ErrorCode";
export type { ExportAudioTrack } from "./bindings/ExportAudioTrack";
export type { ExportDefaults } from "./bindings/ExportDefaults";
export type { ExportEstimate } from "./bindings/ExportEstimate";
export type { ExportFailed } from "./bindings/ExportFailed";
export type { ExportProgress } from "./bindings/ExportProgress";
export type { ExportRequest } from "./bindings/ExportRequest";
export type { ExportResult } from "./bindings/ExportResult";
export type { ExportStage } from "./bindings/ExportStage";
export type { FolderKind } from "./bindings/FolderKind";
export type { GeneralSettings } from "./bindings/GeneralSettings";
export type { HotkeyAction } from "./bindings/HotkeyAction";
export type { HotkeySettings } from "./bindings/HotkeySettings";
export type { HotkeyStatus } from "./bindings/HotkeyStatus";
export type { HotkeyTriggered } from "./bindings/HotkeyTriggered";
export type { HudPosition } from "./bindings/HudPosition";
export type { HudSize } from "./bindings/HudSize";
export type { HudSettings } from "./bindings/HudSettings";
export type { LogLevel } from "./bindings/LogLevel";
export type { Navigate } from "./bindings/Navigate";
export type { PostSaveActions } from "./bindings/PostSaveActions";
export type { QualityPreset } from "./bindings/QualityPreset";
export type { RateMode } from "./bindings/RateMode";
export type { RecordingStatus } from "./bindings/RecordingStatus";
export type { RegionRect } from "./bindings/RegionRect";
export type { SaveEditMode } from "./bindings/SaveEditMode";
export type { SaveMode } from "./bindings/SaveMode";
export type { SavedClip } from "./bindings/SavedClip";
export type { SavingSettings } from "./bindings/SavingSettings";
export type { ScalePreset } from "./bindings/ScalePreset";
export type { Settings } from "./bindings/Settings";
export type { Severity } from "./bindings/Severity";
export type { SaveSound } from "./bindings/SaveSound";
export type { UpdateInfo } from "./bindings/UpdateInfo";
export type { UpdateProgress } from "./bindings/UpdateProgress";
export type { SidebarMode } from "./bindings/SidebarMode";
export type { SourceState } from "./bindings/SourceState";
export type { TrackKind } from "./bindings/TrackKind";
export type { TrackLevel } from "./bindings/TrackLevel";
export type { Vendor } from "./bindings/Vendor";
export type { VideoCodec } from "./bindings/VideoCodec";
export type { WindowInfo } from "./bindings/WindowInfo";
export type { WindowRef } from "./bindings/WindowRef";
export type { DiskSpace } from "./bindings/DiskSpace";

import type { AppError } from "./bindings/AppError";

/** Commands reject with a serialized `AppError`. */
export function isAppError(e: unknown): e is AppError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    "message" in e &&
    typeof (e as { message: unknown }).message === "string"
  );
}

export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.hint ? `${e.message}. ${e.hint}` : e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

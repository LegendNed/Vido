import type { Option } from "../components/ui";
import type {
  AudioCodec,
  BufferQuality,
  BufferStorage,
  Container,
  EncoderCapabilities,
  EncoderPreference,
  EncoderSpeed,
  HudPosition,
  LogLevel,
  QualityPreset,
  RateMode,
  ScalePreset,
  SidebarMode,
  Vendor,
  VideoCodec,
} from "./tauri/types";

export const FPS_OPTIONS: Option<0 | 24 | 30 | 48 | 60 | 120>[] = [
  { value: 0, label: "Native (display)" },
  { value: 24, label: "24 fps" },
  { value: 30, label: "30 fps" },
  { value: 48, label: "48 fps" },
  { value: 60, label: "60 fps" },
  { value: 120, label: "120 fps" },
];

export const SCALE_OPTIONS: Option<ScalePreset>[] = [
  { value: "native", label: "Source (100%)" },
  { value: "1440p", label: "1440p" },
  { value: "1080p", label: "1080p" },
  { value: "720p", label: "720p" },
];

export const QUALITY_OPTIONS: Option<BufferQuality>[] = [
  { value: "low", label: "Low (smallest)" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "ultra", label: "Ultra" },
  { value: "custom", label: "Custom bitrate" },
];

export const STORAGE_OPTIONS: Option<BufferStorage>[] = [
  { value: "auto", label: "Auto" },
  { value: "memory", label: "RAM" },
  { value: "disk", label: "Disk" },
];

export const CONTAINER_OPTIONS: Option<Container>[] = [
  { value: "mp4", label: "MP4" },
  { value: "mkv", label: "MKV" },
  { value: "webm", label: "WebM" },
];

export const PRESET_OPTIONS: Option<QualityPreset>[] = [
  { value: "speed", label: "Fast" },
  { value: "balanced", label: "Balanced" },
  { value: "quality", label: "Best quality" },
];

export const RATE_MODE_OPTIONS: Option<RateMode>[] = [
  { value: "quality", label: "Quality" },
  { value: "bitrate", label: "Bitrate" },
  { value: "targetSize", label: "Target size" },
];

export const AUDIO_CODEC_OPTIONS: Option<AudioCodec>[] = [
  { value: "aac", label: "AAC" },
  { value: "opus", label: "Opus" },
];

export const HUD_POSITIONS: Option<HudPosition>[] = [
  { value: "topLeft", label: "Top left" },
  { value: "topCenter", label: "Top centre" },
  { value: "topRight", label: "Top right" },
  { value: "bottomLeft", label: "Bottom left" },
  { value: "bottomCenter", label: "Bottom centre" },
  { value: "bottomRight", label: "Bottom right" },
];

export const SIDEBAR_OPTIONS: Option<SidebarMode>[] = [
  { value: "full", label: "Full" },
  { value: "small", label: "Small" },
  { value: "none", label: "None" },
];

export const LOG_LEVELS: Option<LogLevel>[] = [
  { value: "error", label: "Errors only" },
  { value: "warn", label: "Warnings" },
  { value: "info", label: "Info" },
  { value: "debug", label: "Debug" },
  { value: "trace", label: "Trace" },
];

const CODEC_LABEL: Record<VideoCodec, string> = { h264: "H.264", hevc: "HEVC (H.265)", av1: "AV1" };
const VENDOR_FOR: Record<Exclude<EncoderPreference, "auto">, Vendor> = {
  nvenc: "nvidia",
  amf: "amd",
  qsv: "intel",
  cpu: "cpu",
};

export function codecOptions(caps: EncoderCapabilities | null): Option<VideoCodec>[] {
  return (["h264", "hevc", "av1"] as VideoCodec[]).map((c) => {
    const best = caps
      ? c === "h264"
        ? caps.bestH264
        : c === "hevc"
          ? caps.bestHevc
          : caps.bestAv1
      : null;
    const hw = best && !best.startsWith("lib") ? " · GPU" : best ? " · CPU" : "";
    return {
      value: c,
      label: `${CODEC_LABEL[c]}${caps ? (best ? hw : " · unavailable") : ""}`,
      disabled: Boolean(caps) && !best,
    };
  });
}

export function encoderOptions(
  caps: EncoderCapabilities | null,
  codec: VideoCodec,
): Option<EncoderPreference>[] {
  const avail = (pref: Exclude<EncoderPreference, "auto">) =>
    !caps ||
    caps.encoders.some((e) => e.codec === codec && e.vendor === VENDOR_FOR[pref] && e.available);
  const opts: [EncoderPreference, string][] = [
    ["auto", "Auto (best available)"],
    ["nvenc", "NVIDIA NVENC"],
    ["amf", "AMD AMF"],
    ["qsv", "Intel Quick Sync"],
    ["cpu", "CPU (x264 / x265 / SVT-AV1)"],
  ];
  return opts.map(([value, label]) => {
    const ok = value === "auto" || avail(value);
    return { value, label: ok ? label : `${label} — not available`, disabled: !ok };
  });
}

export const CODEC_NAMES = CODEC_LABEL;

export const ENCODER_SPEED_OPTIONS: Option<EncoderSpeed>[] = [
  { value: "performance", label: "Performance" },
  { value: "balanced", label: "Balanced" },
  { value: "quality", label: "Quality" },
];

/** Buffer bitrate: "auto" follows the resolution (`quality: high`), numbers are kbps. */
export const BITRATE_OPTIONS: Option<"auto" | number>[] = [
  { value: "auto", label: "Auto" },
  ...[5, 8, 10, 15, 20, 25, 30, 40, 50, 60].map((m) => ({ value: m * 1000, label: `${m} Mbps` })),
];

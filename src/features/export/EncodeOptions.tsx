import { useState } from "react";

import { Expander } from "../../components/Expander";
import { Field, NumberInput, Segmented, Select, Toggle } from "../../components/ui";
import { formatBitrate, parseSizeMb } from "../../lib/format";
import {
  AUDIO_CODEC_OPTIONS,
  codecOptions,
  CONTAINER_OPTIONS,
  encoderOptions,
  FPS_OPTIONS,
  PRESET_OPTIONS,
  RATE_MODE_OPTIONS,
  SCALE_OPTIONS,
} from "../../lib/options";
import type { VideoCodec } from "../../lib/tauri/types";
import { useRememberedFlag } from "../../lib/useRememberedFlag";
import { useApp } from "../../stores/app";
import { activePreset, presetsFor, type EncodeValues } from "./presets";

/** Stream-copy state of an editor export (from its estimate). */
export interface CopyState {
  /** The video can be copied as-is. */
  available: boolean;
  /** Why it can't, when it can't. */
  blocker?: string | null;
  /** This export copies the video, so the video encode options don't apply. */
  active: boolean;
}

const SHORT_CODEC: Record<VideoCodec, string> = { h264: "H.264", hevc: "HEVC", av1: "AV1" };
const AUDIO_BITRATES = [64, 96, 128, 160, 192, 256, 320].map((k) => ({
  value: k,
  label: `${k} kbps`,
}));
const FPS = FPS_OPTIONS.map((o) => (o.value === 0 ? { ...o, label: "Same as source" } : o));

/** One-line summary for the collapsed Advanced header. */
function summary(v: EncodeValues, copying: boolean): string {
  const container = v.container.toUpperCase();
  if (copying) return `Video copied as-is · ${container}`;
  const rate =
    v.rateMode === "quality"
      ? `quality ${v.quality}`
      : v.rateMode === "bitrate"
        ? formatBitrate(v.bitrateKbps)
        : `${v.targetSizeMb} MB`;
  const size = v.scale === "native" ? "source size" : v.scale;
  return `${SHORT_CODEC[v.videoCodec]} · ${container} · ${rate} · ${size}`;
}

/**
 * Encode settings shared by the editor's Export panel, Saving → Re-encode and the
 * Export settings page: preset chips for everyone, the rest under a collapsed
 * "Advanced" expander. Picking a preset fills in the advanced values; changing one
 * of those shows "Custom".
 */
export function EncodeOptions<T extends EncodeValues>({
  value: v,
  onChange,
  scope,
  copy,
  errorPrefix,
}: {
  value: T;
  onChange: (fn: (v: T) => T) => void;
  /** Where this is shown (`editor`, `saving`, `defaults`); Advanced stays open or closed per place. */
  scope: string;
  /** Editor exports only (values with `streamCopy`). */
  copy?: CopyState;
  /** Settings path of `value` for highlighting validation errors, e.g. `saving.reencode`. */
  errorPrefix?: string;
}) {
  const caps = useApp((s) => s.caps);
  const fieldError = useApp((s) => s.fieldError);
  const [open, setOpen] = useRememberedFlag(`vido.encode.advanced.${scope}`, false);
  const preset = activePreset(v);
  const hasCopy = v.streamCopy !== undefined;
  const copying = copy?.active ?? false;

  const err = (name: string) =>
    errorPrefix !== undefined && (fieldError?.field?.startsWith(`${errorPrefix}.${name}`) ?? false);
  const patch = (p: Partial<EncodeValues>) => {
    onChange((x) => ({ ...x, ...p }));
  };

  let hint = preset ? preset.hint : "Your own settings (see Advanced).";
  if (preset?.needsCopy && copy && !copy.available && copy.blocker) {
    hint = `${copy.blocker.charAt(0).toUpperCase()}${copy.blocker.slice(1)}.`;
  }

  return (
    <div className="encode">
      <div className="preset-row" role="radiogroup" aria-label="Preset">
        {presetsFor(v).map((p) => (
          <button
            type="button"
            key={p.id}
            role="radio"
            aria-checked={preset?.id === p.id}
            className={`preset${preset?.id === p.id ? " on" : ""}`}
            title={p.hint}
            onClick={() => {
              onChange(p.apply);
            }}
          >
            {p.label}
          </button>
        ))}
        <button
          type="button"
          role="radio"
          aria-checked={preset === null}
          className={`preset${preset === null ? " on" : ""}`}
          title="Your own settings, under Advanced"
          onClick={() => {
            setOpen(true);
          }}
        >
          Custom
        </button>
      </div>
      <div className="preset-hint">{hint}</div>

      {v.rateMode === "targetSize" && (
        <div className="encode-card">
          <Field
            label="Target size"
            description='e.g. "8 MB", "25", "1.5 GB"'
            error={err("targetSizeMb")}
          >
            <TargetSizeInput
              mb={v.targetSizeMb}
              onCommit={(mb) => {
                patch({ targetSizeMb: mb });
              }}
            />
          </Field>
        </div>
      )}

      <Expander title="Advanced" description={summary(v, copying)} open={open} onToggle={setOpen}>
        {hasCopy && copy && (
          <Field
            label="Re-encode video"
            description={
              copy.available
                ? "Off: the video is copied as-is (lossless, instant)"
                : (copy.blocker ?? undefined)
            }
          >
            <Toggle
              label="Re-encode video"
              value={!v.streamCopy || !copy.available}
              disabled={!copy.available}
              onChange={(on) => {
                patch({ streamCopy: !on });
              }}
            />
          </Field>
        )}
        <Field label="Container" error={err("container")}>
          <Select
            value={v.container}
            options={CONTAINER_OPTIONS}
            onChange={(c) => {
              // WebM holds only AV1 + Opus.
              patch(
                c === "webm"
                  ? { container: c, videoCodec: "av1", audioCodec: "opus" }
                  : { container: c },
              );
            }}
          />
        </Field>
        <Field label="Video codec" error={err("videoCodec")}>
          <Select
            value={v.videoCodec}
            disabled={copying}
            options={codecOptions(caps)}
            onChange={(c) => {
              patch(
                v.container === "webm" && c !== "av1"
                  ? { videoCodec: c, container: "mp4" }
                  : { videoCodec: c },
              );
            }}
          />
        </Field>
        <Field label="Encoder">
          <Select
            value={v.encoder}
            disabled={copying}
            options={encoderOptions(caps, v.videoCodec)}
            onChange={(e) => {
              patch({ encoder: e });
            }}
          />
        </Field>
        <Field label="Speed / quality">
          <Select
            value={v.preset}
            disabled={copying}
            options={PRESET_OPTIONS}
            onChange={(p) => {
              patch({ preset: p });
            }}
          />
        </Field>
        <Field label="Rate control">
          <Segmented
            value={v.rateMode}
            options={RATE_MODE_OPTIONS}
            onChange={(m) => {
              // A size target always re-encodes.
              patch(
                m === "targetSize" && hasCopy
                  ? { rateMode: m, streamCopy: false }
                  : { rateMode: m },
              );
            }}
          />
        </Field>
        {v.rateMode === "quality" && (
          <Field
            label="Quality"
            description="Lower is better: 18 ≈ visually lossless, 23 default, 28 small"
            error={err("quality")}
          >
            <NumberInput
              value={v.quality}
              min={0}
              max={51}
              onChange={(q) => {
                patch({ quality: q });
              }}
            />
          </Field>
        )}
        {v.rateMode === "bitrate" && (
          <>
            <Field label="Bitrate" error={err("bitrateKbps")}>
              <NumberInput
                value={v.bitrateKbps}
                min={100}
                max={200000}
                step={100}
                suffix="kbps"
                onChange={(k) => {
                  patch({ bitrateKbps: k });
                }}
              />
            </Field>
            <Field
              label="Peak bitrate"
              description="0 = 1.5× the bitrate"
              error={err("maxBitrateKbps")}
            >
              <NumberInput
                value={v.maxBitrateKbps}
                min={0}
                max={400000}
                step={100}
                suffix="kbps"
                onChange={(k) => {
                  patch({ maxBitrateKbps: k });
                }}
              />
            </Field>
          </>
        )}
        <Field label="Resolution">
          <Select
            value={v.scale}
            options={SCALE_OPTIONS}
            onChange={(s) => {
              patch({ scale: s });
            }}
          />
        </Field>
        <Field label="Frame rate" error={err("fps")}>
          <Select
            value={v.fps}
            options={FPS}
            onChange={(f) => {
              patch({ fps: f });
            }}
          />
        </Field>
        <Field label="Audio" error={err("audio")}>
          <Select
            value={v.audioCodec}
            options={AUDIO_CODEC_OPTIONS}
            onChange={(a) => {
              patch(
                v.container === "webm" && a !== "opus"
                  ? { audioCodec: a, container: "mp4" }
                  : { audioCodec: a },
              );
            }}
          />
          <Select
            value={v.audioBitrateKbps}
            options={AUDIO_BITRATES}
            onChange={(k) => {
              patch({ audioBitrateKbps: k });
            }}
          />
        </Field>
        <Field
          label="Keep separate audio tracks"
          description="Off: all audio is mixed into one track, which plays everywhere (Discord, browsers)"
        >
          <Toggle
            label="Keep separate audio tracks"
            value={!v.mixAudio}
            onChange={(keep) => {
              patch({ mixAudio: !keep });
            }}
          />
        </Field>
      </Expander>
    </div>
  );
}

/** Size field that understands "8 MB", "25", "1.5 GB"; commits on blur / Enter. */
function TargetSizeInput({ mb, onCommit }: { mb: number; onCommit: (mb: number) => void }) {
  // Only set while the user is typing; otherwise the stored value is shown.
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? `${mb} MB`;
  return (
    <input
      className={`input${parseSizeMb(text) === null ? " invalid" : ""}`}
      style={{ width: 110 }}
      value={text}
      aria-label="Target size"
      onChange={(e) => {
        setDraft(e.target.value);
      }}
      onBlur={() => {
        const parsed = parseSizeMb(text);
        if (parsed !== null) {
          onCommit(Math.min(10000, Math.max(1, Math.round(parsed * 10) / 10)));
          setDraft(null);
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") setDraft(null);
      }}
    />
  );
}

import { Field, NumberInput, Section, Segmented, Select, Toggle } from "../../components/ui";
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
import { useApp } from "../../stores/app";

export function ExportDefaultsPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const caps = useApp((s) => s.caps);
  const fieldError = useApp((s) => s.fieldError);
  if (!settings) return null;
  const x = settings.export;
  const err = (f: string) => fieldError?.field?.startsWith(f) ?? false;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Export & compression</h1>
      </div>
      <p className="muted" style={{ margin: 0 }}>
        Defaults for the editor's export panel. Trims without other changes are exported without
        re-encoding.
      </p>
      <Section title="Video">
        <Field label="Container" error={err("export.container")}>
          <Select
            value={x.container}
            options={CONTAINER_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.export.container = v;
                if (v === "webm") {
                  d.export.videoCodec = "av1";
                  d.export.audioCodec = "opus";
                }
              });
            }}
          />
        </Field>
        <Field label="Codec">
          <Select
            value={x.videoCodec}
            options={codecOptions(caps)}
            onChange={(v) => {
              update((d) => {
                d.export.videoCodec = v;
              });
            }}
          />
        </Field>
        <Field label="Encoder">
          <Select
            value={x.encoder}
            options={encoderOptions(caps, x.videoCodec)}
            onChange={(v) => {
              update((d) => {
                d.export.encoder = v;
              });
            }}
          />
        </Field>
        <Field label="Speed / quality">
          <Segmented
            value={x.preset}
            options={PRESET_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.export.preset = v;
              });
            }}
          />
        </Field>
        <Field label="Rate control">
          <Segmented
            value={x.rateMode}
            options={RATE_MODE_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.export.rateMode = v;
              });
            }}
          />
        </Field>
        {x.rateMode === "quality" && (
          <Field
            label="Quality (CRF / CQ)"
            description="Lower is better. 18 ≈ visually lossless, 23 default, 28 small."
            error={err("export.quality")}
          >
            <NumberInput
              value={x.quality}
              min={0}
              max={51}
              onChange={(v) => {
                update((d) => {
                  d.export.quality = v;
                });
              }}
            />
          </Field>
        )}
        {x.rateMode === "bitrate" && (
          <>
            <Field label="Average bitrate" error={err("export.bitrateKbps")}>
              <NumberInput
                value={x.bitrateKbps}
                min={100}
                max={200000}
                step={100}
                suffix="kbps"
                onChange={(v) => {
                  update((d) => {
                    d.export.bitrateKbps = v;
                  });
                }}
              />
            </Field>
            <Field
              label="Peak bitrate"
              description="0 = 1.5× average."
              error={err("export.maxBitrateKbps")}
            >
              <NumberInput
                value={x.maxBitrateKbps}
                min={0}
                max={400000}
                step={100}
                suffix="kbps"
                onChange={(v) => {
                  update((d) => {
                    d.export.maxBitrateKbps = v;
                  });
                }}
              />
            </Field>
          </>
        )}
        {x.rateMode === "targetSize" && (
          <Field
            label="Target file size"
            description="Common limits: 10 MB, 25 MB, 50 MB, 500 MB."
            error={err("export.targetSizeMb")}
          >
            <NumberInput
              value={x.targetSizeMb}
              min={1}
              max={10000}
              step={0.5}
              decimals={1}
              suffix="MB"
              onChange={(v) => {
                update((d) => {
                  d.export.targetSizeMb = v;
                });
              }}
            />
          </Field>
        )}
        <Field label="Resolution">
          <Select
            value={x.scale}
            options={SCALE_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.export.scale = v;
              });
            }}
          />
        </Field>
        <Field label="Frame rate">
          <Select
            value={x.fps}
            options={FPS_OPTIONS.map((o) =>
              o.value === 0 ? { ...o, label: "Same as source" } : o,
            )}
            onChange={(v) => {
              update((d) => {
                d.export.fps = v;
              });
            }}
          />
        </Field>
      </Section>
      <Section title="Audio">
        <Field label="Codec" error={err("export.audioCodec")}>
          <Select
            value={x.audioCodec}
            options={AUDIO_CODEC_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.export.audioCodec = v;
              });
            }}
          />
        </Field>
        <Field label="Bitrate" error={err("export.audioBitrateKbps")}>
          <Select
            value={x.audioBitrateKbps}
            options={[64, 96, 128, 160, 192, 256, 320].map((k) => ({
              value: k,
              label: `${k} kbps`,
            }))}
            onChange={(v) => {
              update((d) => {
                d.export.audioBitrateKbps = v;
              });
            }}
          />
        </Field>
        <Field
          label="Mix tracks into one"
          description="Best compatibility (Discord, browsers play only the first track)."
        >
          <Toggle
            value={x.mixAudio}
            onChange={(v) => {
              update((d) => {
                d.export.mixAudio = v;
              });
            }}
          />
        </Field>
      </Section>
    </div>
  );
}

import { useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import {
  Badge,
  Button,
  Field,
  Meter,
  NumberInput,
  Section,
  Segmented,
  Select,
  Slider,
  TextInput,
  Toggle,
} from "../../components/ui";
import { AUDIO_CODEC_OPTIONS } from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import {
  errorMessage,
  type AudioDeviceInfo,
  type AudioTrackSettings,
  type SourceState,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

interface Level {
  level: number;
  peak: number;
}

const KIND_LABEL: Record<AudioTrackSettings["kind"], string> = {
  desktop: "Desktop audio (loopback)",
  microphone: "Microphone",
  input: "Extra input",
};

export function AudioPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const notify = useApp((s) => s.notify);
  const status = useApp((s) => s.status);
  const fieldError = useApp((s) => s.fieldError);
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [levels, setLevels] = useState<Record<string, Level>>({});

  useEffect(() => {
    commands.listAudioDevices().then(setDevices, (e: unknown) => {
      notify("error", "Could not list audio devices", errorMessage(e));
    });
    void commands.setMetersEnabled(true);
    return () => {
      void commands.setMetersEnabled(false);
    };
  }, [notify]);

  useEvent("audio-devices-changed", setDevices);
  useEvent("audio-levels", (ls) => {
    setLevels((prev) => {
      const next: Record<string, Level> = {};
      for (const l of ls) {
        const p = prev[l.id];
        next[l.id] = {
          level: Math.max(l.rms * 1.4, (p?.level ?? 0) * 0.6),
          peak: Math.max(l.peak, (p?.peak ?? 0) * 0.94),
        };
      }
      return next;
    });
  });

  if (!settings) return null;
  const a = settings.audio;
  const err = (f: string) => fieldError?.field?.startsWith(f) ?? false;
  const stateOf = (id: string): SourceState | undefined =>
    status?.audio.find((t) => t.id === id)?.state;
  const deviceOf = (id: string) => status?.audio.find((t) => t.id === id)?.device;

  const setTrack = (i: number, fn: (t: AudioTrackSettings) => void) => {
    update((d) => {
      const t = d.audio.tracks[i];
      if (t) fn(t);
    });
  };

  const addInput = () => {
    const n = a.tracks.filter((t) => t.kind === "input").length + 1;
    const firstInput = devices.find((d) => d.kind === "input");
    update((d) => {
      d.audio.tracks.push({
        id: `input-${Date.now().toString(36)}`,
        kind: "input",
        name: `Input ${n}`,
        deviceId: firstInput?.id ?? null,
        enabled: true,
        muted: false,
        volume: 1,
        gainDb: 0,
        mono: false,
        noiseSuppression: false,
      });
    });
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Audio</h1>
        <Button small icon="plus" disabled={a.tracks.length >= 6} onClick={addInput}>
          Add input
        </Button>
      </div>

      {a.tracks.map((t, i) => {
        const kind = t.kind === "desktop" ? "output" : "input";
        const list = devices.filter((d) => d.kind === kind);
        const def = list.find((d) => d.isDefault);
        const st = stateOf(t.id);
        const lv = levels[t.id] ?? { level: 0, peak: 0 };
        return (
          <section key={t.id} className="card section">
            <header className="row" style={{ paddingBottom: 8 }}>
              <Icon name={t.kind === "desktop" ? "speaker" : "mic"} />
              <h2 style={{ flex: "none" }}>{t.name}</h2>
              <span className="muted small">{KIND_LABEL[t.kind]}</span>
              {st === "lost" && <Badge tone="err">Device missing</Badge>}
              {st === "active" && t.enabled && <Badge tone="ok">Live</Badge>}
              <span className="spacer" />
              {t.enabled && <Meter level={lv.level} peak={lv.peak} width={140} />}
              <Toggle
                label={`Record ${t.name}`}
                value={t.enabled}
                onChange={(v) => {
                  setTrack(i, (x) => (x.enabled = v));
                }}
              />
              {t.kind === "input" && (
                <Button
                  small
                  variant="ghost"
                  icon="trash"
                  title="Remove input"
                  onClick={() => {
                    update((d) => {
                      d.audio.tracks.splice(i, 1);
                    });
                  }}
                />
              )}
            </header>
            {t.enabled && (
              <div className="body">
                <Field
                  label="Device"
                  description={deviceOf(t.id) ? `Using: ${deviceOf(t.id)}` : undefined}
                  error={err(`audio.tracks.${i}.deviceId`)}
                >
                  <Select<string>
                    value={t.deviceId ?? "__default"}
                    width={320}
                    options={[
                      {
                        value: "__default",
                        label: `Windows default${def ? ` (${def.name})` : ""}`,
                      },
                      ...list.map((d) => ({ value: d.id, label: d.name })),
                      ...(t.deviceId && !list.some((d) => d.id === t.deviceId)
                        ? [{ value: t.deviceId, label: "Disconnected device" }]
                        : []),
                    ]}
                    onChange={(v) => {
                      setTrack(i, (x) => (x.deviceId = v === "__default" ? null : v));
                    }}
                  />
                </Field>
                <Field label="Track name">
                  <TextInput
                    value={t.name}
                    width={200}
                    onCommit={(v) => {
                      setTrack(i, (x) => (x.name = v.trim() || x.name));
                    }}
                  />
                </Field>
                <Field label="Volume" error={err(`audio.tracks.${i}.volume`)}>
                  <Toggle
                    label="Mute"
                    value={t.muted}
                    onChange={(v) => {
                      setTrack(i, (x) => (x.muted = v));
                    }}
                  />
                  <span className="muted small">Mute</span>
                  <Slider
                    value={Math.round(t.volume * 100)}
                    min={0}
                    max={200}
                    width={160}
                    onChange={(v) => {
                      setTrack(i, (x) => (x.volume = v / 100));
                    }}
                  />
                  <span className="mono" style={{ width: 44, textAlign: "right" }}>
                    {Math.round(t.volume * 100)}%
                  </span>
                </Field>
                <Field label="Gain" description="Extra boost for quiet microphones.">
                  <NumberInput
                    value={t.gainDb}
                    min={-20}
                    max={30}
                    step={0.5}
                    decimals={1}
                    suffix="dB"
                    onChange={(v) => {
                      setTrack(i, (x) => (x.gainDb = v));
                    }}
                  />
                </Field>
                <Field
                  label="Channels"
                  description="Mono fixes microphones that only play on one side."
                >
                  <Segmented<"stereo" | "mono">
                    value={t.mono ? "mono" : "stereo"}
                    options={[
                      { value: "stereo", label: "Stereo" },
                      { value: "mono", label: "Mono" },
                    ]}
                    onChange={(v) => {
                      setTrack(i, (x) => (x.mono = v === "mono"));
                    }}
                  />
                </Field>
                {t.kind !== "desktop" && (
                  <Field
                    label="Noise suppression"
                    description={
                      a.sampleRate === 48000 ? "RNNoise, runs locally." : "Requires 48 kHz."
                    }
                    error={err(`audio.tracks.${i}.noiseSuppression`)}
                  >
                    <Toggle
                      value={t.noiseSuppression}
                      disabled={a.sampleRate !== 48000}
                      onChange={(v) => {
                        setTrack(i, (x) => (x.noiseSuppression = v));
                      }}
                    />
                  </Field>
                )}
              </div>
            )}
          </section>
        );
      })}

      <Section title="Format" description="Applies to the replay buffer and saved clips.">
        <Field label="Sample rate" error={err("audio.sampleRate")}>
          <Segmented<44100 | 48000>
            value={a.sampleRate}
            options={[
              { value: 44100, label: "44.1 kHz" },
              { value: 48000, label: "48 kHz" },
            ]}
            onChange={(v) => {
              update((d) => {
                d.audio.sampleRate = v;
                if (v !== 48000) {
                  d.audio.codec = "aac";
                  for (const t of d.audio.tracks) t.noiseSuppression = false;
                }
              });
            }}
          />
        </Field>
        <Field label="Codec" description="Opus needs 48 kHz.">
          <Select
            value={a.codec}
            options={AUDIO_CODEC_OPTIONS.map((o) => ({
              ...o,
              disabled: o.value === "opus" && a.sampleRate !== 48000,
            }))}
            onChange={(v) => {
              update((d) => {
                d.audio.codec = v;
              });
            }}
          />
        </Field>
        <Field label="Bitrate" error={err("audio.bitrateKbps")}>
          <Select
            value={a.bitrateKbps}
            options={[96, 128, 160, 192, 256, 320].map((k) => ({ value: k, label: `${k} kbps` }))}
            onChange={(v) => {
              update((d) => {
                d.audio.bitrateKbps = v;
              });
            }}
          />
        </Field>
        <Field
          label="Mixed track"
          description="Track 1 contains everything — what players and Discord play."
        >
          <Toggle
            value={a.mixedTrack}
            onChange={(v) => {
              update((d) => {
                d.audio.mixedTrack = v;
                if (!v) d.audio.separateTracks = true;
              });
            }}
          />
        </Field>
        <Field
          label="Separate tracks"
          description="One track per source, so the editor can mute or rebalance them."
        >
          <Toggle
            value={a.separateTracks}
            onChange={(v) => {
              update((d) => {
                d.audio.separateTracks = v;
                if (!v) d.audio.mixedTrack = true;
              });
            }}
          />
        </Field>
      </Section>
    </div>
  );
}

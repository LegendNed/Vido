import { useEffect, useState } from "react";

import {
  Badge,
  Field,
  NumberInput,
  SearchBlock,
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
import { errorMessage, type AudioDeviceInfo, type AudioTrackSettings } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { AudioDeviceChecklist, type Level } from "./AudioDeviceChecklist";
import { trackSummary } from "./devices";

/** Audio devices and format: the audio half of the Recording page. */
export function AudioSettings() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const notify = useApp((s) => s.notify);
  const status = useApp((s) => s.status);
  const fieldError = useApp((s) => s.fieldError);
  const [devices, setDevices] = useState<AudioDeviceInfo[] | null>(null);
  const [levels, setLevels] = useState<Record<string, Level>>({});

  useEffect(() => {
    commands.listAudioDevices().then(setDevices, (e: unknown) => {
      setDevices([]);
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

  const setTrack = (i: number, fn: (t: AudioTrackSettings) => void) => {
    update((d) => {
      const t = d.audio.tracks[i];
      if (t) fn(t);
    });
  };

  const trackSettings = (t: AudioTrackSettings, i: number) => (
    <>
      <Field label="Track name" description="Title of this track in saved clips.">
        <TextInput
          value={t.name}
          width={220}
          onCommit={(v) => {
            const name = Array.from(v.trim()).slice(0, 64).join("");
            setTrack(i, (x) => (x.name = name || x.name));
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
      <Field label="Channels" description="Mono fixes microphones that only play on one side.">
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
          description={a.sampleRate === 48000 ? "RNNoise, runs locally." : "Requires 48 kHz."}
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
    </>
  );

  return (
    <>
      <SearchBlock
        title="Audio devices"
        description="Tick every device you want in your clips - each one is recorded on its own track. Changing the selection restarts the replay buffer."
        keywords="microphone mic speakers headphones desktop sound output input track volume mute gain mono stereo noise suppression"
      >
        <div className="row audio-summary">
          <span className="muted small">Saved clips get</span>
          <Badge tone="accent">{trackSummary(a)}</Badge>
        </div>
        <AudioDeviceChecklist
          audio={a}
          devices={devices}
          onEdit={(fn) => {
            update((d) => {
              fn(d.audio);
            });
          }}
          status={status?.audio}
          levels={levels}
          renderSettings={trackSettings}
        />
      </SearchBlock>

      <Section title="Audio format" description="Applies to the replay buffer and saved clips.">
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
        <Field label="Bitrate" description="Per track." error={err("audio.bitrateKbps")}>
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
          description="Track 1 contains everything - what players and Discord play."
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
          description="One track per device, so the editor can mute or rebalance them."
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
    </>
  );
}

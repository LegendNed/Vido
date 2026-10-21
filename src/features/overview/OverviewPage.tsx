import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import { Badge, Button, Field, Keys, Section, Segmented, Toggle } from "../../components/controls";
import { formatBytes, formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage, type BufferStatus, type ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { directionOf, setDeviceChecked } from "../audio/devices";

const STATE_TEXT: Record<BufferStatus["state"], string> = {
  stopped: "Off",
  starting: "Starting…",
  buffering: "Buffering",
  paused: "Paused",
  waiting: "Waiting for the capture source",
  error: "Error",
};

const LENGTHS = [15, 30, 60, 120, 300];

function lengthLabel(s: number): string {
  return s < 60 || s % 60 !== 0 ? `${s} s` : `${s / 60} min`;
}

/**
 * Home page: what Vido is doing right now and the handful of settings people
 * change often. Everything else is in Settings (`go` jumps to a group there).
 */
export function OverviewPage({
  go,
  openClip,
}: {
  go: (page: string) => void;
  /** Opens a clip in the Clips page viewer (closing it leaves you on Clips). */
  openClip: (path: string) => void;
}) {
  const status = useApp((s) => s.status);
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const notify = useApp((s) => s.notify);
  const [clips, setClips] = useState<ClipInfo[]>([]);
  const [busy, setBusy] = useState(false);

  const loadClips = useCallback(() => {
    commands.listClips().then(
      (c) => {
        setClips(c.slice(0, 4));
      },
      () => undefined,
    );
  }, []);
  useEffect(loadClips, [loadClips]);
  useEvent("clips-changed", loadClips);
  useEvent("clip-saved", loadClips);

  if (!settings) return null;

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      notify("error", "Action failed", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const state = status?.state ?? "stopped";
  const running = state !== "stopped" && state !== "error";
  const live = state === "buffering";
  const length = settings.capture.bufferSeconds;
  const lengths = LENGTHS.includes(length) ? LENGTHS : [...LENGTHS, length].sort((a, b) => a - b);
  const inputs = settings.audio.tracks.filter((t) => directionOf(t) === "input");
  const micOn = inputs.some((t) => t.enabled);
  const saveKey = settings.hotkeys.saveClip;
  const hud = settings.general.hud;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Overview</h1>
      </div>

      <section className={`card overview-status${live ? " live" : ""}`}>
        <div className="overview-state">
          <span className={`dot ${live ? "live" : running ? "warn" : ""}`} />
          <div>
            <div className="overview-title">
              {status?.recording
                ? `Recording · ${formatTime(status.recording.elapsedSeconds, 0)}`
                : STATE_TEXT[state]}
            </div>
            <div className="muted small">
              {running && status
                ? [
                    `${Math.floor(status.bufferedSeconds)} of ${status.targetSeconds} s kept`,
                    status.source,
                    status.width > 0
                      ? `${status.width}×${status.height} @ ${Math.round(status.fps)}`
                      : null,
                    status.sizeBytes > 0 ? formatBytes(status.sizeBytes) : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")
                : "The replay buffer isn't running, so nothing can be saved."}
            </div>
          </div>
        </div>
        <div className="row">
          {running ? (
            <Button icon="stop" disabled={busy} onClick={() => void run(commands.stopBuffer)}>
              Stop
            </Button>
          ) : (
            <Button icon="record" disabled={busy} onClick={() => void run(commands.startBuffer)}>
              Start buffer
            </Button>
          )}
          <Button
            variant="primary"
            icon="save"
            disabled={busy || !status || status.bufferedSeconds <= 0}
            onClick={() =>
              void run(async () => {
                const clip = await commands.saveClip();
                notify("info", `Clip saved (${formatTime(Math.round(clip.durationSec), 0)})`);
              })
            }
          >
            Save clip
          </Button>
        </div>
      </section>

      <section className="card overview-clips-card">
        <div className="overview-clips-head">
          <h3>Latest clips</h3>
          <Button small variant="ghost" onClick={() => go("clips")}>
            All clips
          </Button>
        </div>
        {clips.length === 0 ? (
          <p className="muted small">No clips yet - press the Save clip hotkey in a game.</p>
        ) : (
          <div className="overview-clips">
            {clips.map((c) => (
              <button
                type="button"
                key={c.path}
                className="overview-clip"
                title={c.fileName}
                onClick={() => {
                  openClip(c.path);
                }}
              >
                <span
                  className="thumb"
                  style={
                    c.thumbnail
                      ? { backgroundImage: `url("${convertFileSrc(c.thumbnail)}")` }
                      : undefined
                  }
                >
                  {!c.thumbnail && <Icon name="film" />}
                  <span className="duration">{formatTime(Math.round(c.durationSec), 0)}</span>
                </span>
                <span className="ellipsis small">{c.fileName}</span>
              </button>
            ))}
          </div>
        )}
      </section>

      <Section title="Quick settings" description="The rest is in Settings.">
        <Field label="Clip length" description="How much Save clip keeps.">
          <Segmented
            value={length}
            options={lengths.map((s) => ({ value: s, label: lengthLabel(s) }))}
            onChange={(v) => {
              update((d) => {
                d.capture.bufferSeconds = v;
              });
            }}
          />
        </Field>
        <Field label="Save clip hotkey" description="Works while games are focused.">
          {saveKey ? <Keys accelerator={saveKey} /> : <Badge tone="warn">Not set</Badge>}
          <Button small variant="ghost" onClick={() => go("hotkeys")}>
            Change
          </Button>
        </Field>
        <Field
          label="Microphone"
          description={
            inputs.length > 0
              ? "Recorded on its own track."
              : "Records the Windows default microphone on its own track."
          }
        >
          <Toggle
            value={micOn}
            onChange={(on) => {
              update(
                (d) => {
                  const ins = d.audio.tracks.filter((t) => directionOf(t) === "input");
                  if (!on) ins.forEach((t) => (t.enabled = false));
                  else if (ins.length > 0) ins.forEach((t) => (t.enabled = true));
                  else
                    setDeviceChecked(
                      d.audio,
                      {
                        direction: "input",
                        deviceId: null,
                        label: "Windows default input",
                        connected: true,
                      },
                      true,
                    );
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field label="Overlay" description="Small always-on-top pill showing the buffer.">
          <Toggle
            value={hud.overlay}
            onChange={(v) => {
              update(
                (d) => {
                  d.general.hud.overlay = v;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field label="Clip saved popup" description="Shows briefly over the game when you save.">
          <Toggle
            value={hud.clipPopup}
            onChange={(v) => {
              update(
                (d) => {
                  d.general.hud.clipPopup = v;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field
          label="Copy saved clips to the clipboard"
          description="Paste them straight into Discord or a folder."
        >
          <Toggle
            value={settings.saving.postSave.copyVideo}
            onChange={(v) => {
              update(
                (d) => {
                  d.saving.postSave.copyVideo = v;
                  if (v) d.saving.postSave.copyPath = false;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <div className="overview-links">
          <Button small variant="ghost" onClick={() => go("video")}>
            Video
          </Button>
          <Button small variant="ghost" onClick={() => go("audio")}>
            Audio
          </Button>
          <Button small variant="ghost" onClick={() => go("overlay")}>
            Overlay
          </Button>
          <Button small variant="ghost" onClick={() => go("saving")}>
            Saving
          </Button>
          <Button small variant="ghost" onClick={() => go("settings")}>
            All settings
          </Button>
        </div>
      </Section>
    </div>
  );
}

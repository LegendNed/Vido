import { useEffect, useState } from "react";

import { HotkeyRecorder } from "../../components/HotkeyRecorder";
import { Icon } from "../../components/Icon";
import { Badge, Button, Field, Select } from "../../components/ui";
import { QUALITY_OPTIONS } from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import {
  errorMessage,
  type AudioDeviceInfo,
  type DisplayInfo,
  type Settings,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { AudioDeviceChecklist } from "../audio/AudioDeviceChecklist";
import { trackSummary } from "../audio/devices";

const STEPS = ["Display", "Audio", "Buffer", "Hotkey"] as const;
const LENGTHS = [15, 30, 60, 120, 300];

/** 30 → "30 seconds", 120 → "2 minutes", 90 → "90 seconds". */
function formatLength(s: number): string {
  if (s < 60 || s % 60 !== 0) return `${s} seconds`;
  return `${s / 60} minute${s > 60 ? "s" : ""}`;
}

export function FirstRun() {
  const current = useApp((s) => s.settings);
  const setSettings = useApp((s) => s.setSettingsFromBackend);
  const notify = useApp((s) => s.notify);
  const redo = useApp((s) => s.redoSetup);
  const setRedo = useApp((s) => s.setRedoSetup);
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<Settings | null>(current);
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [devices, setDevices] = useState<AudioDeviceInfo[] | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void commands.listDisplays().then(setDisplays);
    commands.listAudioDevices().then(setDevices, () => {
      setDevices([]);
    });
  }, []);
  useEvent("audio-devices-changed", setDevices);

  if (!draft) return null;
  const edit = (fn: (d: Settings) => void) => {
    const next = structuredClone(draft);
    fn(next);
    setDraft(next);
  };

  const finish = async () => {
    setBusy(true);
    try {
      await commands.completeFirstRun(draft);
    } catch (e) {
      // Settings may still have been saved (e.g. capture source busy); the buffer retries on its own.
      notify("error", "Could not start the buffer", errorMessage(e));
    } finally {
      setBusy(false);
      setRedo(false);
      commands.getSettings().then(setSettings, () => undefined);
    }
  };

  return (
    <div className="overlay">
      <div className="wizard">
        <div className="steps">
          {STEPS.map((s, i) => (
            <span key={s} className={i <= step ? "done" : ""} />
          ))}
        </div>

        {step === 0 && (
          <>
            <h2>Welcome to Vido</h2>
            <p className="muted">
              Vido keeps the last moments of your screen in memory and saves them as a clip with one
              key. Everything stays on this PC. Pick the display to record:
            </p>
            <div className="choice-grid">
              {displays.map((d) => {
                const on =
                  (draft.capture.monitorId ?? displays.find((x) => x.primary)?.id) === d.id;
                return (
                  <button
                    type="button"
                    key={d.id}
                    className={`choice${on ? " on" : ""}`}
                    onClick={() => {
                      edit((s) => {
                        s.capture.mode = "monitor";
                        s.capture.monitorId = d.primary ? null : d.id;
                      });
                    }}
                  >
                    <Icon name="monitor" size={20} />
                    <div className="t">{d.name}</div>
                    <div className="muted small">
                      {d.width}×{d.height} · {Math.round(d.refreshHz)} Hz
                      {d.primary ? " · primary" : ""}
                    </div>
                  </button>
                );
              })}
            </div>
            <p className="muted small">
              You can switch to a specific window or region later under Recording.
            </p>
          </>
        )}

        {step === 1 && (
          <>
            <h2>Audio</h2>
            <p className="muted">
              Tick every device you want in your clips. Each one is recorded on its own track, so
              you can rebalance them later. Microphones stay off unless you tick them.
            </p>
            <AudioDeviceChecklist
              audio={draft.audio}
              devices={devices}
              onEdit={(fn) => {
                edit((s) => {
                  fn(s.audio);
                });
              }}
            />
            <div className="row">
              <span className="muted small">Saved clips get</span>
              <Badge tone="accent">{trackSummary(draft.audio)}</Badge>
            </div>
          </>
        )}

        {step === 2 && (
          <>
            <h2>How much to keep</h2>
            <p className="muted">
              Save clip keeps this much of what just happened — it is the length of your clips.
            </p>
            <div className="row wrap" style={{ margin: "16px 0" }}>
              {LENGTHS.map((s) => (
                <Button
                  key={s}
                  variant={draft.capture.bufferSeconds === s ? "primary" : undefined}
                  onClick={() => {
                    edit((d) => {
                      d.capture.bufferSeconds = s;
                    });
                  }}
                >
                  {formatLength(s)}
                </Button>
              ))}
            </div>
            <div className="card" style={{ padding: "0 14px" }}>
              <Field label="Quality">
                <Select
                  value={draft.capture.quality}
                  options={QUALITY_OPTIONS.filter((o) => o.value !== "custom")}
                  onChange={(v) => {
                    edit((d) => {
                      d.capture.quality = v;
                    });
                  }}
                />
              </Field>
            </div>
          </>
        )}

        {step === 3 && (
          <>
            <h2>Save clip</h2>
            <p className="muted">
              Press this anytime — even in a game — to save everything in the buffer as a clip (the
              last {formatLength(draft.capture.bufferSeconds)}, chosen in the previous step).
            </p>
            <div className="card" style={{ padding: "0 14px", margin: "14px 0" }}>
              <Field label="Save clip">
                <HotkeyRecorder
                  value={draft.hotkeys.saveClip}
                  onChange={(a) => {
                    edit((d) => {
                      d.hotkeys.saveClip = a;
                    });
                  }}
                />
              </Field>
            </div>
            <p className="muted small">
              More shortcuts (recording, pause, …) are on the Hotkeys page.
            </p>
          </>
        )}

        <div className="row" style={{ marginTop: 20 }}>
          {redo && (
            <Button
              variant="ghost"
              onClick={() => {
                setRedo(false);
              }}
            >
              Cancel
            </Button>
          )}
          {step > 0 && (
            <Button
              variant="ghost"
              onClick={() => {
                setStep(step - 1);
              }}
            >
              Back
            </Button>
          )}
          <span className="spacer" />
          {step < STEPS.length - 1 ? (
            <Button
              variant="primary"
              onClick={() => {
                setStep(step + 1);
              }}
            >
              Continue
            </Button>
          ) : (
            <Button variant="primary" icon="record" disabled={busy} onClick={() => void finish()}>
              {busy ? "Starting…" : "Start buffering"}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

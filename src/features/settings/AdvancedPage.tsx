import { useState } from "react";

import {
  Badge,
  Button,
  ConfirmButton,
  Field,
  NumberInput,
  Section,
  Select,
  TextInput,
  Toggle,
} from "../../components/ui";
import { LOG_LEVELS } from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import { errorMessage } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

export function AdvancedPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const info = useApp((s) => s.info);
  const caps = useApp((s) => s.caps);
  const loadCaps = useApp((s) => s.loadCaps);
  const notify = useApp((s) => s.notify);
  const setSettings = useApp((s) => s.setSettingsFromBackend);
  const fieldError = useApp((s) => s.fieldError);
  const [probing, setProbing] = useState(false);
  if (!settings) return null;
  const adv = settings.advanced;
  const g = settings.general;
  const err = (f: string) => fieldError?.field?.startsWith(f) ?? false;

  const pickCache = async () => {
    const dir = await commands.pickFolder(info?.cacheDir);
    if (dir) {
      update(
        (d) => {
          d.advanced.cacheDir = dir;
        },
        { immediate: true },
      );
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Advanced</h1>
      </div>

      <Section title="Startup & tray">
        <Field
          label="Start with Windows"
          description="Off by default. Starts minimized to the tray."
        >
          <Toggle
            value={g.startWithWindows}
            onChange={(v) => {
              update((d) => {
                d.general.startWithWindows = v;
              });
            }}
          />
        </Field>
        <Field label="Start minimized to tray">
          <Toggle
            value={g.startMinimized}
            onChange={(v) => {
              update((d) => {
                d.general.startMinimized = v;
              });
            }}
          />
        </Field>
        <Field label="Start the buffer on launch">
          <Toggle
            value={g.startBufferOnLaunch}
            onChange={(v) => {
              update((d) => {
                d.general.startBufferOnLaunch = v;
              });
            }}
          />
        </Field>
        <Field
          label="Closing the window keeps Vido in the tray"
          description="Otherwise closing quits the app."
        >
          <Toggle
            value={g.closeToTray}
            onChange={(v) => {
              update((d) => {
                d.general.closeToTray = v;
              });
            }}
          />
        </Field>
      </Section>

      <Section title="Encoding">
        <Field
          label="Keyframe interval"
          description="Clips start on a keyframe; shorter = more precise saves, slightly larger buffer."
          error={err("capture.keyframeIntervalSec")}
        >
          <NumberInput
            value={settings.capture.keyframeIntervalSec}
            min={1}
            max={5}
            suffix="s"
            onChange={(v) => {
              update((d) => {
                d.capture.keyframeIntervalSec = v;
              });
            }}
          />
        </Field>
        <Field
          label="Hardware queue depth"
          description="Frames the GPU encoder may buffer (1–8)."
          error={err("advanced.hwQueueDepth")}
        >
          <NumberInput
            value={adv.hwQueueDepth}
            min={1}
            max={8}
            onChange={(v) => {
              update((d) => {
                d.advanced.hwQueueDepth = v;
              });
            }}
          />
        </Field>
        <Field
          label="CPU encoder threads"
          description="0 = automatic."
          error={err("advanced.cpuEncoderThreads")}
        >
          <NumberInput
            value={adv.cpuEncoderThreads}
            min={0}
            max={64}
            onChange={(v) => {
              update((d) => {
                d.advanced.cpuEncoderThreads = v;
              });
            }}
          />
        </Field>
        <Field
          label="Extra encoder options"
          description={
            <>
              FFmpeg options for the buffer encoder, e.g.{" "}
              <span className="mono">spatial_aq=1; rc-lookahead=8</span>
            </>
          }
          error={err("advanced.encoderOptions")}
        >
          <TextInput
            value={adv.encoderOptions}
            mono
            placeholder="key=value; key=value"
            onCommit={(v) => {
              update(
                (d) => {
                  d.advanced.encoderOptions = v;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field
          label="Encoders on this PC"
          description={
            caps ? `FFmpeg ${caps.ffmpegVersion} · ${caps.gpus.join(", ")}` : "Detecting…"
          }
        >
          <div className="row wrap" style={{ justifyContent: "flex-end", maxWidth: 420 }}>
            {caps?.encoders.map((e) => (
              <Badge key={e.id} tone={e.available ? "ok" : undefined}>
                {e.id}
              </Badge>
            ))}
          </div>
          <Button
            small
            icon="refresh"
            disabled={probing}
            onClick={() => {
              setProbing(true);
              void loadCaps(true).finally(() => {
                setProbing(false);
              });
            }}
          >
            {probing ? "Testing…" : "Re-test"}
          </Button>
        </Field>
      </Section>

      <Section title="Capture">
        <Field
          label="Use Desktop Duplication for displays"
          description="Fallback API; the mouse cursor is not captured with it."
        >
          <Toggle
            value={adv.preferDesktopDuplication}
            onChange={(v) => {
              update((d) => {
                d.advanced.preferDesktopDuplication = v;
              });
            }}
          />
        </Field>
        <Field
          label="Warn about black frames"
          description="Detects exclusive-fullscreen games that cannot be captured."
        >
          <Toggle
            value={adv.detectBlackFrames}
            onChange={(v) => {
              update((d) => {
                d.advanced.detectBlackFrames = v;
              });
            }}
          />
        </Field>
      </Section>

      <Section title="Storage & logs">
        <Field
          label="Cache folder"
          description={<span className="mono">{info?.cacheDir}</span>}
          error={err("advanced.cacheDir")}
        >
          <Button small icon="folder" onClick={() => void pickCache()}>
            Change…
          </Button>
          {adv.cacheDir !== null && (
            <Button
              small
              variant="ghost"
              onClick={() => {
                update(
                  (d) => {
                    d.advanced.cacheDir = null;
                  },
                  { immediate: true },
                );
              }}
            >
              Use default
            </Button>
          )}
          <Button
            small
            variant="ghost"
            icon="external"
            title="Open cache folder"
            onClick={() => void commands.openFolder("cache")}
          />
        </Field>
        <Field label="Log level" description="Logs rotate at 5 MB (5 files kept).">
          <Select
            value={adv.logLevel}
            options={LOG_LEVELS}
            onChange={(v) => {
              update((d) => {
                d.advanced.logLevel = v;
              });
            }}
          />
          <Button small icon="folder" onClick={() => void commands.openFolder("logs")}>
            Open logs
          </Button>
        </Field>
        <Field label="Reset all settings" description="Restores defaults. Clips are not touched.">
          <ConfirmButton
            small
            icon="refresh"
            onConfirm={() => {
              commands.resetSettings().then(
                (s) => {
                  setSettings(s);
                  notify("info", "Settings reset");
                },
                (e: unknown) => {
                  notify("error", "Reset failed", errorMessage(e));
                },
              );
            }}
          >
            Reset
          </ConfirmButton>
        </Field>
      </Section>

      {info && (
        <Section title="About">
          <Field label="Version">
            <span className="mono">
              Vido {info.version} · FFmpeg {info.ffmpegVersion}
              {info.portable ? " · portable" : ""}
            </span>
          </Field>
          <Field label="Settings file">
            <span
              className="mono small ellipsis"
              style={{ maxWidth: 420 }}
              title={info.settingsFile}
            >
              {info.settingsFile}
            </span>
          </Field>
          <Field label="Privacy">
            <span className="muted">
              No accounts, no uploads, no telemetry, no automatic updates.
            </span>
          </Field>
        </Section>
      )}
    </div>
  );
}

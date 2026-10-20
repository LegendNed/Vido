import { useCallback, useEffect, useState } from "react";

import { HotkeyRecorder } from "../../components/HotkeyRecorder";
import { Badge, Banner, Button, Field, Section } from "../../components/ui";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import type { HotkeyAction, HotkeySettings, HotkeyStatus } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

const ACTIONS: { action: HotkeyAction; key: keyof HotkeySettings; label: string; desc?: string }[] =
  [
    {
      action: "saveClip",
      key: "saveClip",
      label: "Save clip",
      desc: "Saves everything in the buffer — set its length under Recording.",
    },
    {
      action: "toggleRecording",
      key: "toggleRecording",
      label: "Start / stop recording",
      desc: "Manual recording to a file.",
    },
    { action: "togglePause", key: "togglePause", label: "Pause / resume buffer" },
    {
      action: "openLastClip",
      key: "openLastClip",
      label: "Open last clip",
      desc: "In your default video player.",
    },
    {
      action: "openEditor",
      key: "openEditor",
      label: "Open editor",
      desc: "Edits the most recent clip.",
    },
  ];

const DEFAULTS: HotkeySettings = {
  saveClip: "Ctrl+Shift+F6",
  toggleRecording: "Ctrl+Shift+F9",
  togglePause: "Ctrl+Shift+F10",
  openLastClip: "Ctrl+Shift+F11",
  openEditor: "Ctrl+Shift+F12",
};

export function HotkeysPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const fieldError = useApp((s) => s.fieldError);
  const [statuses, setStatuses] = useState<HotkeyStatus[]>([]);

  const refresh = useCallback(() => {
    void commands.getHotkeyStatus().then(setStatuses);
  }, []);
  useEffect(refresh, [refresh]);
  useEvent("settings-changed", refresh);

  if (!settings) return null;
  const h = settings.hotkeys;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Hotkeys</h1>
        <Button
          small
          onClick={() => {
            update(
              (d) => {
                d.hotkeys = { ...DEFAULTS };
              },
              { immediate: true },
            );
          }}
        >
          Restore defaults
        </Button>
      </div>
      <Banner tone="info">
        Hotkeys work while games are focused. If a game runs as administrator, run Vido as
        administrator too — Windows blocks shortcuts from reaching more privileged apps.
      </Banner>
      <Section
        title="Shortcuts"
        description="Click a shortcut, then press the new key combination."
      >
        {ACTIONS.map((a) => {
          const value = h[a.key];
          const st = statuses.find((s) => s.action === a.action);
          const isErr = fieldError?.field === `hotkeys.${a.key}`;
          return (
            <Field
              key={a.action}
              label={a.label}
              description={isErr ? fieldError.message : (st?.error ?? a.desc)}
              error={isErr || Boolean(st?.error)}
            >
              {st &&
                value &&
                (st.registered ? (
                  <Badge tone="ok">Active</Badge>
                ) : (
                  <Badge tone="err">Not active</Badge>
                ))}
              <HotkeyRecorder
                value={value}
                onChange={(accel) => {
                  update(
                    (d) => {
                      d.hotkeys[a.key] = accel;
                    },
                    { immediate: true },
                  );
                }}
              />
            </Field>
          );
        })}
      </Section>
    </div>
  );
}

import { useCallback, useEffect, useState } from "react";

import { HotkeyRecorder } from "../../components/HotkeyRecorder";
import { Badge, Banner, Button, Field, NumberInput, Section, Toggle } from "../../components/ui";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import type { HotkeyAction, HotkeySettings, HotkeyStatus } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

type ShortcutKey = Exclude<keyof HotkeySettings, "customSeconds">;

const ACTIONS: { action: HotkeyAction; key: ShortcutKey; label: string; desc?: string }[] = [
  { action: "save15", key: "save15", label: "Save last 15 seconds" },
  { action: "save30", key: "save30", label: "Save last 30 seconds" },
  { action: "save60", key: "save60", label: "Save last 60 seconds" },
  {
    action: "saveCustom",
    key: "saveCustom",
    label: "Save custom length",
    desc: "Length is set below.",
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
  const defaults: HotkeySettings = {
    save15: "Ctrl+Shift+F5",
    save30: "Ctrl+Shift+F6",
    save60: "Ctrl+Shift+F7",
    saveCustom: "Ctrl+Shift+F8",
    customSeconds: h.customSeconds,
    toggleRecording: "Ctrl+Shift+F9",
    togglePause: "Ctrl+Shift+F10",
    openLastClip: "Ctrl+Shift+F11",
    openEditor: "Ctrl+Shift+F12",
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Hotkeys</h1>
        <Button
          small
          onClick={() => {
            update(
              (d) => {
                d.hotkeys = defaults;
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
      <Section title="Custom length">
        <Field
          label="Save the whole buffer"
          description="Otherwise save a fixed number of seconds."
        >
          <Toggle
            value={h.customSeconds === 0}
            onChange={(v) => {
              update((d) => {
                d.hotkeys.customSeconds = v ? 0 : 90;
              });
            }}
          />
        </Field>
        {h.customSeconds !== 0 && (
          <Field label="Seconds" description="5–600. Limited by the buffer length.">
            <NumberInput
              value={h.customSeconds}
              min={5}
              max={600}
              suffix="s"
              onChange={(v) => {
                update((d) => {
                  d.hotkeys.customSeconds = v;
                });
              }}
            />
          </Field>
        )}
      </Section>
    </div>
  );
}

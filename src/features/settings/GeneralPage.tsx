import { useState } from "react";

import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Button, Field, Section, Segmented, Toggle } from "../../components/controls";
import { SIDEBAR_OPTIONS } from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import { errorMessage } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

/** Settings most people touch: appearance, the setup wizard, startup and tray. */
export function GeneralPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const setRedoSetup = useApp((s) => s.setRedoSetup);
  const setSettings = useApp((s) => s.setSettingsFromBackend);
  const notify = useApp((s) => s.notify);
  const [confirmReset, setConfirmReset] = useState(false);
  if (!settings) return null;
  const g = settings.general;

  return (
    <div className="page">
      <div className="page-head">
        <h1>General</h1>
      </div>
      <Section
        title="Appearance"
        keywords="theme dark light mode accent color navigation tabs"
        description="Light or dark follows Windows (Settings → Personalization → Colors)."
      >
        <Field
          label="Sidebar"
          description="Full shows names next to the icons, Icons only is narrow, Hidden leaves just the page (the button at the top left brings it back)."
        >
          <Segmented
            value={g.sidebar}
            options={SIDEBAR_OPTIONS}
            onChange={(v) => {
              update(
                (d) => {
                  d.general.sidebar = v;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
      </Section>

      <Section title="Setup" keywords="wizard first run welcome">
        <Field
          label="Run setup again"
          description="Walks through display, audio, clip length and the save hotkey again. Your current settings are the starting point; Cancel keeps them."
        >
          <Button
            icon="refresh"
            onClick={() => {
              setRedoSetup(true);
            }}
          >
            Run setup
          </Button>
        </Field>
        <Field
          label="Reset settings and run setup"
          description="Puts every setting back to its default (recommended quality for this PC included), then walks through setup. Clips are not touched."
        >
          <Button
            icon="refresh"
            onClick={() => {
              setConfirmReset(true);
            }}
          >
            Reset & run setup
          </Button>
        </Field>
        {confirmReset && (
          <ConfirmDialog
            title="Reset all settings?"
            confirmLabel="Reset"
            onConfirm={() => {
              setConfirmReset(false);
              commands.resetSettings().then(
                (s) => {
                  setSettings(s);
                  setRedoSetup("fresh");
                },
                (e: unknown) => {
                  notify("error", "Could not reset settings", errorMessage(e));
                },
              );
            }}
            onCancel={() => {
              setConfirmReset(false);
            }}
          >
            Every setting goes back to its default, then setup starts. Your clips stay where they
            are.
          </ConfirmDialog>
        )}
      </Section>

      <Section title="Startup & tray" keywords="autostart boot minimize close quit">
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
    </div>
  );
}

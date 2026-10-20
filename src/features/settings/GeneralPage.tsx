import { Button, Field, Section, Segmented, Toggle } from "../../components/ui";
import { SIDEBAR_OPTIONS } from "../../lib/options";
import { useApp } from "../../stores/app";

/** Settings most people touch: appearance, the setup wizard, startup and tray. */
export function GeneralPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const setRedoSetup = useApp((s) => s.setRedoSetup);
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
        description="Light or dark mode and the accent color follow Windows (Settings → Personalization → Colors)."
      >
        <Field
          label="Sidebar"
          description="Full shows names under the icons, Small only icons, None moves the pages into tabs above the content."
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

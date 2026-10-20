import { Field, Section, Select, Toggle } from "../../components/ui";
import { HUD_POSITIONS } from "../../lib/options";
import { useApp } from "../../stores/app";

/** The on-screen recording indicator (HUD). */
export function OverlayPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  if (!settings) return null;
  const hud = settings.general.hud;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Overlay</h1>
      </div>
      <Section
        title="Recording indicator"
        description="A small overlay that is hidden from your recordings."
        keywords="hud"
      >
        <Field label="Show overlay">
          <Toggle
            value={hud.enabled}
            onChange={(v) => {
              update((d) => {
                d.general.hud.enabled = v;
              });
            }}
          />
        </Field>
        <Field label="Position" keywords="corner">
          <Select
            value={hud.position}
            options={HUD_POSITIONS}
            onChange={(v) => {
              update((d) => {
                d.general.hud.position = v;
              });
            }}
          />
        </Field>
        <Field label="Show buffer details">
          <Toggle
            value={hud.showDetails}
            onChange={(v) => {
              update((d) => {
                d.general.hud.showDetails = v;
              });
            }}
          />
        </Field>
      </Section>
    </div>
  );
}

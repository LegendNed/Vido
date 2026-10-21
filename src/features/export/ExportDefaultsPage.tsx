import { Section } from "../../components/controls";
import { useApp } from "../../stores/app";
import { EncodeOptions } from "./EncodeOptions";

export function ExportDefaultsPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  if (!settings) return null;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Export & compression</h1>
      </div>
      <p className="muted" style={{ margin: 0 }}>
        What the editor's Export panel starts with. Original copies the video when you only trim
        (lossless and instant).
      </p>
      <Section
        title="Default export"
        description="Pick a preset, or fine-tune under Advanced."
        keywords="compression preset target size codec encoder bitrate quality resolution frame rate container"
      >
        <EncodeOptions
          value={settings.export}
          onChange={(fn) => {
            update((d) => {
              d.export = fn(d.export);
            });
          }}
          scope="defaults"
          errorPrefix="export"
        />
      </Section>
    </div>
  );
}

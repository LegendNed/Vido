import { Icon } from "../../components/Icon";
import { Button, Segmented } from "../../components/controls";
import type { CropRect } from "../../lib/tauri/types";

const ASPECTS: { label: string; value: number | null }[] = [
  { label: "Free", value: null },
  { label: "16:9", value: 16 / 9 },
  { label: "9:16", value: 9 / 16 },
  { label: "1:1", value: 1 },
  { label: "4:3", value: 4 / 3 },
];

/** Shown under the video while cropping: aspect presets, the size, Reset and Done. */
export function CropBar({
  crop,
  width,
  height,
  aspect,
  onAspect,
  onReset,
  onDone,
}: {
  /** `null` = the whole frame. */
  crop: CropRect | null;
  width: number;
  height: number;
  aspect: number | null;
  onAspect: (aspect: number | null) => void;
  onReset: () => void;
  onDone: () => void;
}) {
  const w = crop?.width ?? width;
  const h = crop?.height ?? height;
  return (
    <div className="crop-bar" role="toolbar" aria-label="Crop">
      <span className="crop-bar-title">
        <Icon name="crop" size={14} />
        Crop
      </span>
      <Segmented<string>
        value={ASPECTS.find((a) => a.value === aspect)?.label ?? "Free"}
        options={ASPECTS.map((a) => ({ value: a.label, label: a.label }))}
        onChange={(label) => {
          onAspect(ASPECTS.find((a) => a.label === label)?.value ?? null);
        }}
      />
      <span className="small muted tabular">
        {w}×{h}
      </span>
      <span className="spacer" />
      <Button small disabled={!crop} title="Use the whole frame" onClick={onReset}>
        Reset
      </Button>
      <Button small variant="primary" icon="check" title="Done (Enter)" onClick={onDone}>
        Done
      </Button>
    </div>
  );
}

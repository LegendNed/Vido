import { Icon } from "../../components/Icon";
import { Button } from "../../components/controls";

/** Shown under the video while placing blur areas: Add, Remove, the count and Done. */
export function BlurBar({
  count,
  selected,
  max,
  onAdd,
  onRemove,
  onDone,
}: {
  count: number;
  /** Whether an area is selected (Remove applies to it). */
  selected: boolean;
  max: number;
  onAdd: () => void;
  onRemove: () => void;
  onDone: () => void;
}) {
  return (
    <div className="crop-bar" role="toolbar" aria-label="Blur">
      <span className="crop-bar-title">
        <Icon name="blur" size={14} />
        Blur
      </span>
      <Button small icon="plus" disabled={count >= max} onClick={onAdd}>
        Add area
      </Button>
      <Button
        small
        disabled={!selected}
        title="Remove the selected area (Delete)"
        onClick={onRemove}
      >
        Remove
      </Button>
      <span className="small muted">
        {count === 0
          ? "Hide names, chat or anything private"
          : `${count} area${count === 1 ? "" : "s"} · click one to edit it`}
      </span>
      <span className="spacer" />
      <Button small variant="primary" icon="check" title="Done (Enter)" onClick={onDone}>
        Done
      </Button>
    </div>
  );
}

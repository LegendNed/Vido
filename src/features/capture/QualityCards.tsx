import { Icon } from "../../components/Icon";
import type { CaptureSettings, QualityTier } from "../../lib/tauri/types";
import {
  activePreset,
  RECORDING_PRESETS,
  SCALE_LABEL,
  type RecordingPreset,
} from "./recordingPresets";

/**
 * Cards for the recording quality (plus Custom). `nativeSize` labels the Native card
 * with the screen's resolution; `recommended` marks the card suggested for this PC.
 */
export function QualityCards({
  value,
  onPick,
  onCustom,
  nativeSize,
  recommended,
}: {
  value: CaptureSettings;
  onPick: (p: RecordingPreset) => void;
  /** Custom: e.g. reveal the detailed options. Omit to hide the Custom card. */
  onCustom?: () => void;
  nativeSize?: string;
  recommended?: QualityTier | null;
}) {
  const active = activePreset(value);
  return (
    <div className="quality-cards" role="radiogroup" aria-label="Recording quality">
      {RECORDING_PRESETS.map((p) => {
        const on = active === p.id;
        const res = p.scale === "native" ? (nativeSize ?? "Native") : SCALE_LABEL[p.scale];
        return (
          <button
            type="button"
            key={p.id}
            role="radio"
            aria-checked={on}
            className={`quality-card${on ? " on" : ""}`}
            onClick={() => {
              onPick(p);
            }}
          >
            <span className="qc-title">
              {p.label}
              {on && <Icon name="check" size={14} />}
            </span>
            <span className="qc-hint">{p.hint}</span>
            <span className="qc-spec">
              {res} {p.fps} FPS · {p.kbps === null ? "auto bitrate" : `${p.kbps / 1000} Mbps`}
            </span>
            {recommended === p.id && <span className="qc-badge">Recommended</span>}
          </button>
        );
      })}
      {onCustom && (
        <button
          type="button"
          role="radio"
          aria-checked={active === null}
          className={`quality-card${active === null ? " on" : ""}`}
          onClick={onCustom}
        >
          <span className="qc-title">
            Custom
            {active === null && <Icon name="check" size={14} />}
          </span>
          <span className="qc-hint">Your own resolution, frame rate and bitrate</span>
        </button>
      )}
    </div>
  );
}

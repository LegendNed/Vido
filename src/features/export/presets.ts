import presetData from "../../../config/export-presets.json";
import type { ExportDefaults } from "../../lib/tauri/types";

/**
 * Encode options as edited by `EncodeOptions`: the export-defaults shape, plus
 * `streamCopy` where copying the video is possible (the editor's export).
 */
export type EncodeValues = Omit<ExportDefaults, "streamCopy"> & { streamCopy?: boolean };

export interface Preset {
  id: string;
  label: string;
  /** One line shown under the chips while the preset is selected. */
  hint: string;
  /** Only offered where the video can be copied (`streamCopy`). */
  needsCopy?: boolean;
  apply: <T extends EncodeValues>(v: T) => T;
}

/** Sets `values` (and `streamCopy`, where the value has it). */
function set<T extends EncodeValues>(
  v: T,
  values: Partial<ExportDefaults>,
  streamCopy: boolean,
): T {
  return { ...v, ...values, ...(v.streamCopy === undefined ? {} : { streamCopy }) };
}

/** A preset as stored in `config/export-presets.json`. */
interface PresetData {
  id: string;
  label: string;
  hint: string;
  /** "Original": copies the video when it can (only offered where that is possible). */
  copy?: boolean;
  /** Every quality-related option it sets (not `encoder` or `mixAudio`). */
  values: Partial<ExportDefaults>;
}

/**
 * Basic choices for people who don't want to think about codecs
 * (`config/export-presets.json`). Each preset sets every quality-related option, so
 * changing one of them under Advanced shows "Custom". "High quality" matches
 * `ExportDefaults::high_quality()` in Rust (the re-encode-on-save default; a Rust
 * test checks this).
 */
export const PRESETS: Preset[] = (presetData as PresetData[]).map((p) => ({
  id: p.id,
  label: p.label,
  hint: p.hint,
  needsCopy: p.copy,
  apply: (v) =>
    p.copy
      ? // Copying keeps the encode options, but target size and WebM can't copy.
        set(
          v,
          {
            ...p.values,
            rateMode: v.rateMode === "targetSize" ? "quality" : v.rateMode,
            container: v.container === "webm" ? "mp4" : v.container,
          },
          true,
        )
      : set(v, p.values, false),
}));

/** Presets offered for `v` (no "Original" where nothing can be copied). */
export function presetsFor(v: EncodeValues): Preset[] {
  return v.streamCopy === undefined ? PRESETS.filter((p) => !p.needsCopy) : PRESETS;
}

/** The preset `v` is set to, or `null` for custom settings. */
export function activePreset(v: EncodeValues): Preset | null {
  const same = (a: EncodeValues, b: EncodeValues) =>
    (Object.keys(a) as (keyof EncodeValues)[]).every((k) => a[k] === b[k]);
  return presetsFor(v).find((p) => same(p.apply(v), v)) ?? null;
}

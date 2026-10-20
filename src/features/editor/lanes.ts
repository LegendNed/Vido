// Audio lanes as the editor shows and previews them (export semantics live in model.ts).

import { editableLanes, isAudible, lanesChanged, UNITY, type LaneSet } from "./model";

/**
 * Volume for the preview, which can only play the clip's first audio track: the
 * recorded mix follows the loudest audible lane (silent when all are muted); a
 * plain first track follows its own lane. Values above 1 are capped by the caller.
 */
export function previewGain(s: LaneSet): number {
  if (s.lanes.length === 0) return 1;
  if (s.recordedMix) {
    if (!lanesChanged(s)) return 1;
    const loud = editableLanes(s)
      .map((i) => s.lanes[i] ?? UNITY)
      .filter(isAudible);
    return loud.length > 0 ? Math.max(...loud.map((l) => l.volume)) : 0;
  }
  const first = s.lanes[0] ?? UNITY;
  return isAudible(first) ? first.volume : 0;
}

/** What the preview cannot play as exported, if anything. */
export function previewNote(s: LaneSet): string | null {
  if (s.lanes.length < 2) return null;
  if (!s.recordedMix)
    return "The preview plays the first track only; export and save use all lanes.";
  return lanesChanged(s)
    ? "The preview plays the recorded mix; export and save use your lane changes."
    : null;
}

/**
 * Rough waveform of the editable lanes mixed (peaks scaled by volume and summed
 * per bucket), or `null` while one of them is still loading.
 */
export function mixWave(waves: (number[] | null)[], s: LaneSet): number[] | null {
  const parts: { w: number[]; v: number }[] = [];
  for (const i of editableLanes(s)) {
    const l = s.lanes[i] ?? UNITY;
    if (!isAudible(l)) continue;
    const w = waves[i];
    if (!w) return null;
    parts.push({ w, v: l.volume });
  }
  const n = parts[0]?.w.length ?? waves[0]?.length ?? 0;
  const out = new Array<number>(n).fill(0);
  for (const p of parts) {
    for (let k = 0; k < n; k++) out[k] = (out[k] ?? 0) + (p.w[k] ?? 0) * p.v;
  }
  return out;
}

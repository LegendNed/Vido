// How the Clips page orders, groups and sizes its grid (remembered per viewer).

import type { ClipInfo } from "../../lib/tauri/types";

export type ClipSort = "newest" | "oldest" | "longest" | "shortest" | "largest" | "name";
export type GridSize = "small" | "medium" | "large";

export interface ClipView {
  sort: ClipSort;
  groupByDate: boolean;
  size: GridSize;
}

export const SORT_OPTIONS: { value: ClipSort; label: string }[] = [
  { value: "newest", label: "Newest first" },
  { value: "oldest", label: "Oldest first" },
  { value: "longest", label: "Longest" },
  { value: "shortest", label: "Shortest" },
  { value: "largest", label: "Largest file" },
  { value: "name", label: "Name" },
];

export const SIZE_OPTIONS: { value: GridSize; label: string }[] = [
  { value: "small", label: "S" },
  { value: "medium", label: "M" },
  { value: "large", label: "L" },
];

const KEY = "vido.clips.view";
const DEFAULT_VIEW: ClipView = { sort: "newest", groupByDate: false, size: "medium" };

export function loadView(): ClipView {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { ...DEFAULT_VIEW, ...(JSON.parse(raw) as Partial<ClipView>) };
  } catch {
    // Storage unavailable or garbage: defaults.
  }
  return DEFAULT_VIEW;
}

export function saveView(v: ClipView) {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch {
    // Not remembered; fine.
  }
}

export function sortClips(clips: ClipInfo[], sort: ClipSort): ClipInfo[] {
  const by: Record<ClipSort, (a: ClipInfo, b: ClipInfo) => number> = {
    newest: (a, b) => b.modifiedMs - a.modifiedMs,
    oldest: (a, b) => a.modifiedMs - b.modifiedMs,
    longest: (a, b) => b.durationSec - a.durationSec,
    shortest: (a, b) => a.durationSec - b.durationSec,
    largest: (a, b) => b.sizeBytes - a.sizeBytes,
    name: (a, b) => a.fileName.localeCompare(b.fileName, undefined, { numeric: true }),
  };
  return [...clips].sort(by[sort]);
}

/** Date heading for a clip: Today, Yesterday, This week, This month, or "October 2026". */
export function dateGroup(ms: number, now = new Date()): string {
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const today = day(now);
  const d = new Date(ms);
  const diffDays = Math.round((today - day(d)) / 86_400_000);
  if (diffDays <= 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  if (diffDays < 7) return "This week";
  if (d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth()) return "This month";
  return d.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

/** Consecutive runs of clips with the same heading (keeps the current sort order). */
export function groupClips(
  clips: ClipInfo[],
  byDate: boolean,
): { title: string | null; clips: ClipInfo[] }[] {
  if (!byDate) return [{ title: null, clips }];
  const out: { title: string; clips: ClipInfo[] }[] = [];
  for (const c of clips) {
    const title = dateGroup(c.modifiedMs);
    const last = out[out.length - 1];
    if (last?.title === title) last.clips.push(c);
    else out.push({ title, clips: [c] });
  }
  return out;
}

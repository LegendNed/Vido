// How the Clips page orders, groups and sizes its grid (remembered per viewer).

import type { ClipInfo } from "../../lib/tauri/types";

export type ClipSort = "newest" | "oldest" | "longest" | "shortest" | "largest" | "name";

export type ClipGrouping = "date" | "game" | "none";

export interface ClipView {
  sort: ClipSort;
  group: ClipGrouping;
  /** Grid size step, 1 (smallest cards) to `GRID_STEPS`. */
  size: number;
}

export const GROUP_OPTIONS: { value: ClipGrouping; label: string }[] = [
  { value: "date", label: "Group by date" },
  { value: "game", label: "Group by game" },
  { value: "none", label: "No grouping" },
];

export const SORT_OPTIONS: { value: ClipSort; label: string }[] = [
  { value: "newest", label: "Newest first" },
  { value: "oldest", label: "Oldest first" },
  { value: "longest", label: "Longest" },
  { value: "shortest", label: "Shortest" },
  { value: "largest", label: "Largest file" },
  { value: "name", label: "Name" },
];

export const GRID_STEPS = 8;

/** Minimum card width (px) for each grid size step. */
const CARD_WIDTHS = [190, 220, 250, 280, 320, 360, 400, 450];

export function cardWidth(step: number): number {
  return CARD_WIDTHS[Math.min(Math.max(Math.round(step), 1), GRID_STEPS) - 1] ?? 250;
}

// v2: grouping by date became the default.
const KEY = "vido.clips.view.v2";
const DEFAULT_VIEW: ClipView = { sort: "newest", group: "date", size: 3 };
/** Sizes saved before the slider (S / M / L). */
const OLD_SIZES: Record<string, number> = { small: 1, medium: 3, large: 7 };

export function loadView(): ClipView {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const saved = JSON.parse(raw) as Partial<ClipView> & { groupByDate?: boolean };
      const v = { ...DEFAULT_VIEW, ...saved };
      // Saved before grouping by game: a checkbox.
      if (saved.group === undefined && saved.groupByDate === false) v.group = "none";
      if (!GROUP_OPTIONS.some((o) => o.value === v.group)) v.group = DEFAULT_VIEW.group;
      const size: unknown = v.size;
      if (typeof size === "string") v.size = OLD_SIZES[size] ?? DEFAULT_VIEW.size;
      return v;
    }
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

/**
 * Date heading for a clip: This week (the last 7 days, so a few recent clips share
 * one row), This month, or "October 2026". Cards still show "Today 19:23".
 */
export function dateGroup(ms: number, now = new Date()): string {
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const today = day(now);
  const d = new Date(ms);
  const diffDays = Math.round((today - day(d)) / 86_400_000);
  if (diffDays < 7) return "This week";
  if (d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth()) return "This month";
  return d.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

/** Heading for clips that are not in a game's folder. */
const NO_GAME = "Other clips";

/**
 * Clips under headings. By date: newest heading first. By game: in the order the
 * current sort first shows each game, clips without one last. Within a group the
 * clips keep the current sort order.
 */
export function groupClips(
  clips: ClipInfo[],
  by: ClipGrouping,
  root: string | undefined,
): { title: string | null; clips: ClipInfo[] }[] {
  if (by === "none") return [{ title: null, clips }];
  if (by === "game") {
    const games = new Map<string, ClipInfo[]>();
    for (const c of clips) {
      const title = clipGame(c.path, root) ?? NO_GAME;
      const g = games.get(title);
      if (g) g.push(c);
      else games.set(title, [c]);
    }
    const other = games.get(NO_GAME);
    games.delete(NO_GAME);
    if (other) games.set(NO_GAME, other);
    return [...games.entries()].map(([title, g]) => ({ title, clips: g }));
  }
  const groups = new Map<string, { clips: ClipInfo[]; newest: number }>();
  for (const c of clips) {
    const title = dateGroup(c.modifiedMs);
    const g = groups.get(title);
    if (g) {
      g.clips.push(c);
      g.newest = Math.max(g.newest, c.modifiedMs);
    } else groups.set(title, { clips: [c], newest: c.modifiedMs });
  }
  return [...groups.entries()]
    .sort((a, b) => b[1].newest - a[1].newest)
    .map(([title, g]) => ({ title, clips: g.clips }));
}

/**
 * The game a clip belongs to: the folder it is in under the clips folder
 * (`<clips folder>\<game>\clip.mp4`), or `null` for clips in the clips folder itself.
 */
export function clipGame(path: string, root: string | undefined): string | null {
  if (!root) return null;
  const p = path.replace(/\//g, "\\");
  const r = `${root.replace(/\//g, "\\").replace(/\\+$/, "")}\\`;
  if (!p.toLowerCase().startsWith(r.toLowerCase())) return null;
  const rest = p.slice(r.length);
  const i = rest.indexOf("\\");
  return i > 0 ? rest.slice(0, i) : null;
}

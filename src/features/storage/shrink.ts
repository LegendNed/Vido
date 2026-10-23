// "Shrink old clips": re-encodes a clip in place (Save & override) with a smaller
// encoding, keeping every audio track. The original goes to the Recycle Bin.

import { formatBytes } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { on } from "../../lib/tauri/events";
import {
  errorMessage,
  type AppError,
  type ClipInfo,
  type ExportDefaults,
  type ExportResult,
} from "../../lib/tauri/types";
import { buildRequest, initialState } from "../editor/model";
import { PRESETS } from "../export/presets";

export type ShrinkLevel = "small" | "compatible";

export const SHRINK_LEVELS: { value: ShrinkLevel; label: string; hint: string }[] = [
  {
    value: "small",
    label: "Smallest (HEVC)",
    hint: "HEVC at 1080p: about a quarter of the size. Some apps can't play HEVC.",
  },
  {
    value: "compatible",
    label: "Compatible (H.264)",
    hint: "H.264 at 1080p: plays everywhere, about half the size.",
  },
];

/** Clips already at or below this bitrate (kbps) are left alone. */
const ALREADY_SMALL_KBPS: Record<ShrinkLevel, number> = { small: 5000, compatible: 8000 };

/** Average bitrate of a clip in kbps. */
export const clipKbps = (c: ClipInfo) =>
  c.durationSec > 0 ? (c.sizeBytes * 8) / c.durationSec / 1000 : 0;

/** Whether shrinking `c` at `level` is likely to save space. */
export const worthShrinking = (c: ClipInfo, level: ShrinkLevel) =>
  c.durationSec > 0 && clipKbps(c) > ALREADY_SMALL_KBPS[level];

function encoding(defaults: ExportDefaults, level: ShrinkLevel): ExportDefaults {
  const small = PRESETS.find((p) => p.id === "small");
  const x = small ? small.apply({ ...defaults }) : { ...defaults };
  // Every recorded audio track is kept as it is.
  const base = { ...x, mixAudio: false };
  return level === "small" ? base : { ...base, videoCodec: "h264", quality: 26 };
}

/** Runs one export job to its end; `onProgress` gets 0–100. */
async function runJob(
  start: () => Promise<string>,
  onProgress: (pct: number) => void,
  onJob: (id: string) => void,
): Promise<{ ok: true; r: ExportResult } | { ok: false; e: AppError }> {
  // Events can arrive before `start` returns the job id.
  const done = new Map<string, { ok: true; r: ExportResult } | { ok: false; e: AppError }>();
  let id: string | null = null;
  let settle: ((o: { ok: true; r: ExportResult } | { ok: false; e: AppError }) => void) | null =
    null;
  const finished = (
    jobId: string,
    o: { ok: true; r: ExportResult } | { ok: false; e: AppError },
  ) => {
    if (jobId === id && settle) settle(o);
    else done.set(jobId, o);
  };
  const offs = await Promise.all([
    on("export-progress", (p) => {
      if (p.jobId === id) onProgress(p.percent);
    }),
    on("export-completed", (r) => {
      finished(r.jobId, { ok: true, r });
    }),
    on("export-failed", (f) => {
      finished(f.jobId, { ok: false, e: f.error });
    }),
  ]);
  try {
    id = await start();
    onJob(id);
    const early = done.get(id);
    const o =
      early ??
      (await new Promise<{ ok: true; r: ExportResult } | { ok: false; e: AppError }>((res) => {
        settle = res;
      }));
    return o;
  } finally {
    for (const off of offs) off();
  }
}

export type ShrinkOutcome =
  | { kind: "shrunk"; saved: number }
  /** The estimate says it would barely get smaller. */
  | { kind: "skipped" }
  | { kind: "cancelled" };

/** Shrinks one clip in place (throws when the export fails). */
export async function shrinkClip(
  clip: ClipInfo,
  defaults: ExportDefaults,
  level: ShrinkLevel,
  onProgress: (pct: number) => void,
  onJob: (id: string) => void,
): Promise<ShrinkOutcome> {
  const details = await commands.getClipDetails(clip.path);
  const state = initialState(details, encoding(defaults, level));
  const request = buildRequest(clip.path, { ...state, x: { ...state.x, streamCopy: false } });
  const estimate = await commands.estimateExport(request);
  if (estimate.estimatedBytes > clip.sizeBytes * 0.85) return { kind: "skipped" };
  const o = await runJob(() => commands.saveEdit(request, "override"), onProgress, onJob);
  if (o.ok) return { kind: "shrunk", saved: Math.max(0, clip.sizeBytes - o.r.sizeBytes) };
  if (o.e.code === "CANCELLED") return { kind: "cancelled" };
  throw new Error(errorMessage(o.e));
}

export interface ShrinkRun {
  /** Index of the clip being shrunk. */
  at: number;
  total: number;
  name: string;
  /** Progress of the current clip, 0–100. */
  pct: number;
  saved: number;
  shrunk: number;
  skipped: number;
  failed: number;
  done: boolean;
  cancelled: boolean;
}

// The run outlives the Storage page (module state, read with useSyncExternalStore).
let run: ShrinkRun | null = null;
let job: { id: string | null; cancelled: boolean } | null = null;
const listeners = new Set<() => void>();

export const shrinkRun = {
  get: () => run,
  subscribe: (fn: () => void) => {
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  },
};

function show(patch: Partial<ShrinkRun>) {
  if (!run) return;
  run = { ...run, ...patch };
  for (const fn of listeners) fn();
}

/**
 * Shrinks `clips` one after another; resolves to the finished run, or `null` when
 * another run is still going.
 */
export async function runShrink(
  clips: ClipInfo[],
  defaults: ExportDefaults,
  level: ShrinkLevel,
): Promise<ShrinkRun | null> {
  if (job) return null;
  const me = { id: null as string | null, cancelled: false };
  job = me;
  run = {
    at: 0,
    total: clips.length,
    name: "",
    pct: 0,
    saved: 0,
    shrunk: 0,
    skipped: 0,
    failed: 0,
    done: false,
    cancelled: false,
  };
  show({});
  for (const [i, c] of clips.entries()) {
    if (me.cancelled) break;
    show({ at: i, name: c.fileName, pct: 0 });
    try {
      const o = await shrinkClip(
        c,
        defaults,
        level,
        (pct) => {
          show({ pct });
        },
        (id) => {
          me.id = id;
        },
      );
      if (o.kind === "cancelled") break;
      if (o.kind === "skipped") show({ skipped: run.skipped + 1 });
      else show({ saved: run.saved + o.saved, shrunk: run.shrunk + 1 });
    } catch (e) {
      console.warn(`Shrinking ${c.fileName} failed:`, e);
      show({ failed: run.failed + 1 });
    }
    me.id = null;
  }
  job = null;
  show({ done: true, cancelled: me.cancelled, pct: 100 });
  return run;
}

/** One line about a finished run, e.g. "Freed 1.2 GB from 4 clips." */
export function shrinkSummary(r: ShrinkRun): string {
  const n = (k: number) => `${k} clip${k === 1 ? "" : "s"}`;
  return [
    r.shrunk > 0 ? `Freed ${formatBytes(r.saved)} from ${n(r.shrunk)}.` : "Nothing was shrunk.",
    r.skipped > 0 ? `${n(r.skipped)} already small.` : "",
    r.failed > 0 ? `${r.failed} failed.` : "",
  ]
    .filter(Boolean)
    .join(" ");
}

/** Stops the run after cancelling the clip being shrunk (it stays as it was). */
export function cancelShrink() {
  if (!job) return;
  job.cancelled = true;
  if (job.id) void commands.cancelExport(job.id);
}

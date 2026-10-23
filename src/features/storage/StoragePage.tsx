import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Icon } from "../../components/Icon";
import { Badge, Button, Progress, Segmented, Select, Spinner } from "../../components/controls";
import { formatBytes, formatDate, formatDuration } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage, type ClipInfo, type DiskSpace } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { clipGame } from "../clips/clipView";
import { dragOut } from "../clips/media";
import {
  cancelShrink,
  runShrink,
  shrinkRun,
  SHRINK_LEVELS,
  worthShrinking,
  type ShrinkLevel,
} from "./shrink";

/** Rows in "Biggest clips". */
const BIGGEST = 8;
const DAY_MS = 24 * 60 * 60 * 1000;
const AGES = [7, 30, 90, 180].map((d) => ({ value: d, label: `Older than ${d} days` }));
const NO_GAME = "Other clips";
/** Games listed before "Show all"; the full list scrolls. */
const TOP_GAMES = 5;

/**
 * Where the disk space goes: the clips folder's drive, usage per game, the
 * biggest clips, and "Shrink old clips" (re-encode older clips in place).
 */
export function StoragePage({ openClip }: { openClip: (path: string) => void }) {
  const [clips, setClips] = useState<ClipInfo[] | null>(null);
  const [disk, setDisk] = useState<DiskSpace | null>(null);
  const [reload, setReload] = useState(0);
  // "Older than" counts from when the page was opened.
  const [now] = useState(() => Date.now());
  const [allGames, setAllGames] = useState(false);
  const [deleting, setDeleting] = useState<ClipInfo | null>(null);
  const [age, setAge] = useState(30);
  const [level, setLevel] = useState<ShrinkLevel>("small");
  const [confirmShrink, setConfirmShrink] = useState(false);
  // Keeps going (and showing) when this page is left and opened again.
  const run = useSyncExternalStore(shrinkRun.subscribe, shrinkRun.get);
  const notify = useApp((s) => s.notify);
  const info = useApp((s) => s.info);
  const defaults = useApp((s) => s.settings?.export);
  const root = info?.outputDir;

  useEffect(() => {
    let alive = true;
    commands.listClips().then(
      (c) => {
        if (alive) setClips(c);
      },
      (e: unknown) => {
        if (!alive) return;
        notify("error", "Could not list clips", errorMessage(e));
        setClips([]);
      },
    );
    commands.getDiskSpace().then(
      (d) => {
        if (alive) setDisk(d);
      },
      () => {
        if (alive) setDisk(null);
      },
    );
    return () => {
      alive = false;
    };
  }, [reload, notify, run?.done]); // also when a shrink run ends
  const load = () => {
    setReload((n) => n + 1);
  };
  useEvent("clips-changed", () => {
    // A shrink run replaces clips one by one; refresh once it is over.
    if (!shrinkRun.get() || shrinkRun.get()?.done) load();
  });

  const total = useMemo(() => (clips ?? []).reduce((n, c) => n + c.sizeBytes, 0), [clips]);
  const games = useMemo(() => {
    const m = new Map<string, { bytes: number; count: number }>();
    for (const c of clips ?? []) {
      const g = clipGame(c.path, root) ?? NO_GAME;
      const e = m.get(g) ?? { bytes: 0, count: 0 };
      m.set(g, { bytes: e.bytes + c.sizeBytes, count: e.count + 1 });
    }
    return [...m.entries()].sort((a, b) => b[1].bytes - a[1].bytes);
  }, [clips, root]);
  const biggest = useMemo(
    () => [...(clips ?? [])].sort((a, b) => b.sizeBytes - a.sizeBytes).slice(0, BIGGEST),
    [clips],
  );
  const candidates = useMemo(
    () =>
      (clips ?? []).filter(
        (c) => !c.favorite && now - c.modifiedMs > age * DAY_MS && worthShrinking(c, level),
      ),
    [clips, age, level, now],
  );
  const candidateBytes = candidates.reduce((n, c) => n + c.sizeBytes, 0);

  const shrink = () => {
    setConfirmShrink(false);
    if (defaults) void runShrink(candidates, defaults, level);
  };

  const remove = async (c: ClipInfo) => {
    setDeleting(null);
    try {
      await commands.deleteClip(c.path);
      load();
    } catch (e) {
      notify("error", "Could not delete the clip", errorMessage(e));
    }
  };

  const used = disk ? disk.totalBytes - disk.freeBytes : 0;
  const pct = (n: number) => (disk && disk.totalBytes > 0 ? (n / disk.totalBytes) * 100 : 0);
  const running = run !== null && !run.done;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Storage</h1>
        <Button small icon="folder" onClick={() => void commands.openFolder("output")}>
          Open folder
        </Button>
        <Button small icon="refresh" variant="ghost" title="Refresh" onClick={load} />
      </div>

      <section className="card storage-card storage-disk">
        <div className="storage-figures">
          <div>
            <div className="storage-big">{clips ? formatBytes(total) : "…"}</div>
            <div className="muted small">
              in {clips?.length ?? 0} clip{clips?.length === 1 ? "" : "s"}
            </div>
          </div>
          {disk && (
            <div className="text-right">
              <div className="storage-big">{formatBytes(disk.freeBytes)}</div>
              <div className="muted small">free of {formatBytes(disk.totalBytes)}</div>
            </div>
          )}
        </div>
        {disk && (
          <div
            className="storage-bar"
            role="img"
            aria-label={`Clips ${formatBytes(total)}, other files ${formatBytes(Math.max(0, used - total))}, free ${formatBytes(disk.freeBytes)}`}
          >
            <span className="clips" style={{ width: `${pct(total)}%` }} />
            <span className="other" style={{ width: `${pct(Math.max(0, used - total))}%` }} />
          </div>
        )}
        <div className="storage-legend muted small">
          <span>
            <i className="clips" /> Clips
          </span>
          <span>
            <i className="other" /> Other files
          </span>
          <span>
            <i /> Free
          </span>
          <span className="spacer" />
          <span className="ellipsis" title={disk?.folder ?? root}>
            {disk?.folder ?? root}
          </span>
        </div>
      </section>

      {clips === null ? (
        <div className="empty">
          <Spinner />
        </div>
      ) : clips.length === 0 ? (
        <div className="card empty">
          <Icon name="storage" size={32} />
          <h3>No clips yet</h3>
          <p>Saved clips and how much space they use show up here.</p>
        </div>
      ) : (
        <>
          <section className="card storage-card">
            <h2 className="storage-title">By game</h2>
            <div className={`storage-games${allGames ? " all" : ""}`}>
              {(allGames ? games : games.slice(0, TOP_GAMES)).map(([g, e]) => (
                <div key={g} className="storage-game">
                  <span className="ellipsis" title={g}>
                    {g}
                  </span>
                  <span className="storage-game-bar">
                    <span style={{ width: `${total > 0 ? (e.bytes / total) * 100 : 0}%` }} />
                  </span>
                  <span className="muted small tabular">
                    {e.count} clip{e.count === 1 ? "" : "s"}
                  </span>
                  <span className="tabular text-right">{formatBytes(e.bytes)}</span>
                </div>
              ))}
            </div>
            {games.length > TOP_GAMES && (
              <Button
                small
                variant="ghost"
                className="storage-more"
                onClick={() => {
                  setAllGames((v) => !v);
                }}
              >
                {allGames ? "Show top 5" : `Show all ${games.length} games`}
              </Button>
            )}
          </section>

          <section className="card storage-card">
            <h2 className="storage-title">Biggest clips</h2>
            <div className="storage-list">
              {biggest.map((c) => (
                <div key={c.path} className="storage-clip">
                  <button
                    type="button"
                    className="storage-thumb"
                    {...dragOut(c.path)}
                    title="Play"
                    style={
                      c.thumbnail
                        ? { backgroundImage: `url("${convertFileSrc(c.thumbnail)}")` }
                        : undefined
                    }
                    onClick={() => {
                      openClip(c.path);
                    }}
                  >
                    <Icon name="play" size={14} />
                  </button>
                  <div className="min-w-0 flex-1">
                    <div className="ellipsis" title={c.path}>
                      {c.favorite && (
                        <Icon
                          name="star"
                          size={12}
                          className="storage-star"
                          aria-label="Favorite"
                        />
                      )}
                      {c.fileName}
                    </div>
                    <div className="muted small">
                      {formatDate(c.modifiedMs)} · {formatDuration(c.durationSec)}
                      {clipGame(c.path, root) && (
                        <>
                          {" "}
                          · <Badge tone="accent">{clipGame(c.path, root)}</Badge>
                        </>
                      )}
                    </div>
                  </div>
                  <span className="tabular">{formatBytes(c.sizeBytes)}</span>
                  <Button
                    small
                    variant="ghost"
                    icon="folder"
                    title="Show in folder"
                    onClick={() => void commands.revealClip(c.path)}
                  />
                  <Button
                    small
                    variant="ghost"
                    icon="trash"
                    title="Delete (to the Recycle Bin)"
                    onClick={() => {
                      setDeleting(c);
                    }}
                  />
                </div>
              ))}
            </div>
          </section>

          <section className="card storage-card storage-shrink">
            <h2 className="storage-title">Shrink old clips</h2>
            <p className="muted small">
              Re-encodes older clips smaller and replaces them; the originals go to the Recycle Bin.
              Favorites and clips that are already small are left alone. All audio tracks are kept.
            </p>
            <div className="storage-shrink-row">
              <Select<number> value={age} options={AGES} onChange={setAge} disabled={running} />
              <Segmented<ShrinkLevel>
                value={level}
                options={SHRINK_LEVELS.map((l) => ({ value: l.value, label: l.label }))}
                onChange={(v) => {
                  if (!running) setLevel(v);
                }}
              />
            </div>
            <p className="muted small">{SHRINK_LEVELS.find((l) => l.value === level)?.hint}</p>
            {run && (
              <div className="storage-run">
                {running ? (
                  <>
                    <div className="small ellipsis">
                      Shrinking {run.at + 1} of {run.total} · {run.name}
                    </div>
                    <Progress value={((run.at + run.pct / 100) / Math.max(1, run.total)) * 100} />
                  </>
                ) : (
                  <div className="small">
                    {run.cancelled ? "Stopped. " : "Done. "}
                    {run.shrunk > 0
                      ? `Freed ${formatBytes(run.saved)} from ${run.shrunk} clip${run.shrunk === 1 ? "" : "s"}.`
                      : "Nothing was shrunk."}
                    {run.skipped > 0 &&
                      ` ${run.skipped} wouldn't get much smaller and ${run.skipped === 1 ? "was" : "were"} left alone.`}
                    {run.failed > 0 && ` ${run.failed} failed.`}
                  </div>
                )}
              </div>
            )}
            <div className="storage-shrink-row">
              <span className="small">
                {candidates.length === 0
                  ? "No clips to shrink."
                  : `${candidates.length} clip${candidates.length === 1 ? "" : "s"} · ${formatBytes(candidateBytes)}`}
              </span>
              <span className="spacer" />
              {running ? (
                <Button small icon="x" onClick={cancelShrink}>
                  Stop
                </Button>
              ) : (
                <Button
                  small
                  variant="primary"
                  disabled={candidates.length === 0 || !defaults}
                  onClick={() => {
                    setConfirmShrink(true);
                  }}
                >
                  Shrink {candidates.length > 0 ? candidates.length : ""} clip
                  {candidates.length === 1 ? "" : "s"}
                </Button>
              )}
            </div>
          </section>
        </>
      )}

      {deleting && (
        <ConfirmDialog
          title={`Delete ${deleting.fileName}?`}
          confirmLabel="Delete"
          onConfirm={() => void remove(deleting)}
          onCancel={() => {
            setDeleting(null);
          }}
        >
          It goes to the Recycle Bin.
        </ConfirmDialog>
      )}
      {confirmShrink && (
        <ConfirmDialog
          title={`Shrink ${candidates.length} clip${candidates.length === 1 ? "" : "s"}?`}
          confirmLabel="Shrink"
          onConfirm={shrink}
          onCancel={() => {
            setConfirmShrink(false);
          }}
        >
          Each clip is re-encoded and replaced; the originals go to the Recycle Bin. This can take a
          while. You can keep using Vido meanwhile.
        </ConfirmDialog>
      )}
    </div>
  );
}

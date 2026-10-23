import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

import { Icon } from "../../components/Icon";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  Badge,
  Button,
  Checkbox,
  Segmented,
  Slider,
  Select,
  Spinner,
} from "../../components/controls";
import { Button as UiButton } from "../../components/ui/button";
import { Input } from "../../components/ui/misc";
import { formatBytes, formatDate, formatDuration } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage, type ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import {
  cancelShrink,
  runShrink,
  SHRINK_LEVELS,
  shrinkRun,
  shrinkSummary,
  type ShrinkLevel,
} from "../storage/shrink";
import { ClipActions } from "./ClipActions";
import { ClipsHero } from "./ClipsHero";
import {
  groupClips,
  GROUP_OPTIONS,
  loadView,
  saveView,
  SORT_OPTIONS,
  sortClips,
  cardWidth,
  clipGame,
  GRID_STEPS,
  type ClipView,
} from "./clipView";
import { ClipViewer } from "./ClipViewer";
import { clipSrc } from "./media";

/** `open`: a clip to show in the viewer right away (e.g. picked on the Overview page). */
export function ClipsPage({ open }: { open?: string | null }) {
  const [clips, setClips] = useState<ClipInfo[] | null>(null);
  const [reload, setReload] = useState(0);
  /** Path of the clip open in the viewer. */
  const [viewing, setViewing] = useState<string | null>(open ?? null);
  const notify = useApp((s) => s.notify);
  const status = useApp((s) => s.status);
  const info = useApp((s) => s.info);
  const saveKey = useApp((s) => s.settings?.hotkeys.saveClip);
  const openEditor = useApp((s) => s.openEditor);
  const [view, setViewState] = useState<ClipView>(loadView);
  /** "" = all games, "\u0000" = clips without a game, else a game's name. */
  const [game, setGame] = useState("");
  const [search, setSearch] = useState("");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  /** Select mode: paths of the selected clips (null = not selecting). */
  const [selected, setSelected] = useState<Set<string> | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmShrink, setConfirmShrink] = useState(false);
  const [shrinkLevel, setShrinkLevel] = useState<ShrinkLevel>("small");
  const shrinking = useSyncExternalStore(shrinkRun.subscribe, shrinkRun.get);
  const exportDefaults = useApp((s) => s.settings?.export);
  const setView = (patch: Partial<ClipView>) => {
    setViewState((v) => {
      const next = { ...v, ...patch };
      saveView(next);
      return next;
    });
  };

  useEffect(() => {
    let alive = true;
    commands.listClips().then(
      (c) => {
        if (!alive) return;
        setClips(c);
        // Close the viewer if its clip is gone (deleted or renamed elsewhere).
        setViewing((v) => (v !== null && c.some((x) => x.path === v) ? v : null));
      },
      (e: unknown) => {
        if (!alive) return;
        notify("error", "Could not list clips", errorMessage(e));
        setClips([]);
      },
    );
    return () => {
      alive = false;
    };
  }, [reload, notify]);
  const load = () => {
    setReload((n) => n + 1);
  };
  useEvent("clips-changed", load);

  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      notify("error", "Action failed", errorMessage(e));
    }
  };

  const canSave = status?.state === "buffering" || status?.state === "paused";
  const root = info?.outputDir;
  // Games with clips, for the filter (alphabetical; "No game" last).
  const games = useMemo(() => {
    const names = new Set<string>();
    let other = false;
    for (const c of clips ?? []) {
      const g = clipGame(c.path, root);
      if (g) names.add(g);
      else other = true;
    }
    return { names: [...names].sort((a, b) => a.localeCompare(b)), other };
  }, [clips, root]);
  const gameOptions = [
    { value: "", label: "All games" },
    ...games.names.map((n) => ({ value: n, label: n })),
    ...(games.other ? [{ value: "\u0000", label: "No game" }] : []),
  ];
  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (
      clips?.filter((c) => {
        const g = clipGame(c.path, root);
        if (game && (game === "\u0000" ? g !== null : g !== game)) return false;
        if (favoritesOnly && !c.favorite) return false;
        return (
          !q || c.fileName.toLowerCase().includes(q) || (g?.toLowerCase().includes(q) ?? false)
        );
      }) ?? null
    );
  }, [clips, game, root, search, favoritesOnly]);
  const sorted = useMemo(
    () => (filtered ? sortClips(filtered, view.sort) : null),
    [filtered, view.sort],
  );
  // The newest clip gets the "Latest clip" card (Play / Edit), only when sorting newest
  // first and not while selecting.
  const featured = view.sort === "newest" && !selected ? sorted?.[0] : undefined;
  const groups = useMemo(
    () => groupClips(sorted ?? [], view.group, root),
    [sorted, view.group, root],
  );
  // The viewer steps through clips in the order shown (groups included).
  const shown = useMemo(() => groups.flatMap((g) => g.clips), [groups]);
  const viewIndex = viewing ? shown.findIndex((c) => c.path === viewing) : -1;

  const toggle = (path: string) => {
    setSelected((cur) => {
      const next = new Set(cur ?? []);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };
  const chosen = shown.filter((c) => selected?.has(c.path));
  const allFavorite = chosen.length > 0 && chosen.every((c) => c.favorite);
  const bulk = async (fn: (c: ClipInfo) => Promise<unknown>, done?: string) => {
    let failed = 0;
    for (const c of chosen) {
      try {
        await fn(c);
      } catch {
        failed += 1;
      }
    }
    if (failed > 0) notify("error", `${failed} of ${chosen.length} clips failed`);
    else if (done) notify("info", done);
  };

  return (
    <div className="page page-wide">
      <div className="page-head">
        <h1>Clips</h1>
        <Button
          small
          icon="save"
          disabled={!canSave}
          title="Save everything in the buffer"
          onClick={() => void act(commands.saveClip)}
        >
          Save clip
        </Button>
        <Button small icon="folder" onClick={() => void act(() => commands.openFolder("output"))}>
          Open folder
        </Button>
        <Button small icon="refresh" variant="ghost" title="Refresh" onClick={load} />
      </div>
      {info && (
        <div className="muted small ellipsis" title={info.outputDir}>
          {info.outputDir}
        </div>
      )}

      {clips === null ? (
        <div className="empty">
          <Spinner />
        </div>
      ) : clips.length === 0 ? (
        <div className="card empty">
          <Icon name="film" size={32} />
          <h3>No clips yet</h3>
          <p>
            While the buffer is running,{" "}
            {saveKey ? (
              <>
                press <b>{saveKey}</b>
              </>
            ) : (
              <>
                click <b>Save clip</b>
              </>
            )}{" "}
            to save everything in the buffer as a clip. How much it keeps is set on the Overview
            page or in Settings → Recording.
          </p>
        </div>
      ) : (
        <>
          <div className="clips-toolbar">
            <div className="relative w-56">
              <Icon
                name="search"
                size={14}
                className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                className="h-9 pl-8"
                placeholder="Search clips"
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                }}
              />
            </div>
            <Select
              value={view.sort}
              options={SORT_OPTIONS}
              width={150}
              onChange={(v) => {
                setView({ sort: v });
              }}
            />
            {games.names.length > 0 && (
              <Select
                value={gameOptions.some((o) => o.value === game) ? game : ""}
                options={gameOptions}
                width={180}
                onChange={setGame}
              />
            )}
            <UiButton
              variant={favoritesOnly ? "secondary" : "ghost"}
              size="sm"
              aria-pressed={favoritesOnly}
              onClick={() => {
                setFavoritesOnly((v) => !v);
              }}
            >
              <Icon
                name="star"
                size={14}
                filled={favoritesOnly}
                className={favoritesOnly ? "text-warning" : undefined}
              />
              Favorites
            </UiButton>
            <Select
              value={view.group}
              options={GROUP_OPTIONS}
              width={150}
              onChange={(v) => {
                setView({ group: v });
              }}
            />
            <div className="ml-auto flex items-center gap-3">
              <Button
                small
                variant={selected ? "primary" : undefined}
                icon="check"
                onClick={() => {
                  setSelected((s) => (s ? null : new Set()));
                }}
              >
                {selected ? "Done" : "Select"}
              </Button>
              <span className="muted small">
                {shown.length} clip{shown.length === 1 ? "" : "s"}
              </span>
              <div className="grid-size" title="Card size">
                <Icon name="gridSmall" size={14} />
                <Slider
                  value={view.size}
                  min={1}
                  max={GRID_STEPS}
                  width={120}
                  onChange={(v) => {
                    setView({ size: v });
                  }}
                />
                <Icon name="gridLarge" size={14} />
              </div>
            </div>
          </div>
          {groups.map((g, gi) => (
            <section key={g.title ?? "all-clips"} className="clip-group">
              {g.title && <h2 className="clip-group-title">{g.title}</h2>}
              <div
                className="clip-grid"
                style={{
                  gridTemplateColumns: `repeat(auto-fill, minmax(${cardWidth(view.size)}px, 1fr))`,
                }}
              >
                {gi === 0 && featured && (
                  <div className="clip-feature">
                    <ClipsHero
                      key={clipSrc(featured)}
                      clip={featured}
                      onPlay={() => {
                        setViewing(featured.path);
                      }}
                      onEdit={() => {
                        openEditor(featured.path);
                      }}
                      tools={<ClipActions clip={featured} />}
                    />
                  </div>
                )}
                {g.clips
                  .filter((c) => c !== featured)
                  .map((c) => (
                    <div
                      key={c.path}
                      className={`card clip-card${selected?.has(c.path) ? " selected" : ""}`}
                    >
                      <div
                        className="clip-thumb"
                        role="button"
                        tabIndex={0}
                        title={selected ? "Select" : "Play"}
                        style={
                          c.thumbnail
                            ? { backgroundImage: `url("${convertFileSrc(c.thumbnail)}")` }
                            : undefined
                        }
                        onClick={() => {
                          if (selected) toggle(c.path);
                          else setViewing(c.path);
                        }}
                        onKeyDown={(e) => {
                          if (e.key !== "Enter") return;
                          if (selected) toggle(c.path);
                          else setViewing(c.path);
                        }}
                      >
                        {selected ? (
                          <span className="clip-check">
                            <Checkbox
                              checked={selected.has(c.path)}
                              onChange={() => {
                                toggle(c.path);
                              }}
                            />
                          </span>
                        ) : (
                          <div className="play">
                            <Icon name="play" size={26} />
                          </div>
                        )}
                        <span className="duration">{formatDuration(c.durationSec)}</span>
                      </div>
                      <div className="clip-meta">
                        <div className="ellipsis" title={c.fileName}>
                          {c.fileName}
                        </div>
                        <div className="row small muted">
                          <span>{formatDate(c.modifiedMs)}</span>
                          <span className="spacer" />
                          <span>{formatBytes(c.sizeBytes)}</span>
                        </div>
                        <div className="clip-tags">
                          {clipGame(c.path, root) && (
                            <Badge tone="accent">{clipGame(c.path, root)}</Badge>
                          )}
                          <Badge>
                            {c.height}p{Math.round(c.fps)}
                          </Badge>
                          <Badge>{c.videoCodec.toUpperCase()}</Badge>
                          {c.audioTracks.length > 0 && (
                            <Badge>
                              {c.audioTracks.length} audio track
                              {c.audioTracks.length > 1 ? "s" : ""}
                            </Badge>
                          )}
                        </div>
                      </div>
                      <div className="clip-actions">
                        <Button
                          small
                          variant="ghost"
                          icon="scissors"
                          onClick={() => {
                            openEditor(c.path);
                          }}
                        >
                          Edit
                        </Button>
                        <span className="spacer" />
                        <ClipActions clip={c} />
                      </div>
                    </div>
                  ))}
              </div>
            </section>
          ))}
        </>
      )}

      {selected && (
        <div className="sticky bottom-4 z-20 mx-auto mt-4 flex w-fit flex-wrap items-center gap-2 rounded-lg border bg-popover px-3 py-2 shadow-lg">
          <span className="px-1 text-sm font-medium">{chosen.length} selected</span>
          <Button
            small
            variant="ghost"
            onClick={() => {
              setSelected(
                chosen.length === shown.length ? new Set() : new Set(shown.map((c) => c.path)),
              );
            }}
          >
            {chosen.length === shown.length ? "Select none" : "Select all"}
          </Button>
          <span className="h-5 w-px bg-border" aria-hidden />
          <UiButton
            variant="ghost"
            size="icon-sm"
            disabled={chosen.length === 0}
            title={allFavorite ? "Remove from favorites" : "Add to favorites"}
            aria-label={allFavorite ? "Remove from favorites" : "Add to favorites"}
            onClick={() => void bulk((c) => commands.setFavorite(c.path, !allFavorite))}
          >
            <Icon
              name="star"
              size={14}
              filled={allFavorite}
              className={allFavorite ? "text-warning" : undefined}
            />
          </UiButton>
          <span className="h-5 w-px bg-border" aria-hidden />
          <Button
            small
            icon="audio"
            disabled={!chosen.some((c) => c.audioTracks.length > 0)}
            onClick={() =>
              void bulk(
                (c) =>
                  c.audioTracks.length > 0
                    ? commands.exportAudioTracks(c.path, false)
                    : Promise.resolve(),
                "Audio tracks exported next to each clip",
              )
            }
          >
            Export audio
          </Button>
          {shrinking && !shrinking.done ? (
            <Button small icon="x" title="Stop shrinking" onClick={cancelShrink}>
              Shrinking {shrinking.at + 1}/{shrinking.total}
            </Button>
          ) : (
            <Button
              small
              icon="storage"
              disabled={chosen.length === 0 || !exportDefaults}
              title="Re-encode the selected clips smaller and replace them"
              onClick={() => {
                setConfirmShrink(true);
              }}
            >
              Shrink size
            </Button>
          )}
          <span className="h-5 w-px bg-border" aria-hidden />
          <Button
            small
            variant="danger"
            icon="trash"
            disabled={chosen.length === 0}
            onClick={() => {
              setConfirmDelete(true);
            }}
          >
            Delete
          </Button>
        </div>
      )}
      {confirmShrink && (
        <ConfirmDialog
          title={`Shrink ${chosen.length} clip${chosen.length === 1 ? "" : "s"}?`}
          confirmLabel="Shrink"
          onConfirm={() => {
            setConfirmShrink(false);
            if (!exportDefaults) return;
            void runShrink(chosen, exportDefaults, shrinkLevel).then((r) => {
              if (r)
                notify(
                  "info",
                  r.cancelled ? "Shrinking stopped" : "Clips shrunk",
                  shrinkSummary(r),
                );
            });
          }}
          onCancel={() => {
            setConfirmShrink(false);
          }}
        >
          <div className="flex flex-col gap-2">
            <Segmented<ShrinkLevel>
              value={shrinkLevel}
              options={SHRINK_LEVELS.map((l) => ({ value: l.value, label: l.label }))}
              onChange={setShrinkLevel}
            />
            <span>
              {SHRINK_LEVELS.find((l) => l.value === shrinkLevel)?.hint} Each clip is replaced; the
              originals go to the Recycle Bin. Clips that wouldn't get much smaller are left alone.
            </span>
          </div>
        </ConfirmDialog>
      )}
      {confirmDelete && (
        <ConfirmDialog
          title={`Delete ${chosen.length} clip${chosen.length === 1 ? "" : "s"}?`}
          confirmLabel="Delete"
          onConfirm={() => {
            setConfirmDelete(false);
            void bulk((c) => commands.deleteClip(c.path)).then(() => {
              setSelected(new Set());
            });
          }}
          onCancel={() => {
            setConfirmDelete(false);
          }}
        >
          The files are deleted from disk.
        </ConfirmDialog>
      )}

      {viewIndex >= 0 && (
        <ClipViewer
          clips={shown}
          index={viewIndex}
          onNavigate={setViewing}
          onClose={() => {
            setViewing(null);
          }}
        />
      )}
    </div>
  );
}

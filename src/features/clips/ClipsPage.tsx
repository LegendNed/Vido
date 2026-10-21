import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";

import { Icon } from "../../components/Icon";
import {
  Badge,
  Button,
  Checkbox,
  ConfirmButton,
  Slider,
  Select,
  Spinner,
} from "../../components/controls";
import { formatBytes, formatDate, formatDuration } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage, type ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { ClipsHero } from "./ClipsHero";
import {
  groupClips,
  loadView,
  saveView,
  SORT_OPTIONS,
  sortClips,
  cardWidth,
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
  const sorted = useMemo(() => (clips ? sortClips(clips, view.sort) : null), [clips, view.sort]);
  // The newest clip gets the "Latest clip" card (Play / Edit), only when sorting newest first.
  const featured = view.sort === "newest" ? sorted?.[0] : undefined;
  const groups = useMemo(
    () => groupClips(sorted ?? [], view.groupByDate),
    [sorted, view.groupByDate],
  );
  // The viewer steps through clips in the order shown (groups included).
  const shown = useMemo(() => groups.flatMap((g) => g.clips), [groups]);
  const viewIndex = viewing ? shown.findIndex((c) => c.path === viewing) : -1;

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
            <Select
              value={view.sort}
              options={SORT_OPTIONS}
              width={150}
              onChange={(v) => {
                setView({ sort: v });
              }}
            />
            <Checkbox
              checked={view.groupByDate}
              onChange={(v) => {
                setView({ groupByDate: v });
              }}
            >
              Group by date
            </Checkbox>
            <span className="spacer" />
            <span className="muted small">
              {clips.length} clip{clips.length === 1 ? "" : "s"}
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
                      tools={<ClipTools path={featured.path} act={act} />}
                    />
                  </div>
                )}
                {g.clips
                  .filter((c) => c !== featured)
                  .map((c) => (
                    <div key={c.path} className="card clip-card">
                      <div
                        className="clip-thumb"
                        role="button"
                        tabIndex={0}
                        title="Play"
                        style={
                          c.thumbnail
                            ? { backgroundImage: `url("${convertFileSrc(c.thumbnail)}")` }
                            : undefined
                        }
                        onClick={() => {
                          setViewing(c.path);
                        }}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") setViewing(c.path);
                        }}
                      >
                        <div className="play">
                          <Icon name="play" size={26} />
                        </div>
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
                        <ClipTools path={c.path} act={act} />
                      </div>
                    </div>
                  ))}
              </div>
            </section>
          ))}
        </>
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

/** Open in default player, show in folder, copy path, delete (cards and the featured tile). */
function ClipTools({
  path,
  act,
}: {
  path: string;
  act: (fn: () => Promise<unknown>) => Promise<void>;
}) {
  const notify = useApp((s) => s.notify);
  return (
    <>
      <Button
        small
        variant="ghost"
        icon="external"
        title="Open in default player"
        onClick={() => void act(() => commands.openClipExternal(path))}
      />
      <Button
        small
        variant="ghost"
        icon="folder"
        title="Show in folder"
        onClick={() => void act(() => commands.revealClip(path))}
      />
      <Button
        small
        variant="ghost"
        icon="copy"
        title="Copy file path"
        onClick={() =>
          void act(async () => {
            await commands.copyClipPath(path);
            notify("info", "Path copied");
          })
        }
      />
      <ConfirmButton
        small
        icon="trash"
        title="Delete clip"
        onConfirm={() => void act(() => commands.deleteClip(path))}
      />
    </>
  );
}

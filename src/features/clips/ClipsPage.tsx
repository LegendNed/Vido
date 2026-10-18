import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import { Badge, Button, ConfirmButton, Spinner } from "../../components/ui";
import { formatBytes, formatDate, formatDuration } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage, type ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

export function ClipsPage() {
  const [clips, setClips] = useState<ClipInfo[] | null>(null);
  const [reload, setReload] = useState(0);
  const notify = useApp((s) => s.notify);
  const status = useApp((s) => s.status);
  const info = useApp((s) => s.info);

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

  return (
    <div className="page" style={{ maxWidth: 1200 }}>
      <div className="page-head">
        <h1>Clips</h1>
        {[15, 30, 60].map((s) => (
          <Button
            key={s}
            small
            disabled={!canSave}
            onClick={() => void act(() => commands.saveLastClip(s))}
          >
            Save {s} s
          </Button>
        ))}
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
            While the buffer is running, press your save hotkey (default <b>Ctrl+Shift+F6</b>) to
            keep the last 30 seconds.
          </p>
        </div>
      ) : (
        <div className="clip-grid">
          {clips.map((c) => (
            <div key={c.path} className="card clip-card">
              <div
                className="clip-thumb"
                role="button"
                tabIndex={0}
                title="Open in editor"
                style={
                  c.thumbnail
                    ? { backgroundImage: `url("${convertFileSrc(c.thumbnail)}")` }
                    : undefined
                }
                onClick={() => void act(() => commands.openEditor(c.path))}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void act(() => commands.openEditor(c.path));
                }}
              >
                <div className="play">
                  <Icon name="scissors" size={26} />
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
                <div className="row" style={{ gap: 4, marginTop: 2 }}>
                  <Badge>
                    {c.height}p{Math.round(c.fps)}
                  </Badge>
                  <Badge>{c.videoCodec.toUpperCase()}</Badge>
                  {c.audioTracks.length > 0 && (
                    <Badge>
                      {c.audioTracks.length} audio track{c.audioTracks.length > 1 ? "s" : ""}
                    </Badge>
                  )}
                </div>
              </div>
              <div className="clip-actions">
                <Button
                  small
                  variant="ghost"
                  icon="scissors"
                  onClick={() => void act(() => commands.openEditor(c.path))}
                >
                  Edit
                </Button>
                <span className="spacer" />
                <Button
                  small
                  variant="ghost"
                  icon="external"
                  title="Open in default player"
                  onClick={() => void act(() => commands.openClipExternal(c.path))}
                />
                <Button
                  small
                  variant="ghost"
                  icon="folder"
                  title="Show in folder"
                  onClick={() => void act(() => commands.revealClip(c.path))}
                />
                <Button
                  small
                  variant="ghost"
                  icon="copy"
                  title="Copy file path"
                  onClick={() =>
                    void act(async () => {
                      await commands.copyClipPath(c.path);
                      notify("info", "Path copied");
                    })
                  }
                />
                <ConfirmButton
                  small
                  icon="trash"
                  title="Delete clip"
                  onConfirm={() => void act(() => commands.deleteClip(c.path))}
                />
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

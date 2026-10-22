import { convertFileSrc } from "@tauri-apps/api/core";
import { useState, type ReactNode } from "react";

import { Button } from "../../components/controls";
import { formatBytes, formatDate, formatDuration } from "../../lib/format";
import type { ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { clipGame } from "./clipView";
import { clipSrc } from "./media";

/**
 * Store-style banner for the newest clip. The small thumbnail shows at once;
 * a full-resolution frame from the clip itself fades in over it when loaded.
 */
export function ClipsHero({
  clip,
  onPlay,
  onEdit,
  tools,
}: {
  clip: ClipInfo;
  onPlay: () => void;
  onEdit: () => void;
  /** Extra buttons in the top-right corner (open, folder, copy, delete). */
  tools?: ReactNode;
}) {
  const [frameReady, setFrameReady] = useState(false);
  const game = clipGame(
    clip.path,
    useApp((s) => s.info?.outputDir),
  );
  // Same moment as the thumbnail, so the sharper frame doesn't jump.
  const at = Math.min(clip.durationSec / 3, 1).toFixed(2);
  return (
    <section
      className="clips-hero theme-dark"
      style={
        clip.thumbnail ? { backgroundImage: `url("${convertFileSrc(clip.thumbnail)}")` } : undefined
      }
    >
      <video
        className={`hero-frame${frameReady ? " ready" : ""}`}
        src={`${clipSrc(clip)}#t=${at}`}
        muted
        playsInline
        preload="metadata"
        tabIndex={-1}
        aria-hidden="true"
        onLoadedData={() => {
          setFrameReady(true);
        }}
      />
      <div className="hero-scrim" />
      {tools && <div className="hero-tools">{tools}</div>}
      <div className="hero-body">
        <span className="hero-eyebrow">Latest clip</span>
        <h2 className="ellipsis" title={clip.fileName}>
          {clip.fileName}
        </h2>
        <div className="hero-meta">
          {game && <span>{game}</span>}
          <span>{formatDate(clip.modifiedMs)}</span>
          <span>{formatDuration(clip.durationSec)}</span>
          <span>{formatBytes(clip.sizeBytes)}</span>
          <span>
            {clip.height}p{Math.round(clip.fps)}
          </span>
        </div>
        <div className="hero-actions">
          <Button variant="primary" icon="play" onClick={onPlay}>
            Play
          </Button>
          <Button icon="scissors" onClick={onEdit}>
            Edit
          </Button>
        </div>
      </div>
    </section>
  );
}

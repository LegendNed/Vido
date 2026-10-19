import { convertFileSrc } from "@tauri-apps/api/core";
import { useState } from "react";

import { Button } from "../../components/ui";
import { formatBytes, formatDate, formatDuration } from "../../lib/format";
import type { ClipInfo } from "../../lib/tauri/types";
import { clipSrc } from "./media";

/**
 * Store-style banner for the newest clip. The small thumbnail shows at once;
 * a full-resolution frame from the clip itself fades in over it when loaded.
 */
export function ClipsHero({
  clip,
  onPlay,
  onEdit,
}: {
  clip: ClipInfo;
  onPlay: () => void;
  onEdit: () => void;
}) {
  const [frameReady, setFrameReady] = useState(false);
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
      <div className="hero-body">
        <span className="hero-eyebrow">Latest clip</span>
        <h2 className="ellipsis" title={clip.fileName}>
          {clip.fileName}
        </h2>
        <div className="hero-meta">
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

import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { Icon } from "../../components/Icon";
import { Button, ConfirmButton, Slider } from "../../components/controls";
import { clamp, formatBytes, formatDate, formatDuration, formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { errorMessage, type ClipInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { clipGame } from "./clipView";
import { clipSrc, unloadVideo } from "./media";

const VOLUME_KEY = "vido.viewer.volume";
/** Controls fade out after this long without mouse movement (while playing). */
const IDLE_MS = 2500;
const SEEK_STEP = 5;

interface VolumeState {
  volume: number;
  muted: boolean;
}

function loadVolume(): VolumeState {
  try {
    const v = JSON.parse(localStorage.getItem(VOLUME_KEY) ?? "null") as Partial<VolumeState> | null;
    if (v && typeof v.volume === "number" && Number.isFinite(v.volume)) {
      return { volume: clamp(v.volume, 0, 1), muted: v.muted === true };
    }
  } catch {
    // Storage unavailable or corrupt: use defaults.
  }
  return { volume: 1, muted: false };
}

function storeVolume(v: VolumeState) {
  try {
    localStorage.setItem(VOLUME_KEY, JSON.stringify(v));
  } catch {
    // Not critical.
  }
}

/**
 * Full-window player over the main window: custom controls, prev/next in Clips
 * page order, Edit / Show in folder / Delete, window fullscreen.
 */
export function ClipViewer({
  clips,
  index,
  onNavigate,
  onClose,
}: {
  clips: ClipInfo[];
  /** Position of the shown clip in `clips` (must be valid). */
  index: number;
  onNavigate: (path: string) => void;
  onClose: () => void;
}) {
  const notify = useApp((s) => s.notify);
  const outputDir = useApp((s) => s.info?.outputDir);
  const clip = clips[index] as ClipInfo;
  const prev = clips[index - 1];
  const next = clips[index + 1];
  const src = clipSrc(clip);

  const video = useRef<HTMLVideoElement>(null);
  const [playing, setPlaying] = useState(false);
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(clip.durationSec);
  const [scrub, setScrub] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);
  const [{ volume, muted }, setVolumeState] = useState(loadVolume);
  const [fullscreen, setFullscreenState] = useState(false);
  const [idle, setIdle] = useState(false);
  const [overChrome, setOverChrome] = useState(false);
  const idleTimer = useRef<number | undefined>(undefined);

  // Load the current clip; unload it when switching or closing.
  useEffect(() => {
    const v = video.current;
    if (!v) return;
    v.src = src;
    v.play().catch(() => undefined);
    return () => {
      unloadVideo(v);
    };
  }, [src]);

  useEffect(() => {
    const v = video.current;
    if (v) {
      v.volume = volume;
      v.muted = muted;
    }
    storeVolume({ volume, muted });
  }, [volume, muted]);

  // Smooth time display while playing (`timeupdate` only fires ~4x/s).
  useEffect(() => {
    if (!playing) return;
    let id = 0;
    const tick = () => {
      if (video.current) setCurrent(video.current.currentTime);
      id = requestAnimationFrame(tick);
    };
    id = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(id);
    };
  }, [playing]);

  // Window fullscreen: keep the icon in sync and never leave the window
  // fullscreen once the viewer is gone.
  useEffect(() => {
    const w = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void w
      .onResized(() => {
        void w.isFullscreen().then(setFullscreenState, () => undefined);
      })
      .then((u) => {
        if (disposed) u();
        else unlisten = u;
      });
    return () => {
      disposed = true;
      unlisten?.();
      void w.isFullscreen().then(
        (f) => (f ? w.setFullscreen(false) : undefined),
        () => undefined,
      );
    };
  }, []);

  const setFullscreen = useCallback(
    (on: boolean) => {
      getCurrentWindow()
        .setFullscreen(on)
        .then(
          () => {
            setFullscreenState(on);
          },
          (e: unknown) => {
            notify("error", "Fullscreen is not available", errorMessage(e));
          },
        );
    },
    [notify],
  );

  // Auto-hide the controls while playing and the mouse rests.
  const wake = useCallback(() => {
    setIdle(false);
    window.clearTimeout(idleTimer.current);
    idleTimer.current = window.setTimeout(() => {
      setIdle(true);
    }, IDLE_MS);
  }, []);
  useEffect(() => {
    idleTimer.current = window.setTimeout(() => {
      setIdle(true);
    }, IDLE_MS);
    return () => {
      window.clearTimeout(idleTimer.current);
    };
  }, []);

  const togglePlay = useCallback(() => {
    const v = video.current;
    if (!v || failed) return;
    if (v.paused || v.ended) v.play().catch(() => undefined);
    else v.pause();
  }, [failed]);

  const seekTo = useCallback(
    (t: number) => {
      const v = video.current;
      if (!v) return;
      const tt = clamp(t, 0, duration || v.duration || 0);
      v.currentTime = tt;
      setCurrent(tt);
    },
    [duration],
  );

  const setVolume = (v: number) => {
    setVolumeState({ volume: clamp(v, 0, 1), muted: v <= 0 });
  };
  const toggleMute = useCallback(() => {
    setVolumeState((s) => ({
      volume: s.muted && s.volume === 0 ? 0.5 : s.volume,
      muted: !s.muted,
    }));
  }, []);

  const go = useCallback(
    (c: ClipInfo | undefined) => {
      if (c) onNavigate(c.path);
    },
    [onNavigate],
  );

  // The editor modal grows out of this dialog and continues at the same moment.
  const edit = () => {
    const v = video.current;
    const r = v?.closest(".viewer")?.getBoundingClientRect();
    const from = r ? { left: r.left, top: r.top, width: r.width, height: r.height } : undefined;
    useApp.getState().openEditor(clip.path, from, v?.currentTime);
    onClose();
  };

  const remove = async () => {
    const v = video.current;
    if (v) unloadVideo(v);
    const after = next ?? prev;
    try {
      await commands.deleteClip(clip.path);
      if (after) onNavigate(after.path);
      else onClose();
    } catch (e) {
      notify("error", "Could not delete clip", errorMessage(e));
      if (v) v.src = src;
    }
  };

  // Keyboard: Space/K play, ←/→ previous/next clip, J/L ∓5 s, M mute,
  // F fullscreen, Esc leaves fullscreen, then closes.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.metaKey) return;
      const tag = (e.target as HTMLElement).tagName;
      const typing = tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA";
      const key = e.key.toLowerCase();
      if (e.key === "Escape") {
        e.preventDefault();
        if (fullscreen) setFullscreen(false);
        else onClose();
        return;
      }
      if (typing) return;
      if (e.key === " " || key === "k") {
        // Space on a focused button activates that button instead.
        if (e.key === " " && tag === "BUTTON") return;
        e.preventDefault();
        togglePlay();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        go(prev);
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        go(next);
      } else if (key === "j" || key === "l") {
        seekTo((video.current?.currentTime ?? 0) + (key === "j" ? -SEEK_STEP : SEEK_STEP));
      } else if (key === "m") toggleMute();
      else if (key === "f") setFullscreen(!fullscreen);
      else return;
      wake();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [fullscreen, setFullscreen, onClose, togglePlay, go, prev, next, seekTo, toggleMute, wake]);

  const hidden = idle && playing && !overChrome && scrub === null;
  const shown = scrub ?? current;
  const chrome = {
    onMouseEnter: () => {
      setOverChrome(true);
    },
    onMouseLeave: () => {
      setOverChrome(false);
    },
  };

  // Portal: a fixed overlay must not be clipped by a transformed/filtered ancestor.
  // A floating dialog over a dimmed backdrop (click outside to close); fullscreen fills.
  return createPortal(
    <div
      className={`viewer-backdrop${fullscreen ? " full" : ""}`}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`viewer theme-dark${hidden ? " idle" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={clip.fileName}
        onMouseMove={wake}
        onPointerUp={() => {
          setScrub(null);
          // Mouse clicks should not leave focus on a control: keys stay shortcuts.
          const el = document.activeElement;
          if (el instanceof HTMLElement) el.blur();
        }}
      >
        <video
          ref={video}
          className="viewer-video"
          poster={clip.thumbnail ? convertFileSrc(clip.thumbnail) : undefined}
          playsInline
          onClick={togglePlay}
          onDoubleClick={() => {
            setFullscreen(!fullscreen);
          }}
          onLoadStart={() => {
            setFailed(false);
            setCurrent(0);
            setDuration(clip.durationSec);
          }}
          onDurationChange={(e) => {
            const d = e.currentTarget.duration;
            if (Number.isFinite(d) && d > 0) setDuration(d);
          }}
          onTimeUpdate={(e) => {
            if (!playing) setCurrent(e.currentTarget.currentTime);
          }}
          onSeeked={(e) => {
            setCurrent(e.currentTarget.currentTime);
          }}
          onPlay={() => {
            setPlaying(true);
          }}
          onPause={() => {
            setPlaying(false);
          }}
          onEnded={() => {
            setPlaying(false);
          }}
          onError={(e) => {
            // Unloading (no src) is not a failure.
            if (e.currentTarget.getAttribute("src")) setFailed(true);
          }}
        />

        {failed && (
          <div className="viewer-message">
            <Icon name="alert" size={28} />
            <div>
              This clip can't be played here ({clip.videoCodec.toUpperCase()}).
              <br />
              Your default player may support it.
            </div>
            <Button
              icon="external"
              onClick={() => void commands.openClipExternal(clip.path).catch(() => undefined)}
            >
              Open in default player
            </Button>
          </div>
        )}

        <header className="viewer-top" {...chrome}>
          <div className="viewer-title">
            <div className="name ellipsis" title={clip.fileName}>
              {clip.fileName}
            </div>
            <div className="meta">
              {clipGame(clip.path, outputDir) && <span>{clipGame(clip.path, outputDir)}</span>}
              <span>{formatDate(clip.modifiedMs)}</span>
              <span>{formatDuration(clip.durationSec)}</span>
              <span>{formatBytes(clip.sizeBytes)}</span>
              <span>
                {clip.height}p{Math.round(clip.fps)} · {clip.videoCodec.toUpperCase()}
              </span>
              <span>
                {index + 1} / {clips.length}
              </span>
            </div>
          </div>
          <Button icon="scissors" onClick={edit}>
            Edit
          </Button>
          <Button
            variant="ghost"
            icon="folder"
            title="Show in folder"
            onClick={() => void commands.revealClip(clip.path).catch(() => undefined)}
          />
          <ConfirmButton icon="trash" title="Delete clip" onConfirm={() => void remove()} />
          <Button variant="ghost" icon="x" title="Close (Esc)" onClick={onClose} />
        </header>

        {prev && (
          <button
            type="button"
            className="viewer-nav prev"
            title="Previous clip (←)"
            aria-label="Previous clip"
            onClick={() => {
              go(prev);
            }}
          >
            <Icon name="chevronLeft" size={26} />
          </button>
        )}
        {next && (
          <button
            type="button"
            className="viewer-nav next"
            title="Next clip (→)"
            aria-label="Next clip"
            onClick={() => {
              go(next);
            }}
          >
            <Icon name="chevronRight" size={26} />
          </button>
        )}

        <footer className="viewer-bottom" {...chrome}>
          <div className="viewer-seek">
            <Slider
              value={shown}
              min={0}
              max={Math.max(duration, 0.01)}
              step={0.01}
              onChange={(t) => {
                setScrub(t);
                seekTo(t);
              }}
              onCommit={() => {
                setScrub(null);
              }}
            />
          </div>
          <div className="viewer-controls">
            <Button
              icon={playing ? "pause" : "play"}
              title={playing ? "Pause (Space)" : "Play (Space)"}
              disabled={failed}
              onClick={togglePlay}
            />
            <span className="viewer-time">
              {formatTime(shown, 0)} / {formatTime(duration, 0)}
            </span>
            <span className="spacer" />
            <Button
              variant="ghost"
              icon={muted || volume === 0 ? "mute" : "speaker"}
              title={muted ? "Unmute (M)" : "Mute (M)"}
              onClick={toggleMute}
            />
            <div className="viewer-volume">
              <Slider
                width={100}
                value={muted ? 0 : Math.round(volume * 100)}
                min={0}
                max={100}
                onChange={(v) => {
                  setVolume(v / 100);
                }}
              />
            </div>
            <Button
              variant="ghost"
              icon={fullscreen ? "fullscreenExit" : "fullscreen"}
              title={fullscreen ? "Exit fullscreen (F)" : "Fullscreen (F)"}
              onClick={() => {
                setFullscreen(!fullscreen);
              }}
            />
          </div>
        </footer>
      </div>
    </div>,
    document.body,
  );
}

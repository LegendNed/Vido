import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Icon } from "../../components/Icon";
import { Banner, Button, Spinner, Toggle } from "../../components/controls";
import { clamp, formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import {
  errorMessage,
  type ClipDetails,
  type CropRect,
  type ExportDefaults,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { clipSrc, unloadVideo } from "../clips/media";
import { ExportPanel } from "../export/ExportPanel";
import { activePreset } from "../export/presets";
import type { LaneView } from "./AudioLane";
import { CropBar } from "./CropBar";
import { CropOverlay } from "./CropOverlay";
import { aspectCrop } from "./cropMath";
import { laneGains, mixWave, previewGain, previewNote } from "./lanes";
import {
  hasEdits,
  initialState,
  isAudible,
  lanesChanged,
  UNITY,
  type EditState,
  type ExportOpts,
  type Lane,
} from "./model";
import { SaveActions } from "./SaveEdit";
import { Timeline } from "./Timeline";
import { useHistory } from "./useHistory";
import { useLaneAudio } from "./useLaneAudio";

/** Waveform resolution (buckets across the whole clip). */
const WAVE_BUCKETS = 1600;
/** Waveforms decoded at the same time (each one reads the whole file). */
const WAVE_JOBS = 3;

/** Loads a clip and shows the editor for it (inside the main window's editor modal). */
export function ClipLoader({
  path,
  defaults,
  at,
  onClose,
}: {
  path: string;
  defaults: ExportDefaults;
  /** Start at this time (seconds). */
  at?: number;
  onClose: () => void;
}) {
  // `version` counts in-place replacements ("Save & override") of this file.
  const [version, setVersion] = useState(0);
  const [loaded, setLoaded] = useState<{ details: ClipDetails; version: number } | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    commands.getClipDetails(path).then(
      (d) => {
        if (cancelled) return;
        setLoaded({ details: d, version });
      },
      (e: unknown) => {
        if (!cancelled) setLoadError(errorMessage(e));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [path, version]);

  if (loadError) {
    return (
      <div className="empty">
        <h3>Cannot open this clip</h3>
        <p>{loadError}</p>
      </div>
    );
  }
  if (!loaded) {
    return (
      <div className="empty">
        <Spinner />
      </div>
    );
  }
  // A replaced file gets a fresh editor (full-length trim, empty history).
  return (
    <ClipEditor
      key={loaded.version}
      details={loaded.details}
      initial={initialState(loaded.details, defaults)}
      at={loaded.version === 0 ? at : undefined}
      onClose={onClose}
      onReplaced={() => {
        setVersion((v) => v + 1);
      }}
    />
  );
}

function ClipEditor({
  details,
  initial,
  at,
  onClose,
  onReplaced,
}: {
  details: ClipDetails;
  initial: EditState;
  at?: number;
  onClose: () => void;
  onReplaced: () => void;
}) {
  const notify = useApp((s) => s.notify);
  const info = details.info;
  const path = info.path;
  const src = clipSrc(info);
  const tracks = info.audioTracks;
  // Per audio track; `[]` when a waveform could not be read.
  const [waves, setWaves] = useState<(number[] | null)[]>(() => tracks.map(() => null));
  const [current, setCurrent] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [loop, setLoop] = useState(true);
  const [cropMode, setCropMode] = useState(false);
  const [aspect, setAspect] = useState<number | null>(null);
  const [videoError, setVideoError] = useState(false);
  const video = useRef<HTMLVideoElement>(null);
  const {
    state: s,
    set: edit,
    preview,
    commit,
    undo,
    redo,
    canUndo,
    canRedo,
  } = useHistory<EditState>(initial);

  const fps = info.fps > 0 ? info.fps : 60;
  const duration = info.durationSec;
  const frame = 1 / fps;

  // Waveforms, a few tracks at a time.
  useEffect(() => {
    let cancelled = false;
    const live = () => !cancelled;
    const queue = tracks.map((t) => t.index);
    const work = async () => {
      for (let i = queue.shift(); i !== undefined && live(); i = queue.shift()) {
        let w: number[] = [];
        try {
          w = await commands.getWaveform(path, i, WAVE_BUCKETS);
        } catch {
          // Drawn as an empty lane.
        }
        if (live()) setWaves((cur) => cur.map((x, j) => (j === i ? w : x)));
      }
    };
    for (let k = 0; k < WAVE_JOBS; k++) void work();
    return () => {
      cancelled = true;
    };
  }, [path, tracks]);

  // Frame-accurate time display.
  useEffect(() => {
    const v = video.current;
    if (!v) return;
    let id = 0;
    const onFrame: VideoFrameRequestCallback = (_now, meta) => {
      setCurrent(meta.mediaTime);
      id = v.requestVideoFrameCallback(onFrame);
    };
    id = v.requestVideoFrameCallback(onFrame);
    const tick = () => {
      setCurrent(v.currentTime);
    };
    v.addEventListener("seeked", tick);
    return () => {
      v.removeEventListener("seeked", tick);
      v.cancelVideoFrameCallback(id);
    };
  }, []);

  // Stay within the selection while playing.
  useEffect(() => {
    const v = video.current;
    if (!v || !playing) return;
    if (current >= s.trimEnd - frame / 2) {
      if (loop) v.currentTime = s.trimStart;
      else {
        v.pause();
        v.currentTime = s.trimEnd;
      }
    }
  }, [current, playing, loop, s.trimStart, s.trimEnd, frame]);

  // The lanes play live (one Web Audio source per track) while the video stays muted.
  // Until that is ready, the video's own first track approximates them with its volume.
  const live = useLaneAudio(video, path, src, laneGains(s));
  const gain = previewGain(s);
  useEffect(() => {
    const v = video.current;
    if (!v) return;
    if (live) {
      v.volume = 1;
      v.muted = true;
      return;
    }
    v.volume = clamp(gain, 0, 1);
    v.muted = gain <= 0;
  }, [gain, live]);

  const seek = useCallback(
    (t: number) => {
      const v = video.current;
      if (!v) return;
      const tt = clamp(t, 0, duration);
      v.currentTime = tt;
      setCurrent(tt);
    },
    [duration],
  );

  const togglePlay = useCallback(() => {
    const v = video.current;
    if (!v) return;
    if (v.paused) {
      if (v.currentTime < s.trimStart || v.currentTime >= s.trimEnd - frame)
        v.currentTime = s.trimStart;
      void v.play();
    } else v.pause();
  }, [s.trimStart, s.trimEnd, frame]);

  const setOpts = useCallback(
    (fn: (x: ExportOpts) => ExportOpts) => {
      edit((d) => ({ ...d, x: fn(d.x) }));
    },
    [edit],
  );
  const setIn = useCallback(() => {
    edit((d) => ({ ...d, trimStart: Math.min(Math.round(current * fps) / fps, d.trimEnd - 0.1) }));
  }, [edit, current, fps]);
  const setOut = useCallback(() => {
    edit((d) => ({ ...d, trimEnd: Math.max(Math.round(current * fps) / fps, d.trimStart + 0.1) }));
  }, [edit, current, fps]);

  const setLane = useCallback(
    (index: number, patch: Partial<Lane>, record: boolean) => {
      (record ? edit : preview)((d) => ({
        ...d,
        lanes: d.lanes.map((l, i) => (i === index ? { ...l, ...patch } : l)),
      }));
    },
    [edit, preview],
  );

  // Crop: the whole frame is stored as "no crop" (keeps stream copy possible).
  const full = useMemo<CropRect>(
    () => ({ x: 0, y: 0, width: info.width, height: info.height }),
    [info.width, info.height],
  );
  const orNull = useCallback(
    (c: CropRect): CropRect | null =>
      c.x === 0 && c.y === 0 && c.width >= (info.width & ~1) && c.height >= (info.height & ~1)
        ? null
        : c,
    [info.width, info.height],
  );
  const chooseAspect = (a: number | null) => {
    setAspect(a);
    if (a !== null) edit((d) => ({ ...d, crop: orNull(aspectCrop(a, info.width, info.height)) }));
  };
  const resetCrop = () => {
    setAspect(null);
    edit((d) => ({ ...d, crop: null }));
  };
  const closeCrop = useCallback(() => {
    setCropMode(false);
  }, []);

  // Save & override: the preview must not hold the file while it is replaced.
  const releaseVideo = useCallback(() => {
    if (video.current) unloadVideo(video.current);
  }, []);
  const restoreVideo = useCallback(() => {
    const v = video.current;
    if (!v) return;
    v.src = src;
    v.load();
  }, [src]);

  const screenshot = useCallback(async () => {
    try {
      const out = await commands.takeScreenshot(path, current, s.crop ?? undefined);
      notify("info", "Screenshot saved", out);
    } catch (e) {
      notify("error", "Screenshot failed", errorMessage(e));
    }
  }, [path, current, s.crop, notify]);

  // Keyboard shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      const tag = el.tagName;
      // Lane volume sliders keep the arrow keys; everything else still works there.
      const slider = el instanceof HTMLInputElement && el.type === "range";
      if ((tag === "INPUT" && !slider) || tag === "SELECT" || tag === "TEXTAREA") return;
      if (slider && /^(Arrow|Page|Home|End)/.test(e.key)) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (ctrl && key === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (ctrl && (key === "y" || (e.shiftKey && key === "z"))) {
        e.preventDefault();
        redo();
      } else if (cropMode && (e.key === "Enter" || e.key === "Escape")) {
        e.preventDefault();
        closeCrop();
      } else if (e.key === " " || key === "k") {
        e.preventDefault();
        togglePlay();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        video.current?.pause();
        const d = e.shiftKey ? 1 : frame;
        seek(current + (e.key === "ArrowLeft" ? -d : d));
      } else if (key === "i") setIn();
      else if (key === "o") setOut();
      else if (e.key === "Home") seek(s.trimStart);
      else if (e.key === "End") seek(s.trimEnd);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [
    undo,
    redo,
    togglePlay,
    seek,
    current,
    frame,
    setIn,
    setOut,
    s.trimStart,
    s.trimEnd,
    cropMode,
    closeCrop,
  ]);

  // Lanes: a recorded mix is shown on top, derived from the source lanes below it.
  const lanes = useMemo<LaneView[]>(() => {
    const set = { lanes: s.lanes, recordedMix: s.recordedMix };
    const changed = lanesChanged(set);
    return tracks.map((t) => {
      const lane = s.lanes[t.index] ?? UNITY;
      if (!(s.recordedMix && t.index === 0)) {
        return { index: t.index, title: t.title, lane, wave: waves[t.index] ?? null };
      }
      const audible = s.lanes.some((l, i) => i > 0 && isAudible(l));
      return {
        index: t.index,
        title: t.title,
        lane: { ...UNITY, muted: !audible },
        wave: changed ? mixWave(waves, set) : (waves[0] ?? null),
        derived: changed ? "Rebuilt from the lanes below" : "Follows the lanes below",
      };
    });
  }, [tracks, s.lanes, s.recordedMix, waves]);
  const note = live ? null : previewNote(s);

  return (
    <div className="editor">
      <header className="editor-head">
        <Icon name="film" />
        <span className="ellipsis" style={{ fontWeight: 600 }} title={info.path}>
          {info.fileName}
        </span>
        <span className="muted small">
          {info.width}×{info.height} · {Math.round(info.fps)} fps · {info.videoCodec.toUpperCase()}
        </span>
        <span className="spacer" />
        <Button variant="ghost" icon="x" title="Close editor (Esc)" onClick={onClose} />
      </header>

      <div className="editor-main">
        <div className="editor-stage">
          <div className="video-box">
            <video
              ref={video}
              src={src}
              preload="auto"
              onLoadedMetadata={(e) => {
                // Continue where the clip preview was.
                if (at) e.currentTarget.currentTime = at;
              }}
              onPlay={() => {
                setPlaying(true);
              }}
              onPause={() => {
                setPlaying(false);
              }}
              onError={() => {
                setVideoError(true);
              }}
              onClick={togglePlay}
            />
            {cropMode && (
              <CropOverlay
                videoWidth={info.width}
                videoHeight={info.height}
                crop={s.crop ?? full}
                aspect={aspect}
                onChange={(c) => {
                  preview((d) => ({ ...d, crop: orNull(c) }));
                }}
                onCommit={commit}
              />
            )}
          </div>
          {cropMode && (
            <CropBar
              crop={s.crop}
              width={info.width}
              height={info.height}
              aspect={aspect}
              onAspect={chooseAspect}
              onReset={resetCrop}
              onDone={closeCrop}
            />
          )}
          {videoError && (
            <Banner>
              This codec can't be previewed here ({info.videoCodec.toUpperCase()}). Trimming by time
              and exporting still work.
            </Banner>
          )}

          <div className="transport">
            <Button
              small
              variant="ghost"
              icon="stepBack"
              title="Previous frame (←)"
              onClick={() => {
                video.current?.pause();
                seek(current - frame);
              }}
            />
            <Button
              small
              icon={playing ? "pause" : "play"}
              title="Play/pause (Space)"
              onClick={togglePlay}
            />
            <Button
              small
              variant="ghost"
              icon="stepFwd"
              title="Next frame (→)"
              onClick={() => {
                video.current?.pause();
                seek(current + frame);
              }}
            />
            <span className="mono">
              {formatTime(current)} / {formatTime(duration)}
            </span>
            <span className="transport-sep" />
            <Button
              small
              variant="ghost"
              icon="undo"
              title="Undo (Ctrl+Z)"
              disabled={!canUndo}
              onClick={undo}
            />
            <Button
              small
              variant="ghost"
              icon="redo"
              title="Redo (Ctrl+Y)"
              disabled={!canRedo}
              onClick={redo}
            />
            <span className="spacer" />
            <div className="transport-tools">
              <Button small onClick={setIn} title="Set the start at the playhead (I)">
                Set in
              </Button>
              <Button small onClick={setOut} title="Set the end at the playhead (O)">
                Set out
              </Button>
              <Button
                small
                icon="crop"
                variant={cropMode ? "primary" : undefined}
                title={cropMode ? "Done cropping (Enter)" : "Crop the video"}
                onClick={() => {
                  setCropMode((v) => !v);
                }}
              >
                {s.crop && !cropMode ? `Crop ${s.crop.width}×${s.crop.height}` : "Crop"}
              </Button>
              <Button
                small
                variant="ghost"
                icon="camera"
                title="Save this frame as PNG"
                onClick={() => void screenshot()}
              />
              <Button
                small
                variant="ghost"
                icon="folder"
                title="Show in folder"
                onClick={() => void commands.revealClip(info.path)}
              />
              <span className="transport-sep" />
              <span className="muted small">Loop</span>
              <Toggle value={loop} onChange={setLoop} label="Loop the selection" />
            </div>
          </div>

          <Timeline
            duration={duration}
            fps={fps}
            current={current}
            trimStart={s.trimStart}
            trimEnd={s.trimEnd}
            keyframes={details.keyframes}
            lanes={lanes}
            onSeek={seek}
            onTrim={(a, b) => {
              preview((d) => ({ ...d, trimStart: a, trimEnd: b }));
            }}
            onTrimCommit={commit}
            onLane={setLane}
            onLaneCommit={commit}
          />
          <div className="editor-hint">
            {note && (
              <span className="editor-note">
                <Icon name="info" size={12} />
                {note}
              </span>
            )}
            <span>
              Space play · ←/→ frame (Shift: 1 s) · I/O set in/out · Home/End go to in/out · Ctrl+Z
              / Ctrl+Y undo/redo · blue ticks are keyframes
            </span>
          </div>
        </div>

        <aside className="editor-side">
          <ExportPanel
            path={info.path}
            state={s}
            setOpts={setOpts}
            actions={
              <SaveActions
                path={info.path}
                state={s}
                canOverride={hasEdits(s, initial) || activePreset(s.x)?.id !== "original"}
                release={releaseVideo}
                restore={restoreVideo}
                onReplaced={onReplaced}
              />
            }
          />
        </aside>
      </div>
    </div>
  );
}

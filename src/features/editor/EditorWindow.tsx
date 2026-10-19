import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";

import { Icon } from "../../components/Icon";
import { Notices } from "../../components/Notices";
import {
  Banner,
  Button,
  Field,
  NumberInput,
  Segmented,
  Select,
  Slider,
  Spinner,
  Toggle,
} from "../../components/ui";
import { clamp, formatTime } from "../../lib/format";
import { windowInit } from "../../lib/init";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import {
  errorMessage,
  type ClipDetails,
  type ExportAudioTrack,
  type ExportDefaults,
} from "../../lib/tauri/types";
import { useBackendSync } from "../../lib/useBackendSync";
import { useApp } from "../../stores/app";
import { clipSrc, unloadVideo } from "../clips/media";
import { ExportPanel } from "../export/ExportPanel";
import { CropOverlay } from "./CropOverlay";
import { aspectCrop } from "./cropMath";
import { hasEdits, initialState, type AudioMode, type EditState, type ExportOpts } from "./model";
import { SaveActions } from "./SaveEdit";
import { Timeline } from "./Timeline";
import { useHistory } from "./useHistory";

const SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];
const ASPECTS: { label: string; value: number | null }[] = [
  { label: "Free", value: null },
  { label: "16:9", value: 16 / 9 },
  { label: "9:16", value: 9 / 16 },
  { label: "1:1", value: 1 },
  { label: "4:3", value: 4 / 3 },
];

/** Editor window shell: follows `editor-open` events; each clip gets a fresh editor. */
export function EditorWindow() {
  const init = useApp((s) => s.init);
  const settings = useApp((s) => s.settings);
  const [path, setPath] = useState<string | null>(windowInit().clip ?? null);

  useEffect(() => {
    void init();
  }, [init]);
  useBackendSync();
  useEvent("editor-open", (e) => {
    setPath(e.path);
  });

  if (!path) {
    return (
      <div className="empty">
        <h3>No clip selected</h3>
        <p>Open a clip from the Clips page.</p>
      </div>
    );
  }
  return (
    <>
      {settings ? (
        <ClipLoader key={path} path={path} defaults={settings.export} />
      ) : (
        <div className="empty">
          <Spinner />
        </div>
      )}
      <Notices />
    </>
  );
}

function ClipLoader({ path, defaults }: { path: string; defaults: ExportDefaults }) {
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
        void getCurrentWindow().setTitle(`${d.info.fileName} — Vido Editor`);
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
      onReplaced={() => {
        setVersion((v) => v + 1);
      }}
    />
  );
}

function ClipEditor({
  details,
  initial,
  onReplaced,
}: {
  details: ClipDetails;
  initial: EditState;
  onReplaced: () => void;
}) {
  const notify = useApp((s) => s.notify);
  const info = details.info;
  const path = info.path;
  const src = clipSrc(info);
  const [waveform, setWaveform] = useState<number[] | null>(null);
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

  useEffect(() => {
    if (info.audioTracks.length === 0) return;
    let cancelled = false;
    commands.getWaveform(path, 0, 1200).then(
      (w) => {
        if (!cancelled) setWaveform(w);
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [path, info.audioTracks.length]);

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

  useEffect(() => {
    if (video.current) video.current.playbackRate = s.speed;
  }, [s.speed]);

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
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
      const ctrl = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (ctrl && key === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (ctrl && (key === "y" || (e.shiftKey && key === "z"))) {
        e.preventDefault();
        redo();
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
  }, [undo, redo, togglePlay, seek, current, frame, setIn, setOut, s.trimStart, s.trimEnd]);

  const setTrack = (i: number, patch: Partial<ExportAudioTrack>) => {
    edit((d) => ({
      ...d,
      // Touching a source track switches from the recorded mix to a rebuilt mix.
      audioMode: d.audioMode === "mix" && i > 0 ? "separate" : d.audioMode,
      tracks: d.tracks.map((t, j) => (j === i ? { ...t, ...patch } : t)),
    }));
  };
  const visibleTracks = info.audioTracks.filter((_, i) =>
    s.audioMode === "plain" ? true : s.audioMode === "mix" ? i === 0 : i > 0,
  );

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
        <span className="editor-head-sep" />
        <SaveActions
          path={info.path}
          state={s}
          dirty={hasEdits(s, initial)}
          release={releaseVideo}
          restore={restoreVideo}
          onReplaced={onReplaced}
        />
      </header>

      <div className="editor-main">
        <div className="editor-stage">
          <div className="video-box">
            <video
              ref={video}
              src={src}
              preload="auto"
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
            {cropMode && s.crop && (
              <CropOverlay
                videoWidth={info.width}
                videoHeight={info.height}
                crop={s.crop}
                aspect={aspect}
                onChange={(c) => {
                  preview((d) => ({ ...d, crop: c }));
                }}
                onCommit={commit}
              />
            )}
          </div>
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
            <span className="spacer" />
            <Button small onClick={setIn} title="Set start at playhead (I)">
              Set in
            </Button>
            <Button small onClick={setOut} title="Set end at playhead (O)">
              Set out
            </Button>
            <span className="muted small">Loop</span>
            <Toggle value={loop} onChange={setLoop} label="Loop selection" />
          </div>

          <Timeline
            duration={duration}
            fps={fps}
            current={current}
            trimStart={s.trimStart}
            trimEnd={s.trimEnd}
            keyframes={details.keyframes}
            waveform={waveform}
            onSeek={seek}
            onTrim={(a, b) => {
              preview((d) => ({ ...d, trimStart: a, trimEnd: b }));
            }}
            onTrimCommit={commit}
          />
          <div className="muted small">
            Space play · ←/→ frame (Shift: 1 s) · I/O set in/out · Ctrl+Z / Ctrl+Y undo/redo · blue
            ticks are keyframes
          </div>
        </div>

        <aside className="editor-side">
          <div className="side-title">Edit</div>
          <div className="card" style={{ padding: "0 12px" }}>
            <Field label="Start">
              <NumberInput
                value={s.trimStart}
                min={0}
                max={Math.max(0, s.trimEnd - 0.1)}
                step={frame}
                decimals={3}
                suffix="s"
                onChange={(v) => {
                  edit((d) => ({ ...d, trimStart: v }));
                }}
              />
            </Field>
            <Field
              label="End"
              description={`Length ${formatTime((s.trimEnd - s.trimStart) / s.speed)}`}
            >
              <NumberInput
                value={s.trimEnd}
                min={s.trimStart + 0.1}
                max={duration}
                step={frame}
                decimals={3}
                suffix="s"
                onChange={(v) => {
                  edit((d) => ({ ...d, trimEnd: v }));
                }}
              />
            </Field>
            <Field label="Speed">
              <Select
                value={s.speed}
                options={SPEEDS.map((v) => ({ value: v, label: `${v}×` }))}
                onChange={(v) => {
                  edit((d) => ({ ...d, speed: v }));
                }}
              />
            </Field>
            <Field label="Crop">
              {s.crop && (
                <Select<string>
                  value={ASPECTS.find((a) => a.value === aspect)?.label ?? "Free"}
                  options={ASPECTS.map((a) => ({ value: a.label, label: a.label }))}
                  onChange={(label) => {
                    const a = ASPECTS.find((x) => x.label === label)?.value ?? null;
                    setAspect(a);
                    if (a) edit((d) => ({ ...d, crop: aspectCrop(a, info.width, info.height) }));
                  }}
                />
              )}
              <Toggle
                value={s.crop !== null}
                onChange={(v) => {
                  setCropMode(v);
                  edit((d) => ({
                    ...d,
                    crop: v
                      ? aspect
                        ? aspectCrop(aspect, info.width, info.height)
                        : { x: 0, y: 0, width: info.width & ~1, height: info.height & ~1 }
                      : null,
                  }));
                }}
              />
            </Field>
            {s.crop && !cropMode && (
              <Field label="">
                <Button
                  small
                  icon="crop"
                  onClick={() => {
                    setCropMode(true);
                  }}
                >
                  Adjust crop
                </Button>
              </Field>
            )}
            <Field label="Fade in / out" description="Audio, seconds">
              <NumberInput
                value={s.fadeIn}
                min={0}
                max={10}
                step={0.1}
                decimals={1}
                width={56}
                onChange={(v) => {
                  edit((d) => ({ ...d, fadeIn: v }));
                }}
              />
              <NumberInput
                value={s.fadeOut}
                min={0}
                max={10}
                step={0.1}
                decimals={1}
                width={56}
                onChange={(v) => {
                  edit((d) => ({ ...d, fadeOut: v }));
                }}
              />
            </Field>
          </div>

          {info.audioTracks.length > 0 && (
            <>
              <div className="side-title">Audio</div>
              <div className="card" style={{ padding: "0 12px" }}>
                {s.audioMode !== "plain" && (
                  <Field label="Source">
                    <Segmented<AudioMode>
                      value={s.audioMode}
                      options={[
                        { value: "mix", label: "Recorded mix" },
                        { value: "separate", label: "Per track" },
                      ]}
                      onChange={(v) => {
                        edit((d) => ({ ...d, audioMode: v }));
                      }}
                    />
                  </Field>
                )}
                {visibleTracks.map((t) => {
                  const tr = s.tracks[t.index];
                  if (!tr) return null;
                  return (
                    <Field
                      key={t.index}
                      label={t.title}
                      description={tr.muted ? "Muted" : !tr.include ? "Removed" : undefined}
                    >
                      <Slider
                        value={Math.round(tr.volume * 100)}
                        min={0}
                        max={200}
                        width={90}
                        onChange={(v) => {
                          preview((d) => ({
                            ...d,
                            audioMode:
                              d.audioMode === "mix" && t.index > 0 ? "separate" : d.audioMode,
                            tracks: d.tracks.map((x, j) =>
                              j === t.index ? { ...x, volume: v / 100 } : x,
                            ),
                          }));
                        }}
                        onCommit={commit}
                      />
                      <span className="mono small" style={{ width: 36, textAlign: "right" }}>
                        {Math.round(tr.volume * 100)}%
                      </span>
                      <Button
                        small
                        variant={tr.muted ? "danger" : "ghost"}
                        icon="speaker"
                        title={tr.muted ? "Unmute" : "Mute"}
                        onClick={() => {
                          setTrack(t.index, { muted: !tr.muted });
                        }}
                      />
                      <Toggle
                        label="Keep track"
                        value={tr.include}
                        onChange={(v) => {
                          setTrack(t.index, { include: v });
                        }}
                      />
                    </Field>
                  );
                })}
                {s.audioMode !== "mix" && info.audioTracks.length > 1 && (
                  <Field label="Mix into one track" description="Best compatibility">
                    <Toggle
                      value={s.mixAudio}
                      onChange={(v) => {
                        edit((d) => ({ ...d, mixAudio: v }));
                      }}
                    />
                  </Field>
                )}
              </div>
            </>
          )}

          <div className="side-title">Export</div>
          <ExportPanel path={info.path} state={s} setOpts={setOpts} />
        </aside>
      </div>
    </div>
  );
}

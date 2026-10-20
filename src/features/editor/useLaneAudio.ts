import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type RefObject } from "react";

import { commands } from "../../lib/tauri/commands";

/** Tracks further than this from the video (seconds) are re-synced. */
const MAX_DRIFT = 0.06;
const LOAD_TIMEOUT_MS = 15_000;
/** Gain changes glide over ~15 ms instead of clicking. */
const GLIDE = 0.015;

interface Graph {
  ctx: AudioContext;
  els: HTMLAudioElement[];
  gains: GainNode[];
}

function waitPlayable(a: HTMLAudioElement): Promise<void> {
  return new Promise((resolve, reject) => {
    if (a.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA) {
      resolve();
      return;
    }
    const timer = window.setTimeout(() => {
      reject(new Error("audio preview timed out"));
    }, LOAD_TIMEOUT_MS);
    a.addEventListener(
      "canplay",
      () => {
        window.clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
    a.addEventListener(
      "error",
      () => {
        window.clearTimeout(timer);
        reject(new Error("audio preview failed to load"));
      },
      { once: true },
    );
  });
}

function build(files: string[]): Graph {
  const ctx = new AudioContext();
  const els = files.map((f) => {
    const a = new Audio();
    // The asset protocol answers with CORS headers; without them Web Audio hears silence.
    a.crossOrigin = "anonymous";
    a.preload = "auto";
    a.src = convertFileSrc(f);
    // Hidden in the DOM so they show up in devtools.
    a.hidden = true;
    a.className = "lane-audio";
    document.body.append(a);
    return a;
  });
  const gains = els.map((a) => {
    const g = ctx.createGain();
    g.gain.value = 0;
    ctx.createMediaElementSource(a).connect(g).connect(ctx.destination);
    return g;
  });
  return { ctx, els, gains };
}

function dispose(g: Graph) {
  for (const a of g.els) {
    a.pause();
    a.removeAttribute("src");
    a.load();
    a.remove();
  }
  void g.ctx.close();
}

/**
 * Live lane mix for the editor preview. A `<video>` plays only a file's first
 * audio track, so every track plays from its own copy (`prepareAudioPreview`)
 * through a Web Audio gain (up to 200 %), following the video's play, pause and
 * seeks. Returns `true` once that runs; until then, or if a track can't be
 * played, the caller keeps using the video's own audio.
 */
export function useLaneAudio(
  video: RefObject<HTMLVideoElement | null>,
  path: string,
  /** Changes when the file is replaced in place. */
  version: string,
  /** Per audio track, linear (see `laneGains`). */
  gains: number[],
): boolean {
  const graph = useRef<Graph | null>(null);
  const [readyFor, setReadyFor] = useState<string | null>(null);
  const live = readyFor === version;

  useEffect(() => {
    let cancelled = false;
    const stale = () => cancelled;
    // Filled in asynchronously; the cleanup disposes whatever was built.
    const made: { graph: Graph | null } = { graph: null };
    commands
      .prepareAudioPreview(path)
      .then(async (files) => {
        const playable = files.filter((f): f is string => f !== null);
        if (stale() || playable.length === 0 || playable.length !== files.length) return;
        const g = build(playable);
        made.graph = g;
        await Promise.all(g.els.map(waitPlayable));
        if (stale()) return;
        graph.current = g;
        setReadyFor(version);
      })
      .catch(() => {
        // Keep previewing with the video's own audio.
      });
    return () => {
      cancelled = true;
      graph.current = null;
      if (made.graph) dispose(made.graph);
    };
  }, [path, version]);

  const key = gains.join(",");
  useEffect(() => {
    const g = graph.current;
    if (!live || !g) return;
    const values = key.split(",").map(Number);
    g.gains.forEach((node, i) => {
      const v = values[i] ?? 0;
      node.gain.setTargetAtTime(v, g.ctx.currentTime, GLIDE);
      g.els[i]?.setAttribute("data-gain", String(v));
    });
  }, [live, key]);

  useEffect(() => {
    const v = video.current;
    const g = graph.current;
    if (!live || !v || !g) return;
    const align = () => {
      for (const a of g.els) a.currentTime = v.currentTime;
    };
    const start = () => {
      void g.ctx.resume();
      align();
      for (const a of g.els) void a.play().catch(() => undefined);
    };
    const stop = () => {
      for (const a of g.els) a.pause();
      align();
    };
    v.addEventListener("play", start);
    v.addEventListener("pause", stop);
    v.addEventListener("seeking", align);
    // Separate elements drift apart slowly; pull them back while playing.
    const timer = window.setInterval(() => {
      if (v.paused) return;
      for (const a of g.els) {
        if (Math.abs(a.currentTime - v.currentTime) > MAX_DRIFT) a.currentTime = v.currentTime;
        if (a.paused) void a.play().catch(() => undefined);
      }
    }, 250);
    if (v.paused) align();
    else start();
    return () => {
      v.removeEventListener("play", start);
      v.removeEventListener("pause", stop);
      v.removeEventListener("seeking", align);
      window.clearInterval(timer);
      for (const a of g.els) a.pause();
    };
  }, [live, video]);

  return live;
}

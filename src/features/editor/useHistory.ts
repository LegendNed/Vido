import { useCallback, useState } from "react";

interface History<T> {
  past: T[];
  present: T;
  future: T[];
  /** State before an in-progress continuous change (e.g. a drag). */
  anchor: T | null;
}

const LIMIT = 100;

/**
 * Undo/redo state. `set` records a step; `preview` updates without recording
 * (for drags) and `commit` turns the whole preview sequence into one step.
 */
export function useHistory<T>(initial: T) {
  const [h, setH] = useState<History<T>>({ past: [], present: initial, future: [], anchor: null });

  const set = useCallback((next: T | ((p: T) => T)) => {
    setH((cur) => {
      const value = typeof next === "function" ? (next as (p: T) => T)(cur.present) : next;
      const base = cur.anchor ?? cur.present;
      return { past: [...cur.past, base].slice(-LIMIT), present: value, future: [], anchor: null };
    });
  }, []);

  const preview = useCallback((next: T | ((p: T) => T)) => {
    setH((cur) => {
      const value = typeof next === "function" ? (next as (p: T) => T)(cur.present) : next;
      return { ...cur, present: value, anchor: cur.anchor ?? cur.present };
    });
  }, []);

  const commit = useCallback(() => {
    setH((cur) => {
      if (cur.anchor === null) return cur;
      if (JSON.stringify(cur.anchor) === JSON.stringify(cur.present))
        return { ...cur, anchor: null };
      return {
        past: [...cur.past, cur.anchor].slice(-LIMIT),
        present: cur.present,
        future: [],
        anchor: null,
      };
    });
  }, []);

  const undo = useCallback(() => {
    setH((cur) => {
      const prev = cur.past.at(-1);
      if (prev === undefined) return cur;
      return {
        past: cur.past.slice(0, -1),
        present: prev,
        future: [cur.present, ...cur.future],
        anchor: null,
      };
    });
  }, []);

  const redo = useCallback(() => {
    setH((cur) => {
      const [next, ...rest] = cur.future;
      if (next === undefined) return cur;
      return { past: [...cur.past, cur.present], present: next, future: rest, anchor: null };
    });
  }, []);

  const reset = useCallback((value: T) => {
    setH({ past: [], present: value, future: [], anchor: null });
  }, []);

  return {
    state: h.present,
    set,
    preview,
    commit,
    undo,
    redo,
    reset,
    canUndo: h.past.length > 0,
    canRedo: h.future.length > 0,
  };
}

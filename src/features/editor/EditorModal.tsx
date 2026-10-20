import { useEffect, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";

import { useApp } from "../../stores/app";
import { ClipLoader } from "./Editor";

/** Grow-in duration (ms) when opened from the clip preview. */
const GROW_MS = 300;

/**
 * The editor, as a large modal over the main window. Opened from the clip preview,
 * it grows out of the preview dialog and continues at the same moment.
 */
export function EditorModal() {
  const editor = useApp((s) => s.editor);
  const close = useApp((s) => s.closeEditor);
  const defaults = useApp((s) => s.settings?.export);
  const box = useRef<HTMLDivElement>(null);

  // Grow from the preview dialog's rect (FLIP), unless motion is reduced.
  useLayoutEffect(() => {
    const el = box.current;
    const from = editor?.from;
    if (!el || !from) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const to = el.getBoundingClientRect();
    if (to.width === 0 || to.height === 0) return;
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    const sx = from.width / to.width;
    const sy = from.height / to.height;
    el.animate(
      [
        {
          transformOrigin: "top left",
          transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`,
        },
        { transformOrigin: "top left", transform: "none" },
      ],
      { duration: GROW_MS, easing: "cubic-bezier(0.2, 0, 0, 1)" },
    );
  }, [editor]);

  // Esc closes (after the editor had its say: e.g. Esc first leaves crop mode).
  useEffect(() => {
    if (!editor) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT") return;
      close();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [editor, close]);

  if (!editor || !defaults) return null;
  return createPortal(
    <div
      className="editor-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div
        ref={box}
        className={`editor-modal${editor.from ? "" : " fade-in"}`}
        role="dialog"
        aria-modal="true"
        aria-label="Editor"
      >
        <ClipLoader
          key={editor.path}
          path={editor.path}
          defaults={defaults}
          at={editor.at}
          onClose={close}
        />
      </div>
    </div>,
    document.body,
  );
}

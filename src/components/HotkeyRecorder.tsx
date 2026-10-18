import { useEffect, useRef, useState } from "react";

import { captureKey } from "../lib/keys";
import { commands } from "../lib/tauri/commands";
import { Icon } from "./Icon";
import { Keys } from "./ui";

/**
 * Click, then press a key combination. Global hotkeys are suspended in Rust
 * while listening so the existing binding doesn't fire. Rust validates the result.
 */
export function HotkeyRecorder({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (accelerator: string | null) => void;
}) {
  const [listening, setListening] = useState(false);
  const [preview, setPreview] = useState("");
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  });

  useEffect(() => {
    if (!listening) return;
    void commands.setHotkeysSuspended(true);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        setListening(false);
        return;
      }
      const r = captureKey(e);
      if (r.kind === "modifiers-only") setPreview(r.preview);
      else if (r.kind === "complete") {
        setListening(false);
        onChangeRef.current(r.accelerator);
      }
    };
    const stop = () => {
      setListening(false);
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", stop);
      void commands.setHotkeysSuspended(false);
      setPreview("");
    };
  }, [listening]);

  return (
    <span className="row">
      <button
        type="button"
        className={`btn recorder${listening ? " listening" : ""}`}
        onClick={() => {
          setListening(!listening);
        }}
      >
        <Icon name="keyboard" size={14} />
        {listening ? (
          preview ? (
            `${preview}+…`
          ) : (
            "Press keys… (Esc to cancel)"
          )
        ) : value ? (
          <Keys accelerator={value} />
        ) : (
          <span className="faint">Not set</span>
        )}
      </button>
      {value && !listening && (
        <button
          type="button"
          className="btn ghost small icon"
          title="Clear"
          aria-label="Clear shortcut"
          onClick={() => {
            onChange(null);
          }}
        >
          <Icon name="x" size={14} />
        </button>
      )}
    </span>
  );
}

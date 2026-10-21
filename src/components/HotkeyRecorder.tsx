import { useEffect, useRef, useState } from "react";

import { captureKey } from "../lib/keys";
import { commands } from "../lib/tauri/commands";
import { cn } from "../lib/utils";
import { Icon } from "./Icon";
import { Button as UiButton } from "./ui/button";
import { Keys } from "./controls";

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
      <UiButton
        type="button"
        variant="outline"
        size="sm"
        className={cn(
          "min-w-[170px] justify-start",
          listening && "border-ring ring-[3px] ring-ring/50",
        )}
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
          <span className="text-muted-foreground">Not set</span>
        )}
      </UiButton>
      {value && !listening && (
        <UiButton
          type="button"
          variant="ghost"
          size="icon-sm"
          title="Clear"
          aria-label="Clear shortcut"
          onClick={() => {
            onChange(null);
          }}
        >
          <Icon name="x" size={14} />
        </UiButton>
      )}
    </span>
  );
}

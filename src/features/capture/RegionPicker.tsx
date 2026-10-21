import { useEffect, useState } from "react";

import { Button } from "../../components/controls";
import { windowInit } from "../../lib/init";
import { commands } from "../../lib/tauri/commands";
import { errorMessage } from "../../lib/tauri/types";

interface Pt {
  x: number;
  y: number;
}

/** Full-display overlay: drag a rectangle, Enter to confirm, Esc to cancel. */
export function RegionPicker() {
  const display = windowInit().display;
  const [a, setA] = useState<Pt | null>(null);
  const [b, setB] = useState<Pt | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dpr = window.devicePixelRatio || 1;

  const rect =
    a && b
      ? {
          x: Math.min(a.x, b.x),
          y: Math.min(a.y, b.y),
          w: Math.abs(a.x - b.x),
          h: Math.abs(a.y - b.y),
        }
      : null;
  const phys = rect
    ? {
        x: Math.round(rect.x * dpr),
        y: Math.round(rect.y * dpr),
        width: Math.round(rect.w * dpr),
        height: Math.round(rect.h * dpr),
      }
    : null;
  const valid = phys !== null && phys.width >= 64 && phys.height >= 64;

  const confirm = async (region = phys) => {
    if (!display || !region) return;
    try {
      await commands.setCaptureRegion({ monitorId: display.id, ...region });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void commands.closeRegionPicker();
      if (e.key === "Enter" && valid) void confirm();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  });

  if (!display) return null;

  return (
    <div
      className="region"
      onPointerDown={(e) => {
        if ((e.target as HTMLElement).closest(".region-bar")) return;
        (e.target as Element).setPointerCapture(e.pointerId);
        setA({ x: e.clientX, y: e.clientY });
        setB({ x: e.clientX, y: e.clientY });
        setDragging(true);
      }}
      onPointerMove={(e) => {
        if (dragging) setB({ x: e.clientX, y: e.clientY });
      }}
      onPointerUp={() => {
        setDragging(false);
      }}
    >
      {rect && (
        <div
          className="region-rect"
          style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
        >
          {phys && (
            <span className="region-size">
              {phys.width}×{phys.height}
            </span>
          )}
        </div>
      )}
      <div className="region-bar">
        <span>
          Drag to select the capture area on <b>{display.name}</b>
          {error ? <span style={{ color: "var(--danger)" }}> - {error}</span> : null}
        </span>
        <Button
          small
          onClick={() => void confirm({ x: 0, y: 0, width: display.width, height: display.height })}
        >
          Whole display
        </Button>
        <Button small variant="ghost" onClick={() => void commands.closeRegionPicker()}>
          Cancel (Esc)
        </Button>
        <Button small variant="primary" disabled={!valid} onClick={() => void confirm()}>
          Use region (Enter)
        </Button>
      </div>
    </div>
  );
}

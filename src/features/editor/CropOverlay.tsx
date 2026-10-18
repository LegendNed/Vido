import { useEffect, useRef, useState } from "react";

import { clamp } from "../../lib/format";
import type { CropRect } from "../../lib/tauri/types";
import { normalizeCrop } from "./cropMath";

type Handle = "move" | "nw" | "ne" | "sw" | "se" | "n" | "s" | "e" | "w";

const HANDLES: Exclude<Handle, "move">[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/**
 * Crop rectangle editor drawn over a `object-fit: contain` video element.
 * Coordinates are in source pixels.
 */
export function CropOverlay({
  videoWidth,
  videoHeight,
  crop,
  aspect,
  onChange,
  onCommit,
}: {
  videoWidth: number;
  videoHeight: number;
  crop: CropRect;
  aspect: number | null;
  onChange: (c: CropRect) => void;
  onCommit: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const drag = useRef<{ handle: Handle; startX: number; startY: number; orig: CropRect } | null>(
    null,
  );

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setSize({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
    };
  }, []);

  // Displayed video area inside the container (letterboxed).
  const scale =
    videoWidth > 0 && videoHeight > 0 ? Math.min(size.w / videoWidth, size.h / videoHeight) : 0;
  const offX = (size.w - videoWidth * scale) / 2;
  const offY = (size.h - videoHeight * scale) / 2;

  const onDown = (e: React.PointerEvent, handle: Handle) => {
    e.preventDefault();
    e.stopPropagation();
    (e.target as Element).setPointerCapture(e.pointerId);
    drag.current = { handle, startX: e.clientX, startY: e.clientY, orig: crop };
  };
  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d || scale <= 0) return;
    const dx = (e.clientX - d.startX) / scale;
    const dy = (e.clientY - d.startY) / scale;
    const o = d.orig;
    let { x, y, width, height } = o;
    if (d.handle === "move") {
      x = clamp(o.x + dx, 0, videoWidth - o.width);
      y = clamp(o.y + dy, 0, videoHeight - o.height);
    } else {
      if (d.handle.includes("w")) {
        x = clamp(o.x + dx, 0, o.x + o.width - 64);
        width = o.x + o.width - x;
      }
      if (d.handle.includes("e")) width = clamp(o.width + dx, 64, videoWidth - o.x);
      if (d.handle.includes("n")) {
        y = clamp(o.y + dy, 0, o.y + o.height - 64);
        height = o.y + o.height - y;
      }
      if (d.handle.includes("s")) height = clamp(o.height + dy, 64, videoHeight - o.y);
      if (aspect) {
        // Keep the aspect ratio, driven by the dominant axis.
        if (d.handle === "n" || d.handle === "s") width = height * aspect;
        else height = width / aspect;
        width = Math.min(width, videoWidth - x);
        height = Math.min(height, videoHeight - y);
      }
    }
    onChange(normalizeCrop({ x, y, width, height }, videoWidth, videoHeight));
  };
  const onUp = () => {
    if (drag.current) onCommit();
    drag.current = null;
  };

  const r = {
    left: offX + crop.x * scale,
    top: offY + crop.y * scale,
    width: crop.width * scale,
    height: crop.height * scale,
  };

  return (
    <div ref={box} className="crop-layer" onPointerMove={onMove} onPointerUp={onUp}>
      {scale > 0 && (
        <>
          <div
            className="crop-shade"
            style={{ left: offX, top: offY, width: videoWidth * scale, height: r.top - offY }}
          />
          <div
            className="crop-shade"
            style={{
              left: offX,
              top: r.top + r.height,
              width: videoWidth * scale,
              height: offY + videoHeight * scale - r.top - r.height,
            }}
          />
          <div
            className="crop-shade"
            style={{ left: offX, top: r.top, width: r.left - offX, height: r.height }}
          />
          <div
            className="crop-shade"
            style={{
              left: r.left + r.width,
              top: r.top,
              width: offX + videoWidth * scale - r.left - r.width,
              height: r.height,
            }}
          />
          <div
            className="crop-rect"
            style={r}
            onPointerDown={(e) => {
              onDown(e, "move");
            }}
          >
            <span className="crop-size">
              {crop.width}×{crop.height}
            </span>
            {HANDLES.map((h) => (
              <span
                key={h}
                className={`crop-handle ${h}`}
                onPointerDown={(e) => {
                  onDown(e, h);
                }}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

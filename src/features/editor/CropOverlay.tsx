import { useEffect, useRef, useState } from "react";

import { clamp } from "../../lib/format";
import type { CropRect } from "../../lib/tauri/types";
import { normalizeCrop } from "./cropMath";

type Handle = "move" | "nw" | "ne" | "sw" | "se" | "n" | "s" | "e" | "w";

const HANDLES: Exclude<Handle, "move">[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/**
 * Rectangle editor drawn over a `object-fit: contain` video element, for the crop
 * or (`variant="blur"`) the selected blur area. Coordinates are in source pixels.
 * Blur areas show a live blurred preview; `others` are the remaining blur areas
 * (click one to edit it). With `crop` null only `others` are shown.
 */
export function CropOverlay({
  videoWidth,
  videoHeight,
  crop,
  aspect,
  onChange,
  onCommit,
  variant = "crop",
  others = [],
  onSelectOther,
}: {
  videoWidth: number;
  videoHeight: number;
  crop: CropRect | null;
  aspect: number | null;
  onChange: (c: CropRect) => void;
  onCommit: () => void;
  variant?: "crop" | "blur";
  others?: CropRect[];
  onSelectOther?: (index: number) => void;
}) {
  const blur = variant === "blur";
  const min = blur ? 16 : 64;
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
    if (!crop) return;
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
        x = clamp(o.x + dx, 0, o.x + o.width - min);
        width = o.x + o.width - x;
      }
      if (d.handle.includes("e")) width = clamp(o.width + dx, min, videoWidth - o.x);
      if (d.handle.includes("n")) {
        y = clamp(o.y + dy, 0, o.y + o.height - min);
        height = o.y + o.height - y;
      }
      if (d.handle.includes("s")) height = clamp(o.height + dy, min, videoHeight - o.y);
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

  const place = (c: CropRect) => ({
    left: offX + c.x * scale,
    top: offY + c.y * scale,
    width: c.width * scale,
    height: c.height * scale,
  });
  const r = place(crop ?? { x: 0, y: 0, width: 0, height: 0 });
  // Blur strength in screen pixels, about what the export will look like.
  const blurPx = (c: CropRect) => Math.max(4, Math.min(c.width, c.height) * scale * 0.12);

  return (
    <div
      ref={box}
      className={`crop-layer${crop ? "" : " passive"}`}
      onPointerMove={onMove}
      onPointerUp={onUp}
    >
      {scale > 0 &&
        others.map((o, i) => (
          <div
            key={i}
            className="blur-rect other"
            style={{ ...place(o), backdropFilter: `blur(${blurPx(o)}px)` }}
            onPointerDown={(e) => {
              if (!onSelectOther) return;
              e.stopPropagation();
              onSelectOther(i);
            }}
          />
        ))}
      {scale > 0 && crop && blur && (
        <div
          className="crop-rect blur-rect"
          style={{ ...r, backdropFilter: `blur(${blurPx(crop)}px)` }}
          onPointerDown={(e) => {
            onDown(e, "move");
          }}
        >
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
      )}
      {scale > 0 && crop && !blur && (
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

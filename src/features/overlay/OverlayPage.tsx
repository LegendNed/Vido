import { useRef, type KeyboardEvent } from "react";

import { Banner, Button, Field, Section, Toggle } from "../../components/ui";
import { HUD_POSITIONS } from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import { errorMessage, type HudPosition } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";

/** The six spots, row by row on the screen diagram (2 rows × 3 columns). */
const SPOTS: readonly HudPosition[] = [
  "topLeft",
  "topCenter",
  "topRight",
  "bottomLeft",
  "bottomCenter",
  "bottomRight",
];

/** Arrow keys / Home / End move between the spots; `null` for other keys. */
function step(index: number, key: string): number | null {
  const [row, col] = [Math.floor(index / 3), index % 3];
  switch (key) {
    case "ArrowLeft":
      return row * 3 + Math.max(0, col - 1);
    case "ArrowRight":
      return row * 3 + Math.min(2, col + 1);
    case "ArrowUp":
      return col;
    case "ArrowDown":
      return 3 + col;
    case "Home":
      return 0;
    case "End":
      return 5;
    default:
      return null;
  }
}

const label = (p: HudPosition) => HUD_POSITIONS.find((o) => o.value === p)?.label ?? p;

/** A 16:9 screen with six clickable spots; the selected one shows a mini pill. */
function PositionPicker({
  value,
  onChange,
}: {
  value: HudPosition;
  onChange: (p: HudPosition) => void;
}) {
  const spots = useRef<(HTMLButtonElement | null)[]>([]);

  const move = (e: KeyboardEvent) => {
    const next = step(SPOTS.indexOf(value), e.key);
    const p = next === null ? undefined : SPOTS[next];
    if (next === null || !p) return;
    e.preventDefault();
    if (p !== value) onChange(p);
    spots.current[next]?.focus();
  };

  return (
    <div
      className="overlay-screen"
      role="radiogroup"
      aria-label="Overlay position"
      onKeyDown={move}
    >
      {SPOTS.map((p, i) => {
        const on = p === value;
        return (
          <button
            key={p}
            ref={(el) => {
              spots.current[i] = el;
            }}
            type="button"
            role="radio"
            aria-checked={on}
            aria-label={label(p)}
            title={label(p)}
            tabIndex={on ? 0 : -1}
            className={`spot r${Math.floor(i / 3)} c${i % 3}${on ? " on" : ""}`}
            onClick={() => {
              onChange(p);
            }}
          >
            {on ? (
              <span className="mini-pill">
                <span className="dot" />
              </span>
            ) : (
              <span className="mark" />
            )}
          </button>
        );
      })}
      <span className="taskbar" aria-hidden />
    </div>
  );
}

export function OverlayPage() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const notify = useApp((s) => s.notify);
  if (!settings) return null;
  const hud = settings.general.hud;

  // Applied right away so Preview (and the live overlay) follow at once.
  const set = (mutate: (h: typeof hud) => void) => {
    update(
      (d) => {
        mutate(d.general.hud);
      },
      { immediate: true },
    );
  };

  const preview = () => {
    commands.previewHud().catch((e: unknown) => {
      notify("error", "Could not show the preview", errorMessage(e));
    });
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Overlay</h1>
      </div>
      <p className="muted" style={{ margin: 0 }}>
        Floats over games, never in your recordings.
      </p>

      <Section title="Placement" description="On your main display, clear of the taskbar.">
        <Field label="Position" description={label(hud.position)}>
          <PositionPicker
            value={hud.position}
            onChange={(p) => {
              set((h) => {
                h.position = p;
              });
            }}
          />
        </Field>
        <Field
          label="Preview"
          description="Shows the clip saved popup there for a few seconds, even when it's turned off."
        >
          <Button icon="play" onClick={preview}>
            Preview
          </Button>
        </Field>
      </Section>

      <Section title="What shows">
        <Field
          label="Show the overlay"
          description="A small pill that's always visible: red dot while the replay buffer runs, grey when it's off."
        >
          <Toggle
            label="Show the overlay"
            value={hud.overlay}
            onChange={(v) => {
              set((h) => {
                h.overlay = v;
              });
            }}
          />
        </Field>
        <Field
          label="Buffer details"
          description={
            hud.overlay
              ? "Buffered seconds and the REC timer next to the dot."
              : "Buffered seconds and the REC timer next to the dot. Needs the overlay."
          }
        >
          <Toggle
            label="Buffer details"
            value={hud.showDetails}
            disabled={!hud.overlay}
            onChange={(v) => {
              set((h) => {
                h.showDetails = v;
              });
            }}
          />
        </Field>
        <Field
          label="Clip saved popup"
          description="“✓ Clip saved (0:30) · ready to paste” for a few seconds after saving, plus recording and error messages. Shows even when the overlay is hidden."
        >
          <Toggle
            label="Clip saved popup"
            value={hud.clipPopup}
            onChange={(v) => {
              set((h) => {
                h.clipPopup = v;
              });
            }}
          />
        </Field>
      </Section>

      <Banner tone="info">
        The overlay can't appear over exclusive-fullscreen games; borderless and windowed games
        work. “Play a sound when saved” still works there.
      </Banner>
    </div>
  );
}

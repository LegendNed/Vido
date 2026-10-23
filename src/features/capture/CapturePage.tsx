import { useEffect, useState } from "react";

import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  Badge,
  Banner,
  Button,
  Field,
  NumberInput,
  Section,
  Segmented,
  Select,
  Slider,
  Toggle,
} from "../../components/controls";
import { formatBitrate } from "../../lib/format";
import {
  codecOptions,
  encoderOptions,
  BITRATE_OPTIONS,
  ENCODER_SPEED_OPTIONS,
  FPS_OPTIONS,
  SCALE_OPTIONS,
  STORAGE_OPTIONS,
} from "../../lib/options";
import { commands } from "../../lib/tauri/commands";
import {
  errorMessage,
  type CaptureMode,
  type DisplayInfo,
  type WindowInfo,
} from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { QualityCards } from "./QualityCards";
import { applyPreset } from "./recordingPresets";
import BUFFER_PRESETS from "../../../config/clip-lengths.json";

/** Video source, quality and replay buffer: the video half of the Recording page. */
export function CaptureSettings() {
  const [confirmDisk, setConfirmDisk] = useState(false);
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const caps = useApp((s) => s.caps);
  const status = useApp((s) => s.status);
  const notify = useApp((s) => s.notify);
  const fieldError = useApp((s) => s.fieldError);
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [windowsReload, setWindowsReload] = useState(0);

  useEffect(() => {
    commands.listDisplays().then(setDisplays, (e: unknown) => {
      notify("error", "Could not list displays", errorMessage(e));
    });
  }, [notify]);
  useEffect(() => {
    commands.listWindows().then(setWindows, (e: unknown) => {
      notify("error", "Could not list windows", errorMessage(e));
    });
  }, [windowsReload, notify]);
  const loadWindows = () => {
    setWindowsReload((n) => n + 1);
  };

  if (!settings) return null;
  const c = settings.capture;
  const err = (f: string) => fieldError?.field?.startsWith(f) ?? false;
  const primary = displays.find((d) => d.primary);
  const selectedDisplay = displays.find((d) => d.id === c.monitorId) ?? primary;
  const savedWindowFound =
    c.window !== null && windows.some((w) => w.exe.toLowerCase() === c.window?.exe.toLowerCase());

  const setMode = (mode: CaptureMode) => {
    if (mode === "window" && !c.window) {
      const first = windows[0];
      if (!first) {
        notify("warning", "No windows to capture", "Open the game or app first, then refresh.");
        return;
      }
      update((d) => {
        d.capture.mode = mode;
        d.capture.window = { exe: first.exe, title: first.title };
      });
      return;
    }
    if (mode === "region" && !c.region) {
      void commands.openRegionPicker(c.monitorId ?? undefined);
      return;
    }
    update((d) => {
      d.capture.mode = mode;
    });
  };

  return (
    <>
      <Section
        title="Video source"
        description="What the replay buffer records."
        keywords="capture screen monitor"
      >
        <Field label="Capture" error={err("capture.window") || err("capture.region")}>
          <Segmented<CaptureMode>
            value={c.mode}
            options={[
              { value: "monitor", label: "Display" },
              { value: "window", label: "Window" },
              { value: "region", label: "Region" },
            ]}
            onChange={setMode}
          />
        </Field>

        {c.mode !== "window" && (
          <Field
            label="Display"
            description={
              selectedDisplay
                ? `${selectedDisplay.width}×${selectedDisplay.height} · ${Math.round(selectedDisplay.refreshHz)} Hz${selectedDisplay.adapter ? ` · ${selectedDisplay.adapter}` : ""}`
                : undefined
            }
          >
            {selectedDisplay?.hdr && <Badge tone="warn">HDR → SDR</Badge>}
            <Select<string>
              value={c.monitorId ?? "__primary"}
              options={[
                {
                  value: "__primary",
                  label: `Primary display${primary ? ` (${primary.name})` : ""}`,
                },
                ...displays.map((d) => ({
                  value: d.id,
                  label: `${d.name} - ${d.width}×${d.height}${d.primary ? " (primary)" : ""}`,
                })),
              ]}
              onChange={(v) => {
                update((d) => {
                  d.capture.monitorId = v === "__primary" ? null : v;
                });
              }}
            />
          </Field>
        )}

        {c.mode === "window" && (
          <Field
            label="Window"
            description={
              c.window && !savedWindowFound
                ? `"${c.window.title}" is not running - capture starts when it appears.`
                : "Matched by program name, so it is found again after restarts."
            }
          >
            <Select<string>
              value={c.window ? `${c.window.exe}\u0000${c.window.title}` : ""}
              width={320}
              options={[
                ...(c.window &&
                !windows.some((w) => w.exe === c.window?.exe && w.title === c.window.title)
                  ? [
                      {
                        value: `${c.window.exe}\u0000${c.window.title}`,
                        label: `${c.window.title} (${c.window.exe})`,
                      },
                    ]
                  : []),
                ...windows.map((w) => ({
                  value: `${w.exe}\u0000${w.title}`,
                  label: `${w.title} (${w.exe})${w.fullscreen ? " · fullscreen" : ""}`,
                })),
              ]}
              onChange={(v) => {
                const [exe = "", title = ""] = v.split("\u0000");
                update((d) => {
                  d.capture.window = { exe, title };
                });
              }}
            />
            <Button
              small
              icon="refresh"
              variant="ghost"
              title="Refresh window list"
              onClick={loadWindows}
            />
          </Field>
        )}

        {c.mode === "region" && (
          <Field
            label="Region"
            description={
              c.region
                ? `${c.region.width}×${c.region.height} at ${c.region.x}, ${c.region.y}`
                : "No region selected"
            }
          >
            <Button
              icon="region"
              onClick={() =>
                void commands.openRegionPicker(c.region?.monitorId ?? c.monitorId ?? undefined)
              }
            >
              Select region…
            </Button>
          </Field>
        )}

        <Field label="Capture mouse cursor">
          <Toggle
            value={c.captureCursor}
            onChange={(v) => {
              update((d) => {
                d.capture.captureCursor = v;
              });
            }}
          />
        </Field>
      </Section>

      <Section
        title="Video quality"
        description="Higher settings use more GPU encoder time and bigger files. If games stutter, pick a lower one."
        keywords="quality preset low standard high native custom bitrate resolution fps"
      >
        <div className="quality-cards-row">
          <QualityCards
            value={c}
            nativeSize={
              selectedDisplay ? `${selectedDisplay.width}×${selectedDisplay.height}` : undefined
            }
            onPick={(p) => {
              update((d) => {
                applyPreset(d.capture, p);
              });
            }}
            onCustom={() => {
              document.getElementById("capture-resolution")?.focus();
            }}
          />
        </div>
        <Field label="Resolution" description="Downscaling never upscales smaller sources.">
          <Select
            id="capture-resolution"
            value={c.scale}
            options={SCALE_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.capture.scale = v;
              });
            }}
          />
        </Field>
        <Field label="Frame rate" error={err("capture.fps")}>
          <Select
            value={c.fps}
            options={FPS_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.capture.fps = v;
              });
            }}
          />
        </Field>
        <Field
          label="Bitrate"
          description={
            status?.videoBitrateKbps
              ? `Auto follows the resolution · currently ${formatBitrate(status.videoBitrateKbps)}`
              : "Auto follows the resolution."
          }
          error={err("capture.customBitrateKbps")}
        >
          <Select<"auto" | number>
            value={c.quality === "custom" ? c.customBitrateKbps : "auto"}
            options={
              c.quality === "custom" &&
              !BITRATE_OPTIONS.some((o) => o.value === c.customBitrateKbps)
                ? [
                    ...BITRATE_OPTIONS,
                    { value: c.customBitrateKbps, label: formatBitrate(c.customBitrateKbps) },
                  ]
                : BITRATE_OPTIONS
            }
            onChange={(v) => {
              update((d) => {
                if (v === "auto") d.capture.quality = "high";
                else {
                  d.capture.quality = "custom";
                  d.capture.customBitrateKbps = v;
                }
              });
            }}
          />
        </Field>
        <Field
          label="Encoder speed"
          description="Performance is lightest on the GPU; Quality squeezes more detail into the same bitrate."
        >
          <Segmented
            value={c.encoderSpeed}
            options={ENCODER_SPEED_OPTIONS}
            onChange={(v) => {
              update((d) => {
                d.capture.encoderSpeed = v;
              });
            }}
          />
        </Field>
        <Field
          label="Codec"
          description="H.264 plays everywhere; HEVC/AV1 are smaller but less compatible."
        >
          <Select
            value={c.videoCodec}
            options={codecOptions(caps)}
            onChange={(v) => {
              update((d) => {
                d.capture.videoCodec = v;
              });
            }}
          />
        </Field>
        <Field label="Encoder">
          <Select
            value={c.encoder}
            options={encoderOptions(caps, c.videoCodec)}
            onChange={(v) => {
              update((d) => {
                d.capture.encoder = v;
              });
            }}
          />
        </Field>
      </Section>

      <Section
        title="Replay buffer"
        description="Only the most recent material is kept; older footage is discarded continuously."
      >
        <Field
          label="Buffer length"
          description="How much Save clip keeps - the length of your clips (5 seconds to 10 minutes)."
          error={err("capture.bufferSeconds")}
        >
          <Slider
            value={c.bufferSeconds}
            min={5}
            max={600}
            step={5}
            width={180}
            onChange={(v) => {
              update((d) => {
                d.capture.bufferSeconds = v;
              });
            }}
          />
          <NumberInput
            value={c.bufferSeconds}
            min={5}
            max={600}
            suffix="s"
            width={64}
            onChange={(v) => {
              update((d) => {
                d.capture.bufferSeconds = v;
              });
            }}
          />
        </Field>
        <Field label="Presets">
          <div className="row wrap">
            {BUFFER_PRESETS.map((s) => (
              <Button
                key={s}
                small
                variant={c.bufferSeconds === s ? "primary" : undefined}
                onClick={() => {
                  update((d) => {
                    d.capture.bufferSeconds = s;
                  });
                }}
              >
                {s < 60 ? `${s} s` : `${s / 60} min`}
              </Button>
            ))}
          </div>
        </Field>
        <Field label="Storage" description="Auto keeps short buffers in RAM and long ones on disk.">
          <Select
            value={c.bufferStorage}
            options={STORAGE_OPTIONS}
            onChange={(v) => {
              // Disk wears SSDs: confirm first.
              if (v === "disk") {
                setConfirmDisk(true);
                return;
              }
              update((d) => {
                d.capture.bufferStorage = v;
              });
            }}
          />
        </Field>
        {confirmDisk && (
          <ConfirmDialog
            title="Keep the replay buffer on disk?"
            confirmLabel="Use disk"
            onConfirm={() => {
              setConfirmDisk(false);
              update(
                (d) => {
                  d.capture.bufferStorage = "disk";
                },
                { immediate: true },
              );
            }}
            onCancel={() => {
              setConfirmDisk(false);
            }}
          >
            The buffer is written to your drive non-stop.{" "}
            <b>This wears out SSDs and can damage your drive over time.</b> Use RAM unless you need
            it.
          </ConfirmDialog>
        )}
        <Field
          label="Only while a game runs"
          description="Starts the buffer when a game is detected and stops it a minute after the game closes. Off: the buffer also runs on the desktop."
        >
          <Toggle
            label="Only while a game runs"
            value={c.bufferOnlyInGames}
            onChange={(v) => {
              update(
                (d) => {
                  d.capture.bufferOnlyInGames = v;
                },
                { immediate: true },
              );
            }}
          />
        </Field>
        <Field
          label="Size limit"
          description="Oldest footage is dropped when the limit is reached."
          error={err("capture.bufferMaxMb")}
        >
          {c.bufferMaxMb !== null && (
            <NumberInput
              value={c.bufferMaxMb / 1024}
              min={1}
              max={20}
              suffix="GB"
              width={56}
              onChange={(v) => {
                update((d) => {
                  d.capture.bufferMaxMb = Math.round(v) * 1024;
                });
              }}
            />
          )}
          <Toggle
            value={c.bufferMaxMb !== null}
            onChange={(v) => {
              update((d) => {
                d.capture.bufferMaxMb = v ? 4096 : null;
              });
            }}
          />
        </Field>
      </Section>

      {status?.captureApi && (
        <Banner tone="info">
          Capturing with <b>{status.captureApi}</b> using <b>{status.encoder}</b>
          {status.zeroCopy ? " - frames stay on the GPU (zero-copy)." : "."} Exclusive-fullscreen
          games may show black frames; use borderless/windowed mode for those.
        </Banner>
      )}
    </>
  );
}

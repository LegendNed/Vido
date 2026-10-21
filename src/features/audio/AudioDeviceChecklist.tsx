import { useState, type ReactNode } from "react";

import { Icon } from "../../components/Icon";
import { Badge, Banner, Button, Checkbox, Meter } from "../../components/controls";
import type {
  AudioDeviceInfo,
  AudioSettings,
  AudioTrackSettings,
  AudioTrackStatus,
  DeviceKind,
} from "../../lib/tauri/types";
import {
  deviceRows,
  enabledCount,
  MAX_AUDIO_TRACKS,
  recordedTwice,
  setDeviceChecked,
  type DeviceRow,
} from "./devices";

export interface Level {
  level: number;
  peak: number;
}

const GROUPS: {
  direction: DeviceKind;
  title: string;
  description: string;
  icon: "speaker" | "mic";
}[] = [
  {
    direction: "output",
    title: "Outputs",
    description: "System and game sound, recorded from speakers or headphones.",
    icon: "speaker",
  },
  {
    direction: "input",
    title: "Inputs",
    description: "Microphones and line-in.",
    icon: "mic",
  },
];

/**
 * Every output and input device with a checkbox; each ticked device is recorded on
 * its own track. Used by the Audio page and (without meters/settings) the first-run wizard.
 */
export function AudioDeviceChecklist({
  audio,
  devices,
  onEdit,
  status,
  levels,
  renderSettings,
}: {
  audio: AudioSettings;
  /** `null` while the device list is loading. */
  devices: AudioDeviceInfo[] | null;
  onEdit: (mutate: (audio: AudioSettings) => void) => void;
  /** Live per-track state while the buffer runs. */
  status?: AudioTrackStatus[];
  /** Meter levels per track id; meters are shown only when given. */
  levels?: Record<string, Level>;
  /** Settings of a ticked track, shown below its row when expanded. */
  renderSettings?: (track: AudioTrackSettings, index: number) => ReactNode;
}) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const full = enabledCount(audio) >= MAX_AUDIO_TRACKS;

  const toggleExpanded = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };

  const renderRow = (r: DeviceRow, current: AudioDeviceInfo | undefined) => {
    const t = r.checked ? r.track : undefined;
    const state = t && status?.find((s) => s.id === t.id)?.state;
    const sub: string[] = [];
    if (r.deviceId === null && devices) {
      sub.push(current ? `Now: ${current.name}` : "No device connected");
    }
    if (t && t.name !== r.label) sub.push(`Track “${t.name}”`);
    const open = t !== undefined && renderSettings !== undefined && expanded.has(t.id);
    const level = t && levels ? (levels[t.id] ?? { level: 0, peak: 0 }) : undefined;
    return (
      <li
        key={r.key}
        className={`device-row${r.checked ? " on" : ""}${r.connected ? "" : " missing"}`}
      >
        <div className="line">
          <Checkbox
            checked={r.checked}
            disabled={!r.checked && full}
            title={
              !r.checked && full ? `Up to ${MAX_AUDIO_TRACKS} sources can be recorded` : undefined
            }
            onChange={(v) => {
              onEdit((a) => {
                setDeviceChecked(a, r, v);
              });
            }}
          >
            <span className="device-label">
              <span className="name ellipsis">{r.label}</span>
              {sub.length > 0 && <span className="sub ellipsis">{sub.join(" · ")}</span>}
            </span>
          </Checkbox>
          {r.isDefault && <Badge>Default</Badge>}
          {r.deviceId !== null && !r.connected && <Badge tone="warn">Not connected</Badge>}
          {r.connected && state === "lost" && (
            <span title="Windows reports an error for this device. Vido keeps retrying and records silence meanwhile.">
              <Badge tone="err">Error</Badge>
            </span>
          )}
          {t?.muted && <Badge>Muted</Badge>}
          <span className="spacer" />
          {level && r.connected && <Meter level={level.level} peak={level.peak} width={120} />}
          {t && renderSettings && (
            <Button
              small
              variant="ghost"
              icon="sliders"
              title={open ? "Hide track settings" : "Track settings"}
              onClick={() => {
                toggleExpanded(t.id);
              }}
            />
          )}
        </div>
        {open && <div className="device-settings">{renderSettings(t, r.index)}</div>}
      </li>
    );
  };

  return (
    <>
      {GROUPS.map((g) => {
        const rows = deviceRows(audio, devices, g.direction);
        const current = devices?.find((d) => d.kind === g.direction && d.isDefault);
        const twice = recordedTwice(rows);
        return (
          <section key={g.direction} className="card device-group">
            <header>
              <Icon name={g.icon} />
              <div>
                <h2>{g.title}</h2>
                <p>{g.description}</p>
              </div>
            </header>
            <ul className="device-list">{rows.map((r) => renderRow(r, current))}</ul>
            {twice && (
              <Banner>
                “{rows[0]?.label}” is “{twice.label}” right now, so that sound is recorded twice
                {audio.mixedTrack ? " (and doubled in the mix)" : ""}. Untick one of them.
              </Banner>
            )}
          </section>
        );
      })}
    </>
  );
}

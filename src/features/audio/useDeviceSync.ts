import { useEffect, useState } from "react";

import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import type { AudioDeviceInfo } from "../../lib/tauri/types";
import { useApp } from "../../stores/app";
import { syncDevices } from "./devices";

/**
 * Keeps the saved audio devices current with what is connected (names recorded,
 * ids followed when a device comes back under a new one). Runs in the main window,
 * which lives for the whole session.
 */
export function useDeviceSync() {
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.update);
  const [devices, setDevices] = useState<AudioDeviceInfo[] | null>(null);

  useEffect(() => {
    commands.listAudioDevices().then(setDevices, () => undefined);
  }, []);
  useEvent("audio-devices-changed", setDevices);

  useEffect(() => {
    if (!settings || !devices) return;
    if (!syncDevices(structuredClone(settings.audio), devices)) return;
    update(
      (d) => {
        syncDevices(d.audio, devices);
      },
      { immediate: true },
    );
  }, [settings, devices, update]);
}

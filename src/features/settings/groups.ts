/** Settings pages, in order, under their category headings. */
export const SETTINGS_GROUPS = [
  { id: "video", label: "Video", category: "Recording" },
  { id: "audio", label: "Audio", category: "Recording" },
  { id: "hotkeys", label: "Hotkeys", category: "Recording" },
  { id: "overlay", label: "Overlay", category: "Recording" },
  { id: "saving", label: "Saving", category: "Clips" },
  { id: "export", label: "Export", category: "Clips" },
  { id: "general", label: "General", category: "App" },
  { id: "advanced", label: "Advanced", category: "App" },
] as const;

export type SettingsGroupId = (typeof SETTINGS_GROUPS)[number]["id"];

/** Older ids that now land on one of the groups. */
const ALIASES: Partial<Record<string, SettingsGroupId>> = {
  recording: "video",
  capture: "video",
};

/** The group an id refers to (a group id or an older alias), or `null`. */
export function settingsGroupOf(id: string): SettingsGroupId | null {
  const g = SETTINGS_GROUPS.find((x) => x.id === id);
  return g ? g.id : (ALIASES[id] ?? null);
}

/** Ask the Settings page to show a group; `seq` makes repeated requests distinct. */
export interface SettingsTarget {
  group: SettingsGroupId;
  seq: number;
}

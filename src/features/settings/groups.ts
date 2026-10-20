/** Groups on the Settings page, in order. */
export const SETTINGS_GROUPS = [
  { id: "recording", label: "Recording" },
  { id: "hotkeys", label: "Hotkeys" },
  { id: "overlay", label: "Overlay" },
  { id: "saving", label: "Saving" },
  { id: "export", label: "Export" },
  { id: "general", label: "General" },
  { id: "advanced", label: "Advanced" },
] as const;

export type SettingsGroupId = (typeof SETTINGS_GROUPS)[number]["id"];

export function isSettingsGroup(id: string): id is SettingsGroupId {
  return SETTINGS_GROUPS.some((g) => g.id === id);
}

/** Ask the Settings page to scroll to a group; `seq` makes repeated requests distinct. */
export interface SettingsTarget {
  group: SettingsGroupId;
  seq: number;
}

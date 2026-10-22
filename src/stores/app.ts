import { create } from "zustand";

import { commands } from "../lib/tauri/commands";
import {
  errorMessage,
  isAppError,
  type AppError,
  type AppInfo,
  type BufferStatus,
  type EncoderCapabilities,
  type Settings,
  type Severity,
  type UpdateInfo,
} from "../lib/tauri/types";

export interface NoticeAction {
  label: string;
  run: () => void;
}

export interface Notice {
  id: number;
  severity: Severity;
  title: string;
  hint?: string;
  actions?: NoticeAction[];
  at: number;
}

interface AppStore {
  info: AppInfo | null;
  settings: Settings | null;
  /** Last settings confirmed by Rust (rollback target). */
  confirmed: Settings | null;
  status: BufferStatus | null;
  caps: EncoderCapabilities | null;
  notices: Notice[];
  fieldError: AppError | null;
  /** The clip open in the editor modal (main window), if any. */
  editor: EditorRequest | null;
  openEditor: (path: string, from?: EditorRequest["from"], at?: number) => void;
  closeEditor: () => void;
  /**
   * The setup wizard was reopened (not persisted): `true` keeps the current settings
   * as its starting point, `"fresh"` follows a reset (it suggests a quality again).
   */
  redoSetup: boolean | "fresh";
  /** A newer version on GitHub (from the last check). */
  pendingUpdate: UpdateInfo | null;
  setPendingUpdate: (u: UpdateInfo | null) => void;
  /** The update dialog is open. */
  updateOpen: boolean;
  setUpdateOpen: (open: boolean) => void;
  setRedoSetup: (v: boolean | "fresh") => void;
  init: () => Promise<void>;
  setStatus: (s: BufferStatus) => void;
  setSettingsFromBackend: (s: Settings) => void;
  /** Optimistically applies a change; persisted (validated by Rust) after a short debounce. */
  update: (mutate: (draft: Settings) => void, opts?: { immediate?: boolean }) => void;
  flush: () => Promise<boolean>;
  notify: (severity: Severity, title: string, hint?: string, actions?: NoticeAction[]) => void;
  dismiss: (id: number) => void;
  loadCaps: (refresh?: boolean) => Promise<void>;
}

export interface EditorRequest {
  path: string;
  /** Screen rect the editor grows from (the clip preview dialog). */
  from?: { left: number; top: number; width: number; height: number };
  /** Start at this time (seconds), e.g. where the preview was. */
  at?: number;
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let nextNoticeId = 1;

export const useApp = create<AppStore>((set, get) => ({
  info: null,
  settings: null,
  confirmed: null,
  status: null,
  caps: null,
  notices: [],
  fieldError: null,
  redoSetup: false,
  pendingUpdate: null,
  updateOpen: false,
  editor: null,

  openEditor: (path, from, at) => {
    set({ editor: { path, from, at } });
  },
  closeEditor: () => {
    set({ editor: null });
  },

  setRedoSetup: (redoSetup) => {
    set({ redoSetup });
  },

  setPendingUpdate: (pendingUpdate) => {
    set({ pendingUpdate });
  },

  setUpdateOpen: (updateOpen) => {
    set({ updateOpen });
  },

  init: async () => {
    const [info, settings, status, pendingUpdate] = await Promise.all([
      commands.getAppInfo(),
      commands.getSettings(),
      commands.getBufferStatus(),
      commands.getPendingUpdate(),
    ]);
    set({ info, settings, confirmed: settings, status, pendingUpdate });
    void get().loadCaps();
  },

  setStatus: (status) => {
    set({ status });
  },

  setSettingsFromBackend: (s) => {
    // Ignore echoes while a local edit is pending.
    if (saveTimer) return;
    set({ settings: s, confirmed: s });
  },

  update: (mutate, opts) => {
    const cur = get().settings;
    if (!cur) return;
    const draft = structuredClone(cur);
    mutate(draft);
    set({ settings: draft, fieldError: null });
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(
      () => {
        saveTimer = undefined;
        void get().flush();
      },
      opts?.immediate ? 0 : 350,
    );
  },

  flush: async () => {
    const draft = get().settings;
    if (!draft) return false;
    try {
      const saved = await commands.updateSettings(draft);
      set({ settings: saved, confirmed: saved, fieldError: null });
      void commands.getAppInfo().then((info) => {
        set({ info });
      });
      return true;
    } catch (e) {
      set({ settings: get().confirmed, fieldError: isAppError(e) ? e : null });
      get().notify("error", "Settings not saved", errorMessage(e));
      return false;
    }
  },

  notify: (severity, title, hint, actions) => {
    const n: Notice = {
      id: nextNoticeId++,
      severity,
      title,
      at: Date.now(),
      ...(hint ? { hint } : {}),
      ...(actions?.length ? { actions } : {}),
    };
    set({ notices: [...get().notices.slice(-4), n] });
    if (severity !== "error") {
      setTimeout(
        () => {
          get().dismiss(n.id);
        },
        n.actions ? 12000 : 6000,
      );
    }
  },

  dismiss: (id) => {
    set({ notices: get().notices.filter((n) => n.id !== id) });
  },

  loadCaps: async (refresh = false) => {
    try {
      set({ caps: await commands.getEncoderCapabilities(refresh) });
    } catch (e) {
      get().notify("warning", "Could not detect encoders", errorMessage(e));
    }
  },
}));

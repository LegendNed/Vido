import { PanelLeft } from "lucide-react";
import { useEffect, useState } from "react";

import { Notices } from "../../components/Notices";
import { StatusBar } from "../../components/StatusBar";
import { Banner, Spinner } from "../../components/controls";
import { Button as UiButton } from "../../components/ui/button";
import { Separator } from "../../components/ui/misc";
import { windowInit } from "../../lib/init";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage } from "../../lib/tauri/types";
import { useBackendSync } from "../../lib/useBackendSync";
import { useApp } from "../../stores/app";
import { useDeviceSync } from "../audio/useDeviceSync";
import { ClipsPage } from "../clips/ClipsPage";
import { EditorModal } from "../editor/EditorModal";
import { OverviewPage } from "../overview/OverviewPage";
import { FirstRun } from "../settings/FirstRun";
import {
  SETTINGS_GROUPS,
  settingsGroupOf,
  type SettingsGroupId,
  type SettingsTarget,
} from "../settings/groups";
import { SettingsPage } from "../settings/SettingsPage";
import { AppSidebar, type PageId } from "./AppSidebar";
import { WindowControls } from "./WindowControls";

const PAGE_TITLES: Record<PageId, string> = {
  overview: "Overview",
  clips: "Clips",
  settings: "Settings",
};

function isPage(p: string | null | undefined): p is PageId {
  return p === "overview" || p === "clips" || p === "settings";
}

/**
 * Resolves a page id from `navigate` / window init: a page, a Settings group
 * (`video`, `audio`, `hotkeys`, `overlay`, `saving`, …: Settings opened on it) or an old id.
 */
function resolvePage(
  p: string | null | undefined,
): { page: PageId; group?: SettingsGroupId } | null {
  if (!p) return null;
  if (isPage(p)) return { page: p };
  const group = settingsGroupOf(p);
  return group ? { page: "settings", group } : null;
}

export function MainWindow() {
  const init = useApp((s) => s.init);
  const settings = useApp((s) => s.settings);
  const status = useApp((s) => s.status);
  const redoSetup = useApp((s) => s.redoSetup);
  const update = useApp((s) => s.update);
  const [page, setPage] = useState<PageId>(() => resolvePage(windowInit().page)?.page ?? "clips");
  const [target, setTarget] = useState<SettingsTarget | null>(() => {
    const group = resolvePage(windowInit().page)?.group;
    return group ? { group, seq: 1 } : null;
  });
  const [loadError, setLoadError] = useState<string | null>(null);
  /** A clip to open in the Clips page viewer (picked on Overview). */
  const [openPath, setOpenPath] = useState<string | null>(null);

  useEffect(() => {
    init().catch((e: unknown) => {
      setLoadError(errorMessage(e));
    });
  }, [init]);
  useBackendSync();
  useDeviceSync();
  // The editor opens as a modal here (hotkey, after saving, tray, window init).
  const openEditor = useApp((s) => s.openEditor);
  useEffect(() => {
    const clip = windowInit().clip;
    if (clip) openEditor(clip);
  }, [openEditor]);
  useEvent("editor-open", (e) => {
    setPage("clips");
    openEditor(e.path);
  });
  useEvent("navigate", (n) => {
    const r = resolvePage(n.page);
    if (!r) return;
    setPage(r.page);
    const group = r.group;
    if (group) setTarget((t) => ({ group, seq: (t?.seq ?? 0) + 1 }));
  });
  const select = (p: PageId) => {
    setOpenPath(null);
    setPage(p);
    setTarget(null);
  };
  const openClip = (path: string) => {
    setOpenPath(path);
    setPage("clips");
    setTarget(null);
  };
  const go = (id: string) => {
    const r = resolvePage(id);
    if (!r) return;
    setOpenPath(null);
    setPage(r.page);
    const group = r.group;
    setTarget(group ? (t) => ({ group, seq: (t?.seq ?? 0) + 1 }) : null);
  };

  if (loadError) {
    return (
      <div className="empty">
        <h3>Vido could not load</h3>
        <p>{loadError}</p>
      </div>
    );
  }
  if (!settings) {
    return (
      <div className="empty">
        <Spinner />
      </div>
    );
  }

  const sidebar = settings.general.sidebar;
  const collapsed = sidebar === "small";
  const group: SettingsGroupId = target?.group ?? SETTINGS_GROUPS[0].id;
  const groupLabel = SETTINGS_GROUPS.find((g) => g.id === group)?.label;
  const toggleSidebar = () => {
    update(
      (d) => {
        d.general.sidebar = d.general.sidebar === "full" ? "small" : "full";
      },
      { immediate: true },
    );
  };
  return (
    <div className="flex h-full overflow-hidden">
      {sidebar !== "none" && (
        <AppSidebar
          page={page}
          group={page === "settings" ? group : null}
          collapsed={collapsed}
          onPage={select}
          onSettings={(g) => {
            go(g);
          }}
        />
      )}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* The window's title bar too: drag it (double-click maximizes); the caption
            buttons sit at the right end. */}
        <header
          data-tauri-drag-region
          className="flex h-14 shrink-0 items-center gap-2 border-b px-3"
        >
          <UiButton
            variant="ghost"
            size="icon-sm"
            title={sidebar === "full" ? "Collapse sidebar" : "Expand sidebar"}
            aria-label="Toggle sidebar"
            onClick={toggleSidebar}
          >
            <PanelLeft />
          </UiButton>
          <Separator orientation="vertical" className="mr-1 !h-4" />
          <nav
            data-tauri-drag-region
            aria-label="Breadcrumb"
            className="flex min-w-0 items-center gap-1.5 text-sm"
          >
            <span
              data-tauri-drag-region
              className={page === "settings" ? "text-muted-foreground" : "font-medium"}
            >
              {PAGE_TITLES[page]}
            </span>
            {page === "settings" && groupLabel && (
              <>
                <span data-tauri-drag-region className="text-muted-foreground">
                  /
                </span>
                <span data-tauri-drag-region className="truncate font-medium">
                  {groupLabel}
                </span>
              </>
            )}
          </nav>
          <StatusBar />
          <WindowControls />
        </header>
        <main
          className={
            page === "settings"
              ? "flex min-h-0 flex-1 flex-col overflow-hidden"
              : "min-h-0 flex-1 overflow-y-auto px-6 pb-10 pt-6"
          }
        >
          {status?.warnings.map((w) => (
            <div key={w} className="mx-auto mb-3 max-w-3xl">
              <Banner>{w}</Banner>
            </div>
          ))}
          {page === "overview" && <OverviewPage go={go} openClip={openClip} />}
          {page === "clips" && <ClipsPage key={openPath ?? ""} open={openPath} />}
          {page === "settings" && <SettingsPage key={target?.seq ?? 0} target={target} />}
        </main>
      </div>
      <EditorModal />
      {(!settings.firstRunCompleted || redoSetup) && <FirstRun />}
      <Notices />
    </div>
  );
}

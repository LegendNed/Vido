import { useEffect, useState } from "react";

import { Icon, type IconName } from "../../components/Icon";
import { Notices } from "../../components/Notices";
import { StatusBar } from "../../components/StatusBar";
import { Banner, Spinner } from "../../components/ui";
import { windowInit } from "../../lib/init";
import { useEvent } from "../../lib/tauri/events";
import { errorMessage } from "../../lib/tauri/types";
import { useBackendSync } from "../../lib/useBackendSync";
import { useApp } from "../../stores/app";
import { useDeviceSync } from "../audio/useDeviceSync";
import { ClipsPage } from "../clips/ClipsPage";
import { HotkeysPage } from "../hotkeys/HotkeysPage";
import { OverlayPage } from "../overlay/OverlayPage";
import { RecordingPage } from "../recording/RecordingPage";
import { FirstRun } from "../settings/FirstRun";
import { isSettingsGroup, type SettingsGroupId, type SettingsTarget } from "../settings/groups";
import { SettingsPage } from "../settings/SettingsPage";

const PAGES = [
  { id: "clips", label: "Clips", icon: "film" },
  { id: "recording", label: "Recording", icon: "record" },
  { id: "hotkeys", label: "Hotkeys", icon: "keyboard" },
  { id: "overlay", label: "Overlay", icon: "overlay" },
  { id: "settings", label: "Settings", icon: "cog" },
] as const satisfies readonly { id: string; label: string; icon: IconName }[];

type PageId = (typeof PAGES)[number]["id"];

function isPage(p: string | null | undefined): p is PageId {
  return PAGES.some((x) => x.id === p);
}

/** Page ids from before the regrouping that are pages of their own again. */
const OLD_PAGES: Partial<Record<string, PageId>> = {
  capture: "recording",
  audio: "recording",
};

/**
 * Resolves a page id from `navigate` / window init: a page, a Settings group
 * (`saving`, `export`, `general`, `advanced`, …: Settings scrolled to it) or an old id.
 */
function resolvePage(
  p: string | null | undefined,
): { page: PageId; group?: SettingsGroupId } | null {
  if (!p) return null;
  if (isPage(p)) return { page: p };
  const old = OLD_PAGES[p];
  if (old) return { page: old };
  if (isSettingsGroup(p)) return { page: "settings", group: p };
  return null;
}

/**
 * Store-style navigation rail: icon above a label ("full") or icons only ("small").
 * Clips stands apart at the top and Settings sits at the bottom, like in Windows apps.
 */
function Rail({
  page,
  small,
  onSelect,
}: {
  page: PageId;
  small: boolean;
  onSelect: (p: PageId) => void;
}) {
  const info = useApp((s) => s.info);
  const item = (p: (typeof PAGES)[number]) => (
    <button
      key={p.id}
      type="button"
      className={`nav-item${page === p.id ? " active" : ""}`}
      aria-current={page === p.id ? "page" : undefined}
      // Small rails show no labels, and the selected item drops its label (Store style).
      title={small || page === p.id ? p.label : undefined}
      onClick={() => {
        onSelect(p.id);
      }}
    >
      <Icon name={p.icon} size={20} />
      <span className="nav-label">{p.label}</span>
    </button>
  );
  const edge = (p: { id: PageId }) => p.id === "clips" || p.id === "settings";
  return (
    <nav className="sidebar" aria-label="Pages">
      {PAGES.filter((p) => p.id === "clips").map(item)}
      <div className="nav-sep" />
      {PAGES.filter((p) => !edge(p)).map(item)}
      <div className="nav-fill" />
      {PAGES.filter((p) => p.id === "settings").map(item)}
      {!small && info && (
        <div
          className="foot"
          title={`Vido ${info.version}${info.portable ? " (portable)" : ""} · local only, no telemetry`}
        >
          v{info.version}
          {info.portable && <div>portable</div>}
        </div>
      )}
    </nav>
  );
}

/** Tab strip above the content when the rail is turned off. */
function Pivot({ page, onSelect }: { page: PageId; onSelect: (p: PageId) => void }) {
  return (
    <nav className="pivot" aria-label="Pages">
      {PAGES.map((p) => (
        <button
          key={p.id}
          type="button"
          className={`pivot-item${page === p.id ? " active" : ""}`}
          aria-current={page === p.id ? "page" : undefined}
          onClick={() => {
            onSelect(p.id);
          }}
        >
          {p.label}
        </button>
      ))}
    </nav>
  );
}

export function MainWindow() {
  const init = useApp((s) => s.init);
  const settings = useApp((s) => s.settings);
  const status = useApp((s) => s.status);
  const redoSetup = useApp((s) => s.redoSetup);
  const [page, setPage] = useState<PageId>(() => resolvePage(windowInit().page)?.page ?? "clips");
  const [target, setTarget] = useState<SettingsTarget | null>(() => {
    const group = resolvePage(windowInit().page)?.group;
    return group ? { group, seq: 1 } : null;
  });
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    init().catch((e: unknown) => {
      setLoadError(errorMessage(e));
    });
  }, [init]);
  useBackendSync();
  useDeviceSync();
  useEvent("navigate", (n) => {
    const r = resolvePage(n.page);
    if (!r) return;
    setPage(r.page);
    const group = r.group;
    if (group) setTarget((t) => ({ group, seq: (t?.seq ?? 0) + 1 }));
  });
  const select = (p: PageId) => {
    setPage(p);
    setTarget(null);
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
  return (
    <div className="shell" data-sidebar={sidebar}>
      <StatusBar />
      {sidebar !== "none" && <Rail page={page} small={sidebar === "small"} onSelect={select} />}
      <main className={`content${page === "settings" ? " fill" : ""}`}>
        {sidebar === "none" && <Pivot page={page} onSelect={select} />}
        {status?.warnings.map((w) => (
          <div key={w} className="content-banner">
            <Banner>{w}</Banner>
          </div>
        ))}
        {page === "clips" && <ClipsPage />}
        {page === "recording" && <RecordingPage />}
        {page === "hotkeys" && <HotkeysPage />}
        {page === "overlay" && <OverlayPage />}
        {page === "settings" && <SettingsPage target={target} />}
      </main>
      {(!settings.firstRunCompleted || redoSetup) && <FirstRun />}
      <Notices />
    </div>
  );
}

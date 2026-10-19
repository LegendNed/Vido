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
import { AudioPage } from "../audio/AudioPage";
import { CapturePage } from "../capture/CapturePage";
import { ClipsPage } from "../clips/ClipsPage";
import { ExportDefaultsPage } from "../export/ExportDefaultsPage";
import { HotkeysPage } from "../hotkeys/HotkeysPage";
import { AdvancedPage } from "../settings/AdvancedPage";
import { FirstRun } from "../settings/FirstRun";
import { SavingPage } from "../settings/SavingPage";

const PAGES = [
  { id: "clips", label: "Clips", icon: "film" },
  { id: "capture", label: "Capture", icon: "monitor" },
  { id: "audio", label: "Audio", icon: "speaker" },
  { id: "hotkeys", label: "Hotkeys", icon: "keyboard" },
  { id: "saving", label: "Saving", icon: "save" },
  { id: "export", label: "Export", icon: "sliders" },
  { id: "advanced", label: "Advanced", icon: "cog" },
] as const satisfies readonly { id: string; label: string; icon: IconName }[];

type PageId = (typeof PAGES)[number]["id"];

function isPage(p: string | null | undefined): p is PageId {
  return PAGES.some((x) => x.id === p);
}

/**
 * Store-style navigation rail: icon above a label ("full") or icons only ("small").
 * Clips stands apart at the top and Advanced sits at the bottom, like Settings in
 * Windows apps.
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
  const edge = (p: { id: PageId }) => p.id === "clips" || p.id === "advanced";
  return (
    <nav className="sidebar" aria-label="Pages">
      {PAGES.filter((p) => p.id === "clips").map(item)}
      <div className="nav-sep" />
      {PAGES.filter((p) => !edge(p)).map(item)}
      <div className="nav-fill" />
      {PAGES.filter((p) => p.id === "advanced").map(item)}
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
  const [page, setPage] = useState<PageId>(() => {
    const p = windowInit().page;
    return isPage(p) ? p : "clips";
  });
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    init().catch((e: unknown) => {
      setLoadError(errorMessage(e));
    });
  }, [init]);
  useBackendSync();
  useEvent("navigate", (n) => {
    if (isPage(n.page)) setPage(n.page);
  });

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
      {sidebar !== "none" && <Rail page={page} small={sidebar === "small"} onSelect={setPage} />}
      <main className="content">
        {sidebar === "none" && <Pivot page={page} onSelect={setPage} />}
        {status?.warnings.map((w) => (
          <div key={w} className="content-banner">
            <Banner>{w}</Banner>
          </div>
        ))}
        {page === "clips" && <ClipsPage />}
        {page === "capture" && <CapturePage />}
        {page === "audio" && <AudioPage />}
        {page === "hotkeys" && <HotkeysPage />}
        {page === "saving" && <SavingPage />}
        {page === "export" && <ExportDefaultsPage />}
        {page === "advanced" && <AdvancedPage />}
      </main>
      {!settings.firstRunCompleted && <FirstRun />}
      <Notices />
    </div>
  );
}

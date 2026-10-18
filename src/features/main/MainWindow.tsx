import { useEffect, useState } from "react";

import { Icon, type IconName } from "../../components/Icon";
import { Notices } from "../../components/Notices";
import { StatusBar } from "../../components/StatusBar";
import { Spinner } from "../../components/ui";
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

export function MainWindow() {
  const init = useApp((s) => s.init);
  const settings = useApp((s) => s.settings);
  const info = useApp((s) => s.info);
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

  return (
    <div className="shell">
      <StatusBar />
      <nav className="sidebar">
        {PAGES.map((p, i) => (
          <div key={p.id}>
            {i === 1 && <div className="nav-sep" />}
            <button
              type="button"
              className={`nav-item${page === p.id ? " active" : ""}`}
              onClick={() => {
                setPage(p.id);
              }}
            >
              <Icon name={p.icon} />
              {p.label}
            </button>
          </div>
        ))}
        <div className="foot">
          Vido {info?.version}
          {info?.portable ? " · portable" : ""}
          <br />
          Local only · no telemetry
        </div>
      </nav>
      <main className="content">
        {status?.warnings.map((w) => (
          <div key={w} style={{ maxWidth: 860, marginBottom: 12 }}>
            <div className="banner">
              <Icon name="alert" />
              <div>{w}</div>
            </div>
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

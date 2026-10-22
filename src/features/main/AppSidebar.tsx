import { useEffect, useState } from "react";

import idlePhrases from "../../../config/idle-phrases.json";
import vidoIcon from "../../assets/vido-icon.png";
import { Icon, type IconName } from "../../components/Icon";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "../../components/ui/misc";
import { formatBitrate, formatBytes, formatTime } from "../../lib/format";
import { commands } from "../../lib/tauri/commands";
import { useEvent } from "../../lib/tauri/events";
import { cn } from "../../lib/utils";
import { useApp } from "../../stores/app";
import type { SettingsGroupId } from "../settings/groups";

export type PageId = "overview" | "clips" | "settings";

const LIBRARY: { id: PageId; label: string; icon: IconName }[] = [
  { id: "overview", label: "Overview", icon: "home" },
  { id: "clips", label: "Clips", icon: "film" },
];

/** Settings pages in the sidebar, under shadcn-style group labels. */
const SETTINGS: {
  label: string;
  items: { id: SettingsGroupId; label: string; icon: IconName }[];
}[] = [
  {
    label: "Recording",
    items: [
      { id: "video", label: "Video", icon: "monitor" },
      { id: "audio", label: "Audio", icon: "speaker" },
      { id: "hotkeys", label: "Hotkeys", icon: "keyboard" },
      { id: "overlay", label: "Overlay", icon: "overlay" },
    ],
  },
  {
    label: "Clips",
    items: [
      { id: "saving", label: "Saving", icon: "save" },
      { id: "export", label: "Export", icon: "sliders" },
    ],
  },
  {
    label: "App",
    items: [
      { id: "general", label: "General", icon: "cog" },
      { id: "advanced", label: "Advanced", icon: "bolt" },
    ],
  },
];

function Item({
  label,
  icon,
  active,
  collapsed,
  onClick,
}: {
  label: string;
  icon: IconName;
  active: boolean;
  collapsed: boolean;
  onClick: () => void;
}) {
  const button = (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      data-active={active || undefined}
      onClick={onClick}
      className={cn(
        "flex h-8 w-full items-center gap-2 overflow-hidden rounded-md px-2 text-left text-sm outline-none ring-sidebar-ring transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 data-[active]:bg-sidebar-accent data-[active]:font-medium data-[active]:text-sidebar-accent-foreground",
        collapsed && "size-8 justify-center px-0",
      )}
    >
      <Icon name={icon} size={16} className="shrink-0" />
      {!collapsed && <span className="truncate">{label}</span>}
    </button>
  );
  if (!collapsed) return button;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
}

/** Buffer state and memory, capture source, resolution and bitrate in the sidebar footer. */
/** Shown under the buffer state while no game is detected (`config/idle-phrases.json`). */
const randomPhrase = () => idlePhrases[Math.floor(Math.random() * idlePhrases.length)] ?? "";

/** The game a clip saved now would be filed under, kept current by `game-changed`. */
function useGame(): string | null {
  const [game, setGame] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    commands.getCurrentGame().then(
      (g) => {
        if (alive) setGame(g);
      },
      () => {},
    );
    return () => {
      alive = false;
    };
  }, []);
  useEvent("game-changed", setGame);
  return game;
}

/** An idle phrase that changes every 45 s. */
function useIdlePhrase(): string {
  const [phrase, setPhrase] = useState(randomPhrase);
  useEffect(() => {
    const t = setInterval(() => {
      setPhrase(randomPhrase());
    }, 45_000);
    return () => {
      clearInterval(t);
    };
  }, []);
  return phrase;
}

function BufferStatus({ collapsed }: { collapsed: boolean }) {
  const status = useApp((s) => s.status);
  const game = useGame();
  const idle = useIdlePhrase();
  if (!status) return null;
  const activity = game ? `Playing ${game}` : idle;
  const live = status.state === "buffering";
  const memory = status.state !== "stopped" && status.sizeBytes > 0;
  const text = [
    status.recording
      ? `Recording ${formatTime(status.recording.elapsedSeconds, 0)}`
      : live
        ? `Buffering · ${Math.floor(status.bufferedSeconds)} s`
        : status.state === "stopped"
          ? "Buffer off"
          : status.state.charAt(0).toUpperCase() + status.state.slice(1),
    memory ? formatBytes(status.sizeBytes) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const video = [
    status.width > 0 ? `${status.width}×${status.height} @ ${Math.round(status.fps)} fps` : null,
    status.videoBitrateKbps > 0 ? formatBitrate(status.videoBitrateKbps) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const details = [status.source, video].filter((d): d is string => !!d);
  const dot = (
    <span
      className={cn(
        "mt-1.5 size-2 shrink-0 rounded-full",
        live || status.recording
          ? "bg-rec shadow-[0_0_0_3px] shadow-rec/20"
          : status.state === "stopped"
            ? "bg-muted-foreground/50"
            : "bg-warning",
      )}
    />
  );
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-md border bg-background/50 px-2.5 py-2 text-xs leading-5",
        collapsed && "justify-center px-0",
      )}
      title={collapsed ? `${text}\n${activity}` : undefined}
    >
      {dot}
      {!collapsed && (
        <div className="min-w-0">
          <div
            className="truncate font-medium"
            title={memory ? `Buffer memory in ${status.storage}` : undefined}
          >
            {text}
          </div>
          {details.map((d) => (
            <div key={d} className="truncate text-muted-foreground" title={d}>
              {d}
            </div>
          ))}
          <div className="truncate font-medium" title={activity}>
            {activity}
          </div>
          {status.droppedFrames > 0 && (
            <div className="truncate text-warning">{status.droppedFrames} frames dropped</div>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * shadcn-style sidebar: Overview and Clips, then the settings pages under group
 * labels. `collapsed` shows icons only (with tooltips).
 */
export function AppSidebar({
  page,
  group,
  collapsed,
  onPage,
  onSettings,
}: {
  page: PageId;
  group: SettingsGroupId | null;
  collapsed: boolean;
  onPage: (p: PageId) => void;
  onSettings: (g: SettingsGroupId) => void;
}) {
  const info = useApp((s) => s.info);
  return (
    <TooltipProvider>
      <aside
        className={cn(
          "flex h-full shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground transition-[width] duration-200",
          collapsed ? "w-14" : "w-60",
        )}
      >
        {/* Part of the title bar: drags the window. */}
        <div
          data-tauri-drag-region
          className={cn("flex h-14 items-center gap-2 px-3", collapsed && "justify-center px-0")}
        >
          <img
            src={vidoIcon}
            alt=""
            draggable={false}
            className="pointer-events-none size-8 shrink-0 rounded-lg"
          />
          {!collapsed && (
            <div data-tauri-drag-region className="min-w-0 leading-tight">
              <div data-tauri-drag-region className="text-sm font-semibold">
                Vido
              </div>
              {info && (
                <div data-tauri-drag-region className="truncate text-xs text-muted-foreground">
                  v{info.version}
                  {info.portable ? " · portable" : ""}
                </div>
              )}
            </div>
          )}
        </div>

        <nav
          className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-2 py-2"
          aria-label="Pages"
        >
          <div className="flex flex-col gap-1">
            {LIBRARY.map((p) => (
              <Item
                key={p.id}
                label={p.label}
                icon={p.icon}
                collapsed={collapsed}
                active={page === p.id}
                onClick={() => {
                  onPage(p.id);
                }}
              />
            ))}
          </div>
          {SETTINGS.map((g) => (
            <div key={g.label} className="flex flex-col gap-1">
              {collapsed ? (
                <div className="mx-auto my-1 h-px w-6 bg-sidebar-border" />
              ) : (
                <div className="px-2 pb-1 text-xs font-medium text-sidebar-foreground/60">
                  {g.label}
                </div>
              )}
              {g.items.map((i) => (
                <Item
                  key={i.id}
                  label={i.label}
                  icon={i.icon}
                  collapsed={collapsed}
                  active={page === "settings" && group === i.id}
                  onClick={() => {
                    onSettings(i.id);
                  }}
                />
              ))}
            </div>
          ))}
        </nav>

        <div className="p-2">
          <BufferStatus collapsed={collapsed} />
        </div>
      </aside>
    </TooltipProvider>
  );
}

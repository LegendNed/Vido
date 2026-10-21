// Small inline SVG icon set (stroke icons, 24x24 viewBox) — no icon font dependency.

const PATHS = {
  home: "M3 11l9-7 9 7M5 9.5V20h5v-6h4v6h5V9.5",
  gridSmall:
    "M4 4h4v4H4zM10 4h4v4h-4zM16 4h4v4h-4zM4 10h4v4H4zM10 10h4v4h-4zM16 10h4v4h-4zM4 16h4v4H4zM10 16h4v4h-4zM16 16h4v4h-4z",
  gridLarge: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
  film: "M4 5h16v14H4zM8 5v14M16 5v14M4 9h4M4 15h4M16 9h4M16 15h4",
  monitor: "M3 4h18v12H3zM8 20h8M12 16v4",
  speaker: "M4 9v6h4l5 4V5L8 9zM16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11",
  mic: "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3",
  keyboard: "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10",
  save: "M5 3h11l3 3v15H5zM8 3v5h7V3M8 21v-7h8v7",
  sliders: "M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M16 4v4M8 10v4M16 16v4",
  cog: "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  record: "M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10z",
  pause: "M8 5v14M16 5v14",
  play: "M7 4v16l13-8z",
  stop: "M6 6h12v12H6z",
  chevronLeft: "M15 5l-7 7 7 7",
  chevronRight: "M9 5l7 7-7 7",
  fullscreen: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
  fullscreenExit: "M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5",
  mute: "M4 9v6h4l5 4V5L8 9zM16 9.5l5 5M21 9.5l-5 5",
  folder: "M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  external: "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5",
  copy: "M9 9h11v11H9zM5 15H4V4h11v1",
  trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
  scissors: "M6 6a3 3 0 1 0 0 .01M6 18a3 3 0 1 0 0 .01M8.6 7.6 20 18M8.6 16.4 20 6",
  refresh: "M4 4v6h6M20 20v-6h-6M5 15a8 8 0 0 0 14 2M19 9A8 8 0 0 0 5 7",
  plus: "M12 5v14M5 12h14",
  x: "M6 6l12 12M18 6 6 18",
  check: "M5 12l5 5L20 7",
  undo: "M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3",
  redo: "M15 14l5-5-5-5M20 9H10a6 6 0 0 0 0 12h3",
  camera: "M4 8h3l2-3h6l2 3h3v11H4zM12 10a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z",
  crop: "M6 2v14a2 2 0 0 0 2 2h14M18 22V8a2 2 0 0 0-2-2H2",
  edit: "M4 20h4L19 9l-4-4L4 16zM14 6l4 4",
  stepBack: "M18 6v12L9 12zM6 6v12",
  stepFwd: "M6 6v12l9-6zM18 6v12",
  region: "M4 4h5M4 4v5M20 4h-5M20 4v5M4 20h5M4 20v-5M20 20h-5M20 20v-5",
  window: "M3 5h18v14H3zM3 9h18",
  alert: "M12 3 2 20h20zM12 10v4M12 17h.01",
  info: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v6M12 7.5h.01",
  bolt: "M13 2 4 14h7l-1 8 9-12h-7z",
  download: "M12 4v11M7 10l5 5 5-5M5 20h14",
  search: "M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5 20 20",
  // A screen with a small pill in its corner (the HUD).
  overlay: "M3 4h18v14H3zM13 7h5v3h-5zM9 21h6",
  // Bare glyphs for filled status icons (InfoBar style, see `SeverityIcon`).
  glyphInfo: "M12 11v6M12 7h.01",
  glyphWarning: "M12 6v7M12 17.5h.01",
  glyphError: "M8 8l8 8M16 8l-8 8",
  glyphSuccess: "M7 12.5l3.5 3.5L17 9",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({
  name,
  size = 16,
  filled = false,
  className,
}: {
  name: IconName;
  size?: number;
  filled?: boolean;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={filled ? 0 : 1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}

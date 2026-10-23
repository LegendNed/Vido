// App icons by name, drawn with lucide (shadcn/ui's icon set).

import {
  AppWindow,
  AudioLines,
  Ellipsis,
  Ban,
  Camera,
  Check,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleDot,
  CirclePause,
  CirclePlay,
  CircleStop,
  CircleX,
  Copy,
  Crop,
  Download,
  ExternalLink,
  Film,
  Folder,
  House,
  Info,
  Keyboard,
  LayoutGrid,
  Grid3x3,
  Maximize,
  Mic,
  Minimize,
  Monitor,
  Pause,
  PanelsTopLeft,
  Pencil,
  Play,
  Plus,
  Redo2,
  RefreshCw,
  Save,
  Scissors,
  Search,
  Settings,
  SkipBack,
  SkipForward,
  SlidersHorizontal,
  Square,
  SquareDashed,
  Star,
  TriangleAlert,
  Trash2,
  Undo2,
  Volume2,
  VolumeX,
  X,
  Zap,
  type LucideIcon,
} from "lucide-react";

const ICONS = {
  home: House,
  gridSmall: Grid3x3,
  gridLarge: LayoutGrid,
  film: Film,
  monitor: Monitor,
  speaker: Volume2,
  mic: Mic,
  keyboard: Keyboard,
  save: Save,
  sliders: SlidersHorizontal,
  cog: Settings,
  record: CircleDot,
  pause: Pause,
  play: Play,
  stop: Square,
  /** Buffer controls: circles, so Stop never looks like the window's Maximize. */
  bufferPause: CirclePause,
  bufferResume: CirclePlay,
  bufferStop: CircleStop,
  chevronLeft: ChevronLeft,
  chevronRight: ChevronRight,
  fullscreen: Maximize,
  fullscreenExit: Minimize,
  mute: VolumeX,
  folder: Folder,
  external: ExternalLink,
  copy: Copy,
  trash: Trash2,
  scissors: Scissors,
  refresh: RefreshCw,
  plus: Plus,
  x: X,
  check: Check,
  undo: Undo2,
  redo: Redo2,
  camera: Camera,
  crop: Crop,
  edit: Pencil,
  stepBack: SkipBack,
  stepFwd: SkipForward,
  region: SquareDashed,
  window: AppWindow,
  alert: TriangleAlert,
  info: Info,
  bolt: Zap,
  download: Download,
  search: Search,
  overlay: PanelsTopLeft,
  glyphInfo: Info,
  glyphWarning: CircleAlert,
  glyphError: CircleX,
  glyphSuccess: CircleCheck,
  blocked: Ban,
  star: Star,
  more: Ellipsis,
  audio: AudioLines,
} satisfies Record<string, LucideIcon>;

export type IconName = keyof typeof ICONS;

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
  const C = ICONS[name];
  return (
    <C
      size={size}
      className={className}
      fill={filled ? "currentColor" : "none"}
      strokeWidth={2}
      aria-hidden="true"
    />
  );
}

import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";

import { cn } from "../../lib/utils";

/**
 * Minimize / maximize / close for the undecorated main window, Windows-style:
 * full-height 46 px buttons at the right end of the top bar. Close goes through
 * the normal close request (hide to tray when that setting is on).
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const w = getCurrentWindow();
    let alive = true;
    const sync = () => {
      void w.isMaximized().then((m) => {
        if (alive) setMaximized(m);
      });
    };
    sync();
    const off = w.onResized(sync);
    return () => {
      alive = false;
      void off.then((f) => {
        f();
      });
    };
  }, []);

  const button =
    "flex h-full w-[46px] items-center justify-center text-foreground/80 outline-none transition-colors hover:bg-foreground/10 hover:text-foreground focus-visible:bg-foreground/10";
  return (
    <div className="-mr-3 ml-2 flex h-full shrink-0 self-stretch">
      <button
        type="button"
        className={button}
        title="Minimize"
        aria-label="Minimize"
        onClick={() => void getCurrentWindow().minimize()}
      >
        <Minus size={16} strokeWidth={1.5} />
      </button>
      <button
        type="button"
        className={button}
        title={maximized ? "Restore" : "Maximize"}
        aria-label={maximized ? "Restore" : "Maximize"}
        onClick={() => void getCurrentWindow().toggleMaximize()}
      >
        {maximized ? (
          <Copy size={13} strokeWidth={1.5} className="-scale-x-100" />
        ) : (
          <Square size={13} strokeWidth={1.5} />
        )}
      </button>
      <button
        type="button"
        className={cn(button, "hover:bg-[#c42b1c] hover:text-white focus-visible:bg-[#c42b1c]")}
        title="Close"
        aria-label="Close"
        onClick={() => void getCurrentWindow().close()}
      >
        <X size={17} strokeWidth={1.5} />
      </button>
    </div>
  );
}

import type { AccentColors, DisplayInfo } from "./tauri/types";

/** Data injected by Rust into each window (`window.__VIDO_INIT__`). */
export interface WindowInit {
  page?: string | null;
  clip?: string;
  display?: DisplayInfo;
  /** The window sits on a Mica backdrop (Windows 11): keep the page background transparent. */
  mica?: boolean;
  /** Windows accent palette at window creation; `null` if it could not be read. */
  accent?: AccentColors | null;
}

declare global {
  interface Window {
    __VIDO_INIT__?: WindowInit;
  }
}

export function windowInit(): WindowInit {
  return window.__VIDO_INIT__ ?? {};
}

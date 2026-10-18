import type { DisplayInfo } from "./tauri/types";

/** Data injected by Rust into each window (`window.__CLIPDECK_INIT__`). */
export interface WindowInit {
  page?: string | null;
  clip?: string;
  display?: DisplayInfo;
}

declare global {
  interface Window {
    __CLIPDECK_INIT__?: WindowInit;
  }
}

export function windowInit(): WindowInit {
  return window.__CLIPDECK_INIT__ ?? {};
}

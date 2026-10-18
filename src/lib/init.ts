import type { DisplayInfo } from "./tauri/types";

/** Data injected by Rust into each window (`window.__VIDO_INIT__`). */
export interface WindowInit {
  page?: string | null;
  clip?: string;
  display?: DisplayInfo;
}

declare global {
  interface Window {
    __VIDO_INIT__?: WindowInit;
  }
}

export function windowInit(): WindowInit {
  return window.__VIDO_INIT__ ?? {};
}

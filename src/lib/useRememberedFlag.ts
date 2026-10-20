import { useState } from "react";

function readFlag(key: string, fallback: boolean): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}

function writeFlag(key: string, value: boolean) {
  try {
    localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // Storage unavailable: the choice just isn't remembered.
  }
}

/**
 * A boolean UI preference (e.g. whether an expander is open) remembered in this
 * webview's local storage. Not a setting: it never reaches Rust.
 */
export function useRememberedFlag(key: string, fallback: boolean): [boolean, (v: boolean) => void] {
  const [value, setValue] = useState(() => readFlag(key, fallback));
  const set = (v: boolean) => {
    setValue(v);
    writeFlag(key, v);
  };
  return [value, set];
}

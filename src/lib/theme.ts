// Appearance: the Mica backdrop flag and the Windows accent color as CSS variables.
// Light/dark needs no code: CSS follows `prefers-color-scheme`, which the webview
// keeps in sync with the Windows app mode.

import { windowInit } from "./init";
import { commands } from "./tauri/commands";
import { on } from "./tauri/events";
import type { AccentColors } from "./tauri/types";

const SHADES = ["accent", "light1", "light2", "light3", "dark1", "dark2", "dark3"] as const;
const HEX = /^#[0-9a-f]{6}$/i;

/** WCAG relative luminance of a `#rrggbb` color. */
function luminance(hex: string): number {
  const n = parseInt(hex.slice(1), 16);
  const lin = (v: number) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
}

/** Black or white, whichever has more contrast on `hex`. */
function textOn(hex: string): string {
  const l = luminance(hex);
  return (l + 0.05) / 0.05 >= 1.05 / (l + 0.05) ? "#000000" : "#ffffff";
}

/**
 * Publishes the palette as `--sys-accent`, `--sys-accent-light1` … `--sys-accent-dark3`
 * plus a readable text color for each (`--sys-on-accent-light2`, …). `global.css` picks
 * the shades per theme and falls back to its own red when these are missing.
 */
export function applyAccent(colors: AccentColors | null | undefined) {
  if (!colors) return;
  const style = document.documentElement.style;
  for (const shade of SHADES) {
    const value = colors[shade];
    if (!HEX.test(value)) continue;
    const name = shade === "accent" ? "accent" : `accent-${shade}`;
    style.setProperty(`--sys-${name}`, value);
    style.setProperty(`--sys-on-${name}`, textOn(value));
  }
}

/** Runs once per window, before the first render. */
export function initAppearance() {
  const init = windowInit();
  if (init.mica) document.documentElement.classList.add("mica");
  if (init.accent) applyAccent(init.accent);
  else commands.getAccentColors().then(applyAccent, () => undefined);
  void on("accent-changed", applyAccent);
}

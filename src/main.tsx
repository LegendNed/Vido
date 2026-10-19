import { getCurrentWindow } from "@tauri-apps/api/window";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { initAppearance } from "./lib/theme";
import "./styles/global.css";
import "./styles/views.css";

// Lets CSS style per-window (e.g. transparent HUD / region overlay).
document.documentElement.classList.add(`win-${getCurrentWindow().label}`);
// Mica backdrop + Windows accent color (light/dark follows the system via CSS).
initAppearance();

// The webview's own context menu (reload, inspect) is not useful in an app.
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => {
    e.preventDefault();
  });
}

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

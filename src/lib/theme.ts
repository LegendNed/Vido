// Appearance: the shadcn theme's `.dark` class follows the Windows app mode, which
// the webview reports as `prefers-color-scheme` and keeps in sync live.

/** Runs once per window, before the first render. */
export function initAppearance() {
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    document.documentElement.classList.toggle("dark", query.matches);
  };
  apply();
  query.addEventListener("change", apply);
}

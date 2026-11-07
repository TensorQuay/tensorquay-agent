/** A pinned colour scheme or the system preference. */
export type Theme = "system" | "light" | "dark";
/** Static pre-hydration bootstrap; unavailable storage leaves the system default. */
export const THEME_SCRIPT = `try { const theme = localStorage.getItem("tensorquay-theme"); if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme; } catch {}`;

/** Reads the explicit choice, falling back to the system preference. */
export function currentTheme(): Theme {
  const theme = document.documentElement.dataset.theme;
  return theme === "light" || theme === "dark" ? theme : "system";
}
/** Changes the current tab and attempts to persist the choice without requiring storage. */
export function setTheme(theme: Theme) {
  if (theme === "system")
    document.documentElement.removeAttribute("data-theme");
  else document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem("tensorquay-theme", theme);
  } catch {
    /* The current tab still changes when storage is unavailable. */
  }
  window.dispatchEvent(new Event("themechange"));
}
/** Subscribes to tab-local theme changes and returns its cleanup. */
export function subscribeTheme(listener: () => void) {
  window.addEventListener("themechange", listener);
  return () => window.removeEventListener("themechange", listener);
}

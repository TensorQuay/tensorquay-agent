/** The sidebar's explicit state; "auto" means nothing was chosen and the width decides (UI-GUIDELINES §6). */
export type SidebarState = "expanded" | "rail" | "auto";

const STORAGE_KEY = "tensorquay-sidebar";
const CHANGE_EVENT = "sidebarchange";
/** Between these widths the rail is the default when nothing was chosen. */
export const RAIL_BY_DEFAULT = "(min-width: 768px) and (max-width: 1024px)";

/** Static pre-hydration bootstrap; unavailable storage or an unknown value leaves the automatic state. */
export const SIDEBAR_SCRIPT = `try { const state = localStorage.getItem("${STORAGE_KEY}"); if (state === "expanded" || state === "rail") document.documentElement.dataset.sidebar = state; } catch {}`; // copy-lint: not user copy (a script, not text)

/** Reads the explicit choice from the document; the server and the first client render see "auto". */
export function currentSidebar(): SidebarState {
  const state = document.documentElement.dataset.sidebar;
  return state === "expanded" || state === "rail" ? state : "auto";
}

/** Whether the width alone asks for the rail; false where no media queries exist (server, tests). */
export function railByWidth(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia(RAIL_BY_DEFAULT).matches
  );
}

/** The state the reader actually sees. */
export function effectiveSidebar(
  state: SidebarState,
  narrow: boolean,
): "expanded" | "rail" {
  if (state !== "auto") return state;
  return narrow ? "rail" : "expanded";
}

/** Changes the current tab and attempts to remember the choice without requiring storage. */
export function setSidebar(state: "expanded" | "rail") {
  document.documentElement.dataset.sidebar = state;
  try {
    localStorage.setItem(STORAGE_KEY, state);
  } catch {
    /* The current tab still changes when storage is unavailable. */
  }
  window.dispatchEvent(new Event(CHANGE_EVENT));
}

/** Subscribes to explicit changes and to the automatic width boundary; returns its cleanup. */
export function subscribeSidebar(listener: () => void) {
  const media =
    typeof window.matchMedia === "function"
      ? window.matchMedia(RAIL_BY_DEFAULT)
      : null;
  window.addEventListener(CHANGE_EVENT, listener);
  media?.addEventListener("change", listener);
  return () => {
    window.removeEventListener(CHANGE_EVENT, listener);
    media?.removeEventListener("change", listener);
  };
}

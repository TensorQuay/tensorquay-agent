import { useSyncExternalStore } from "react";

/**
 * The task search's shared state (the review of d9fb1917, MUST 1): whether it is open, and whether a list is long
 * enough to offer it. Every mounted list (the sidebar's, the phone drawer's) says the second; the shell holds the one
 * shortcut and the one overlay.
 */
/**
 * `available` is null until a list has said (it is loading): a shortcut pressed then is remembered, not lost (WEB-U-218).
 * `why` says why a search isn't offered: the list is short enough to read whole, or it couldn't be read.
 */
type State = {
  open: boolean;
  available: boolean | null;
  why: "short" | "failed";
};

let state: State = { open: false, available: null, why: "short" };
const listeners = new Set<() => void>();
const set = (next: Partial<State>) => {
  const merged = { ...state, ...next };
  if (
    merged.open === state.open &&
    merged.available === state.available &&
    merged.why === state.why
  )
    return;
  state = merged;
  for (const listener of listeners) listener();
};

export const taskSearch = {
  open: () => set({ open: true }),
  close: () => set({ open: false }),
  available: (value: boolean | null, why: State["why"] = "short") =>
    set({ available: value, why }),
  current: () => state,
  subscribe: (listener: () => void) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  /** For tests: closed, and not yet known, as a fresh page starts. */
  reset: () => set({ open: false, available: null, why: "short" }),
};

const closed: State = { open: false, available: null, why: "short" };

export function useTaskSearchState(): State {
  return useSyncExternalStore(
    taskSearch.subscribe,
    taskSearch.current,
    () => closed,
  );
}

/** Whether the shortcut key is Command (a Mac or an iPad); read on the client, the server says Ctrl. */
export const onMac = () => /Mac|iPhone|iPad/.test(navigator.platform);
const never = () => () => undefined;
export function useOnMac(): boolean {
  return useSyncExternalStore(never, onMac, () => false);
}

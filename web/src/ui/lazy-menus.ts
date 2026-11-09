"use client";

import { useEffect, useSyncExternalStore, type RefObject } from "react";

type Menus = typeof import("./MenuSurfaces");

let menus: Menus | null = null;
let loading: Promise<Menus> | null = null;
const waiting = new Set<() => void>();

/**
 * Fetches, once, the code that draws an open menu (`MenuSurfaces`: the ⋯ menus, the composer's +, a picker's list, the
 * account menu), which isn't part of a page's first load (WEB-U-014's budget). A fetch that failed (offline) is tried
 * again the next time.
 */
export function loadMenus(): Promise<Menus> {
  loading ??= import("./MenuSurfaces").then(
    (module) => {
      menus = module;
      for (const wake of waiting) wake();
      return module;
    },
    (error: unknown) => {
      loading = null;
      throw error;
    },
  );
  return loading;
}

/** A trigger's intent (pointer over, pointer down, focus): its menu's code arrives before the click, as a rule. */
export function prefetchMenus() {
  loadMenus().catch(() => undefined);
}

/**
 * The open menus' code once it is here, and null until then; fetched while `open`. A menu opened before it arrives
 * opens when it does. Until then it is dismissed as an open menu is: Escape, a press outside its trigger or the focus
 * leaving the trigger calls `close`, as a fetch that failed does, so the trigger never says it is expanded with nothing
 * open and the menu never opens later by itself. The next opening tries again. `setOpen` is the trigger's own state
 * setter, which React keeps the same.
 */
export function useMenus(
  open: boolean,
  setOpen: (open: boolean) => void,
  trigger: RefObject<HTMLElement | null>,
): Menus | null {
  const ready = useSyncExternalStore(subscribe, current, absent);
  useEffect(() => {
    if (!open || ready) return;
    loadMenus().catch(() => setOpen(false));
    // Once the code is here the menu hears these itself (Popover), and its focusing its first item is no leaving.
    const dismiss = (event: Event) => {
      if (menus) return;
      if ("key" in event) {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
      } else if (trigger.current?.contains(event.target as Node | null)) return;
      setOpen(false);
    };
    for (const kind of KINDS) addEventListener(kind, dismiss, true);
    return () => {
      for (const kind of KINDS) removeEventListener(kind, dismiss, true);
    };
  }, [open, ready, setOpen, trigger]);
  return ready;
}

/** What dismisses a menu still waiting for its code: Escape, a press, the focus moving. */
const KINDS = ["keydown", "pointerdown", "focusin"];

function subscribe(wake: () => void) {
  waiting.add(wake);
  return () => {
    waiting.delete(wake);
  };
}
const current = () => menus;
/** The server's page and its hydration draw no open menu. */
const absent = () => null;

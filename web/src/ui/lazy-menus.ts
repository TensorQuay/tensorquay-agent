"use client";

import { useEffect, useRef, useSyncExternalStore } from "react";

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
 * opens when it does; `onFailed` hears that it couldn't come, so the menu closes and the next opening tries again.
 */
export function useMenus(open: boolean, onFailed: () => void): Menus | null {
  const ready = useSyncExternalStore(subscribe, current, absent);
  const failed = useRef(onFailed);
  useEffect(() => {
    failed.current = onFailed;
  });
  useEffect(() => {
    if (open && !ready) loadMenus().catch(() => failed.current());
  }, [open, ready]);
  return ready;
}

function subscribe(wake: () => void) {
  waiting.add(wake);
  return () => {
    waiting.delete(wake);
  };
}
const current = () => menus;
/** The server's page and its hydration draw no open menu. */
const absent = () => null;

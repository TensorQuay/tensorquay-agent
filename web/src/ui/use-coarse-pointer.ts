"use client";

import { useSyncExternalStore } from "react";

const COARSE = "(pointer: coarse)";

const media = () =>
  typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(COARSE)
    : null;

/** Follows changes where the list can say so (not Safari before 14); a missing hint must never break the composer. */
function subscribe(listener: () => void) {
  const list = media();
  if (typeof list?.addEventListener !== "function") return () => undefined;
  list.addEventListener("change", listener);
  return () => list.removeEventListener("change", listener);
}

/** Whether the main pointer is a finger (a phone or tablet), where keyboard shortcuts mean nothing. */
export function useCoarsePointer(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => media()?.matches ?? false,
    () => false,
  );
}

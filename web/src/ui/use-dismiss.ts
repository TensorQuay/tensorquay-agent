"use client";

import { useEffect, useRef, type RefObject } from "react";

/**
 * How every control that opens is dismissed (the UI standard: Menu, Popover, In-place edit): Esc inside it calls
 * `onEscape`, and a press anywhere outside it calls `onOutside`. What each means (cancel, commit, close) and where focus
 * goes are the caller's.
 *
 * Esc is heard on the element itself and stopped there, so the innermost open control takes it and a dialog under it
 * neither hears it nor cancels. A press outside is heard on the document before anything on the page can stop it;
 * `ignore` (the trigger, which toggles by itself) doesn't count as outside.
 */
export function useDismiss(
  ref: RefObject<HTMLElement | null>,
  {
    onEscape,
    onOutside,
    ignore,
  }: {
    onEscape: () => void;
    onOutside: () => void;
    ignore?: RefObject<HTMLElement | null> | undefined;
  },
) {
  // The latest callbacks, so a caller's inline functions don't re-register the listeners on every render.
  const latest = useRef({ onEscape, onOutside });
  useEffect(() => {
    latest.current = { onEscape, onOutside };
  });
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    function escape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      latest.current.onEscape();
    }
    function outside(event: PointerEvent) {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (ref.current?.contains(target) || ignore?.current?.contains(target))
        return;
      latest.current.onOutside();
    }
    element.addEventListener("keydown", escape);
    document.addEventListener("pointerdown", outside, true);
    return () => {
      element.removeEventListener("keydown", escape);
      document.removeEventListener("pointerdown", outside, true);
    };
  }, [ref, ignore]);
}

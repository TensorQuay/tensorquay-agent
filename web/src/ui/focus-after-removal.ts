import { useEffect, useRef, type RefObject } from "react";

/**
 * Deleting something from its own row's ⋯ removes the control that opened the confirmation, so the focus would fall to
 * the page once the row goes (UI-GUIDELINES §9; WEB-U-275). `removed(id)` notes what was deleted; once the list no
 * longer has it, `target` (the page's heading or the list itself, focusable with tabIndex -1) takes the focus.
 */
export function useFocusAfterRemoval(
  ids: readonly string[],
  target: RefObject<HTMLElement | null>,
): (id: string) => void {
  const pending = useRef<string | null>(null);
  useEffect(() => {
    const id = pending.current;
    if (id === null || ids.includes(id)) return;
    pending.current = null;
    target.current?.focus();
  });
  return (id: string) => {
    pending.current = id;
  };
}

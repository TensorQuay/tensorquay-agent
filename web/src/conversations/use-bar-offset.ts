"use client";

import { useLayoutEffect, type RefObject } from "react";

/**
 * Where the task's header ends, for the files panel that lies over the column below it (UI-GUIDELINES §6, WEB-U-171):
 * keeps `--bar-offset` on the enclosing `.conversation-layout` equal to the bar's bottom, measured, since its lines
 * vary (the pickers' second line on a phone). The bar sets it while it is mounted, so one mounted late or replaced is
 * measured too, and removes it when it goes.
 */
export function useBarOffset(bar: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const element = bar.current;
    const layout = element?.closest<HTMLElement>(".conversation-layout");
    if (!element || !layout) return;
    const update = () =>
      layout.style.setProperty(
        "--bar-offset",
        `${Math.max(0, Math.round(element.getBoundingClientRect().bottom - layout.getBoundingClientRect().top))}px`,
      );
    update();
    if (typeof ResizeObserver === "undefined")
      return () => layout.style.removeProperty("--bar-offset");
    const observer = new ResizeObserver(update);
    observer.observe(element);
    observer.observe(layout);
    return () => {
      observer.disconnect();
      layout.style.removeProperty("--bar-offset");
    };
  }, [bar]);
}

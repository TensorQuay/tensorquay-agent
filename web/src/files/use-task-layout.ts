"use client";

import { useLayoutEffect, useState, type RefObject } from "react";

/**
 * The task layout's width from which the files panel docks beside the conversation (UI-GUIDELINES §6, WEB-U-170):
 * the 360 px panel, its 24 px gap and at least 516 px of conversation. The layout's own width decides, not the
 * window's, so an open sidebar counts. files.css's container query uses the same number.
 */
export const DOCK_WIDTH = 900;

/**
 * The task layout's room, measured as it changes: whether the files panel docks beside the conversation (returned),
 * and `--composer-offset`, the room the composer takes at the bottom, so a panel lying over the column ends above it
 * and the composer stays usable (review of 068c). The composer grows with its text, so it is measured, not fixed;
 * where the panel starts is the bar's own (use-bar-offset).
 */
export function useTaskLayout(layout: RefObject<HTMLElement | null>): boolean {
  const [docks, setDocks] = useState(false);
  useLayoutEffect(() => {
    const element = layout.current;
    if (!element) return;
    const composer = element.querySelector<HTMLElement>(".composer");
    const update = () => {
      const box = element.getBoundingClientRect();
      setDocks(box.width >= DOCK_WIDTH);
      if (composer)
        element.style.setProperty(
          "--composer-offset",
          `${Math.max(0, Math.round(box.bottom - composer.getBoundingClientRect().top))}px`,
        );
    };
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    if (composer) observer.observe(composer);
    update();
    return () => observer.disconnect();
  }, [layout]);
  return docks;
}

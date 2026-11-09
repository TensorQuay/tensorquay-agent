"use client";

import { useLayoutEffect, useRef } from "react";

/** Lines of text the composer shows at most, after which its text scrolls. */
const MOST = 8;

/**
 * Keeps the composer between `least` lines (one, its one row at rest; two where New task's keyboard hint needs them)
 * and eight, without moving the page (UI-GUIDELINES §7 Composer).
 */
export function useComposerSize(draft: string, focus: boolean, least = 1) {
  const input = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    if (focus) input.current?.focus({ preventScroll: true });
  }, [focus]);
  useLayoutEffect(() => {
    const element = input.current;
    if (!element) return;
    const resize = () => {
      const style = getComputedStyle(element);
      const line = Number.parseFloat(style.lineHeight);
      if (!Number.isFinite(line)) return;
      const padding =
        Number.parseFloat(style.paddingTop) +
        Number.parseFloat(style.paddingBottom);
      const border =
        Number.parseFloat(style.borderTopWidth) +
        Number.parseFloat(style.borderBottomWidth);
      element.style.height = "auto";
      element.style.height = `${Math.min(line * MOST + padding + border, Math.max(line * least + padding + border, element.scrollHeight + border))}px`;
    };
    resize();
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [draft, least]);
  return input;
}

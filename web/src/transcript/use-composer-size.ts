"use client";

import { useLayoutEffect, useRef } from "react";

/** Keeps the composer between three and twelve lines without moving the page. */
export function useComposerSize(draft: string, focus: boolean) {
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
      element.style.height = `${Math.min(line * 12 + padding + border, Math.max(line * 3 + padding + border, element.scrollHeight + border))}px`;
    };
    resize();
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [draft]);
  return input;
}

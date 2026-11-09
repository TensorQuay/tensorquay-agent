"use client";

import {
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

/** Whether an item's own label shows whole: on the rail it is hidden (display: none or a 1 px clip); a long one is cut. */
function labelShows(item: HTMLElement) {
  const label = item.querySelector("[data-nav-label]");
  if (!label || label.getClientRects().length === 0) return false;
  if (label.getBoundingClientRect().width <= 1) return false;
  return label.scrollWidth <= label.clientWidth + 1;
}

/**
 * A sidebar item's name as the app's own tooltip (UI-GUIDELINES §6): on the rail, where only its icon shows, or where
 * its label is cut (a long email), beside it on hover or keyboard focus. Where its label shows whole, nothing: the label
 * names it, and no native title repeats it (WEB-E2E-115). The tip is a layer of its own on the page, as IconAction's (WEB-U-429), and never the item's content.
 * `intent` hears the item about to be used (the pointer over it, the keyboard's focus), as IconAction's `onIntent`.
 */
export function useNavTip(
  name: string,
  intent?: () => void,
): {
  handlers: {
    onPointerEnter: (event: PointerEvent<HTMLElement>) => void;
    onPointerLeave: () => void;
    onFocus: (event: FocusEvent<HTMLElement>) => void;
    onBlur: () => void;
  };
  tip: ReactNode;
} {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const show = (item: HTMLElement) => {
    intent?.();
    if (!labelShows(item)) setAnchor(item);
  };
  useLayoutEffect(() => {
    const tip = bubble.current;
    if (!anchor || !tip) return;
    // Beside the item, centred on it, in the window's coordinates.
    const place = () => {
      const item = anchor.getBoundingClientRect();
      const gap =
        parseFloat(getComputedStyle(tip).getPropertyValue("--space-2")) || 0;
      tip.style.left = `${item.right + gap}px`;
      tip.style.top = `${item.top + item.height / 2 - tip.offsetHeight / 2}px`;
    };
    place();
    addEventListener("scroll", place, true);
    addEventListener("resize", place);
    return () => {
      removeEventListener("scroll", place, true);
      removeEventListener("resize", place);
    };
  }, [anchor]);
  return {
    handlers: {
      onPointerEnter: (event) => show(event.currentTarget),
      onPointerLeave: () => setAnchor(null),
      onFocus: (event) => {
        if (event.currentTarget.matches(":focus-visible"))
          show(event.currentTarget);
      },
      onBlur: () => setAnchor(null),
    },
    tip: anchor
      ? createPortal(
          <span
            ref={bubble}
            className="action-tooltip type-small"
            aria-hidden="true"
          >
            {name}
          </span>,
          document.body,
        )
      : null,
  };
}

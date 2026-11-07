"use client";

import {
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";

import { useDismiss } from "./use-dismiss";

/**
 * Anchors a non-modal surface without a backdrop; dismissing it restores the trigger's focus. Inside a modal dialog
 * (the phone's navigation drawer) it renders in that dialog, since everything outside a modal dialog is beneath it and
 * inert.
 */
export function Popover({
  anchor,
  label,
  placement = "below-end",
  onClose,
  children,
}: {
  anchor: RefObject<HTMLButtonElement | null>;
  label: string;
  /**
   * Below the trigger aligned to its end (default, a row's ⋯), below it aligned to its start (a picker), or above it
   * aligned to its start (the sidebar's foot, the composer); it flips when its side has no room.
   */
  placement?: "below-end" | "below-start" | "above-start";
  onClose: () => void;
  children: ReactNode;
}) {
  const surface = useRef<HTMLDivElement>(null);
  const side = useRef<"up" | "down" | null>(null);
  // Where the trigger is: its dialog, else the page; read again once the trigger has mounted.
  const host = useSyncExternalStore(
    noSubscription,
    () => anchor.current?.closest("dialog") ?? document.body,
    () => null,
  );
  useLayoutEffect(() => {
    function position() {
      const button = anchor.current?.getBoundingClientRect();
      const panel = surface.current;
      if (!button || !panel) return;
      // The token gap; an environment without the stylesheet (tests) places it flush.
      const gap =
        parseFloat(getComputedStyle(panel).getPropertyValue("--space-2")) || 0;
      const start = placement !== "below-end";
      const left = start ? button.left : button.right - panel.offsetWidth;
      panel.style.left = `${Math.max(gap, Math.min(left, innerWidth - panel.offsetWidth - gap))}px`;
      // Never over its trigger (the UI standard): on its preferred side when it fits, else the other side when that
      // fits, else the roomier side with its height capped there so its list scrolls. Measured at its natural height.
      panel.style.maxHeight = "";
      const height = panel.offsetHeight;
      const below = innerHeight - button.bottom - 2 * gap;
      const above = button.top - 2 * gap;
      // The side is chosen once, when it opens: a list that shrinks as the person types stays where it opened.
      side.current ??=
        placement === "above-start"
          ? height <= above || above >= below
            ? "up"
            : "down"
          : height <= below || below >= above
            ? "down"
            : "up";
      const upward = side.current === "up";
      const room = upward ? above : below;
      if (height > room) panel.style.maxHeight = `${Math.max(room, 0)}px`;
      // Upward, the bottom edge is pinned above the trigger, so content that arrives later (a lazily loaded menu)
      // grows the surface upward instead of over the trigger (WEB-E2E-018).
      if (upward) {
        panel.style.top = "auto";
        panel.style.bottom = `${innerHeight - button.top + gap}px`;
      } else {
        panel.style.bottom = "auto";
        panel.style.top = `${button.bottom + gap}px`;
      }
    }
    function blur(event: FocusEvent) {
      if (
        event.target instanceof Node &&
        !surface.current?.contains(event.target) &&
        !anchor.current?.contains(event.target)
      )
        onClose();
    }
    position();
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(position);
    if (surface.current) observer?.observe(surface.current);
    // A new window size chooses the side again; its own list shrinking as the person types doesn't.
    function resize() {
      side.current = null;
      position();
    }
    window.addEventListener("resize", resize);
    document.addEventListener("scroll", position, true);
    document.addEventListener("focusin", blur);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", resize);
      document.removeEventListener("scroll", position, true);
      document.removeEventListener("focusin", blur);
    };
  }, [anchor, onClose, placement, host]);
  // The one shared way an open control is dismissed (the UI standard): Esc or a press outside closes it and gives the
  // trigger its focus back. A press on another control still focuses that control: the browser moves focus after this.
  const dismiss = () => {
    onClose();
    anchor.current?.focus();
  };
  useDismiss(surface, {
    onEscape: dismiss,
    onOutside: dismiss,
    ignore: anchor,
  });
  if (!host) return null;
  return createPortal(
    <div className="popover" ref={surface} role="region" aria-label={label}>
      {children}
    </div>,
    host,
  );
}

/** The trigger's place changes only by rendering, which reads it again. */
const noSubscription = () => () => undefined;

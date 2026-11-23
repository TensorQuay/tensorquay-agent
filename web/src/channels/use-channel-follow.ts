"use client";

import { useLayoutEffect, useRef, type RefObject } from "react";

/** How near the bottom still counts as at it, in pixels. */
const nearPixels = 48;

/** The element that scrolls around `node`: its nearest ancestor that may scroll, else the document's. */
function scroller(node: HTMLElement | null): HTMLElement | null {
  for (let at = node?.parentElement ?? null; at; at = at.parentElement) {
    const overflow = getComputedStyle(at).overflowY;
    if (overflow === "auto" || overflow === "scroll") return at;
  }
  return document.scrollingElement instanceof HTMLElement
    ? document.scrollingElement
    : null;
}

function atBottom(element: HTMLElement) {
  return (
    element.scrollHeight - element.clientHeight - element.scrollTop <=
    nearPixels
  );
}

/**
 * A channel opens at its newest message and stays there as posts come, as a task's conversation does (18.2.1;
 * WEB-U-541). On 18.2 it opened at its oldest: in a 31-message channel the newest was six screens down at 1440 px and
 * twenty-one at 390 (the lead's live check, 23 Nov 2026). Once the log is read, the view goes to the bottom and follows
 * what grows there (a proposal's votes and a to-do's claims load after their post) until the reader scrolls up; back at
 * the bottom, it follows again. The returned function brings the view to the newest, for the person's own post.
 */
export function useChannelFollow(
  anchor: RefObject<HTMLElement | null>,
  loaded: boolean,
  last: number,
) {
  const following = useRef(true);
  /** Where the page last put the view: only a reader moving above it stops following. */
  const placed = useRef(0);
  const toNewest = useRef(() => undefined as void);
  useLayoutEffect(() => {
    if (!loaded) return;
    const element = scroller(anchor.current);
    if (!element) return;
    const bottom = () => {
      element.scrollTop = Math.max(
        0,
        element.scrollHeight - element.clientHeight,
      );
      placed.current = element.scrollTop;
    };
    toNewest.current = () => {
      following.current = true;
      bottom();
    };
    const onScroll = () => {
      if (atBottom(element)) following.current = true;
      else if (element.scrollTop < placed.current - nearPixels)
        following.current = false;
    };
    element.addEventListener("scroll", onScroll, { passive: true });
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(() => {
            if (following.current) bottom();
          });
    if (anchor.current) observer?.observe(anchor.current);
    if (following.current) bottom();
    return () => {
      element.removeEventListener("scroll", onScroll);
      observer?.disconnect();
    };
  }, [anchor, loaded]);
  useLayoutEffect(() => {
    // A new message while the view follows: to the bottom now, not a frame later.
    if (loaded && following.current) toNewest.current();
  }, [loaded, last]);
  return () => toNewest.current();
}

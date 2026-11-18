"use client";

import { useLayoutEffect, useRef, useState } from "react";

import { readerScrolled } from "../ui/OverlayScrollbar";

const bottomThresholdPixels = 24;

/** Whether nothing is left below the view, within the threshold. */
function nothingBelow(element: HTMLElement) {
  return (
    element.scrollHeight - element.clientHeight - element.scrollTop <=
    bottomThresholdPixels
  );
}

/**
 * Whether the reader has moved the view up since the page put it there, before their scroll event (a frame later) has
 * come: a commit or a resize in between would undo their scroll (WEB-U-378). A view clamped by shorter content has
 * nothing below, so that is never taken for the reader.
 */
function readerAbove(element: HTMLElement, placed: number) {
  return (
    element.scrollTop < placed - bottomThresholdPixels && !nothingBelow(element)
  );
}

/** Follows committed message and composer layouts until the reader scrolls up; jumps are immediate. */
export function useFollow(sequence: number) {
  const ref = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  // Where the page last put the view. Its own scroll event comes a frame later, and the content may have grown by
  // then (WEB-U-219): only a reader moving above this pauses following.
  const placed = useRef(0);
  const [pausedAt, setPausedAt] = useState<number | null>(null);
  // The latest sequence, for the resize observer, which is set up once.
  const latest = useRef(sequence);
  latest.current = sequence;
  /** Follows, unless the reader has just moved the view up: their scroll pauses following now. */
  function follow() {
    if (ref.current && readerAbove(ref.current, placed.current)) {
      following.current = false;
      setPausedAt(latest.current);
    } else bottom();
  }
  /**
   * Puts the view at the bottom. The reader's Jump to latest (`reader`) is their own scrolling, so it tells the
   * transcript's overlay bar, which shows for it; the page's own following shows nothing (WEB-U-485).
   */
  function bottom(reader = false) {
    const element = ref.current;
    if (!element) return;
    if (reader) readerScrolled(element);
    element.scrollTop = Math.max(
      0,
      element.scrollHeight - element.clientHeight,
    );
    placed.current = element.scrollTop;
  }
  useLayoutEffect(() => {
    if (following.current) follow();
  });
  useLayoutEffect(() => {
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (following.current) {
        if (ref.current && readerAbove(ref.current, placed.current)) {
          following.current = false;
          setPausedAt(latest.current);
        } else bottom();
      }
      // A paused reader whose view grew until nothing is left below (a taller window, a closed panel): the browser
      // moves the view without a scroll event, so following resumes here, and "Jump to latest" goes.
      else if (ref.current && nothingBelow(ref.current)) {
        following.current = true;
        setPausedAt(null);
        bottom();
      }
    });
    if (ref.current) observer.observe(ref.current);
    if (contentRef.current) observer.observe(contentRef.current);
    return () => observer.disconnect();
  }, []);
  /** Follows again from the bottom: the reader's Jump to latest (`"reader"`), or the page's own, as a send is. */
  function resume(by: "reader" | "page" = "page") {
    following.current = true;
    setPausedAt(null);
    bottom(by === "reader");
  }
  function onScroll() {
    const element = ref.current;
    if (!element) return;
    if (nothingBelow(element)) {
      following.current = true;
      setPausedAt(null);
    } else if (
      following.current &&
      element.scrollTop < placed.current - bottomThresholdPixels
    ) {
      following.current = false;
      setPausedAt(sequence);
    } else if (following.current) bottom();
  }
  return {
    ref,
    contentRef,
    onScroll,
    resume,
    paused: pausedAt !== null,
    unseen: pausedAt === null ? 0 : Math.max(0, sequence - pausedAt),
  };
}

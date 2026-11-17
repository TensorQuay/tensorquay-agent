"use client";

import { useEffect, useRef, type RefObject } from "react";

/** How long the bar stays after the last scroll, before it fades (18.1 polish, WEB-U-485). */
export const IDLE_MS = 1000;
/** How near the scroller's right edge the pointer shows the bar, in CSS pixels. */
const EDGE = 16;
/** The thumb's least height, so a very long transcript still has one to see and grab. */
const LEAST = 32;

/**
 * A scroller's scrollbar drawn over its right edge, so it takes no width (the browser's own is hidden on it,
 * conversation.css): hidden at rest, shown while the scroller scrolls by any means (wheel, touch, keys, Jump to latest,
 * following new output) until it has been still for IDLE_MS, and while the pointer is at the edge or on the thumb; the
 * thumb can be dragged. It is a picture of the scroll position, hidden from assistive technology: keys, focus and
 * screen readers use the scroller itself. Placed beside the scroller in the same positioned parent; with forced colours
 * the system's bar stays and this one isn't drawn.
 */
export function OverlayScrollbar({
  target,
}: {
  target: RefObject<HTMLElement | null>;
}) {
  const bar = useRef<HTMLDivElement>(null);
  const thumb = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const scroller = target.current;
    const track = bar.current;
    const handle = thumb.current;
    if (!scroller || !track || !handle) return;
    let idle: ReturnType<typeof setTimeout> | undefined;
    let pointed = false;
    let drag: { y: number; top: number } | null = null;
    const show = () => {
      if (idle !== undefined || pointed || drag)
        track.setAttribute("data-shown", "");
      else track.removeAttribute("data-shown");
    };
    /** The thumb's height and the room it moves in, for the scroller as it is now. */
    const measure = () => {
      const view = scroller.clientHeight;
      const size = Math.max(LEAST, (view * view) / scroller.scrollHeight);
      return { size, room: view - size, range: scroller.scrollHeight - view };
    };
    const place = () => {
      track.style.top = `${scroller.offsetTop}px`;
      track.style.left = `${scroller.offsetLeft + scroller.offsetWidth}px`;
      track.style.height = `${scroller.offsetHeight}px`;
      const { size, room, range } = measure();
      track.hidden = range <= 0;
      if (range <= 0) return;
      handle.style.height = `${size}px`;
      handle.style.transform = `translateY(${(scroller.scrollTop / range) * room}px)`;
    };
    function scroll() {
      place();
      clearTimeout(idle);
      idle = setTimeout(() => {
        idle = undefined;
        show();
      }, IDLE_MS);
      show();
    }
    function move(event: PointerEvent) {
      const box = scroller!.getBoundingClientRect();
      const near =
        event.clientX >= box.right - EDGE && event.clientX <= box.right;
      if (near === pointed) return;
      pointed = near;
      show();
    }
    function leave() {
      pointed = false;
      show();
    }
    function enter() {
      pointed = true;
      show();
    }
    function grab(event: PointerEvent) {
      if (event.button !== 0) return;
      event.preventDefault();
      drag = { y: event.clientY, top: scroller!.scrollTop };
      handle!.setPointerCapture?.(event.pointerId);
      show();
    }
    function pull(event: PointerEvent) {
      if (!drag) return;
      const { room, range } = measure();
      scroller!.scrollTop =
        drag.top + ((event.clientY - drag.y) * range) / Math.max(room, 1);
    }
    function release() {
      drag = null;
      show();
    }
    place();
    scroller.addEventListener("scroll", scroll, { passive: true });
    scroller.addEventListener("pointermove", move);
    scroller.addEventListener("pointerleave", leave);
    handle.addEventListener("pointerenter", enter);
    handle.addEventListener("pointerleave", leave);
    handle.addEventListener("pointerdown", grab);
    handle.addEventListener("pointermove", pull);
    handle.addEventListener("pointerup", release);
    handle.addEventListener("pointercancel", release);
    // The content grows while a run streams and the window changes size: the thumb follows both.
    const observer =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(scroller);
    if (scroller.firstElementChild)
      observer?.observe(scroller.firstElementChild);
    return () => {
      clearTimeout(idle);
      observer?.disconnect();
      scroller.removeEventListener("scroll", scroll);
      scroller.removeEventListener("pointermove", move);
      scroller.removeEventListener("pointerleave", leave);
      handle.removeEventListener("pointerenter", enter);
      handle.removeEventListener("pointerleave", leave);
      handle.removeEventListener("pointerdown", grab);
      handle.removeEventListener("pointermove", pull);
      handle.removeEventListener("pointerup", release);
      handle.removeEventListener("pointercancel", release);
    };
  }, [target]);
  return (
    <div className="overlay-scrollbar" aria-hidden="true" ref={bar}>
      <div className="overlay-thumb" ref={thumb} />
    </div>
  );
}

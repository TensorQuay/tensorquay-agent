import { act, fireEvent, render } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { IDLE_MS, OverlayScrollbar } from "./OverlayScrollbar";

/** A scroller of `content` px in a `view` px box at the page's (16, 48), 600 px wide. */
function geometry(element: HTMLElement, content: number, view: number) {
  let top = 0;
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, get: () => content },
    clientHeight: { configurable: true, get: () => view },
    offsetHeight: { configurable: true, get: () => view },
    offsetWidth: { configurable: true, get: () => 600 },
    offsetTop: { configurable: true, get: () => 48 },
    offsetLeft: { configurable: true, get: () => 16 },
    scrollTop: {
      configurable: true,
      get: () => top,
      set: (value: number) => {
        top = Math.max(0, Math.min(value, content - view));
      },
    },
  });
  element.getBoundingClientRect = () =>
    ({
      left: 16,
      right: 616,
      top: 48,
      bottom: 48 + view,
      width: 600,
      height: view,
    }) as DOMRect;
}

function Page({ content, view }: { content: number; view: number }) {
  const scroller = useRef<HTMLDivElement>(null);
  return (
    <section style={{ position: "relative" }}>
      <div
        ref={(element) => {
          scroller.current = element;
          if (element) geometry(element, content, view);
        }}
        role="region"
        aria-label="Transcript"
        tabIndex={0}
      >
        Long
      </div>
      <OverlayScrollbar target={scroller} />
    </section>
  );
}

const parts = (container: HTMLElement) => {
  const bar = container.querySelector<HTMLElement>(".overlay-scrollbar");
  const thumb = container.querySelector<HTMLElement>(".overlay-thumb");
  if (!bar || !thumb) throw new Error("no overlay scrollbar");
  const region = container.querySelector<HTMLElement>("[role=region]");
  if (!region) throw new Error("no scroller");
  return { bar, thumb, region };
};

// WEB-U-485 (18.1 polish): the transcript's scrollbar is drawn over its edge, so it takes no width; it is hidden at
// rest, shows while the transcript scrolls or the pointer is at its edge, and goes a second after. It is a picture
// of the scroll position, hidden from assistive technology: keys, focus and screen readers use the transcript itself.
describe("WEB-U-485 the transcript's overlay scrollbar", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("sits over the scroller's right edge, its thumb sized and placed by the scroll position", () => {
    const { container } = render(<Page content={2000} view={500} />);
    const { bar, thumb, region } = parts(container);
    expect(bar).toHaveAttribute("aria-hidden", "true");
    // Over the scroller's box: from its top, its height, at its right edge.
    expect(bar.style.top).toBe("48px");
    expect(bar.style.height).toBe("500px");
    expect(bar.style.left).toBe(`${16 + 600}px`);
    // A quarter of the content shows: a quarter of the track, at the top.
    expect(thumb.style.height).toBe("125px");
    expect(thumb.style.transform).toBe("translateY(0px)");
    region.scrollTop = 750;
    fireEvent.scroll(region);
    // Halfway down the 1500 px it can scroll: halfway down the 375 px the thumb can move.
    expect(thumb.style.transform).toBe("translateY(187.5px)");
  });

  it("is hidden at rest, shows while scrolling and goes a second after", () => {
    const { container } = render(<Page content={2000} view={500} />);
    const { bar, region } = parts(container);
    expect(bar).not.toHaveAttribute("data-shown");
    fireEvent.scroll(region);
    expect(bar).toHaveAttribute("data-shown");
    act(() => {
      vi.advanceTimersByTime(IDLE_MS - 1);
    });
    expect(bar).toHaveAttribute("data-shown");
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(bar).not.toHaveAttribute("data-shown");
  });

  it("shows while the pointer is at the scroller's edge, and not elsewhere over it", () => {
    const { container } = render(<Page content={2000} view={500} />);
    const { bar, region } = parts(container);
    fireEvent.pointerMove(region, { clientX: 300, clientY: 200 });
    expect(bar).not.toHaveAttribute("data-shown");
    fireEvent.pointerMove(region, { clientX: 610, clientY: 200 });
    expect(bar).toHaveAttribute("data-shown");
    act(() => {
      vi.advanceTimersByTime(IDLE_MS * 3);
    });
    expect(bar).toHaveAttribute("data-shown");
    fireEvent.pointerLeave(region);
    expect(bar).not.toHaveAttribute("data-shown");
  });

  it("drags: the thumb moves the transcript by the same share", () => {
    const { container } = render(<Page content={2000} view={500} />);
    const { thumb, region } = parts(container);
    fireEvent.pointerDown(thumb, { clientY: 100, pointerId: 1, button: 0 });
    fireEvent.pointerMove(thumb, { clientY: 175, pointerId: 1 });
    // 75 px of the 375 the thumb can move is a fifth: a fifth of the 1500 px the transcript can scroll.
    expect(region.scrollTop).toBe(300);
    fireEvent.pointerUp(thumb, { pointerId: 1 });
    fireEvent.pointerMove(thumb, { clientY: 300, pointerId: 1 });
    expect(region.scrollTop).toBe(300);
  });

  it("has nothing to show when the content fits", () => {
    const { container } = render(<Page content={400} view={500} />);
    const { bar, region } = parts(container);
    fireEvent.scroll(region);
    expect(bar).toHaveAttribute("hidden");
  });

  it("forgets its timer when it goes", () => {
    const { container, unmount } = render(<Page content={2000} view={500} />);
    fireEvent.scroll(parts(container).region);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

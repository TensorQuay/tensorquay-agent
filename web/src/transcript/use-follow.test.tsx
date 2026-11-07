import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useFollow } from "./use-follow";

function Feed({ sequence }: { sequence: number }) {
  const {
    ref: scrollRef,
    contentRef,
    onScroll,
    resume,
    paused,
    unseen,
  } = useFollow(sequence);
  return (
    <>
      <div
        role="region"
        aria-label="Transcript"
        ref={scrollRef}
        onScroll={onScroll}
      >
        <div ref={contentRef} />
      </div>
      {paused && (
        <button onClick={resume}>Jump to latest ({unseen} new)</button>
      )}
      <button onClick={resume}>Send</button>
    </>
  );
}
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
function setup() {
  let height = 200;
  let client = 200;
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(
    () => client,
  );
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(
    () => height,
  );
  const view = render(<Feed sequence={0} />);
  const region = screen.getByRole("region", { name: "Transcript" });
  const update = (sequence: number, contentHeight = 200 + sequence * 10) => {
    height = contentHeight;
    view.rerender(<Feed sequence={sequence} />);
  };
  return {
    region,
    update,
    resize: (value: number) => {
      height = value;
    },
    /** The view's own height, as a taller window or a closed panel changes it. */
    viewport: (value: number) => {
      client = value;
    },
    unmount: view.unmount,
  };
}

describe("WEB-U-015 transcript follow mode", () => {
  it("follows delayed content size changes, preserves a paused reader and releases observation", () => {
    let changed: (() => void) | undefined;
    const disconnect = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          changed = callback;
        }
        observe() {}
        disconnect = disconnect;
      },
    );
    const { region, resize, unmount } = setup();
    resize(1200);
    act(() => changed?.());
    expect(region.scrollTop).toBe(1000);
    region.scrollTop = 100;
    fireEvent.scroll(region);
    resize(1400);
    act(() => changed?.());
    expect(region.scrollTop).toBe(100);
    unmount();
    expect(disconnect).toHaveBeenCalledOnce();
  });

  it("resumes when the view grows until nothing is left below, with no scroll event", () => {
    // The demo's evidence (12 Oct 2026): paused after reading up, then the window grew to fit the whole task; the
    // browser clamps the scroll position without a scroll event, and "Jump to latest" stayed with nothing to jump to.
    let changed: (() => void) | undefined;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          changed = callback;
        }
        observe() {}
        disconnect() {}
      },
    );
    const { region, resize, viewport } = setup();
    resize(1200);
    act(() => changed?.());
    region.scrollTop = 100;
    fireEvent.scroll(region);
    expect(
      screen.getByRole("button", { name: /Jump to latest/ }),
    ).toBeVisible();
    // Still something below: it stays paused.
    viewport(600);
    act(() => changed?.());
    expect(region.scrollTop).toBe(100);
    expect(
      screen.getByRole("button", { name: /Jump to latest/ }),
    ).toBeVisible();
    // Everything fits now.
    viewport(1200);
    region.scrollTop = 0;
    act(() => changed?.());
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
    // And it follows again: new content keeps the view at the bottom.
    resize(1500);
    act(() => changed?.());
    expect(region.scrollTop).toBe(300);
  });

  it("follows layout growth without a new event, but leaves a paused reader in place", () => {
    const { region, update } = setup();
    update(100);
    update(100, 1400);
    expect(region.scrollTop).toBe(1200);
    region.scrollTop = 100;
    fireEvent.scroll(region);
    update(100, 1600);
    expect(region.scrollTop).toBe(100);
    expect(
      screen.getByRole("button", { name: /Jump to latest/ }),
    ).toBeVisible();
  });

  it("keeps all 200 flushes at the bottom", () => {
    const { region, update } = setup();
    for (let sequence = 1; sequence <= 200; sequence++) {
      update(sequence);
      expect(region.scrollTop).toBe(region.scrollHeight - region.clientHeight);
    }
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
  });

  it.each(["Jump to latest (7 new)", "Send"])(
    "pauses on scroll-up, counts new updates and resumes with %s",
    (button) => {
      const { region, update } = setup();
      update(100);
      region.scrollTop = 100;
      fireEvent.scroll(region);
      update(107);
      expect(region.scrollTop).toBe(100);
      expect(
        screen.getByRole("button", { name: "Jump to latest (7 new)" }),
      ).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: button }));
      expect(region.scrollTop).toBe(region.scrollHeight - region.clientHeight);
      update(108);
      expect(region.scrollTop).toBe(region.scrollHeight - region.clientHeight);
      expect(
        screen.queryByRole("button", { name: /Jump to latest/ }),
      ).toBeNull();
    },
  );

  it("resumes when the reader returns to the bottom threshold", () => {
    const { region, update } = setup();
    update(100);
    region.scrollTop = 0;
    fireEvent.scroll(region);
    update(105);
    region.scrollTop = region.scrollHeight - region.clientHeight - 1;
    fireEvent.scroll(region);
    update(106);
    expect(region.scrollTop).toBe(region.scrollHeight - region.clientHeight);
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
  });
});

// WEB-E2E-018 failed once under load: on a flush the latest line was below the view, follow had stopped. The page's own
// scroll to the bottom fires its scroll event a frame later; if the content grows in between (a part loading, the next
// delta), that event finds the view short of the bottom and was taken for the reader scrolling up. Only a reader moving
// up, above where the page last put the view, pauses following.
describe("WEB-U-219 follow survives growth before its own scroll event", () => {
  it("keeps following when the content grows before the page's scroll event arrives", () => {
    const { region, update, resize } = setup();
    update(10);
    expect(region.scrollTop).toBe(100);
    // The content grows; the page's own scroll event arrives after it, the view where the page put it.
    resize(800);
    fireEvent.scroll(region);
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
    // And it catches up at once, rather than waiting for the next change.
    expect(region.scrollTop).toBe(600);
    update(11, 900);
    expect(region.scrollTop).toBe(700);
  });

  it("still pauses when the reader scrolls up above where the page put the view", () => {
    const { region, update } = setup();
    update(10);
    region.scrollTop = 40;
    fireEvent.scroll(region);
    expect(
      screen.getByRole("button", { name: /Jump to latest/ }),
    ).toBeVisible();
    update(11, 900);
    expect(region.scrollTop).toBe(40);
  });
});

// F (16f's combined e2e, WEB-U-015's "a paused view that grows to fit everything follows again" failed once under
// load): the reader moved the view up, and before its scroll event (a frame later) a commit or a resize put the view
// back at the bottom, so the reader's scroll was lost and the view jumped back. A view the page finds above where it
// put it, with something left below, is the reader's: following pauses there and then.
describe("WEB-U-378 a reader's scroll up survives a change before its scroll event", () => {
  it("keeps the reader's place when a commit lands first", () => {
    const { region, update } = setup();
    update(10);
    expect(region.scrollTop).toBe(100);
    region.scrollTop = 0;
    update(11, 900);
    expect(region.scrollTop).toBe(0);
    fireEvent.scroll(region);
    expect(
      screen.getByRole("button", { name: /Jump to latest/ }),
    ).toBeVisible();
  });

  it("keeps the reader's place when a resize lands first", () => {
    let changed: (() => void) | undefined;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          changed = callback;
        }
        observe() {}
        disconnect() {}
      },
    );
    const { region, update, resize } = setup();
    update(10);
    region.scrollTop = 0;
    resize(900);
    act(() => changed?.());
    expect(region.scrollTop).toBe(0);
    // It counts from the latest message, though the observer was set up at the first.
    update(12, 900);
    expect(
      screen.getByRole("button", { name: "Jump to latest (2 new)" }),
    ).toBeVisible();
  });

  it("still follows when the content shrinks under the view", () => {
    const { region, update } = setup();
    update(30);
    expect(region.scrollTop).toBe(300);
    // A shorter transcript clamps the view: nothing is left below, so that isn't the reader.
    region.scrollTop = 100;
    update(31, 300);
    expect(region.scrollTop).toBe(100);
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
  });
});

import { render } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useTaskLayout } from "./use-task-layout";

function Layout() {
  const layout = useRef<HTMLDivElement>(null);
  useTaskLayout(layout);
  return (
    <div ref={layout} data-testid="layout">
      <div className="composer" />
    </div>
  );
}
/** Each element's box, answered by one stub so elements never share a rectangle. */
const boxes = new Map<Element, { top: number; bottom: number }>();
function place(element: Element, top: number, bottom: number) {
  boxes.set(element, { top, bottom });
}

class FakeObserver {
  static last: FakeObserver | null = null;
  observed: Element[] = [];
  disconnected = false;
  constructor(readonly callback: () => void) {
    FakeObserver.last = this;
  }
  observe(element: Element) {
    this.observed.push(element);
  }
  disconnect() {
    this.disconnected = true;
  }
}
afterEach(() => {
  boxes.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

// Below 1280 px the floating files panel ends above the composer (WEB-U-022): the room it takes is measured.
describe("WEB-U-022 the composer's room at the bottom", () => {
  it("keeps --composer-offset equal to the room the composer takes, never negative", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return (boxes.get(this) ?? { top: 0, bottom: 0 }) as DOMRect;
      },
    );
    const { getByTestId, unmount } = render(<Layout />);
    const layout = getByTestId("layout");
    const composer = layout.querySelector(".composer");
    if (!composer) throw new Error("The composer must render");
    expect(FakeObserver.last?.observed).toEqual([layout, composer]);
    place(layout, 0, 900);
    place(composer, 760.4, 900);
    FakeObserver.last?.callback();
    expect(layout.style.getPropertyValue("--composer-offset")).toBe("140px");
    place(composer, 950, 990);
    FakeObserver.last?.callback();
    expect(layout.style.getPropertyValue("--composer-offset")).toBe("0px");
    unmount();
    expect(FakeObserver.last?.disconnected).toBe(true);
  });

  it("leaves the layout alone where the browser cannot observe sizes", () => {
    vi.stubGlobal("ResizeObserver", undefined);
    const { getByTestId } = render(<Layout />);
    expect(
      getByTestId("layout").style.getPropertyValue("--composer-offset"),
    ).toBe("");
  });
});

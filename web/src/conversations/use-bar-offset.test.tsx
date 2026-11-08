import { act, render } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useBarOffset } from "./use-bar-offset";

/** Each element's box, answered by one stub so elements never share a rectangle. */
const boxes = new Map<Element, { top: number; bottom: number }>();
class FakeObserver {
  static all: FakeObserver[] = [];
  constructor(readonly callback: () => void) {
    FakeObserver.all.push(this);
  }
  observe() {}
  disconnect() {}
}
afterEach(() => {
  boxes.clear();
  FakeObserver.all = [];
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function Bar() {
  const bar = useRef<HTMLElement>(null);
  useBarOffset(bar);
  return <header ref={bar} className="conversation-bar" />;
}
function Layout({ shown }: { shown: boolean }) {
  return (
    <div className="conversation-layout" data-testid="layout">
      {shown && <Bar />}
    </div>
  );
}

// A screen review before 17.0: at 390 the open panel started at a fixed bar height, over the header's second
// line. The fix's review: a bar mounted after the layout (or replaced) must be measured too.
describe("WEB-U-171 the header's room at the top", () => {
  it("is measured by the bar itself, also when it mounts late, and cleared when it goes", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.classList.contains("conversation-layout"))
          return { top: 10, bottom: 900 } as DOMRect;
        return (boxes.get(this) ?? { top: 10, bottom: 86.6 }) as DOMRect;
      },
    );
    const { getByTestId, rerender } = render(<Layout shown={false} />);
    const layout = getByTestId("layout");
    expect(layout.style.getPropertyValue("--bar-offset")).toBe("");
    rerender(<Layout shown />);
    expect(layout.style.getPropertyValue("--bar-offset")).toBe("77px");
    const bar = layout.querySelector(".conversation-bar");
    if (!bar) throw new Error("The bar must render");
    // On a wide header, one line.
    boxes.set(bar, { top: 10, bottom: 50 });
    act(() => FakeObserver.all.at(-1)?.callback());
    expect(layout.style.getPropertyValue("--bar-offset")).toBe("40px");
    rerender(<Layout shown={false} />);
    expect(layout.style.getPropertyValue("--bar-offset")).toBe("");
  });
});

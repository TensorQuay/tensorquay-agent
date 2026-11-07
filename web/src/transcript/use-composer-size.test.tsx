import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useComposerSize } from "./use-composer-size";

function Box({ draft, focus }: { draft: string; focus: boolean }) {
  const input = useComposerSize(draft, focus);
  return <textarea aria-label="Message" ref={input} defaultValue={draft} />;
}
/** A line of 20 px, 8 px padding and 1 px border each side: 3 lines = 78 px, 12 lines = 258 px. */
// The padding is what jsdom's computed style reports, not a style this test sets, so it is named rather than inline.
const COMPUTED_PADDING = "8px";
function style(lineHeight: string) {
  vi.spyOn(window, "getComputedStyle").mockReturnValue({
    lineHeight,
    paddingTop: COMPUTED_PADDING,
    paddingBottom: COMPUTED_PADDING,
    borderTopWidth: "1px",
    borderBottomWidth: "1px",
  } as CSSStyleDeclaration);
}
function content(height: number) {
  vi.spyOn(
    HTMLTextAreaElement.prototype,
    "scrollHeight",
    "get",
  ).mockReturnValue(height);
}
afterEach(() => vi.restoreAllMocks());

// The composer grows with its text between three and twelve lines, without moving the page (WEB-U-005 / §7).
describe("WEB-U-020 the composer's height", () => {
  it("stays between three and twelve lines of its own text", () => {
    style("20px");
    content(10);
    const { getByLabelText, rerender } = render(
      <Box draft="a" focus={false} />,
    );
    const box = getByLabelText("Message");
    expect(box.style.height).toBe("78px");
    content(100);
    rerender(<Box draft="a longer draft" focus={false} />);
    expect(box.style.height).toBe("102px");
    content(1000);
    rerender(<Box draft={"line\n".repeat(40)} focus={false} />);
    expect(box.style.height).toBe("258px");
  });

  it("follows a window resize and focuses without scrolling when asked", () => {
    style("20px");
    content(10);
    const focus = vi.spyOn(HTMLTextAreaElement.prototype, "focus");
    const { getByLabelText } = render(<Box draft="a" focus />);
    expect(focus).toHaveBeenCalledWith({ preventScroll: true });
    content(1000);
    act(() => {
      window.dispatchEvent(new Event("resize"));
    });
    expect(getByLabelText("Message").style.height).toBe("258px");
  });

  it("leaves the height alone when the line height is not a number", () => {
    style("normal");
    content(1000);
    const { getByLabelText } = render(<Box draft="a" focus={false} />);
    expect(getByLabelText("Message").style.height).toBe("");
  });
});

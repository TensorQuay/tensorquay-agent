import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { FilesToggle } from "./FilesToggle";
import { preloadFilesPanel } from "./LazyFilesPanel";

vi.mock("./LazyFilesPanel", () => ({ preloadFilesPanel: vi.fn() }));

// WEB-U-176: the panel's code is fetched when the person is about to open it, never just because the page loaded.
describe("WEB-U-176 the Files button fetches the panel's code on intent", () => {
  it("asks for it on pointer over, pointer down and focus, not on render", () => {
    render(<FilesToggle open={false} onToggle={() => undefined} />);
    expect(preloadFilesPanel).not.toHaveBeenCalled();
    const button = screen.getByRole("button", { name: "Files" });
    const holder = button.parentElement as HTMLElement;
    fireEvent.pointerEnter(holder);
    fireEvent.pointerDown(holder);
    fireEvent.focus(button);
    expect(preloadFilesPanel).toHaveBeenCalledTimes(3);
  });
});

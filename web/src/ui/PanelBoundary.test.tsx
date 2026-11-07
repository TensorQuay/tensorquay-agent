import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PanelBoundary } from "./PanelBoundary";

const broken = { now: true };
function Panel() {
  if (broken.now) throw new Error("render failed with private detail");
  return <p>Panel content</p>;
}
afterEach(() => {
  broken.now = true;
  vi.restoreAllMocks();
});

// A panel that fails to render is isolated to itself and can be tried again (WEB-U-007, UI-GUIDELINES §14).
describe("WEB-U-007 a failing panel", () => {
  it("shows a plain failure without the error's details, and renders again on Try again", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <>
        <PanelBoundary>
          <Panel />
        </PanelBoundary>
        <p>Next panel</p>
      </>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "This panel could not be displayed.",
    );
    expect(screen.queryByText(/private detail/)).toBeNull();
    expect(screen.getByText("Next panel")).toBeVisible();
    broken.now = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.getByText("Panel content")).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

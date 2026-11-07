import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { LoadFailure } from "./LoadFailure";

/** A section whose list failed to load, then loads on the retry (or fails again). */
function Section({ works }: { works: boolean }) {
  const [state, setState] = useState<"failed" | "loaded">("failed");
  return (
    <section aria-label="Keys">
      {state === "failed" ? (
        <LoadFailure
          message="Your keys could not be loaded."
          onRetry={() => {
            if (works) setState("loaded");
          }}
        />
      ) : (
        <p>Issue tracker</p>
      )}
    </section>
  );
}

const frames = () =>
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    callback(0);
    return 0;
  });

// WEB-U-276 (the accessibility review of the core screens): a retry that works replaces the failure, Try again with
// it, so the focus fell to the page; the section that now holds the content takes it.
describe("WEB-U-276 the focus after Try again", () => {
  it("goes to the section that now holds the content", () => {
    frames();
    render(<Section works />);
    const retry = screen.getByRole("button", { name: "Try again" });
    retry.focus();
    fireEvent.click(retry);
    expect(screen.getByText("Issue tracker")).toBeVisible();
    expect(screen.getByRole("region", { name: "Keys" })).toHaveFocus();
    vi.restoreAllMocks();
  });

  it("stays on Try again when the retry fails again", () => {
    frames();
    render(<Section works={false} />);
    const retry = screen.getByRole("button", { name: "Try again" });
    retry.focus();
    fireEvent.click(retry);
    expect(retry).toHaveFocus();
    vi.restoreAllMocks();
  });

  it("leaves the focus alone when the content comes without a retry", () => {
    frames();
    const { rerender } = render(
      <section aria-label="Keys">
        <LoadFailure
          message="Your keys could not be loaded."
          onRetry={vi.fn()}
        />
      </section>,
    );
    rerender(
      <section aria-label="Keys">
        <p>Issue tracker</p>
      </section>,
    );
    expect(document.body).toHaveFocus();
    vi.restoreAllMocks();
  });
});

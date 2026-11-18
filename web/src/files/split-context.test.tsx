import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useSplit } from "./split-context";

/** A preview's view of the split outside a panel that docks: no split to offer, and nothing to do. */
function Reader() {
  const split = useSplit();
  split.expand();
  split.toggleFull();
  split.close();
  return (
    <p>
      {String(split.docks)} {split.mode}
    </p>
  );
}

// WEB-U-494: a preview with no panel around it (or one that doesn't dock) offers no split: its controls do nothing.
describe("WEB-U-494 the split's default", () => {
  it("offers no split and does nothing", () => {
    render(<Reader />);
    expect(screen.getByText("false panel")).toBeTruthy();
  });
});

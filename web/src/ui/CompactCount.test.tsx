// WEB-U-461: a short count shows its exact number on hover and to screen readers.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CompactCount, ShortAndExact } from "./CompactCount";

describe("WEB-U-461 a short count keeps its exact number one step away", () => {
  it("shows the short form, titles it with the exact number and gives screen readers the exact number", () => {
    render(
      <p data-testid="cell">
        <CompactCount value={2824643} locale="en" />
      </p>,
    );
    const cell = screen.getByTestId("cell");
    const shown = cell.querySelector('[aria-hidden="true"]');
    expect(shown).toHaveTextContent(/^2\.8M$/);
    expect(shown).toHaveAttribute("title", "2 824 643");
    // Read raw: the matcher would fold the narrow no-break spaces into plain ones.
    expect(cell.querySelector(".visually-hidden")?.textContent).toBe(
      "2\u202f824\u202f643",
    );
  });

  it("does the same for a sentence written both ways", () => {
    render(
      <p data-testid="line">
        <ShortAndExact short="2.8M tokens" exact="2 824 643 tokens" />
      </p>,
    );
    const line = screen.getByTestId("line");
    expect(line.querySelector('[aria-hidden="true"]')).toHaveTextContent(
      "2.8M tokens",
    );
    expect(line.querySelector(".visually-hidden")).toHaveTextContent(
      "2 824 643 tokens",
    );
  });
});

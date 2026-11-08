import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";

import { CheckRow } from "./CheckRow";

function Row({
  type = "checkbox",
  disabled = false,
}: {
  type?: "checkbox" | "radio";
  disabled?: boolean;
}) {
  const [on, setOn] = useState(false);
  return (
    <CheckRow
      type={type}
      label="Shell"
      checked={on}
      disabled={disabled}
      onChange={(event) => setOn(event.target.checked)}
    />
  );
}

// WEB-U-434: the settings forms' check rows are one shared control: the native checkbox or radio, kept for its keyboard,
// its label and its form, under the standard box it is drawn as (UI-GUIDELINES §7 Checkbox).
describe("WEB-U-434 the shared check row", () => {
  it("is a labelled checkbox drawn as the standard box, which its words toggle", () => {
    const { container } = render(<Row />);
    const box = screen.getByRole("checkbox", { name: "Shell" });
    expect(box).not.toBeChecked();
    expect(container.querySelector("svg.lucide-square")).not.toBeNull();
    fireEvent.click(screen.getByText("Shell"));
    expect(box).toBeChecked();
    expect(container.querySelector("svg.lucide-square-check")).not.toBeNull();
    expect(container.querySelector("svg")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });

  it("draws a radio as a circle and its choice as a dot", () => {
    const { container } = render(<Row type="radio" />);
    const radio = screen.getByRole("radio", { name: "Shell" });
    expect(container.querySelector("svg.lucide-circle")).not.toBeNull();
    fireEvent.click(radio);
    expect(radio).toBeChecked();
    expect(container.querySelector("svg.lucide-circle-dot")).not.toBeNull();
  });

  it("keeps a disabled row's control disabled", () => {
    render(<Row disabled />);
    expect(screen.getByRole("checkbox", { name: "Shell" })).toBeDisabled();
  });

  it("draws a mixed checkbox as the minus square, and its input says it is mixed", () => {
    const { container } = render(
      <CheckRow label="All tools" checked={false} mixed onChange={() => {}} />,
    );
    const box = screen.getByRole("checkbox", { name: "All tools" });
    expect((box as HTMLInputElement).indeterminate).toBe(true);
    expect(container.querySelector("svg.lucide-square-minus")).not.toBeNull();
  });
});

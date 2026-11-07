import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Button } from "./Button";

describe("Button", () => {
  it.each(["primary", "secondary", "ghost", "danger"] as const)(
    "renders the %s action",
    (variant) => {
      const onClick = vi.fn();
      render(
        <Button variant={variant} onClick={onClick}>
          Continue
        </Button>,
      );
      fireEvent.click(screen.getByRole("button", { name: "Continue" }));
      expect(onClick).toHaveBeenCalledOnce();
    },
  );
  it("prevents activation while disabled", () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Continue
      </Button>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(onClick).not.toHaveBeenCalled();
  });
  // A loading button keeps the focus it had (WEB-U-007, UI-GUIDELINES §9): a natively disabled one drops it to the
  // page, so a dialog opened by the save, or the refusal shown after it, would return the reader to the top.
  it("keeps its name and focus while loading, and is not activated or submitted again", () => {
    const onClick = vi.fn();
    const onSubmit = vi.fn((event: { preventDefault: () => void }) =>
      event.preventDefault(),
    );
    const { rerender } = render(
      <form onSubmit={onSubmit}>
        <Button type="submit" onClick={onClick}>
          Continue
        </Button>
      </form>,
    );
    const button = screen.getByRole("button", { name: "Continue" });
    button.focus();
    rerender(
      <form onSubmit={onSubmit}>
        <Button type="submit" loading onClick={onClick}>
          Continue
        </Button>
      </form>,
    );
    expect(button).toHaveFocus();
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

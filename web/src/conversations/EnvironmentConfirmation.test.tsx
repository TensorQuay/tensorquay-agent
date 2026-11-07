import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { EnvironmentConfirmation } from "./EnvironmentConfirmation";

describe("WEB-U-018 environment switch confirmation", () => {
  it("states the consequence inline and waits for explicit confirmation", () => {
    const confirm = vi.fn();
    render(
      <EnvironmentConfirmation
        template="Workstation"
        pending={false}
        onConfirm={confirm}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.getByRole("group", { name: "Switch environment" }),
    ).toHaveTextContent("Files and history stay; anything still running stops");
    expect(confirm).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "Switch to Workstation" }),
    );
    expect(confirm).toHaveBeenCalledOnce();
  });

  it("cancels through its control or Escape without applying a change", () => {
    const confirm = vi.fn();
    const cancel = vi.fn();
    render(
      <EnvironmentConfirmation
        template="Minimal"
        pending={false}
        onConfirm={confirm}
        onCancel={cancel}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel switch" }));
    fireEvent.keyDown(
      screen.getByRole("button", { name: "Switch to Minimal" }),
      { key: "Escape" },
    );
    expect(cancel).toHaveBeenCalledTimes(2);
    expect(confirm).not.toHaveBeenCalled();
  });

  it.each([
    { pending: true, disabled: false },
    { pending: false, disabled: true },
  ])(
    "prevents confirming during a pending change or active run: %o",
    (state) => {
      const confirm = vi.fn();
      render(
        <EnvironmentConfirmation
          template="Minimal"
          {...state}
          onConfirm={confirm}
          onCancel={vi.fn()}
        />,
      );
      const button = screen.getByRole("button", { name: "Switch to Minimal" });
      // Unavailable either way: disabled by a run, or busy (focusable, aria-disabled) while the change is pending.
      expect(button.matches(":disabled, [aria-disabled='true']")).toBe(true);
      fireEvent.click(button);
      expect(confirm).not.toHaveBeenCalled();
      expect(
        screen.getByRole("button", { name: "Cancel switch" }),
      ).toHaveProperty("disabled", state.pending);
    },
  );
});

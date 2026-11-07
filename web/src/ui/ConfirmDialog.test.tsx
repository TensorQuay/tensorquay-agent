import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";

import { ConfirmDialog } from "./ConfirmDialog";

// UI-GUIDELINES §5b: destructive actions ask once, name the object, and keep the dialog while the request runs.
describe("WEB-U-023 confirm dialog", () => {
  it("confirms with the named danger action and cancels without it", () => {
    const confirm = vi.fn();
    const close = vi.fn();
    render(
      <ConfirmDialog
        title="Revoke this token?"
        confirm="Revoke token"
        pending={false}
        error={null}
        onConfirm={confirm}
        onClose={close}
      >
        Programs using “CI” stop working at once.
      </ConfirmDialog>,
    );
    expect(
      screen.getByRole("dialog", { name: "Revoke this token?" }),
    ).toHaveTextContent("Programs using “CI” stop working at once.");
    fireEvent.click(screen.getByRole("button", { name: "Revoke token" }));
    expect(confirm).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(close).toHaveBeenCalledOnce();
  });

  it("cannot be dismissed while pending and shows a failure in place", () => {
    const close = vi.fn();
    const { rerender } = render(
      <ConfirmDialog
        title="Disable this account?"
        confirm="Disable account"
        pending
        error={null}
        onConfirm={vi.fn()}
        onClose={close}
      >
        Body
      </ConfirmDialog>,
    );
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    rerender(
      <ConfirmDialog
        title="Disable this account?"
        confirm="Disable account"
        pending={false}
        error={new ApiError(409, "last_admin", "diagnostic")}
        errorCopy={(error) =>
          error instanceof ApiError && error.code === "last_admin"
            ? "At least one administrator must stay active."
            : null
        }
        onConfirm={vi.fn()}
        onClose={close}
      >
        Body
      </ConfirmDialog>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "At least one administrator must stay active.",
    );
  });
});

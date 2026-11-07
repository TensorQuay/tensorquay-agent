import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SecretDialog } from "./SecretDialog";

afterEach(() => {
  vi.unstubAllGlobals();
});

// Task 067: a token or invitation link is shown once, whole, with a copy button.
describe("WEB-U-023 secret shown once", () => {
  it("shows the whole value read-only, copies it and closes once the person has copied it", async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>();
    writeText.mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    const close = vi.fn();
    const value = `tqa_pat_${"x".repeat(43)}`;
    render(
      <SecretDialog
        title="Token created"
        label="Token"
        value={value}
        onClose={close}
      >
        Copy it now. You won’t be able to see it again.
      </SecretDialog>,
    );
    const field = screen.getByRole("textbox", { name: "Token" });
    expect(field).toHaveValue(value);
    expect(field).toHaveAttribute("readonly");
    expect(
      screen.getByRole("dialog", { name: "Token created" }),
    ).toHaveTextContent("You won’t be able to see it again.");
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await screen.findByRole("button", { name: "Copied" });
    expect(writeText).toHaveBeenCalledWith(value);
    fireEvent.click(screen.getByRole("button", { name: "I’ve copied it" }));
    expect(close).toHaveBeenCalledOnce();
  });
});

import { readFileSync } from "node:fs";

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";

import { Dialog } from "./Dialog";

function Example() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button onClick={() => setOpen(true)}>Open dialog</button>
      <Dialog open={open} onClose={() => setOpen(false)} title="Confirm change">
        <button>Confirm</button>
      </Dialog>
    </>
  );
}

describe("Dialog", () => {
  it("opens with a name and returns focus after keyboard cancellation", () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Open dialog" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Confirm change" });
    expect(dialog).toHaveAttribute("open");
    fireEvent(dialog, new Event("cancel", { bubbles: true, cancelable: true }));
    expect(dialog).not.toHaveAttribute("open");
    expect(trigger).toHaveFocus();
  });
  it("closes from its labelled close control", () => {
    render(<Example />);
    fireEvent.click(screen.getByRole("button", { name: "Open dialog" }));
    fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("wraps Tab and Shift+Tab within the dialog controls", () => {
    render(<Example />);
    fireEvent.click(screen.getByRole("button", { name: "Open dialog" }));
    const first = screen.getByRole("button", { name: "Close dialog" });
    const last = screen.getByRole("button", { name: "Confirm" });
    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
  });
});

// WEB-U-230 (UI-GUIDELINES' Side sheet): one sheet primitive. A dialog placed at an edge is a sheet, full height and
// sliding in from that edge: the navigation drawer at the start, a side sheet (All helpers, a key's history) at the
// end. Esc and × close it, focus stays inside, as any dialog.
describe("WEB-U-230 a dialog placed at an edge is a sheet", () => {
  it("is a sheet at the end that Esc and its × close", () => {
    function Sheet() {
      const [open, setOpen] = useState(true);
      return open ? (
        <Dialog
          open
          placement="end"
          onClose={() => setOpen(false)}
          title="All helpers"
        >
          <button>Open researcher</button>
        </Dialog>
      ) : null;
    }
    render(<Sheet />);
    const sheet = screen.getByRole("dialog", { name: "All helpers" });
    expect(sheet).toHaveClass("dialog-sheet");
    expect(sheet).toHaveAttribute("data-placement", "end");
    fireEvent(sheet, new Event("cancel", { bubbles: true, cancelable: true }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("leaves a dialog without a placement in the middle, as before", () => {
    render(<Example />);
    fireEvent.click(screen.getByRole("button", { name: "Open dialog" }));
    const dialog = screen.getByRole("dialog", { name: "Confirm change" });
    expect(dialog).not.toHaveClass("dialog-sheet");
    expect(dialog).not.toHaveAttribute("data-placement");
  });

  it("slides a sheet in from its edge, and only fades it with reduced motion", () => {
    const css = readFileSync("src/app/globals.css", "utf8");
    expect(css).toMatch(
      /\.dialog-sheet\[data-placement="start"\][^}]*animation:\s*sheet-from-start var\(--motion-panel\)/,
    );
    expect(css).toMatch(
      /\.dialog-sheet\[data-placement="end"\][^}]*animation:\s*sheet-from-end var\(--motion-panel\)/,
    );
    const reduced = css.slice(css.indexOf("@media (prefers-reduced-motion"));
    expect(reduced).toMatch(
      /\.dialog-sheet[^{]*\{[^}]*animation:\s*fade-in var\(--motion-panel\)/,
    );
  });
});

import { render, screen } from "@testing-library/react";
import { useRef } from "react";
import { describe, expect, it } from "vitest";

import { Popover } from "./Popover";

function Anchored({ inDialog }: { inDialog: boolean }) {
  const anchor = useRef<HTMLButtonElement>(null);
  const content = (
    <>
      <button ref={anchor} type="button">
        More
      </button>
      <Popover anchor={anchor} label="Actions" onClose={() => undefined}>
        <p>Menu</p>
      </Popover>
    </>
  );
  return inDialog ? <dialog open>{content}</dialog> : content;
}

// WEB-U-071: a task's menu in the phone's navigation drawer (a modal dialog) opens inside the dialog, above it, where it
// can be used; anywhere else it opens on the page.
describe("WEB-U-071 a menu inside a dialog", () => {
  it("renders in the dialog its trigger is in, else in the page", () => {
    const { unmount } = render(<Anchored inDialog />);
    expect(
      screen.getByRole("region", { name: "Actions" }).closest("dialog"),
    ).not.toBeNull();
    unmount();
    render(<Anchored inDialog={false} />);
    expect(screen.getByRole("region", { name: "Actions" }).parentElement).toBe(
      document.body,
    );
  });
});

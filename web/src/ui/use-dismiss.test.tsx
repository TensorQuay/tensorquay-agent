import { fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { useDismiss } from "./use-dismiss";

function Surface({
  onEscape,
  onOutside,
}: {
  onEscape: () => void;
  onOutside: () => void;
}) {
  const surface = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useDismiss(surface, { onEscape, onOutside, ignore: trigger });
  return (
    <>
      <button ref={trigger}>Trigger</button>
      <div ref={surface}>
        <input aria-label="Inside" />
      </div>
      <button>Elsewhere</button>
    </>
  );
}

// The UI standard's one rule for every control that opens (Menu, Popover, In-place edit): Esc and a click elsewhere
// close it; what each means (cancel, commit, close) is the caller's.
describe("WEB-U-084 useDismiss", () => {
  it("calls onEscape for Esc inside, and keeps it from the layers under it", () => {
    const onEscape = vi.fn();
    const under = vi.fn();
    const { container } = render(
      <Surface onEscape={onEscape} onOutside={() => undefined} />,
    );
    // A layer under it (a dialog) listening for the key.
    container.addEventListener("keydown", under);
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    screen.getByRole("textbox", { name: "Inside" }).dispatchEvent(event);
    expect(onEscape).toHaveBeenCalledOnce();
    // A dialog under it neither hears the key nor cancels.
    expect(under).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(true);
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Inside" }), {
      key: "Enter",
    });
    expect(onEscape).toHaveBeenCalledOnce();
  });

  it("calls onOutside for a press elsewhere, never inside or on the trigger", () => {
    const onOutside = vi.fn();
    render(<Surface onEscape={() => undefined} onOutside={onOutside} />);
    fireEvent.pointerDown(screen.getByRole("textbox", { name: "Inside" }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "Trigger" }));
    expect(onOutside).not.toHaveBeenCalled();
    fireEvent.pointerDown(screen.getByRole("button", { name: "Elsewhere" }));
    expect(onOutside).toHaveBeenCalledOnce();
  });

  it("hears a press elsewhere even when the page stops it", () => {
    const onOutside = vi.fn();
    render(
      <>
        <Surface onEscape={() => undefined} onOutside={onOutside} />
        <div onPointerDown={(event) => event.stopPropagation()}>
          <button>Busy</button>
        </div>
      </>,
    );
    fireEvent.pointerDown(screen.getByRole("button", { name: "Busy" }));
    expect(onOutside).toHaveBeenCalledOnce();
  });

  it("stops listening once unmounted", () => {
    const onOutside = vi.fn();
    const { unmount } = render(
      <Surface onEscape={() => undefined} onOutside={onOutside} />,
    );
    unmount();
    fireEvent.pointerDown(document.body);
    expect(onOutside).not.toHaveBeenCalled();
  });
});

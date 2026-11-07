import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// noVNC's own pointer capture, unmodified: what the picture's mousedown calls (rfb.js, `setCapture(this._canvas)`).
import { setCapture } from "../../node_modules/@novnc/novnc/core/util/events.js";
import {
  clients,
  FakeScreenClient,
  FakeSocket,
  socket,
} from "../test/screen-socket";

vi.mock("@novnc/novnc", () => ({ default: FakeScreenClient }));
vi.mock("../api/stream", () => ({
  openScreenSocket: (path: string) => new FakeSocket(path),
}));

beforeEach(() => {
  FakeSocket.opened = [];
  clients.length = 0;
});

const layer = () => document.getElementById("noVNC_mouse_capture_elem");
const captured = () =>
  (document as Document & { captureElement?: Element | null }).captureElement;

// WEB-U-336: a press on the picture makes noVNC capture the pointer: a layer over the whole page, hidden again by a
// mouseup that reaches it or the window. In the expanded screen, a modal dialog in the top layer above that layer, the
// mouseup stops at the picture's canvas, so the layer stayed over the page once the dialog closed and took the next
// click (Hand back's, WEB-E2E-072 on the stack). The panel lets the capture go when the dialog closes, however it
// closes, and when its screen client leaves.
describe("WEB-U-336 the expanded screen lets the pointer go", () => {
  async function pressedInExpand() {
    const { ScreenPanel } = await import("./ScreenPanel");
    const shown = render(
      <ScreenPanel conversationId="task-1" working={false} />,
    );
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    const dialog = screen.getByRole("dialog", { name: "Screen" });
    // The screen client's canvas inside the picture, pressed: noVNC captures the pointer and shows its layer.
    const canvas = document.createElement("canvas");
    within(dialog).getByRole("img").appendChild(canvas);
    setCapture(canvas);
    expect(layer()).toBeVisible();
    return { dialog, shown };
  }

  it("hides noVNC's capture layer when the dialog closes from its button", async () => {
    const { dialog } = await pressedInExpand();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close dialog" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(layer()).not.toBeVisible();
    expect(captured()).toBeNull();
  });

  it("hides it when the dialog closes because the screen stopped being live", async () => {
    await pressedInExpand();
    act(() => socket().shut(4409, "sandbox_not_running"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(layer()).not.toBeVisible();
    expect(captured()).toBeNull();
  });

  it("hides it when the panel goes, taking its screen client", async () => {
    const { shown } = await pressedInExpand();
    shown.unmount();
    expect(layer()).not.toBeVisible();
    expect(captured()).toBeNull();
  });

  it("leaves a capture alone while the dialog stays open, so a press and a drag on the picture still work", async () => {
    await pressedInExpand();
    expect(layer()).toBeVisible();
    expect(captured()).not.toBeNull();
  });
});

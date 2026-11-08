import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

const failed = (reason: "not_cleared" | "unconfirmed") => ({
  kind: "screen.sign_out_failed" as const,
  at: "2026-10-12T10:00:00Z",
  reason,
});

// WEB-U-331 (a pass over the pages before 17.0): a clear asked from the warning about earlier sign-ins answers where it
// was asked, right under its button, before the help paragraphs (at 390 the help pushed the answer below the panel's
// fold).
describe("WEB-U-331 a clear's result sits under the button that asked for it", () => {
  it.each([
    [
      "not_cleared",
      "The browser's sign-ins couldn't be cleared, so they may remain. Try again.",
    ],
    [
      "unconfirmed",
      "Clearing the browser's sign-ins wasn't confirmed, so they may remain. Try again.",
    ],
  ] as const)("%s", async (reason, words) => {
    const { api } = await import("../api/client");
    vi.restoreAllMocks();
    const execute = vi.fn().mockResolvedValue(undefined);
    vi.spyOn(api, "screenSignOut").mockReturnValue({
      execute,
    } as unknown as ReturnType<typeof api.screenSignOut>);
    const { ScreenPanel } = await import("./ScreenPanel");
    const shown = render(
      <ScreenPanel conversationId="task-1" working={false} notices={[]} />,
    );
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
      ),
    );
    const button = screen.getByRole("button", { name: "Clear sign-ins" });
    fireEvent.click(button);
    const dialog = screen.getByRole("dialog", {
      name: "Clear the desktop browser's sign-ins?",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Clear sign-ins" }),
    );
    expect(
      await screen.findByText("Clearing the browser's sign-ins…"),
    ).toBeVisible();
    shown.rerender(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        notices={[failed(reason)]}
      />,
    );
    const answer = screen.getByText(words);
    expect(answer).toHaveAttribute("data-tone", "danger");
    const warningButton = screen.getByRole("button", {
      name: "Clear sign-ins",
    });
    const help = screen.getByText(/^Keys go to the desktop while it has focus/);
    // After the button, before the help: the answer is where the person pressed.
    expect(
      warningButton.compareDocumentPosition(answer) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      answer.compareDocumentPosition(help) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});

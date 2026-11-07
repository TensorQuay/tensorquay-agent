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

// WEB-U-244 (ADR-0039 §6): signing the browser out is in the panel's ⋯, asks first, and isn't offered while the agent
// works. WEB-U-259: the request is accepted with no answer (202); the panel says it is signing out until the sign-out's
// notice arrives, then says that notice: signed out, or why the browser's sign-ins may remain.
describe("WEB-U-244 WEB-U-259 Clear the desktop browser's sign-ins", () => {
  const notice = (
    kind: "screen.signed_out" | "screen.sign_out_failed" | "screen.taken_over",
    reason?: "not_cleared" | "unconfirmed",
  ) => ({ kind, at: "2026-10-05T10:00:00Z", ...(reason ? { reason } : {}) });

  async function signOut(notices: ReturnType<typeof notice>[] = []) {
    const { api } = await import("../api/client");
    const execute = vi.fn().mockResolvedValue(undefined);
    // One spy per test: an earlier test's calls don't count here.
    vi.restoreAllMocks();
    const request = vi
      .spyOn(api, "screenSignOut")
      .mockReturnValue({ execute } as unknown as ReturnType<
        typeof api.screenSignOut
      >);
    const { ScreenPanel } = await import("./ScreenPanel");
    const shown = render(
      <ScreenPanel conversationId="task-1" working={false} notices={notices} />,
    );
    act(() => socket().open());
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for the screen" }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Clear the browser's sign-ins" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Clear the desktop browser's sign-ins?",
    });
    expect(dialog).toHaveTextContent(
      "The browser on the desktop forgets every sign-in now. Other apps on the desktop keep theirs.",
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Clear sign-ins" }),
    );
    expect(
      await screen.findByText("Clearing the browser's sign-ins…"),
    ).toBeVisible();
    expect(request).toHaveBeenCalledOnce();
    expect(execute).toHaveBeenCalledOnce();
    const rerender = (later: ReturnType<typeof notice>[], working = false) =>
      shown.rerender(
        <ScreenPanel
          conversationId="task-1"
          working={working}
          notices={later}
        />,
      );
    return { rerender };
  }

  it("asks, then says it is signing out until the notice says it signed out", async () => {
    // An earlier sign-out's notice is not this one's outcome.
    const earlier = [notice("screen.signed_out"), notice("screen.taken_over")];
    const { rerender } = await signOut(earlier);
    expect(screen.queryByText(/sign-ins were cleared/)).toBeNull();
    rerender([...earlier, notice("screen.signed_out")]);
    expect(
      screen.getByText("The desktop browser's sign-ins were cleared."),
    ).toHaveAttribute("role", "status");
    expect(screen.queryByText("Clearing the browser's sign-ins…")).toBeNull();
  });

  // The c16 UX review, #16: the outcome is the transcript's to keep; under the panel it goes once a run starts, or at
  // the next Take over, so an old "cleared" never sits beside a new sign-in.
  it("lets the outcome line go when a run starts", async () => {
    const cleared = [notice("screen.signed_out")];
    const { rerender } = await signOut();
    rerender(cleared);
    expect(
      screen.getByText("The desktop browser's sign-ins were cleared."),
    ).toBeVisible();
    rerender(cleared, true);
    expect(
      screen.queryByText("The desktop browser's sign-ins were cleared."),
    ).toBeNull();
    rerender(cleared, false);
    expect(
      screen.queryByText("The desktop browser's sign-ins were cleared."),
    ).toBeNull();
  });

  it("lets the outcome line go at the next Take over", async () => {
    const cleared = [notice("screen.signed_out")];
    const { rerender } = await signOut();
    rerender(cleared);
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    expect(
      screen.queryByText("The desktop browser's sign-ins were cleared."),
    ).toBeNull();
  });

  it.each([
    [
      "not_cleared",
      "The browser's sign-ins couldn't be cleared, so they may remain. Try again.",
    ],
    [
      "unconfirmed",
      "Clearing the browser's sign-ins wasn't confirmed, so they may remain. Try again.",
    ],
  ] as const)("says why the sign-ins may remain: %s", async (reason, words) => {
    const { rerender } = await signOut();
    rerender([notice("screen.sign_out_failed", reason)]);
    expect(screen.getByText(words)).toHaveAttribute("data-tone", "danger");
  });

  it("isn't available while the agent works, and says why", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working />);
    act(() => socket().open());
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for the screen" }),
    );
    const item = screen.getByRole("menuitem", {
      name: /Clear the browser's sign-ins/,
    });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAccessibleDescription(
      /Available when the agent isn't working\./,
    );
  });

  // The c16 UX review, #6b: each run ends signed out, so with no sign-in left the action says so instead of clearing
  // what is already clear; offered whenever the page can't tell (the prop's default).
  it("says the browser is already signed out when no sign-in can remain", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    const shown = render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        signInMayRemain={false}
      />,
    );
    act(() => socket().open());
    const menu = () =>
      fireEvent.click(
        screen.getByRole("button", { name: "More actions for the screen" }),
      );
    menu();
    const item = screen.getByRole("menuitem", {
      name: /Clear the browser's sign-ins/,
    });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAccessibleDescription(
      /The browser is already signed out: each run ends that way\./,
    );
    fireEvent.keyDown(item, { key: "Escape" });
    shown.rerender(<ScreenPanel conversationId="task-1" working={false} />);
    if (!screen.queryByRole("menuitem")) menu();
    expect(
      screen.getByRole("menuitem", { name: /Clear the browser's sign-ins/ }),
    ).not.toHaveAttribute("aria-disabled", "true");
  });
});

// WEB-U-247: while the desktop runs and the task has pictures, the panel switches between Live and Replay; once it
// isn't running, the replay is all it shows.

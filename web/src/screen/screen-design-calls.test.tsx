import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
  FakeScreenClient.autoConnect = true;
});
afterEach(() => {
  FakeScreenClient.autoConnect = true;
});

/** Lets the panel's dynamic import of the screen client and its connect land. */
const settle = () =>
  act(async () => {
    for (let index = 0; index < 5; index++) await Promise.resolve();
  });

const grant = (fields: Record<string, unknown>) =>
  act(() =>
    socket().frame(
      JSON.stringify({ type: "control", holder: "you", ...fields }),
    ),
  );

// WEB-U-333 (a pass over the pages before 17.0): while control is granted but the agent may still be stopping, the
// status line is a state to wait out, in the panel's attention tone with a clock; the grant that follows reads as plain
// control.
describe("WEB-U-333 a late grant's line reads as a state to wait out", () => {
  it("is in the warn tone with an icon, then plain once the reset lands", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    grant({ ready: false });
    const late = screen.getByRole("status");
    expect(late).toHaveTextContent("so a sign-in now may not last");
    expect(late).toHaveAttribute("data-tone", "warn");
    expect(late.querySelector("svg")).not.toBeNull();
    grant({});
    const ready = screen.getByRole("status");
    expect(ready).toHaveTextContent(/^You have control\. When you're done/);
    expect(ready).not.toHaveAttribute("data-tone");
    expect(ready.querySelector("svg")).toBeNull();
  });
});

// WEB-U-334 (a pass over the pages before 17.0): the picture box shows only while a picture is being drawn (the screen
// client connected); with none (not connected yet, or the connection dropped) there is no empty box and no Expand.
describe("WEB-U-334 no empty picture box, and Expand only with a picture", () => {
  it("shows the box and Expand only while connected", async () => {
    FakeScreenClient.autoConnect = false;
    const { ScreenPanel } = await import("./ScreenPanel");
    const { container } = render(
      <ScreenPanel conversationId="task-1" working={false} />,
    );
    act(() => socket().open());
    await settle();
    const box = () => container.querySelector(".screen-picture");
    expect(box()).toHaveAttribute("data-empty");
    expect(screen.queryByRole("button", { name: "Expand" })).toBeNull();
    act(() => clients.at(-1)?.emit("connect"));
    expect(box()).not.toHaveAttribute("data-empty");
    expect(screen.getByRole("button", { name: "Expand" })).toBeVisible();
    act(() => clients.at(-1)?.emit("disconnect"));
    expect(box()).toHaveAttribute("data-empty");
    expect(screen.queryByRole("button", { name: "Expand" })).toBeNull();
  });
});

// WEB-U-335 (a pass over the pages before 17.0): the warning about earlier sign-ins always comes with its means: Clear
// sign-ins is there from the grant, disabled while the agent is still stopping and described by why, enabled once it
// has stopped.
describe("WEB-U-335 the sign-ins warning always comes with its button", () => {
  it("is disabled with its reason while the agent works, then enabled", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    const shown = render(<ScreenPanel conversationId="task-1" working />);
    act(() => socket().open());
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    grant({ reset_ok: false });
    expect(
      screen.getByText(/^Earlier sign-ins may still be in this browser/),
    ).toBeVisible();
    const button = screen.getByRole("button", { name: "Clear sign-ins" });
    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleDescription(
      "Available when the agent isn't working.",
    );
    shown.rerender(<ScreenPanel conversationId="task-1" working={false} />);
    expect(
      screen.getByRole("button", { name: "Clear sign-ins" }),
    ).toBeEnabled();
  });

  // The page's order, as the fake front showed it: the grant's warning shows before the take-over notice that tells
  // the page a sign-in may remain. A reset that wasn't clean is itself that news, so the button doesn't wait for it.
  it("comes with the warning even before the page knows a sign-in may remain", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        signInMayRemain={false}
      />,
    );
    act(() => socket().open());
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    grant({ reset_ok: false });
    expect(
      screen.getByText(/^Earlier sign-ins may still be in this browser/),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Clear sign-ins" }),
    ).toBeEnabled();
  });
});

import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  clients,
  control,
  deps,
  FakeScreenClient,
  FakeSocket,
  socket,
} from "../test/screen-socket";

import { ScreenChannel } from "./channel";
import { closedState, retries, screenState } from "./screen-state";
import { useScreen } from "./use-screen";

beforeEach(() => {
  FakeSocket.opened = [];
});
afterEach(() => {
  vi.useRealTimers();
});

// WEB-U-242 (ADR-0039 §5 and §6, its close codes; the computer-use design note §2): the Screen panel's states come from
// the viewer socket: its open, the gateway's control frames, and its close (a 44xx code with the error as reason).
describe("WEB-U-242 the Screen panel's states", () => {
  it("reads each refusal from its close reason, and anything else as a drop", () => {
    expect(closedState("sandbox_not_running")).toEqual({ kind: "notRunning" });
    expect(closedState("screen_viewers")).toEqual({ kind: "tooMany" });
    expect(closedState("no_desktop")).toEqual({
      kind: "cantOpen",
      why: "noDesktop",
    });
    expect(closedState("forbidden")).toEqual({
      kind: "cantOpen",
      why: "notOwner",
    });
    expect(closedState("not_found")).toEqual({
      kind: "cantOpen",
      why: "notFound",
    });
    expect(closedState("")).toEqual({ kind: "reconnecting" });
    // Only a drop and an unreachable screen are tried again by themselves; a refusal waits for the person.
    expect(retries(closedState(""))).toBe(true);
    expect(retries(closedState("screen_unavailable"))).toBe(true);
    for (const reason of [
      "sandbox_not_running",
      "screen_viewers",
      "no_desktop",
      "forbidden",
    ])
      expect(retries(closedState(reason))).toBe(false);
  });

  it("holds the screen while the gateway says this tab does, never on its own", () => {
    let state = screenState({ kind: "connecting" }, { type: "open" });
    // WEB-U-261: open, but connecting until the first control frame says who holds the screen.
    expect(state.kind).toBe("connecting");
    state = screenState(state, {
      type: "control",
      holder: "none",
      ready: true,
      resetOk: true,
    });
    expect(state.kind).toBe("watching");
    state = screenState(state, { type: "take_over" });
    expect(state.kind).toBe("taking");
    // A control frame that still says none (an earlier one) doesn't undo the ask.
    expect(
      screenState(state, {
        type: "control",
        holder: "none",
        ready: true,
        resetOk: true,
      }).kind,
    ).toBe("taking");
    // Control says whether the stopped run had ended (ADR-0039 §6): ready, or granted when the gateway's wait ran out.
    expect(
      screenState(state, {
        type: "control",
        holder: "you",
        ready: true,
        resetOk: true,
      }),
    ).toEqual({ kind: "control", ready: true, resetOk: true });
    expect(
      screenState(state, {
        type: "control",
        holder: "you",
        ready: false,
        resetOk: true,
      }),
    ).toEqual({ kind: "control", ready: false, resetOk: true });
    // No answer in time: offered again, saying so; given up only while still asking.
    expect(screenState(state, { type: "take_over_timeout" })).toEqual({
      kind: "watching",
      takeOverFailed: true,
    });
    expect(
      screenState(
        { kind: "control", ready: true, resetOk: true },
        { type: "take_over_timeout" },
      ),
    ).toEqual({ kind: "control", ready: true, resetOk: true });
    // Given up and handed back: a grant already on its way, and the "none" after the hand-back, change nothing until
    // the next ask; another tab's grant still shows.
    const failed = { kind: "watching" as const, takeOverFailed: true };
    for (const holder of ["you", "none"] as const)
      expect(
        screenState(failed, {
          type: "control",
          holder,
          ready: true,
          resetOk: true,
        }),
      ).toBe(failed);
    expect(
      screenState(failed, {
        type: "control",
        holder: "other",
        ready: true,
        resetOk: true,
      }).kind,
    ).toBe("other");
    expect(screenState(failed, { type: "take_over" }).kind).toBe("taking");
    expect(
      screenState(state, {
        type: "control",
        holder: "other",
        ready: true,
        resetOk: true,
      }).kind,
    ).toBe("other");
    expect(
      screenState(
        { kind: "control", ready: true, resetOk: true },
        { type: "control", holder: "none", ready: true, resetOk: true },
      ).kind,
    ).toBe("watching");
  });

  it("gives the screen client only binary frames, holding those before it attaches", async () => {
    const fake = new FakeSocket("/s");
    const holders: string[] = [];
    const channel = new ScreenChannel(
      fake as unknown as WebSocket,
      (holder, { ready, resetOk }) =>
        holders.push(
          [holder, ready ? "" : "not ready", resetOk ? "" : "reset failed"]
            .filter(Boolean)
            .join(" "),
        ),
    );
    fake.upgrade();
    const first = new Uint8Array([82, 70, 66]).buffer;
    fake.frame(first);
    fake.frame(control("you"));
    // No `ready` means ready; false only when said so.
    fake.frame(
      JSON.stringify({ type: "control", holder: "you", ready: false }),
    );
    fake.frame(
      JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
    );
    fake.frame("not json");
    const received: unknown[] = [];
    channel.onmessage = (event) => received.push(event.data);
    await Promise.resolve();
    expect(received).toEqual([first]);
    expect(holders).toEqual(["you", "you not ready", "you reset failed"]);
    channel.ask("take_over");
    expect(fake.sent).toEqual([JSON.stringify({ type: "take_over" })]);
  });

  // WEB-U-377 (the c16f merge's WEB-E2E-037 failure): noVNC attaches by setting its receiver and then, in the same call,
  // starts its handshake because the socket is already open. A held frame handed over inside that call reached it before
  // the handshake ("Unknown init state"), so the panel kept reconnecting.
  it("hands held frames over after the screen client's attaching call returns, in the server's order", async () => {
    const fake = new FakeSocket("/s");
    const channel = new ScreenChannel(fake as unknown as WebSocket, vi.fn());
    fake.upgrade();
    const hello = new TextEncoder().encode("RFB 003.008\n").buffer;
    fake.frame(hello);
    const seen: string[] = [];
    let handshaking = false;
    // noVNC's attach, in its order: the receiver first, then the handshake on the open socket.
    channel.onmessage = (event) =>
      seen.push(
        `${handshaking ? "after" : "before"} the handshake: ${new Uint8Array(event.data as ArrayBuffer).length} bytes`,
      );
    handshaking = true;
    expect(seen).toEqual([]);
    // A frame arriving before the hand-over queues behind the held one.
    fake.frame(new Uint8Array([1, 2]).buffer);
    await Promise.resolve();
    expect(seen).toEqual([
      "after the handshake: 12 bytes",
      "after the handshake: 2 bytes",
    ]);
    // Once handed over, a frame goes straight through.
    fake.frame(new Uint8Array([3]).buffer);
    expect(seen).toHaveLength(3);
  });

  it("is the raw channel the screen client needs: its socket's state, type and protocol, sending and closing", () => {
    const fake = new FakeSocket("/s");
    fake.protocol = "binary";
    const close = vi.spyOn(fake, "close");
    const channel = new ScreenChannel(fake as unknown as WebSocket, vi.fn());
    expect(channel.binaryType).toBe("arraybuffer");
    channel.binaryType = "blob";
    expect(fake.binaryType).toBe("blob");
    expect(channel.protocol).toBe("binary");
    expect(channel.readyState).toBe(WebSocket.CONNECTING);
    // Asking before the socket is open sends nothing; the screen client's own bytes go as given.
    channel.ask("hand_back");
    expect(fake.sent).toEqual([]);
    fake.open();
    expect(channel.readyState).toBe(WebSocket.OPEN);
    const bytes = new Uint8Array([1, 2]);
    channel.send(bytes);
    expect(fake.sent).toEqual([bytes]);
    const opened = vi.fn();
    const closed = vi.fn();
    const failed = vi.fn();
    channel.onopen = opened;
    channel.onclose = closed;
    channel.onerror = failed;
    fake.dispatchEvent(new Event("open"));
    fake.dispatchEvent(new Event("error"));
    fake.shut(1000);
    expect([opened, failed, closed].map((f) => f.mock.calls.length)).toEqual([
      1, 1, 1,
    ]);
    channel.close(1000, "done");
    expect(close).toHaveBeenCalledWith(1000, "done");
    // A frame of another type, and a receiver taken away, hold nothing.
    channel.onmessage = null;
    expect(channel.onmessage).toBeNull();
  });

  it("opens the task's screen, tries a drop again by itself and a refusal never", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useScreen("task-1", true, deps));
    expect(socket().path).toBe("/api/v1/conversations/task-1/screen");
    act(() => socket().open());
    expect(result.current.state.kind).toBe("watching");
    act(() => socket().shut(1006));
    expect(result.current.state.kind).toBe("reconnecting");
    act(() => {
      vi.advanceTimersByTime(1_500);
    });
    expect(FakeSocket.opened).toHaveLength(2);
    act(() => socket().shut(4409, "sandbox_not_running"));
    expect(result.current.state.kind).toBe("notRunning");
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(FakeSocket.opened).toHaveLength(2);
  });

  // WEB-U-261: the socket opens before the desktop's side is paired (up to 35 s); until the first control frame the
  // panel says it is connecting, not "Watching." over a blank picture.
  it("stays connecting until the first control frame", () => {
    const { result } = renderHook(() => useScreen("task-1", true, deps));
    act(() => socket().upgrade());
    expect(result.current.state.kind).toBe("connecting");
    act(() => socket().frame(control("none")));
    expect(result.current.state.kind).toBe("watching");
  });

  // WEB-U-261: a desktop that wasn't running is tried again once the task starts working, since its run starts it; a
  // task that was already working when it closed, or one that stops, opens nothing new.
  it("opens again when the task starts working after the desktop wasn't running", () => {
    const { result, rerender } = renderHook(
      ({ working }) => useScreen("task-1", true, deps, working),
      { initialProps: { working: false } },
    );
    act(() => socket().shut(4409, "sandbox_not_running"));
    expect(result.current.state.kind).toBe("notRunning");
    expect(FakeSocket.opened).toHaveLength(1);
    rerender({ working: true });
    expect(FakeSocket.opened).toHaveLength(2);
    act(() => socket().open());
    expect(result.current.state.kind).toBe("watching");
    rerender({ working: false });
    rerender({ working: true });
    expect(FakeSocket.opened).toHaveLength(2);
  });

  it("asks to take over and hand back on the socket, and follows the control frames", () => {
    const { result } = renderHook(() => useScreen("task-1", true, deps));
    act(() => socket().open());
    act(() => result.current.takeOver());
    expect(result.current.state.kind).toBe("taking");
    act(() => socket().frame(control("you")));
    expect(result.current.state.kind).toBe("control");
    act(() => result.current.handBack());
    act(() => socket().frame(control("none")));
    expect(result.current.state.kind).toBe("watching");
    expect(socket().sent).toEqual([
      JSON.stringify({ type: "take_over" }),
      JSON.stringify({ type: "hand_back" }),
    ]);
  });
});

// The panel itself, with the screen client stood in for: its words, its buttons, and view-only until control.
vi.mock("@novnc/novnc", () => ({ default: FakeScreenClient }));
vi.mock("../api/stream", () => ({
  openScreenSocket: (path: string) => new FakeSocket(path),
}));

/** The screen client has loaded and connected (noVNC's `connect`), so a grant can give it input. */
async function connected() {
  await vi.waitFor(() => expect(clients.length).toBeGreaterThan(0));
  await act(async () => {
    await Promise.resolve();
  });
}

describe("WEB-U-242 the Screen panel", () => {
  beforeEach(() => {
    clients.length = 0;
    FakeScreenClient.autoConnect = true;
  });

  it("watches view-only, takes over, then gives input to the desktop until it hands back", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    const shown = render(<ScreenPanel conversationId="task-1" working />);
    const working = (now: boolean) =>
      shown.rerender(<ScreenPanel conversationId="task-1" working={now} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Connecting to the desktop…",
    );
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Watching. The agent is working.",
    );
    await vi.waitFor(() => expect(clients).toHaveLength(1));
    expect(clients[0]?.viewOnly).toBe(true);
    // Take over stops the run as Stop does: said before the press, while the agent works.
    expect(
      screen.getByText(
        "Taking over stops the agent. After you hand back, send a message to let it carry on.",
      ),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    // The gateway grants control once the run has stopped and the browser was reset (its end clears it).
    expect(screen.getByRole("status")).toHaveTextContent(
      "Stopping the agent before you take over…",
    );
    expect(
      screen.queryByText(
        "Taking over stops the agent. After you hand back, send a message to let it carry on.",
      ),
    ).toBeNull();
    working(false);
    act(() => socket().frame(control("you")));
    expect(screen.getByRole("status")).toHaveTextContent(
      "You have control. When you're done, hand back, then send a message to let the agent carry on.",
    );
    expect(
      screen.getByText(
        "Keys go to the desktop while it has focus; press Escape twice to give the keyboard back to the page. You can paste in; nothing is copied out.",
      ),
    ).toBeVisible();
    expect(
      screen.getByText(
        "A sign-in here lasts for the agent's next run only. Each run after that starts signed out.",
      ),
    ).toBeVisible();
    expect(clients[0]?.viewOnly).toBe(false);
    // WEB-U-270: the keyboard goes to the desktop once this tab holds it.
    expect(clients[0]?.focus).toHaveBeenCalledOnce();
    // The c16 a11y pass: in control the picture takes the keys, so it is an application (a screen reader passes keys
    // through), described by the help that says how to leave it; watching, it is an image.
    const desktop = screen.getByRole("application", {
      name: "The agent's desktop, live, you have control",
    });
    expect(desktop).toHaveAccessibleDescription(
      "Keys go to the desktop while it has focus; press Escape twice to give the keyboard back to the page. You can paste in; nothing is copied out.",
    );
    const handBack = screen.getByRole("button", { name: "Hand back" });
    handBack.focus();
    fireEvent.click(handBack);
    act(() => socket().frame(control("none")));
    expect(clients[0]?.viewOnly).toBe(true);
    // WEB-U-270: Hand back is gone; Take over, back in its place, has the focus.
    expect(screen.getByRole("button", { name: "Take over" })).toHaveFocus();
    expect(screen.queryByRole("application")).toBeNull();
    expect(
      screen.getByRole("img", { name: "The agent's desktop, live" }),
    ).not.toHaveAttribute("aria-describedby");
    // Hand back resumes nothing (ADR-0039 §6): the next message does, and the status says so until a run starts.
    expect(screen.getByRole("status")).toHaveTextContent(
      "You handed back the desktop. Send a message to let the agent carry on.",
    );
    working(true);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Watching. The agent is working.",
    );
    working(false);
    expect(screen.getByRole("status")).toHaveTextContent(/^Watching\.$/);
  });

  it("says a sign-in may not last when control came before the run had stopped", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working />);
    act(() => socket().open());
    await connected();
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    // The gateway's 10 s wait for the stopped run ran out: control anyway, not ready.
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", ready: false }),
      ),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "You have control. The agent may still be stopping, so a sign-in now may not last: wait until this line changes.",
    );
    expect(screen.getByRole("button", { name: "Hand back" })).toBeVisible();
  });

  // The c16 UX review's second pass, #20: the gateway grants once, late, and re-sends the grant to that viewer when the
  // reset lands, as an on-time grant would have been: `you` with no `ready` (clean), or with `reset_ok: false`.
  it("says control is ready once the gateway re-sends the grant after the reset", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    await connected();
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    const late = JSON.stringify({
      type: "control",
      holder: "you",
      ready: false,
    });
    act(() => socket().frame(late));
    act(() => socket().frame(control("you")));
    expect(screen.getByRole("status")).toHaveTextContent(
      "You have control. When you're done, hand back, then send a message to let the agent carry on.",
    );
    expect(screen.queryByText(/may not last/)).toBeNull();
    // A reset that couldn't empty the browser: ready, with the warning about earlier sign-ins.
    act(() => socket().frame(late));
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
      ),
    );
    expect(screen.queryByText(/may not last/)).toBeNull();
    expect(
      screen.getByText(
        "Earlier sign-ins may still be in this browser. Clear them before you sign in.",
      ),
    ).toBeVisible();
  });

  it("warns that earlier sign-ins may remain when the run's reset couldn't empty the browser", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
      ),
    );
    expect(
      screen.getByText(
        "Earlier sign-ins may still be in this browser. Clear them before you sign in.",
      ),
    ).toHaveAttribute("data-tone", "danger");
    // Nothing runs during control, so ⋯ can clear them.
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for the screen" }),
    );
    expect(
      screen.getByRole("menuitem", { name: "Clear the browser's sign-ins" }),
    ).not.toHaveAttribute("aria-disabled", "true");
  });

  it("lifts the earlier-sign-ins warning once a clear in the same control works, and not for an earlier one", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    const at = (minute: number) => `2026-10-07T14:0${minute}:00Z`;
    const earlier = { kind: "screen.signed_out" as const, at: at(1) };
    const shown = render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        notices={[earlier]}
      />,
    );
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
      ),
    );
    const warning =
      "Earlier sign-ins may still be in this browser. Clear them before you sign in.";
    // A sign-out from before this control doesn't count.
    expect(screen.getByText(warning)).toBeVisible();
    // A clear that failed doesn't lift it.
    shown.rerender(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        notices={[
          earlier,
          { kind: "screen.sign_out_failed", at: at(3), reason: "not_cleared" },
        ]}
      />,
    );
    expect(screen.getByText(warning)).toBeVisible();
    // One that worked does: no new grant comes, the gateway clears its flag for the next Take over.
    shown.rerender(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        notices={[
          earlier,
          { kind: "screen.sign_out_failed", at: at(3), reason: "not_cleared" },
          { kind: "screen.signed_out", at: at(4) },
        ]}
      />,
    );
    expect(screen.queryByText(warning)).toBeNull();
  });

  it("gives up a Take over with no answer, hands back, and offers it again", async () => {
    vi.useFakeTimers();
    const { ScreenPanel } = await import("./ScreenPanel");
    const { TAKE_OVER_GIVE_UP_MS } = await import("./use-screen");
    render(<ScreenPanel conversationId="task-1" working />);
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    expect(socket().sent).toEqual([JSON.stringify({ type: "take_over" })]);
    // Past the gateway's 10 s bound plus a send, but not yet given up.
    act(() => {
      vi.advanceTimersByTime(TAKE_OVER_GIVE_UP_MS - 1);
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Stopping the agent before you take over…",
    );
    act(() => {
      vi.advanceTimersByTime(1);
    });
    // The claim was made at the press and holds the task's turns: the page releases it.
    expect(socket().sent).toEqual([
      JSON.stringify({ type: "take_over" }),
      JSON.stringify({ type: "hand_back" }),
    ]);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Take over didn't go through. Try again.",
    );
    // A grant already on its way, then the gateway's "none" after the hand-back: no control flickering on and off.
    act(() => socket().frame(control("you")));
    act(() => socket().frame(control("none")));
    expect(screen.queryByRole("button", { name: "Hand back" })).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Take over didn't go through. Try again.",
    );
    const again = screen.getByRole("button", { name: "Take over" });
    expect(again).not.toHaveAttribute("aria-busy", "true");
    fireEvent.click(again);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Stopping the agent before you take over…",
    );
  });

  it("says nothing of a hand-back when control was taken away, not handed back", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() => socket().frame(control("you")));
    // A restart ends the Take over (the transcript says so): the person didn't hand back.
    act(() => socket().frame(control("none")));
    expect(screen.getByRole("status")).toHaveTextContent(/^Watching\.$/);
  });

  it("expands the same live picture into a dialog and back, without a second connection", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    await vi.waitFor(() => expect(clients).toHaveLength(1));
    const picture = screen.getByRole("img", {
      name: "The agent's desktop, live",
    });
    const expand = screen.getByRole("button", { name: "Expand" });
    expand.focus();
    fireEvent.click(expand);
    const dialog = screen.getByRole("dialog", { name: "Screen" });
    expect(within(dialog).getByRole("img")).toBe(picture);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close dialog" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    // WEB-U-270: Expand stayed while the dialog was open, so the focus comes back to it.
    expect(expand).toHaveFocus();
    expect(screen.getByRole("img", { name: "The agent's desktop, live" })).toBe(
      picture,
    );
    expect(FakeSocket.opened).toHaveLength(1);
    expect(clients).toHaveLength(1);
  });

  // The c16 UX review, #18: another tab holds the screen; the line says what to do about it.
  it("says how to get the screen back from another tab", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    act(() => socket().frame(control("other")));
    expect(screen.getByRole("status")).toHaveTextContent(
      "Another tab of yours has control. Hand back there, or close that tab.",
    );
  });

  // The c16 UX review's second pass, #26: the warning names an action, so it offers it where it says so.
  it("offers Clear sign-ins in the warning about earlier sign-ins", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear sign-ins" }));
    expect(
      screen.getByRole("dialog", {
        name: "Clear the desktop browser's sign-ins?",
      }),
    ).toBeVisible();
  });

  // The c16 UX review's second pass, #27: after a reload or the panel reopened, the page still knows the desktop was
  // handed back with no run since (from the notices), so the status keeps its next step.
  it("says the desktop was handed back when the page says so, not only after a hand-back here", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} handedBack />);
    act(() => socket().open());
    expect(screen.getByRole("status")).toHaveTextContent(
      "You handed back the desktop. Send a message to let the agent carry on.",
    );
  });

  // The c16 UX review's second pass, #19: Expand is the size a person signs in at, so the status and the controls go
  // with the picture into the dialog; Expand itself stays in the panel, for the focus to come back to.
  it("takes over, says so and hands back inside the expanded screen", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    const dialog = screen.getByRole("dialog", { name: "Screen" });
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      /^Watching\.$/,
    );
    expect(screen.getAllByRole("status")).toHaveLength(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "Take over" }));
    act(() =>
      socket().frame(
        JSON.stringify({ type: "control", holder: "you", reset_ok: false }),
      ),
    );
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      /^You have control\./,
    );
    expect(
      within(dialog).getByText(
        "Earlier sign-ins may still be in this browser. Clear them before you sign in.",
      ),
    ).toBeVisible();
    expect(
      within(dialog).getByText(
        "A sign-in here lasts for the agent's next run only. Each run after that starts signed out.",
      ),
    ).toBeVisible();
    expect(
      within(dialog).getByRole("button", {
        name: "More actions for the screen",
      }),
    ).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Hand back" }));
    act(() => socket().frame(control("none")));
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      "You handed back the desktop. Send a message to let the agent carry on.",
    );
    expect(
      within(dialog).getByRole("button", { name: "Take over" }),
    ).toBeVisible();
  });

  // In the expanded screen, while this tab has control and the keyboard is in the screen, one Escape is the desktop's,
  // whatever the screen client does with it; once the keyboard is back on the page (Escape twice), Escape closes the
  // dialog. The close button always does.
  it("keeps the expanded screen open for an Escape that belongs to the desktop", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    const dialog = screen.getByRole("dialog", { name: "Screen" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Take over" }));
    act(() => socket().frame(control("you")));
    // The screen client's own focusable surface, inside the picture, holds the keyboard.
    const surface = document.createElement("canvas");
    surface.tabIndex = 0;
    within(dialog).getByRole("application").appendChild(surface);
    surface.focus();
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    expect(screen.getByRole("dialog", { name: "Screen" })).toBeVisible();
    // The keyboard back on the page: Escape closes it.
    within(dialog).getByRole("button", { name: "Hand back" }).focus();
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // The screen stops being live while expanded (the desktop stopped): the dialog closes, and the focus goes to the
  // panel, not to the page's body.
  it("closes the expanded screen when it stops being live, with the focus on the panel", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    expect(screen.getByRole("dialog", { name: "Screen" })).toBeVisible();
    act(() => socket().shut(4409, "sandbox_not_running"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("region", { name: "Screen" })).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent(
      "The desktop isn't running. It starts when you send a message.",
    );
  });

  it("says why it can't show the screen, with Try again where trying helps", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().shut(4409, "screen_viewers"));
    expect(screen.getByRole("status")).toHaveTextContent(
      "This desktop is open in four tabs. Close one to watch it here.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(FakeSocket.opened).toHaveLength(2);
    act(() => socket().shut(4409, "sandbox_not_running"));
    expect(screen.getByRole("status")).toHaveTextContent(
      "The desktop isn't running. It starts when you send a message.",
    );
    expect(screen.queryByRole("button", { name: "Take over" })).toBeNull();
    // Not running, the panel replays what the task kept (WEB-U-247); with nothing kept, it says so.
    expect(screen.getByText("No screenshots yet.")).toBeVisible();
  });

  it.each([
    ["no_desktop", "This task's environment has no desktop."],
    ["forbidden", "Only the task's owner can see its screen."],
    // A deleted task or a wrong link: nobody to ask (the review's P3-1).
    ["not_found", "This task's screen can't be opened."],
  ])(
    "says why it can't open the screen (%s), with no Try again that can't help",
    async (reason, words) => {
      const { ScreenPanel } = await import("./ScreenPanel");
      render(<ScreenPanel conversationId="task-1" working={false} />);
      act(() => socket().shut(4403, reason));
      expect(screen.getByRole("status")).toHaveTextContent(words);
      expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    },
  );

  it("says the agent's work isn't affected when the screen stays away", async () => {
    vi.useFakeTimers();
    const { ScreenPanel, LONG_AWAY_MS } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working />);
    act(() => socket().shut(4409, "screen_unavailable"));
    expect(screen.getByRole("status")).toHaveTextContent(
      "The screen isn't available. Trying again…",
    );
    // Its own retries (a socket that doesn't open yet) keep the time running.
    act(() => {
      vi.advanceTimersByTime(LONG_AWAY_MS);
    });
    expect(FakeSocket.opened.length).toBeGreaterThan(1);
    expect(screen.getByRole("status")).toHaveTextContent(
      "The screen isn't available. Still trying. The agent's work on the desktop isn't affected.",
    );
    // Back, then away again: the short words, until it has been away long again.
    act(() => socket().open());
    expect(screen.getByRole("status")).toHaveTextContent(
      "Watching. The agent is working.",
    );
    act(() => socket().shut(4409, "screen_unavailable"));
    expect(screen.getByRole("status")).toHaveTextContent(
      "The screen isn't available. Trying again…",
    );
  });
});

describe("WEB-U-247 Live and Replay", () => {
  const kept = [
    {
      id: "c-1",
      runId: "run-1",
      label: "Clicked “Save”",
      captures: [
        {
          phase: "after" as const,
          id: "a1",
          width: 1280,
          height: 800,
          marked: false,
        },
      ],
    },
  ];

  it("offers Replay beside Live while the desktop runs", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        steps={kept}
        runStarts={{ "run-1": "2026-10-05T09:00:00Z" }}
      />,
    );
    act(() => socket().open());
    const mode = screen.getByRole("group", { name: "Show" });
    expect(within(mode).getByRole("button", { name: "Live" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(within(mode).getByRole("button", { name: "Replay" }));
    expect(screen.getByText("Step 1 of 1 · Clicked “Save”")).toBeVisible();
    fireEvent.click(within(mode).getByRole("button", { name: "Live" }));
    expect(
      screen.getByRole("img", { name: "The agent's desktop, live" }),
    ).toBeVisible();
  });

  it("shows only the replay, with no switch, once the desktop isn't running", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(
      <ScreenPanel conversationId="task-1" working={false} steps={kept} />,
    );
    act(() => socket().shut(4409, "sandbox_not_running"));
    expect(screen.queryByRole("group", { name: "Show" })).toBeNull();
    expect(screen.getByText("Step 1 of 1 · Clicked “Save”")).toBeVisible();
  });
});

// TEST: WEB-U-329
describe("WEB-U-329 typing is offered only once the desktop takes it", () => {
  beforeEach(() => {
    clients.length = 0;
    FakeScreenClient.autoConnect = true;
  });

  it("gives input to a client that loaded after the grant", async () => {
    // A page that takes over at once: control arrives before the screen client has loaded (c16, 12 Oct 2026: the
    // picture stayed view-only under "You have control", and nothing typed reached the desktop).
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() => socket().frame(control("you")));
    await connected();
    expect(clients[0]?.viewOnly).toBe(false);
    expect(clients[0]?.focus).toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "You have control. When you're done, hand back, then send a message to let the agent carry on.",
    );
  });

  it("says it is connecting, not in control, until the client takes input", async () => {
    FakeScreenClient.autoConnect = false;
    const { ScreenPanel } = await import("./ScreenPanel");
    render(<ScreenPanel conversationId="task-1" working={false} />);
    act(() => socket().open());
    await vi.waitFor(() => expect(clients).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() => socket().frame(control("you")));
    // Granted, but the client hasn't connected: no keys, no typing offered; Hand back is still there.
    expect(screen.getByRole("status")).toHaveTextContent(
      "Connecting to the desktop…",
    );
    expect(clients[0]?.viewOnly).toBe(true);
    expect(clients[0]?.focus).not.toHaveBeenCalled();
    expect(screen.queryByRole("application")).toBeNull();
    expect(screen.getByRole("button", { name: "Hand back" })).toBeVisible();
    act(() => clients[0]?.emit("connect"));
    expect(clients[0]?.viewOnly).toBe(false);
    expect(
      screen.getByRole("application", {
        name: "The agent's desktop, live, you have control",
      }),
    ).toBeInTheDocument();
    // The client drops (a reconnect): typing is withdrawn until it connects again.
    act(() => clients[0]?.emit("disconnect"));
    expect(clients[0]?.viewOnly).toBe(true);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Connecting to the desktop…",
    );
  });
});

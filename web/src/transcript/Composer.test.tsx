import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { QueuedMessage } from "../protocol/generated/api";

import { Composer, type HeldMessages } from "./Composer";
import type { Transcript } from "./types";

const idle: Transcript["run"] = { id: null, status: "idle" };
const defaults = {
  run: idle,
  sandbox: "ready" as const,
  sending: false,
  cancelling: false,
  onSend: vi.fn(() => Promise.resolve(true)),
  onStop: vi.fn(() => Promise.resolve()),
};

describe("WEB-U-001 composer states", () => {
  it("sends Enter once, preserves Shift+Enter and composition, and clears on send", async () => {
    const onSend = vi.fn(() => Promise.resolve(true));
    render(<Composer {...defaults} onSend={onSend} />);
    const input = screen.getByRole("textbox", { name: "Message" });
    expect(input).toHaveAttribute("rows", "3");
    fireEvent.change(input, { target: { value: "Inspect the workspace" } });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(input).toHaveValue(""));
    expect(onSend).toHaveBeenCalledExactlyOnceWith("Inspect the workspace");
  });

  it("keeps the draft after a rejected action and rejects blank messages", async () => {
    const onSend = vi.fn(() => Promise.resolve(false));
    render(<Composer {...defaults} onSend={onSend} />);
    const input = screen.getByRole("textbox", { name: "Message" });
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    fireEvent.change(input, { target: { value: "Keep this draft" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledOnce());
    // The draft comes back once the refused send settles, after the call itself: wait for that outcome, not the call.
    expect(await screen.findByDisplayValue("Keep this draft")).toBe(input);
  });

  it.each([
    {
      sending: false,
      sandbox: "creating" as const,
      run: idle,
      reason: "Starting…",
    },
    {
      sending: false,
      sandbox: "ready" as const,
      run: { id: null, status: "queued" as const },
      reason: "You can reply once it starts.",
    },
    // A wait for a free desktop says when one frees, which the header's "Waiting…" leaves out (the 16d page pass, Z24).
    {
      sending: false,
      sandbox: "queued" as const,
      run: idle,
      queue: { ahead: 0, reason: "desktop_sessions" as const },
      reason:
        "Other tasks are using every desktop; one frees soon after another task finishes. You can reply once it starts.",
    },
    {
      sending: false,
      sandbox: "ready" as const,
      run: { id: "run-example", status: "running" as const },
      reason: "The agent is working. Stop it to send a new message.",
    },
  ])("explains why input is disabled: $reason", ({ reason, ...state }) => {
    render(<Composer {...defaults} {...state} />);
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    expect(screen.getByText(reason)).toBeVisible();
    const name =
      state.run.status === "running"
        ? "Stop"
        : state.sending
          ? "Sending"
          : "Send";
    expect(screen.getByRole("button", { name })).toHaveAttribute(
      "type",
      name === "Stop" ? "button" : "submit",
    );
  });

  it("clears immediately during delivery, restores on failure and uses only the button for Sending", async () => {
    let complete: ((success: boolean) => void) | undefined;
    const onSend = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          complete = resolve;
        }),
    );
    const { rerender } = render(<Composer {...defaults} onSend={onSend} />);
    const input = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(input, { target: { value: "One copy while sending" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(input).toHaveValue("");
    rerender(<Composer {...defaults} onSend={onSend} sending />);
    expect(input).toBeDisabled();
    // The draft is gone, so the button is disabled outright rather than busy.
    expect(screen.getByRole("button", { name: "Sending" })).toBeDisabled();
    expect(screen.queryByText("Sending your message…")).toBeNull();
    complete?.(false);
    await waitFor(() => expect(input).toHaveValue("One copy while sending"));
  });

  // The review of ea1e1a3b (SHOULD): Send and Stop are one button, so the second click of a double-click on Send must
  // not stop the run it just started. Only that second click is ignored (the browser counts it): a deliberate click on
  // Stop, however soon, stops (WEB-E2E-004 clicks Stop as soon as the run shows Working…).
  it("ignores the second click of a double-click on Send, never a deliberate Stop", () => {
    const onStop = vi.fn(() => Promise.resolve());
    const { rerender } = render(<Composer {...defaults} onStop={onStop} />);
    rerender(
      <Composer
        {...defaults}
        onStop={onStop}
        run={{ id: "run-example", status: "running" }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop" }), {
      detail: 2,
    });
    expect(onStop).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }), {
      detail: 1,
    });
    expect(onStop).toHaveBeenCalledOnce();
  });

  // Since c9 (WEB-U-001): Stop acts at once, as in the familiar chat apps; the work so far is kept, so there is nothing to confirm.
  it("offers Stop only for an active run, stops at once, and prevents repeated cancel while pending", () => {
    const onStop = vi.fn(() => Promise.resolve());
    const { rerender } = render(
      <Composer
        {...defaults}
        onStop={onStop}
        run={{ id: "run-example", status: "running" }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(onStop).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).toBeNull();
    rerender(
      <Composer
        {...defaults}
        cancelling
        run={{ id: "run-example", status: "running" }}
      />,
    );
    expect(screen.getByRole("button", { name: /Stop/ })).toBeDisabled();
    rerender(<Composer {...defaults} />);
    expect(
      screen.queryByRole("button", { name: "Stop" }),
    ).not.toBeInTheDocument();
  });
});

describe("WEB-U-017 composer card controls", () => {
  it("keeps a single named control inside the card through empty, typing, sending and running", () => {
    const { rerender } = render(<Composer {...defaults} />);
    const input = screen.getByRole("textbox", { name: "Message" });
    const card = input.closest(".composer-card");
    expect(card).not.toBeNull();
    expect(
      screen.getByRole("button", { name: "Send" }).closest(".composer-card"),
    ).toBe(card);
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(
      screen.getByText("Enter to send · Shift+Enter for a new line"),
    ).toBeVisible();
    fireEvent.change(input, { target: { value: "Please inspect this" } });
    expect(
      screen.queryByText("Enter to send · Shift+Enter for a new line"),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
    rerender(<Composer {...defaults} sending />);
    // With a draft, Sending is busy: it keeps its focus but refuses a second send (WEB-U-007).
    const sendingButton = screen.getByRole("button", { name: "Sending" });
    expect(sendingButton).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(sendingButton);
    expect(defaults.onSend).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    rerender(
      <Composer {...defaults} run={{ id: "run-example", status: "running" }} />,
    );
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
  });
});

// Finding T4 (GW-I-129): while the agent works, a message can be written and sent; the gateway holds it until the run
// ends, and the composer lists what waits, what was dropped, and who may cancel it.
describe("WEB-U-079 send after this", () => {
  const running: Transcript["run"] = { id: "run-1", status: "running" };
  const me = { id: "u-me", email: "me@example.invalid" };
  const other = { id: "u-other", email: "colleague@example.invalid" };
  const message = (
    id: string,
    text: string,
    state: QueuedMessage["state"],
    author = me,
    reason?: QueuedMessage["reason"],
  ): QueuedMessage => ({
    id,
    text,
    attachments: [],
    author,
    created_at: "2026-09-29T06:00:00Z",
    state,
    ...(reason ? { reason } : {}),
  });
  const held = (overrides: Partial<HeldMessages> = {}): HeldMessages => ({
    held: [],
    dropped: [],
    me: me.id,
    canCancel: (value) => value.author.id === me.id,
    notice: null,
    onCancel: vi.fn(),
    onSendAgain: vi.fn(),
    ...overrides,
  });

  it("keeps the box usable while the agent works and sends with Send after this or Enter", async () => {
    const onSend = vi.fn(() => Promise.resolve(true));
    render(
      <Composer {...defaults} run={running} onSend={onSend} held={held()} />,
    );
    const input = screen.getByRole("textbox", { name: "Message" });
    expect(input).toBeEnabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Send after this" }),
    ).toBeNull();
    fireEvent.change(input, { target: { value: "Also update the README." } });
    fireEvent.click(screen.getByRole("button", { name: "Send after this" }));
    await waitFor(() =>
      expect(onSend).toHaveBeenCalledExactlyOnceWith("Also update the README."),
    );
    expect(input).toHaveValue("");
    fireEvent.change(input, { target: { value: "And the changelog." } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() =>
      expect(onSend).toHaveBeenLastCalledWith("And the changelog."),
    );
  });

  it("lists held messages; only their author or the task's owner may cancel them", () => {
    const onCancel = vi.fn();
    render(
      <Composer
        {...defaults}
        run={running}
        held={held({
          held: [
            message("1", "Also update the README.", "held"),
            message("2", "Add the licence.", "held", other),
          ],
          onCancel,
        })}
      />,
    );
    const list = screen.getByRole("list", {
      name: "Messages waiting for the agent",
    });
    const [mine, theirs] = within(list).getAllByRole("listitem");
    expect(mine).toHaveTextContent(
      "Sends when the agent finishes: Also update the README.",
    );
    expect(theirs).toHaveTextContent(
      "From colleague@example.invalid, sends when the agent finishes: Add the licence.",
    );
    expect(within(theirs as HTMLElement).queryByRole("button")).toBeNull();
    fireEvent.click(
      within(mine as HTMLElement).getByRole("button", { name: "Cancel" }),
    );
    expect(onCancel).toHaveBeenCalledExactlyOnceWith("1");
  });

  // WEB-U-306 (ADR-0039 §6): while the desktop is taken over, the gateway holds every message until the hand-back, and no agent is
  // working, so the held line says when it really goes.
  it("says a held message sends at the hand-back while the desktop is taken over", () => {
    render(
      <Composer
        {...defaults}
        held={held({
          held: [
            message("1", "I've signed in; carry on.", "held"),
            message("2", "Add the licence.", "held", other),
          ],
          untilHandBack: true,
        })}
      />,
    );
    const [mine, theirs] = within(
      screen.getByRole("list", { name: "Messages waiting for the agent" }),
    ).getAllByRole("listitem");
    expect(mine).toHaveTextContent(
      "Sends when you hand back the desktop: I've signed in; carry on.",
    );
    expect(theirs).toHaveTextContent(
      "From colleague@example.invalid, sends when the desktop is handed back: Add the licence.",
    );
  });

  it("says why a dropped message wasn't sent, and offers Send again and Dismiss", () => {
    const onCancel = vi.fn();
    const onSendAgain = vi.fn();
    const stopped = message(
      "3",
      "Run it again.",
      "dropped",
      me,
      "run_cancelled",
    );
    render(
      <Composer
        {...defaults}
        held={held({
          dropped: [
            stopped,
            message("4", "Check the logs.", "dropped", other, "run_failed"),
          ],
          onCancel,
          onSendAgain,
        })}
      />,
    );
    const [first, second] = screen.getAllByRole("listitem");
    expect(first).toHaveTextContent(
      "Not sent, because the run was stopped: Run it again.",
    );
    expect(second).toHaveTextContent(
      "Not sent, because the run failed: Check the logs.",
    );
    fireEvent.click(
      within(first as HTMLElement).getByRole("button", { name: "Send again" }),
    );
    expect(onSendAgain).toHaveBeenCalledExactlyOnceWith(stopped);
    fireEvent.click(
      within(first as HTMLElement).getByRole("button", { name: "Dismiss" }),
    );
    expect(onCancel).toHaveBeenCalledExactlyOnceWith("3");
    // Someone else's dropped message can be sent again, but only they or the owner dismiss it.
    expect(
      within(second as HTMLElement).queryByRole("button", { name: "Dismiss" }),
    ).toBeNull();
  });

  it("says when a Cancel lost the race to the turn going out", () => {
    render(
      <Composer
        {...defaults}
        held={held({ notice: "It was already sent." })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "It was already sent.",
    );
  });
});

// The 13f screen review, #3: a phone was told "Enter to send · Shift+Enter for a new line", which a touch keyboard can't
// do. With a coarse pointer the hint isn't shown, and the field isn't described by it.
describe("WEB-U-154 no keyboard hint on a touch screen", () => {
  it("leaves the hint out where the pointer is a finger", () => {
    vi.stubGlobal(
      "matchMedia",
      (query: string) =>
        ({
          matches: query === "(pointer: coarse)",
          media: query,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
        }) as unknown as MediaQueryList,
    );
    try {
      render(<Composer {...defaults} />);
      expect(screen.queryByText(/Shift\+Enter/)).toBeNull();
      expect(
        screen.getByRole("textbox", { name: "Message" }),
      ).not.toHaveAttribute("aria-describedby");
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

// WEB-U-273 (the accessibility review of the core screens): Stop pressed with the keyboard became natively disabled
// while the stop was sent, then an empty Send, so the focus fell to the page. While the button has the focus it stays
// focusable and says it's unavailable; activating it then does nothing. Not focused, it is disabled as before.
describe("WEB-U-273 Send and Stop keep the keyboard's focus", () => {
  it("keeps the focus through the stop and the empty Send after it", () => {
    const onStop = vi.fn(() => Promise.resolve());
    const onSend = vi.fn();
    const running = { id: "run-example", status: "running" } as const;
    const { rerender } = render(
      <Composer {...defaults} onStop={onStop} onSend={onSend} run={running} />,
    );
    const button = screen.getByRole("button", { name: "Stop" });
    button.focus();
    fireEvent.click(button);
    expect(onStop).toHaveBeenCalledOnce();
    rerender(
      <Composer
        {...defaults}
        onStop={onStop}
        onSend={onSend}
        run={running}
        cancelling
      />,
    );
    expect(button).toHaveFocus();
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(onStop).toHaveBeenCalledOnce();
    rerender(<Composer {...defaults} onStop={onStop} onSend={onSend} />);
    const send = screen.getByRole("button", { name: "Send" });
    expect(send).toBe(button);
    expect(send).toHaveFocus();
    expect(send).toHaveAttribute("aria-disabled", "true");
    expect(send).not.toBeDisabled();
    fireEvent.click(send);
    expect(onSend).not.toHaveBeenCalled();
    // Once the focus moves on, an empty Send is disabled as before.
    fireEvent.blur(send);
    expect(send).toBeDisabled();
  });
});

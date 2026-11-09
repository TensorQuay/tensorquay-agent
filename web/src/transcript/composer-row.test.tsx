import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useAttachments } from "../files/use-attachments";
import type { QueuedMessage, Upload } from "../protocol/generated/api";
import { conversationId } from "../test/frames";

import { Composer, type HeldMessages } from "./Composer";
import type { Transcript } from "./types";

const idle: Transcript["run"] = { id: null, status: "idle" };
const running: Transcript["run"] = { id: "run-1", status: "running" };
const upload = (_id: string, file: File) =>
  Promise.resolve<Upload>({
    path: `uploads/${file.name}`,
    name: file.name,
    size: file.size,
  });
const me = { id: "u-me", email: "me@example.invalid" };
const waiting = (text: string): QueuedMessage => ({
  id: "m-1",
  text,
  attachments: [],
  author: me,
  created_at: "2026-11-09T06:00:00Z",
  state: "held",
});
const holding = (held: readonly QueuedMessage[]): HeldMessages => ({
  held,
  dropped: [],
  me: me.id,
  canCancel: () => true,
  notice: null,
  onCancel: vi.fn(),
  onSendAgain: vi.fn(),
});

function Harness({
  run = idle,
  held,
  initial = false,
  keyHint = false,
}: {
  run?: Transcript["run"];
  held?: HeldMessages;
  initial?: boolean;
  keyHint?: boolean;
}) {
  const attachments = useAttachments(conversationId, upload);
  return (
    <Composer
      run={run}
      sandbox="ready"
      sending={false}
      cancelling={false}
      onStop={() => Promise.resolve()}
      onSend={() => Promise.resolve(true)}
      attachments={attachments}
      {...(held ? { held } : {})}
      initial={initial}
      keyHint={keyHint}
    />
  );
}
const follows = (first: Element, second: Element) =>
  Boolean(
    first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING,
  );

// WEB-U-457 (UI-GUIDELINES §7 Composer, decided on 9 Nov 2026): the composer is one row, + at its start, the text, then
// what sends or stops at its end, in that order for the keyboard and a screen reader as on the screen. What is attached
// and what waits for the agent are chips above the text, only while there are any. The text starts at one line, and
// at two while New task's placeholder carries the keyboard hint, so the hint shows whole.
describe("WEB-U-457 the composer's one row", () => {
  it("puts +, the text and Send in one row, in that order", () => {
    render(<Harness />);
    const text = screen.getByRole("textbox", { name: "Message" });
    const add = screen.getByRole("button", { name: "Attach files" });
    const send = screen.getByRole("button", { name: "Send" });
    const row = text.closest(".composer-row");
    expect(row).not.toBeNull();
    expect(row).toContainElement(add);
    expect(row).toContainElement(send);
    expect(follows(add, text)).toBe(true);
    expect(follows(text, send)).toBe(true);
  });

  it("keeps Stop at the row's end while a run goes", () => {
    render(<Harness run={running} />);
    const text = screen.getByRole("textbox", { name: "Message" });
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(text.closest(".composer-row")).toContainElement(stop);
    expect(follows(text, stop)).toBe(true);
  });

  it("starts the text at one line, two while New task's placeholder carries the keyboard hint", () => {
    const { unmount } = render(<Harness />);
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveAttribute(
      "rows",
      "1",
    );
    unmount();
    render(<Harness initial keyHint />);
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveAttribute(
      "rows",
      "2",
    );
  });

  it("shows what waits for the agent as chips above the text, and no chip row while nothing does", () => {
    const { rerender } = render(<Harness run={running} held={holding([])} />);
    expect(screen.queryByRole("list")).toBeNull();
    rerender(
      <Harness
        run={running}
        held={holding([waiting("Also check the totals")])}
      />,
    );
    const chips = screen.getByRole("list", {
      name: "Messages waiting for the agent",
    });
    expect(chips).toHaveClass("composer-held");
    expect(
      follows(chips, screen.getByRole("textbox", { name: "Message" })),
    ).toBe(true);
  });
  // While a run goes, "Send after this" sits on its own line under the row, after Send, so the text keeps the row's
  // width on a phone (the review of 9 Nov 2026).
  it("puts Send after this under the row, after Send", () => {
    render(<Harness run={running} held={holding([])} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Also check the totals" },
    });
    const after = screen.getByRole("button", { name: "Send after this" });
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(after.closest(".composer-row")).toBeNull();
    expect(follows(stop, after)).toBe(true);
  });
});

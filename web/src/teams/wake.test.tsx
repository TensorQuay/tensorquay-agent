import { act, render, renderHook, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";
import { TranscriptView } from "../transcript/TranscriptView";
import { usePrompt } from "../transcript/use-prompt";

// The gateway's sentence for the model: never on the page.
const SENTENCE =
  "New messages from your lead since message 0. Read them with wait_for_lead; end your turn when you are waiting for more.";
const wake = (seq: number) =>
  frame(seq, {
    type: "message.user",
    text: SENTENCE,
    attachments: [],
    team_wake: { after: 0 },
  });
const mine = (seq: number, text: string) =>
  frame(seq, { type: "message.user", text, attachments: [] });

// WEB-U-237 (the 14b page pass, H7; protocol revision 13): the gateway's wake is its notice, "New team messages", in
// the reader's words; never the person's message, never the model's sentence with its tool names. Who wrote is in the
// team's log, so the notice names no one.
describe("WEB-U-237 the gateway's wake", () => {
  it("reads as the gateway's notice", () => {
    const state = [wake(1)].reduce(apply, emptyTranscript());
    expect(state.items[0]).toMatchObject({ kind: "user", wake: true });
    render(<TranscriptView transcript={state} />);
    expect(screen.getByText("New team messages")).toHaveClass(
      "transcript-notice",
    );
    expect(screen.queryByRole("article", { name: "Your message" })).toBeNull();
    expect(document.body).not.toHaveTextContent("wait_for_lead");
    expect(document.body).not.toHaveTextContent("since message");
  });

  it("leaves the person's own message as theirs, with no wake", () => {
    const state = [mine(1, "Check the totals again.")].reduce(
      apply,
      emptyTranscript(),
    );
    expect(state.items[0]).not.toHaveProperty("wake");
    render(<TranscriptView transcript={state} />);
    expect(
      screen.getByRole("article", { name: "Your message" }),
    ).toHaveTextContent("Check the totals again.");
    expect(screen.queryByText("New team messages")).toBeNull();
  });

  it("never takes a wake for the message the person is still sending", () => {
    let transcript = emptyTranscript();
    const { result, rerender } = renderHook(
      ({ state }) => usePrompt(state, { pending: false, error: null }, null),
      { initialProps: { state: transcript } },
    );
    act(() => {
      result.current.accepted(result.current.begin("Check the totals again."));
    });
    expect(result.current.visible?.text).toBe("Check the totals again.");
    // A wake arrives first: the person's message is still on its way.
    transcript = apply(transcript, wake(1));
    rerender({ state: transcript });
    expect(result.current.visible?.text).toBe("Check the totals again.");
    // Their own message arrives: it is confirmed.
    transcript = apply(transcript, mine(2, "Check the totals again."));
    rerender({ state: transcript });
    expect(result.current.visible).toBeNull();
  });
});

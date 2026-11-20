import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";
import { gatewayTurn } from "./use-held";

const CHANNEL = "0194f0a2-7c3e-7d1a-9b2c-3d4e5f607182";
// The gateway's sentence for the model: never on the page.
const SENTENCE = `New in channel ${CHANNEL} since message 3: a proposal for your vote (message 4). Read them with channel_read (channel ${CHANNEL}, since 3); end your turn when you are waiting for more.`;

// WEB-U-514 (ADR-0014's amendment of 20 Nov 2026, protocol revision 16): a channel's wake of its agent member is the
// gateway's notice, "New channel messages", never the person's message and never the model's sentence; waiting, it is
// the gateway's own turn, never offered to cancel.
describe("WEB-U-514 a channel's wake", () => {
  it("reads as the gateway's notice", () => {
    const state = [
      frame(1, {
        type: "message.user",
        text: SENTENCE,
        attachments: [],
        channel_wake: { channel: CHANNEL, after: 3 },
      }),
    ].reduce(apply, emptyTranscript());
    expect(state.items[0]).toMatchObject({ kind: "user", channelWake: true });
    render(<TranscriptView transcript={state} />);
    expect(screen.getByText("New channel messages")).toHaveClass(
      "transcript-notice",
    );
    expect(screen.queryByRole("article", { name: "Your message" })).toBeNull();
    expect(document.body).not.toHaveTextContent("channel_read");
    expect(document.body).not.toHaveTextContent("since message");
  });

  it("is the gateway's turn while it waits, never offered to cancel", () => {
    const plain = {
      id: "1",
      text: SENTENCE,
      attachments: [],
      author: { id: "u-1", email: "owner@example.invalid" },
      created_at: "2026-11-20T10:00:00Z",
      state: "waiting_for_slot" as const,
    };
    expect(
      gatewayTurn({ ...plain, channel_wake: { channel: CHANNEL, after: 3 } }),
    ).toBe(true);
    expect(gatewayTurn(plain)).toBe(false);
  });
});

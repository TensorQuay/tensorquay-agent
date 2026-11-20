import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { channel } from "../test/channels";

import { ChannelBudget } from "./ChannelBudget";
import { ChannelTimeline } from "./ChannelTimeline";
import type { ChannelMessage } from "./contract";

const RESETS = "2026-11-22T03:00:00Z";
/** The reset as the viewer reads it: their own weekday and time. */
const local = new Intl.DateTimeFormat("en", {
  weekday: "short",
  hour: "numeric",
  minute: "2-digit",
}).format(new Date(RESETS));

// WEB-U-533 (decided on 21 Nov 2026, C-3 and C-14): the budget is per day. The dialog says what is used today and, in
// neutral words, when it starts again at 0 in the viewer's own time; the used-up notice says when agents are woken
// again and who can raise it sooner; a notice from before (no time) still says who can.
describe("WEB-U-533 a channel's daily budget on the page", () => {
  it("says in the dialog what is used today and when it starts again at 0", () => {
    render(
      <ChannelBudget
        channel={channel("pricing", {
          wake_budget: 2_000_000,
          spent_tokens: 2_000_000,
          budget_resets_at: RESETS,
        })}
        may={false}
        onSave={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Wake budget" });
    expect(dialog).toHaveTextContent(
      "2,000,000 of 2,000,000 tokens used today (100 %)",
    );
    expect(dialog).toHaveTextContent(`It starts again at 0 on ${local}.`);
  });

  it("says nothing of a renewal while no window is open", () => {
    render(
      <ChannelBudget
        channel={channel("pricing", {
          wake_budget: 2_000_000,
          spent_tokens: 0,
        })}
        may
        onSave={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Wake budget" });
    expect(dialog).toHaveTextContent("0 of 2,000,000 tokens used today (0 %)");
    expect(dialog).not.toHaveTextContent("starts again");
    expect(
      screen.getByRole("spinbutton", {
        name: "Tokens woken agents may use here each day",
      }),
    ).toBeVisible();
  });

  it("says in the used-up notice when agents are woken again, and who can raise it", () => {
    const notice = (seq: number, resets_at?: string): ChannelMessage => ({
      seq,
      ts: "2026-11-21T03:00:00Z",
      author: { kind: "gateway", id: "gateway", name: "gateway" },
      body: {
        type: "notice",
        kind: "wake_budget_spent",
        text: "Woken agents have used this channel's budget for today.",
        ...(resets_at ? { resets_at } : {}),
      },
    });
    render(
      <ChannelTimeline
        messages={[notice(1, RESETS), notice(2)]}
        members={[]}
      />,
    );
    const log = screen.getByRole("list", { name: "Channel log" });
    expect(log).toHaveTextContent(
      `Woken agents have used this channel's budget for today: agents are woken here again on ${local}, or sooner if the channel's owner, a lead or an administrator raises it.`,
    );
    expect(log).toHaveTextContent(
      "Woken agents have used this channel's budget for today. The channel's owner, a lead or an administrator can raise it.",
    );
    expect(log).not.toHaveTextContent("until it is raised");
  });
});

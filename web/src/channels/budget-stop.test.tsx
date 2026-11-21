import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { channel } from "../test/channels";

import { ChannelHeader } from "./ChannelHeader";
import { ChannelTimeline } from "./ChannelTimeline";
import type { ChannelMessage } from "./contract";

vi.mock("../shell/NavigationButton", () => ({ NavigationButton: () => null }));

const failed = (seq: number, refusal?: string): ChannelMessage => ({
  seq,
  ts: "2026-11-21T03:00:00Z",
  author: { kind: "gateway", id: "gateway", name: "gateway" },
  body: {
    type: "notice",
    kind: "woken_run_failed",
    member: "Helper",
    code: "budget",
    ...(refusal ? { refusal } : {}),
    text: "Helper's run failed (budget).",
  },
});

const header = (spent: number) =>
  render(
    <ChannelHeader
      channel={channel("pricing", {
        wake_budget: 100_000,
        spent_tokens: spent,
      })}
      members={[]}
      openTasks={0}
      panel={null}
      onPanel={vi.fn()}
      actions={null}
      purpose={null}
    />,
  );

// WEB-U-534 (the live check of a used-up channel, 21 Nov 2026): a woken run the channel's budget stopped is said in the
// channel in the task page's own words, from the gateway's refusal the notice carries; one without it keeps the general
// words. The header reads "Budget used up today" from 100 % (never "137 %"); below, its percentage.
describe("WEB-U-534 a budget-stopped run and a used-up header", () => {
  it("names the channel's budget as the reason the run stopped", () => {
    render(
      <ChannelTimeline
        messages={[failed(1, "gateway.channel_budget 409"), failed(2)]}
        members={[]}
      />,
    );
    const log = screen.getByRole("list", { name: "Channel log" });
    expect(log).toHaveTextContent(
      "Helper's run stopped: The channel's budget for waking agents is used up",
    );
    expect(log).toHaveTextContent(
      "Helper's run stopped: The task reached one of its limits",
    );
    expect(log).not.toHaveTextContent("409");
  });

  it.each([
    [100_000, "Budget used up today"],
    [137_000, "Budget used up today"],
    [99_000, "Budget 99 % used today"],
  ])("spent %i of 100,000 reads “%s”", (spent, words) => {
    header(spent);
    expect(
      screen.getByRole("region", { name: "Channel header" }),
    ).toHaveTextContent(words);
  });
});

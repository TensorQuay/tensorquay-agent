import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { channel, fakeChannels, member } from "../test/channels";

import { ChannelPage } from "./ChannelPage";
import { ChannelTimeline, liveBudgetNotice } from "./ChannelTimeline";
import type { Channel, ChannelMessage } from "./contract";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/channels/x",
}));

const RESETS = "2026-11-22T03:00:00Z";
const spent = (seq: number): ChannelMessage => ({
  seq,
  ts: "2026-11-21T03:00:00Z",
  author: { kind: "gateway", id: "gateway", name: "gateway" },
  body: {
    type: "notice",
    kind: "wake_budget_spent",
    text: "Woken agents have used this channel's budget for today.",
    resets_at: RESETS,
  },
});
const LOG = [spent(1), spent(2)];
const PAST = "Woken agents used up this channel's budget for that day.";
const UNTIL = /agents are woken here again on/;

function page(extra: Partial<Channel>) {
  const pricing = channel("pricing", { role: "lead", ...extra });
  const fake = fakeChannels({
    channels: [pricing],
    members: { [pricing.id]: [member("user", "Ana", { role: "lead" })] },
    messages: { [pricing.id]: LOG },
  });
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ChannelPage id={pricing.id} api={fake.api} />
    </QueryClientProvider>,
  );
}

const article = (seq: number) =>
  screen.getByRole("article", { name: new RegExp(`^#${seq} `) });

// WEB-U-536 (decided on 21 Nov 2026): a used-up notice says when agents wake again, and offers Raise the
// budget, only while it is true: the latest one, while the channel is used up today. Raised, set to 0 or renewed, every
// one reads as a past line, with no time and no button.
describe("WEB-U-536 only the used-up notice still true says when and offers Raise", () => {
  it("picks the latest notice while used up, and none otherwise", () => {
    expect(liveBudgetNotice(LOG, { wake_budget: 100, spent_tokens: 100 })).toBe(
      2,
    );
    expect(liveBudgetNotice(LOG, { wake_budget: 100, spent_tokens: 140 })).toBe(
      2,
    );
    // Raised above the spend, set to 0, or a new day (the spend reads 0).
    expect(
      liveBudgetNotice(LOG, { wake_budget: 200, spent_tokens: 100 }),
    ).toBeNull();
    expect(
      liveBudgetNotice(LOG, { wake_budget: 0, spent_tokens: 100 }),
    ).toBeNull();
    expect(
      liveBudgetNotice(LOG, { wake_budget: 100, spent_tokens: 0 }),
    ).toBeNull();
    expect(
      liveBudgetNotice([], { wake_budget: 100, spent_tokens: 100 }),
    ).toBeNull();
  });

  it("reads an older notice as past and the live one with its time", () => {
    render(<ChannelTimeline messages={LOG} members={[]} liveSpent={2} />);
    const log = screen.getByRole("list", { name: "Channel log" });
    const [older, latest] = within(log).getAllByRole("article");
    expect(older).toHaveTextContent(PAST);
    expect(older).not.toHaveTextContent(UNTIL);
    expect(latest).toHaveTextContent(UNTIL);
  });

  it("offers Raise the budget only under the live notice", async () => {
    page({ wake_budget: 100, spent_tokens: 100 });
    await vi.waitFor(() => expect(article(2)).toHaveTextContent(UNTIL));
    expect(
      screen.getAllByRole("button", { name: "Raise the budget" }),
    ).toHaveLength(1);
    expect(article(1)).toHaveTextContent(PAST);
  });

  it.each([
    ["raised", { wake_budget: 200, spent_tokens: 100 }],
    ["set to 0", { wake_budget: 0, spent_tokens: 100 }],
    ["renewed", { wake_budget: 100, spent_tokens: 0 }],
  ])(
    "once the budget is %s, every notice is past, with no button",
    async (_, extra) => {
      page(extra);
      await vi.waitFor(() => expect(article(2)).toHaveTextContent(PAST));
      expect(article(1)).toHaveTextContent(PAST);
      expect(
        screen.queryByRole("button", { name: "Raise the budget" }),
      ).toBeNull();
    },
  );
});

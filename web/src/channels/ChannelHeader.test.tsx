import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { channel, fakeChannels, member, message } from "../test/channels";

import { ChannelPage } from "./ChannelPage";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/channels/x",
}));

const PRICING = channel("Q4 pricing review", {
  rule: "majority",
  role: "member",
  member_count: 4,
  wake_budget: 300_000,
  spent_tokens: 114_000,
});
const ANALYST = member("conversation", "Agent", {
  title: "Pricing analyst",
  owner_name: "mei",
  state: "working",
});
const CHECKER = member("conversation", "Agent", {
  title: "Data checker",
  owner_name: "mei",
  state: "waiting",
});
const FORECAST = member("conversation", "Agent", {
  title: "Forecast",
  owner_name: "mei",
  state: "idle",
});
const MEI = member("user", "Mei Lin", { role: "lead" });

function show() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ChannelPage
        id={PRICING.id}
        api={
          fakeChannels({
            channels: [PRICING],
            members: { [PRICING.id]: [MEI, ANALYST, CHECKER, FORECAST] },
            messages: {
              [PRICING.id]: [
                message(1, MEI, {
                  type: "task",
                  title: "Draft the customer email",
                  spec: "For the new price.",
                }),
                message(2, MEI, {
                  type: "task",
                  title: "Recheck the VAT rows",
                  spec: "Pages 11 and 12.",
                }),
              ],
            },
          }).api
        }
      />
    </QueryClientProvider>,
  );
}

// WEB-U-519 (18.2 slice 3, the mock of 17 Nov 2026 and its defaults decided on 20 Nov 2026): the channel's header stays
// on top: its name, who is working (an agent in a run or waiting on the channel), every member's face with a dot on a
// working agent, the budget as one muted "Budget N % used today", and To-dos and Members with their counts, each opening its
// panel and closing it again.
describe("WEB-U-519 the channel's header", () => {
  it("says who is working, the budget, and opens To-dos and Members", async () => {
    show();
    const header = await screen.findByRole("region", {
      name: "Channel header",
    });
    expect(
      within(header).getByRole("heading", {
        level: 1,
        name: "Q4 pricing review",
      }),
    ).toBeVisible();
    expect(within(header).getByText("2 working")).toBeVisible();
    expect(within(header).getByText("Budget 38 % used today")).toHaveClass("muted");
    const faces = within(header).getByRole("list", { name: "Who's here" });
    expect(within(faces).getAllByRole("listitem")).toHaveLength(4);
    expect(within(faces).getByTitle("Pricing analyst, working")).toBeVisible();
    expect(within(faces).getByTitle("Data checker, working")).toBeVisible();
    expect(within(faces).getByTitle("Forecast")).toBeVisible();
    expect(within(faces).getByTitle("Mei Lin")).toHaveTextContent("ML");
    const todos = await within(header).findByRole("button", {
      name: "To-dos 2",
    });
    expect(todos).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(todos);
    expect(todos).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("complementary", { name: "To-dos" })).toBeVisible();
    fireEvent.click(within(header).getByRole("button", { name: "Members 4" }));
    expect(screen.queryByRole("complementary", { name: "To-dos" })).toBeNull();
    expect(
      screen.getByRole("complementary", { name: "Members" }),
    ).toBeVisible();
    fireEvent.click(within(header).getByRole("button", { name: "Members 4" }));
    expect(screen.queryByRole("complementary", { name: "Members" })).toBeNull();
  });
});

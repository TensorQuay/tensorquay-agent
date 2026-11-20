import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { channel, fakeChannels, member, message } from "../test/channels";
import { settleLazy } from "../test/lazy";

import { ChannelPage } from "./ChannelPage";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/channels/x",
}));

const PRICING = channel("pricing", {
  rule: "majority",
  role: "member",
  member_count: 2,
});
const MEI = member("user", "Mei Lin", { role: "member" });
const ANALYST = member("conversation", "Agent", { title: "Pricing analyst" });

// WEB-U-523 (18.2 slice 3, the mock of 17 Nov 2026): no nesting in the log. A post with replies shows "N replies"; the
// replies open in a thread panel beside the log (a sheet on a phone) with the post, its replies and a reply box, and a
// reply posted there joins the thread, not the log.
describe("WEB-U-523 replies open in a thread", () => {
  it("shows the count in the log and the thread in its panel", async () => {
    const fake = fakeChannels({
      channels: [PRICING],
      members: { [PRICING.id]: [MEI, ANALYST] },
      messages: {
        [PRICING.id]: [
          message(1, MEI, { type: "text", text: "Which tier first?" }),
          {
            ...message(2, ANALYST, { type: "text", text: "Pro, EU." }),
            parent: 1,
          },
          { ...message(3, MEI, { type: "text", text: "Go on." }), parent: 1 },
          message(4, MEI, { type: "text", text: "Separately: lunch?" }),
        ],
      },
      me: MEI,
    });
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={cache}>
        <ChannelPage id={PRICING.id} api={fake.api} />
      </QueryClientProvider>,
    );
    const log = await screen.findByRole("list", { name: "Channel log" });
    await settleLazy();
    expect(
      within(log)
        .getAllByRole("article")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual(["#1 Mei Lin", "#4 Mei Lin"]);
    fireEvent.click(within(log).getByRole("button", { name: "2 replies" }));
    const thread = screen.getByRole("complementary", { name: "Thread" });
    await settleLazy();
    expect(within(thread).getByText("Which tier first?")).toBeVisible();
    expect(within(thread).getByText("Pro, EU.")).toBeVisible();
    expect(within(thread).getByText("Go on.")).toBeVisible();
    const box = within(thread).getByRole("textbox", {
      name: "Reply in the thread",
    });
    fireEvent.change(box, { target: { value: "Agreed." } });
    fireEvent.click(within(thread).getByRole("button", { name: "Send" }));
    expect(await within(thread).findByText("Agreed.")).toBeVisible();
    expect(
      within(log).getByRole("button", { name: "3 replies" }),
    ).toBeVisible();
    expect(within(log).queryByText("Agreed.")).toBeNull();
    fireEvent.click(
      within(thread).getByRole("button", { name: "Close Thread" }),
    );
    expect(screen.queryByRole("complementary", { name: "Thread" })).toBeNull();
  });
});

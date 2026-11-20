import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import {
  channel,
  fakeChannels,
  FakeStream,
  member,
  message,
} from "../test/channels";

import { ChannelBudget } from "./ChannelBudget";
import { ChannelComposer } from "./ChannelComposer";
import { ChannelHeader } from "./ChannelHeader";
import { ChannelPage } from "./ChannelPage";
import type { Channel, ChannelsApi } from "./contract";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/channels/x",
}));

const ME = member("user", "Ana", { role: "member" });
const CODER = member("conversation", "Coder", { owner_name: "alice" });

function show(api: ChannelsApi, id: string) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], {
    id: ME.id,
    role: "member",
    email: "viewer@example.invalid",
  });
  render(
    <QueryClientProvider client={cache}>
      <ChannelPage id={id} api={api} />
    </QueryClientProvider>,
  );
}

const bar = () => screen.getByRole("region", { name: "Channel header" });

const header = (extra: Partial<Channel>) =>
  render(
    <ChannelHeader
      channel={channel("pricing", extra)}
      members={[]}
      openTasks={0}
      panel={null}
      onPanel={vi.fn()}
      actions={null}
      purpose={null}
    />,
  );

// WEB-U-535 (the web review of 21 Nov 2026): the header never disagrees with the log. A budget notice, or a role
// change Members shows, reads the channel again; the share is floored, so "used up" only from 100 %; a budget of 0
// reads as waking off, not as a used-up day; a post refused for a field says it is too long, and the files' words
// only when the files were refused.
describe("WEB-U-535 the channel's budget stays current, and refusals say why", () => {
  it("reads the channel again when a used-up notice arrives", async () => {
    const pricing = channel("pricing", {
      role: "member",
      wake_budget: 100_000,
      spent_tokens: 38_000,
    });
    const fake = fakeChannels({
      channels: [pricing],
      members: { [pricing.id]: [CODER, ME] },
      messages: {
        [pricing.id]: [message(1, CODER, { type: "text", text: "Working." })],
      },
      me: ME,
    });
    show(fake.api, pricing.id);
    await vi.waitFor(() =>
      expect(bar()).toHaveTextContent("Budget 38 % used today"),
    );
    await vi.waitFor(() => expect(fake.streams).toHaveLength(1));
    fake.api.channel = () =>
      Promise.resolve({ ...pricing, spent_tokens: 100_000 });
    act(() =>
      (fake.streams[0] as FakeStream).send({
        type: "channel.message",
        seq: 2,
        ts: "2026-11-21T03:00:00Z",
        author: { kind: "gateway", id: "gateway", name: "gateway" },
        body: {
          type: "notice",
          kind: "wake_budget_spent",
          text: "Woken agents have used this channel's budget for today.",
        },
      }),
    );
    await vi.waitFor(() =>
      expect(bar()).toHaveTextContent("Budget used up today"),
    );
  });

  it("reads the channel again when Members shows the viewer's new role", async () => {
    const pricing = channel("pricing", {
      role: "member",
      wake_budget: 100_000,
      spent_tokens: 0,
    });
    const fake = fakeChannels({
      channels: [pricing],
      members: { [pricing.id]: [CODER, { ...ME, role: "lead" }] },
      me: ME,
    });
    // The first read still says member; Members already lists the viewer as lead, so the page reads it again.
    let reads = 0;
    fake.api.channel = () => {
      reads += 1;
      return Promise.resolve(
        reads === 1 ? pricing : { ...pricing, role: "lead" as const },
      );
    };
    show(fake.api, pricing.id);
    await vi.waitFor(() => expect(reads).toBeGreaterThanOrEqual(2));
  });

  it.each([
    [99_800, "Budget 99 % used today"],
    [100_000, "Budget used up today"],
  ])("floors the share: %i of 100,000 reads “%s”", (spent, words) => {
    header({ wake_budget: 100_000, spent_tokens: spent });
    expect(bar()).toHaveTextContent(words);
  });

  it("says waking is off for a budget of 0, in the header and the dialog", () => {
    header({ wake_budget: 0, spent_tokens: 0 });
    expect(bar()).toHaveTextContent("Waking agents is off");
    expect(bar()).not.toHaveTextContent("used up");
    render(
      <ChannelBudget
        channel={channel("pricing", { wake_budget: 0, spent_tokens: 0 })}
        may={false}
        onSave={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Wake budget" });
    expect(dialog).toHaveTextContent(
      "Agents aren't woken here: the budget is 0.",
    );
    expect(dialog).not.toHaveTextContent("0 of 0");
  });

  it.each([
    ["body", "This post is too long: shorten it and send it again."],
    ["files", "A file couldn't go with this post: attach it again."],
  ])("a post refused naming %s says so", async (field, words) => {
    const refusal = new ApiError(400, "invalid_request", "x", undefined, [
      { field, reason: "message_invalid" },
    ]);
    render(
      <ChannelComposer
        memberRole="member"
        state="open"
        replyTo={null}
        onCancelReply={vi.fn()}
        onPost={vi.fn(() => Promise.reject(refusal))}
      />,
    );
    const form = screen.getByRole("form", { name: "Post to the channel" });
    fireEvent.change(within(form).getByLabelText("Your message"), {
      target: { value: "x".repeat(20_000) },
    });
    fireEvent.click(within(form).getByRole("button", { name: "Send" }));
    expect(await within(form).findByText(words)).toBeVisible();
  });
});

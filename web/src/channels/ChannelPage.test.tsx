import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import {
  channel,
  fakeChannels,
  FakeStream,
  member,
  message,
} from "../test/channels";
import { settleLazy } from "../test/lazy";

import { ChannelPage } from "./ChannelPage";
import type { ChannelsApi, Member } from "./contract";

const router = vi.hoisted(() => ({ push: vi.fn() }));
vi.mock("next/navigation", () => ({
  useRouter: () => router,
  usePathname: () => "/channels/x",
}));

const RELEASE = channel("release", {
  purpose: "Plan the 1.0 release",
  rule: "majority",
  role: "member",
  member_count: 2,
});
const CODER = member("conversation", "Coder", {
  owner_name: "alice",
  role: "lead",
});
const ME = member("user", "Ana", { role: "member" });

function show(
  api: ChannelsApi,
  id = RELEASE.id,
  viewer?: { id: string; role: "admin" | "member" },
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  if (viewer)
    cache.setQueryData(["session"], {
      ...viewer,
      email: "viewer@example.invalid",
    });
  render(
    <QueryClientProvider client={cache}>
      <ChannelPage id={id} api={api} />
    </QueryClientProvider>,
  );
}

// ADR-0014: one channel's page: its bar, its log in order and the composer; posting adds to the log (WEB-U-063).
describe("WEB-U-063 the channel page", () => {
  it("shows the channel, its members' posts in order, and the composer", async () => {
    show(
      fakeChannels({
        channels: [RELEASE],
        members: { [RELEASE.id]: [CODER, ME] },
        messages: {
          [RELEASE.id]: [
            message(1, CODER, { type: "text", text: "Starting." }),
            message(2, ME, { type: "text", text: "Thanks." }),
          ],
        },
      }).api,
    );
    expect(
      await screen.findByRole("heading", { level: 1, name: "release" }),
    ).toBeVisible();
    const bar = screen.getByRole("region", { name: "Channel header" });
    for (const text of [
      "Plan the 1.0 release",
      "Open",
      "Decided by majority",
      "2 members",
    ])
      expect(within(bar).getByText(text)).toBeVisible();
    await settleLazy();
    const log = screen.getByRole("list", { name: "Channel log" });
    expect(
      within(log)
        .getAllByRole("article")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual(["#1 Coder · alice", "#2 Ana"]);
    expect(
      screen.getByRole("form", { name: "Post to the channel" }),
    ).toBeVisible();
  });

  // The title is usually taken from the purpose, so the header shows the purpose only when it says more.
  it("shows the purpose under the title only when it says more than the title", async () => {
    const same = channel("Get the release out", {
      purpose: "Get the release out.",
      role: "member",
    });
    const fake = fakeChannels({
      channels: [same],
      members: { [same.id]: [ME] },
      me: ME,
    });
    show(fake.api, same.id);
    const bar = await screen.findByRole("region", { name: "Channel header" });
    expect(
      within(bar).getByRole("heading", {
        level: 1,
        name: "Get the release out",
      }),
    ).toBeVisible();
    expect(within(bar).queryByText("Get the release out.")).toBeNull();
  });

  // WEB-U-250 (the 14b page pass, C2): a title cut from the purpose isn't followed by the purpose again.
  it("carries on from a title cut from the purpose, with the whole purpose in its tooltip", async () => {
    const purpose =
      "Plan the spring newsletter: agree the topics and who drafts each part.";
    const cut = channel(
      "Plan the spring newsletter: agree the topics and who drafts…",
      { purpose, role: "member" },
    );
    const fake = fakeChannels({
      channels: [cut],
      members: { [cut.id]: [ME] },
      me: ME,
    });
    show(fake.api, cut.id);
    const bar = await screen.findByRole("region", { name: "Channel header" });
    expect(within(bar).getByText("…each part.")).toHaveAttribute(
      "title",
      purpose,
    );
    expect(within(bar).queryByText(purpose)).toBeNull();
  });

  // WEB-U-063: a new channel's empty log gives a real first step, not a blank page.
  it("gives a new channel a first step: a suggested first message, with the message box focused", async () => {
    const fresh = channel("launch", {
      purpose: "Get the 1.0 release out by Friday.",
      role: "lead",
    });
    const fake = fakeChannels({
      channels: [fresh],
      members: { [fresh.id]: [ME] },
      me: ME,
    });
    show(fake.api, fresh.id);
    await settleLazy();
    const first = await screen.findByRole("region", {
      name: "Tell your agents what you need",
    });
    const form = screen.getByRole("form", { name: "Post to the channel" });
    const box = within(form).getByLabelText("Your message");
    expect(box).toHaveFocus();
    expect(first).toHaveTextContent("Get the 1.0 release out by Friday");
    // The purpose's own full stop isn't doubled by the sentence around it.
    expect(first.textContent).not.toMatch(/\.\./);
    fireEvent.click(
      within(first).getByRole("button", { name: "Use this message" }),
    );
    // The box starts again from the suggestion, so the form is looked up afresh.
    const again = screen.getByRole("form", { name: "Post to the channel" });
    const box2 = within(again).getByLabelText("Your message");
    expect((box2 as HTMLTextAreaElement).value).toContain(
      "Get the 1.0 release out by Friday",
    );
    expect(box2).toHaveFocus();
    fireEvent.click(within(again).getByRole("button", { name: "Post" }));
    expect(await screen.findByRole("article", { name: /^#1 / })).toBeVisible();
    expect(
      screen.queryByRole("region", { name: "Tell your agents what you need" }),
    ).toBeNull();
  });

  it("gives no first step to a channel with posts, or to someone who can't post", async () => {
    const quiet = channel("quiet", { role: "observer" });
    const fake = fakeChannels({
      channels: [RELEASE, quiet],
      members: { [RELEASE.id]: [CODER, ME], [quiet.id]: [ME] },
      messages: {
        [RELEASE.id]: [message(1, CODER, { type: "text", text: "Starting." })],
      },
      me: ME,
    });
    show(fake.api);
    await settleLazy();
    await screen.findByRole("article", { name: /^#1 / });
    expect(
      screen.queryByRole("region", { name: "Tell your agents what you need" }),
    ).toBeNull();
    cleanup();
    show(fake.api, quiet.id);
    await settleLazy();
    await screen.findByText("You can only read this channel, not post.");
    expect(
      screen.queryByRole("region", { name: "Tell your agents what you need" }),
    ).toBeNull();
  });

  it("posts, replies, and shows the new post in the log", async () => {
    const fake = fakeChannels({
      channels: [RELEASE],
      members: { [RELEASE.id]: [CODER, ME] },
      messages: {
        [RELEASE.id]: [message(1, CODER, { type: "text", text: "Starting." })],
      },
      me: ME,
    });
    show(fake.api);
    await settleLazy();
    fireEvent.click(await screen.findByRole("button", { name: "Reply to #1" }));
    const form = screen.getByRole("form", { name: "Post to the channel" });
    expect(within(form).getByText("Replying to #1")).toBeVisible();
    fireEvent.change(within(form).getByLabelText("Your message"), {
      target: { value: "On it." },
    });
    fireEvent.click(within(form).getByRole("button", { name: "Post" }));
    expect(
      await screen.findByRole("article", { name: "#2 Ana" }),
    ).toBeVisible();
    expect(fake.posts).toEqual([
      {
        id: RELEASE.id,
        write: { parent: 1, body: { type: "text", text: "On it." } },
        key: expect.any(String) as string,
      },
    ]);
    expect(
      within(screen.getByRole("list", { name: "Replies to #1" })).getByRole(
        "article",
        { name: "#2 Ana" },
      ),
    ).toBeVisible();
    expect(within(form).queryByText("Replying to #1")).toBeNull();
  });

  it("says so for a channel that doesn't exist or can't be seen", async () => {
    show(fakeChannels().api, "0195d8c0-0000-7000-8000-00000000dead");
    expect(
      await screen.findByText(
        "This channel doesn't exist, or you can't see it.",
      ),
    ).toBeVisible();
  });

  it("shows a proposal's tally under it, and refreshes it when a vote reaches the log (WEB-U-064)", async () => {
    const fake = fakeChannels({
      channels: [RELEASE],
      members: { [RELEASE.id]: [CODER, ME] },
      messages: {
        [RELEASE.id]: [
          message(1, CODER, {
            type: "proposal",
            title: "Ship on Friday",
            text: "All green.",
          }),
        ],
      },
      me: ME,
    });
    show(fake.api);
    const tally = await screen.findByRole("group", { name: "Votes on #1" });
    expect(tally).toHaveTextContent("Accept 0 · Reject 0 · Not voted 2");
    fireEvent.click(within(tally).getByRole("button", { name: "Accept #1" }));
    await vi.waitFor(() =>
      expect(tally).toHaveTextContent("Accept 1 · Reject 0 · Not voted 1"),
    );
    expect(
      await screen.findByRole("article", { name: "#2 Ana" }),
    ).toHaveTextContent("voted Accept on Ship on Friday");
  });

  it("shows the task board and members beside the log, and claims a task (WEB-U-065)", async () => {
    const fake = fakeChannels({
      channels: [RELEASE],
      members: { [RELEASE.id]: [CODER, ME] },
      messages: {
        [RELEASE.id]: [
          message(1, CODER, {
            type: "task",
            title: "Write the changelog",
            spec: "From the merged PRs.",
            claims: 2,
          }),
        ],
      },
      me: ME,
    });
    show(fake.api);
    const tasks = await screen.findByRole("list", { name: "To-dos" });
    expect(tasks).toHaveTextContent("Open · 2 places");
    fireEvent.click(within(tasks).getByRole("button", { name: "Take on #1" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("list", { name: "To-dos" })).toHaveTextContent(
        "1 of 2 places taken",
      ),
    );
    expect(
      await screen.findByRole("article", { name: "#2 Ana" }),
    ).toHaveTextContent("took on Write the changelog");
    // The task's own post carries its claim state too (UI-GUIDELINES §7, Channel post).
    await vi.waitFor(() =>
      expect(
        screen.getByRole("article", { name: "#1 Coder · alice" }),
      ).toHaveTextContent("1 of 2 places taken"),
    );
    fireEvent.click(screen.getByRole("tab", { name: "Members" }));
    expect(screen.getByRole("list", { name: "Members" })).toHaveTextContent(
      "Coder · alice",
    );
  });

  // WEB-U-263 (the c14b first hour, F5): a task added as a member is an agent's task; the channel's own work items are
  // to-dos, so the side panel never says "No tasks yet" beside a member task.
  it("calls the channel's own work items to-dos, apart from the member tasks", async () => {
    const fake = fakeChannels({
      channels: [RELEASE],
      members: { [RELEASE.id]: [CODER, ME] },
      me: ME,
    });
    show(fake.api);
    expect(
      await screen.findByRole("tab", { name: "To-dos", selected: true }),
    ).toBeVisible();
    expect(await screen.findByText("No to-dos yet.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Add a to-do…" })).toBeVisible();
    expect(screen.queryByText(/No tasks yet|Add a task/)).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Members" }));
    expect(screen.getByRole("list", { name: "Members" })).toHaveTextContent(
      "Coder · alice",
    );
  });

  it("follows the channel live from where the log ends, and reads only once it ends (G8)", async () => {
    const fake = fakeChannels({
      channels: [RELEASE],
      members: { [RELEASE.id]: [CODER, ME] },
      messages: {
        [RELEASE.id]: [message(1, CODER, { type: "text", text: "Starting." })],
      },
    });
    show(fake.api);
    await screen.findByRole("article", { name: "#1 Coder · alice" });
    await vi.waitFor(() => expect(fake.streams).toHaveLength(1));
    const stream = fake.streams[0] as FakeStream;
    expect(stream.after).toBe(1);
    act(() =>
      stream.send({
        type: "channel.message",
        ...message(2, CODER, { type: "text", text: "Live now." }),
      }),
    );
    expect(
      await screen.findByRole("article", { name: "#2 Coder · alice" }),
    ).toHaveTextContent("Live now.");
    // A frame the page already has (a replay after a reconnect) is not shown twice.
    act(() =>
      stream.send({
        type: "channel.message",
        ...message(2, CODER, { type: "text", text: "Live now." }),
      }),
    );
    expect(
      screen.getAllByRole("article", { name: "#2 Coder · alice" }),
    ).toHaveLength(1);
    fake.end(RELEASE.id);
    act(() => stream.send({ type: "channel.state", state: "ended" }));
    expect(
      await screen.findByText(
        "This channel has ended; it can be read, not added to.",
      ),
    ).toBeVisible();
  });
});

// ADR-0014 point 13: the owner, or an administrator, ends or deletes a channel after confirming by its name; the
// Members tab manages members through the channel's API (WEB-U-066).
describe("WEB-U-066 ending, deleting and managing a channel", () => {
  const OWNED = channel("release", {
    purpose: "Plan the 1.0 release",
    role: "lead",
    member_count: 2,
  });
  const OWNER = member("user", "Ana", { id: OWNED.owner.id, role: "lead" });
  const setup = () =>
    fakeChannels({
      channels: [OWNED],
      members: { [OWNED.id]: [OWNER, CODER] },
      me: OWNER,
      tasks: [{ id: "0195d8c0-0000-7000-8000-00000000cafe", title: "Docs" }],
    });

  // End and Delete sit in the header's "…" menu, not as two prominent buttons (UI-GUIDELINES §0: one main action).
  const choose = (bar: HTMLElement, action: string) => {
    fireEvent.click(
      within(bar).getByRole("button", { name: "More actions for release" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: action }));
  };

  it("lets the owner end the channel after confirming, which leaves it read-only", async () => {
    const fake = setup();
    show(fake.api, OWNED.id, { id: OWNER.id, role: "member" });
    const bar = await screen.findByRole("region", { name: "Channel header" });
    choose(bar, "End channel");
    const confirm = screen.getByRole("dialog", { name: "End release?" });
    expect(confirm).toHaveTextContent(
      "No one can post, vote or take on to-dos after this, and open proposals are decided now.",
    );
    fireEvent.click(
      within(confirm).getByRole("button", { name: "End channel" }),
    );
    expect(
      await screen.findByText(
        "This channel has ended; it can be read, not added to.",
      ),
    ).toBeVisible();
    expect(within(bar).getByText("Ended")).toBeVisible();
    fireEvent.click(
      within(bar).getByRole("button", { name: "More actions for release" }),
    );
    expect(screen.queryByRole("menuitem", { name: "End channel" })).toBeNull();
    expect(
      screen.getByRole("menuitem", { name: "Delete channel" }),
    ).toBeVisible();
  });

  it("lets the owner delete the channel after confirming, and returns to the list", async () => {
    const fake = setup();
    show(fake.api, OWNED.id, { id: OWNER.id, role: "member" });
    const bar = await screen.findByRole("region", { name: "Channel header" });
    choose(bar, "Delete channel");
    const confirm = screen.getByRole("dialog", { name: "Delete release?" });
    expect(confirm).toHaveTextContent(
      "Its log, to-dos and members are deleted for everyone. This can't be undone.",
    );
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Delete channel" }),
    );
    await vi.waitFor(() =>
      expect(router.push).toHaveBeenCalledWith("/channels"),
    );
    await expect(fake.api.channel(OWNED.id)).rejects.toThrow();
  });

  it("offers End and Delete to an administrator, and neither to another member", async () => {
    show(setup().api, OWNED.id, { id: "someone-else", role: "admin" });
    const bar = await screen.findByRole("region", { name: "Channel header" });
    expect(
      within(bar).queryByRole("button", { name: "End channel" }),
    ).toBeNull();
    fireEvent.click(
      within(bar).getByRole("button", { name: "More actions for release" }),
    );
    expect(screen.getByRole("menuitem", { name: "End channel" })).toBeVisible();
    expect(
      screen.getByRole("menuitem", { name: "Delete channel" }),
    ).toBeVisible();
    cleanup();
    show(setup().api, OWNED.id, { id: ME.id, role: "member" });
    const other = await screen.findByRole("region", { name: "Channel header" });
    expect(within(other).queryByRole("button")).toBeNull();
  });

  it("says when the channel could not be ended", async () => {
    const fake = setup();
    fake.api.endChannel = () =>
      Promise.reject(new ApiError(503, "store_unavailable", "x"));
    show(fake.api, OWNED.id, { id: OWNER.id, role: "member" });
    const bar = await screen.findByRole("region", { name: "Channel header" });
    choose(bar, "End channel");
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "End release?" })).getByRole(
        "button",
        { name: "End channel" },
      ),
    );
    expect(
      await screen.findByText("The channel could not be ended."),
    ).toBeVisible();
  });

  it("adds one of the owner's tasks' agents from the Members tab's + Add, and lists it", async () => {
    const fake = setup();
    show(fake.api, OWNED.id, { id: OWNER.id, role: "member" });
    await screen.findByRole("region", { name: "Channel header" });
    fireEvent.click(screen.getByRole("tab", { name: "Members" }));
    fireEvent.click(await screen.findByRole("button", { name: "Add" }));
    fireEvent.click(await screen.findByRole("option", { name: "Docs" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("list", { name: "Members" })).toHaveTextContent(
        "Docs",
      ),
    );
  });
});

// WEB-U-426: an agent reads a channel only while its task runs, and nothing here starts it (7 Nov 2026: a proposal
// timed out unvoted by two agents whose tasks had finished). Above the box, the page says which agents aren't running
// and how to bring them in.
describe("WEB-U-426 the page says which agents aren't running", () => {
  const SKY = member("conversation", "Sky", {
    title: "Why the Sky Is Blue",
    run_status: "completed",
    can_open: true,
  });
  const FIRST = member("conversation", "First", {
    title: "My first task",
    run_status: "idle",
    can_open: true,
  });
  const BUSY = member("conversation", "Busy", {
    title: "A busy task",
    run_status: "running",
    can_open: true,
  });
  const THEIRS = member("conversation", "Planner", {
    owner_name: "alice",
    run_status: "failed",
  });
  const READER = member("conversation", "Reader", {
    title: "A reading task",
    run_status: "idle",
    can_open: true,
    role: "observer",
  });
  const NEWER = member("conversation", "Newer", {
    title: "A task in a newer state",
    // A state from a newer gateway than this page (the Members tab reads it as "Status unknown").
    run_status: "parked" as unknown as NonNullable<Member["run_status"]>,
    can_open: true,
  });
  const hint = (count: string) => screen.findByRole("region", { name: count });

  it("names each agent whose task isn't running, and links the tasks you may open", async () => {
    show(
      fakeChannels({
        channels: [RELEASE],
        members: { [RELEASE.id]: [SKY, FIRST, BUSY, THEIRS, ME] },
      }).api,
    );
    const region = await hint("3 agents aren’t running");
    // Planner isn't the viewer's: its owner is the one who can start it.
    expect(region).toHaveTextContent(
      "An agent reads this channel only while its task runs: open its task and ask it to work here, or ask its owner when the agent isn’t yours.",
    );
    expect(
      within(region).getByRole("link", { name: "Why the Sky Is Blue" }),
    ).toHaveAttribute("href", `/c/${encodeURIComponent(SKY.id)}`);
    expect(
      within(region).getByRole("link", { name: "My first task" }),
    ).toHaveAttribute("href", `/c/${encodeURIComponent(FIRST.id)}`);
    // Another person's agent can't be opened from here: its name and owner, as the Members tab shows it.
    expect(within(region).getByText("Planner · alice")).toBeVisible();
    expect(
      within(region).queryByRole("link", { name: /Planner/ }),
    ).not.toBeInTheDocument();
    expect(within(region).queryByText("A busy task")).not.toBeInTheDocument();
    // It sits above the box it is about.
    const box = screen.getByRole("textbox", { name: "Your message" });
    expect(
      region.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("says one agent in the singular", async () => {
    show(
      fakeChannels({
        channels: [RELEASE],
        members: { [RELEASE.id]: [SKY, BUSY, ME] },
      }).api,
    );
    const region = await hint("1 agent isn’t running");
    expect(within(region).getAllByRole("listitem")).toHaveLength(1);
    // Only the viewer's own agents: nothing about owners.
    expect(region).toHaveTextContent(
      /^1 agent isn’t runningAn agent reads this channel only while its task runs: open its task and ask it to work here\.Why the Sky Is Blue$/,
    );
  });

  it("says nothing while every agent runs, with people only, or for an agent that reads only or a state it doesn't know", async () => {
    for (const members of [
      [BUSY, ME],
      [ME, member("user", "Bo")],
      [READER, NEWER, ME],
    ]) {
      cleanup();
      show(
        fakeChannels({
          channels: [RELEASE],
          members: { [RELEASE.id]: members },
        }).api,
      );
      expect(
        await screen.findByRole("textbox", { name: "Your message" }),
      ).toBeVisible();
      await settleLazy();
      expect(
        screen.queryByRole("region", { name: /running/ }),
      ).not.toBeInTheDocument();
    }
  });

  it("says nothing to a member who reads only, or on an ended channel", async () => {
    for (const item of [
      { ...RELEASE, role: "observer" as const },
      { ...RELEASE, state: "ended" as const },
    ]) {
      cleanup();
      show(
        fakeChannels({ channels: [item], members: { [item.id]: [SKY, ME] } })
          .api,
      );
      expect(
        await screen.findByRole("heading", { level: 1, name: "release" }),
      ).toBeVisible();
      await settleLazy();
      expect(
        screen.queryByRole("region", { name: /running/ }),
      ).not.toBeInTheDocument();
    }
  });

  it("goes once the agent's task runs", async () => {
    const members = { [RELEASE.id]: [FIRST, ME] };
    show(fakeChannels({ channels: [RELEASE], members }).api);
    await hint("1 agent isn’t running");
    members[RELEASE.id] = [{ ...FIRST, run_status: "running" }, ME];
    // The members list comes again on focus and every 15 s (no stream hint yet).
    act(() => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
    });
    await vi.waitFor(() =>
      expect(
        screen.queryByRole("region", { name: /running/ }),
      ).not.toBeInTheDocument(),
    );
    focusManager.setFocused(undefined);
  });

  it("no longer tells a new channel that everyone reads each post", async () => {
    show(
      fakeChannels({
        channels: [RELEASE],
        members: { [RELEASE.id]: [SKY, ME] },
        messages: { [RELEASE.id]: [] },
      }).api,
    );
    const first = await screen.findByRole("region", {
      name: "Tell your agents what you need",
    });
    expect(first).toHaveTextContent(
      "People here see each post; an agent sees them while its task runs.",
    );
  });
});

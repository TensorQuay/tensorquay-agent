import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { channel, member } from "../test/channels";

import { ChannelMembers } from "./ChannelMembers";
import type { Channel, ChannelRole, Member, MemberKind } from "./contract";

const RELEASE = channel("release", { role: "lead" });
const OWNER = member("user", "Ana", { id: RELEASE.owner.id, role: "lead" });
const BEN = member("user", "Ben", { role: "member" });
const WATCHER = member("user", "Wu", { role: "observer" });
const CODER = member("conversation", "Coder", {
  owner_name: "Ana",
  role: "member",
});
const REVIEWER = member("conversation", "Reviewer", {
  owner_name: "Ben",
  role: "member",
});
/** An agent whose task title the viewer may see (its owner, or an administrator), with its run. */
const PLANNER = member("conversation", "Planner", {
  owner_name: "Ana",
  title: "Plan the release",
  role: "lead",
  run_status: "running",
});
const members: Member[] = [OWNER, BEN, WATCHER, CODER, REVIEWER, PLANNER];
/** The viewer's own tasks: Reviewer is Ben's, already a member; "Tests" is not in the channel yet. */
const BENS_TASKS = [
  { id: REVIEWER.id, title: "Reviewer" },
  { id: "0195d8c0-0000-7000-8000-00000000beef", title: "Tests" },
];

function show({
  me = OWNER,
  item = RELEASE,
  tasks = [{ id: "0195d8c0-0000-7000-8000-00000000cafe", title: "Docs" }],
  set = vi.fn<
    (kind: MemberKind, id: string, role: ChannelRole) => Promise<void>
  >(() => Promise.resolve()),
  remove = vi.fn<(kind: MemberKind, id: string) => Promise<void>>(() =>
    Promise.resolve(),
  ),
  addPerson = vi.fn<(email: string, role: ChannelRole) => Promise<void>>(() =>
    Promise.resolve(),
  ),
  byEmail = true,
}: {
  me?: Member;
  item?: Channel;
  tasks?: { id: string; title: string }[];
  set?: (kind: MemberKind, id: string, role: ChannelRole) => Promise<void>;
  remove?: (kind: MemberKind, id: string) => Promise<void>;
  addPerson?: (email: string, role: ChannelRole) => Promise<void>;
  byEmail?: boolean;
} = {}) {
  // As the gateway computes `can_open` for the caller: their own tasks only (none of these viewers is an administrator).
  const seen = members.map((value) =>
    value.kind === "conversation"
      ? { ...value, can_open: value.owner_name === me.name }
      : value,
  );
  render(
    <ChannelMembers
      channel={item}
      members={seen}
      me={me.id}
      tasks={tasks}
      onSet={set}
      onRemove={remove}
      {...(byEmail ? { onAddPerson: addPerson } : {})}
    />,
  );
  return { set, remove, addPerson };
}
const list = () => screen.getByRole("list", { name: "Members" });
const row = (name: string) =>
  within(list()).getByRole("listitem", { name: new RegExp(`^${name}`) });
/** Opens a member's "…" menu and chooses an item. */
const choose = (name: string, item: string) => {
  fireEvent.click(
    within(row(name)).getByRole("button", { name: `More actions for ${name}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
};
/** Opens "+ Add" and returns its search box. */
const addBox = () => {
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  return screen.getByRole("combobox", { name: "Add to the channel" });
};

// ADR-0014 point 4: the owner manages every member; a conversation's owner adds and removes their own tasks' agents,
// never as lead; a person leaves; an ended channel offers nothing (WEB-U-066). Rows are light (UI-GUIDELINES §0):
// agents by task title with a status dot and its word, a role only when it isn't a plain member, and the changes in
// each row's "…" menu; adding is one search box behind "+ Add", everyone joining as a member.
describe("WEB-U-066 channel membership", () => {
  it("lists agents by task title with their run, and a role only when it isn't a plain member", () => {
    show();
    const planner = row("Plan the release");
    expect(planner).toHaveTextContent("Planner · Ana");
    expect(planner).toHaveTextContent("Working");
    expect(planner.querySelector(".member-dot")).toHaveAttribute(
      "data-tone",
      "info",
    );
    expect(within(planner).getByText("Lead")).toBeVisible();
    // Without a title the agent reads by its name and owner; a plain member carries no badge.
    expect(row("Coder · Ana")).toBeVisible();
    expect(within(row("Ben")).queryByText("Member")).toBeNull();
    expect(within(row("Wu")).getByText("Reads only")).toBeVisible();
    // No role pickers or Remove buttons in the rows themselves.
    expect(screen.queryByRole("button", { name: /^Role for/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Remove/ })).toBeNull();
  });

  // The owner's request: an agent's name opens its task page, where its run shows live; a person's name links nowhere,
  // and neither does a task the viewer may not open (the gateway's `can_open`: its owner, or an administrator).
  it("links each agent's name to its task, and a person's to nothing", () => {
    show({ item: { ...RELEASE, state: "ended" } });
    expect(
      within(row("Plan the release")).getByRole("link", {
        name: "Plan the release",
      }),
    ).toHaveAttribute("href", `/c/${PLANNER.id}`);
    expect(
      within(row("Coder · Ana")).getByRole("link", { name: "Coder · Ana" }),
    ).toHaveAttribute("href", `/c/${CODER.id}`);
    expect(within(row("Ben")).queryByRole("link")).toBeNull();
    // Reviewer is Ben's task: Ana may not open it.
    expect(within(row("Reviewer · Ben")).queryByRole("link")).toBeNull();
    expect(within(list()).getAllByRole("link")).toHaveLength(2);
    cleanup();
    show({ me: BEN, tasks: BENS_TASKS });
    expect(
      within(row("Reviewer · Ben")).getByRole("link", {
        name: "Reviewer · Ben",
      }),
    ).toHaveAttribute("href", `/c/${REVIEWER.id}`);
    expect(within(list()).getAllByRole("link")).toHaveLength(1);
  });

  it("lets the owner change any other member's role and remove them after confirming by name, from the row's menu", async () => {
    const { set, remove } = show();
    expect(
      within(row("Ana")).queryByRole("button", { name: /^More actions/ }),
    ).toBeNull();
    fireEvent.click(
      within(row("Ben")).getByRole("button", { name: "More actions for Ben" }),
    );
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Make lead", "Make read-only", "Remove"]);
    fireEvent.keyDown(screen.getAllByRole("menuitem")[0] as HTMLElement, {
      key: "Escape",
    });
    fireEvent.click(
      within(row("Coder · Ana")).getByRole("button", {
        name: "More actions for Coder · Ana",
      }),
    );
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Open task", "Make lead", "Make read-only", "Remove"]);
    expect(screen.getByRole("menuitem", { name: "Open task" })).toHaveAttribute(
      "href",
      `/c/${CODER.id}`,
    );
    fireEvent.keyDown(screen.getAllByRole("menuitem")[0] as HTMLElement, {
      key: "Escape",
    });
    fireEvent.click(
      within(row("Ben")).getByRole("button", { name: "More actions for Ben" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Make read-only" }));
    await vi.waitFor(() =>
      expect(set).toHaveBeenCalledWith("user", BEN.id, "observer"),
    );
    choose("Coder · Ana", "Remove");
    const confirm = screen.getByRole("dialog", {
      name: "Remove Coder · Ana from the channel?",
    });
    expect(remove).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole("button", { name: "Remove" }));
    await vi.waitFor(() =>
      expect(remove).toHaveBeenCalledWith("conversation", CODER.id),
    );
    // The note follows the removal's own round trip, so it is read until it is in: the status region already holds
    // the role change's note, and a read right after the call saw that one (CI's runner).
    await vi.waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "Removed Coder · Ana.",
      ),
    );
  });

  it("adds one of the owner's tasks' agents as a member from + Add, offering only tasks not already in it", async () => {
    const { set } = show({
      tasks: [
        { id: CODER.id, title: "Coder" },
        { id: "0195d8c0-0000-7000-8000-00000000cafe", title: "Docs" },
      ],
    });
    expect(screen.queryByRole("combobox")).toBeNull();
    const box = addBox();
    expect(box).toHaveFocus();
    expect(
      screen.getAllByRole("option").map((option) => option.textContent),
    ).toEqual(["Docs"]);
    fireEvent.click(screen.getByRole("option", { name: "Docs" }));
    await vi.waitFor(() =>
      expect(set).toHaveBeenCalledWith(
        "conversation",
        "0195d8c0-0000-7000-8000-00000000cafe",
        "member",
      ),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("Added Docs.");
  });

  it("lets the owner add a person by exact email, and names an unknown email as a workspace share does", async () => {
    const addPerson = vi
      .fn<(email: string, role: ChannelRole) => Promise<void>>()
      .mockRejectedValueOnce(new ApiError(404, "user_not_found", "x"))
      .mockResolvedValueOnce(undefined);
    show({ addPerson });
    const box = addBox();
    expect(box).toHaveAttribute(
      "placeholder",
      "Search your tasks, or type an email",
    );
    fireEvent.change(box, { target: { value: " cy@example.invalid " } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No account uses that email.",
    );
    expect(addPerson).toHaveBeenLastCalledWith("cy@example.invalid", "member");
    fireEvent.change(box, { target: { value: "cy@example.invalid" } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Added cy@example.invalid.",
    );
  });

  it("names the by-email refusals as a workspace share does, and a rate limit", async () => {
    const addPerson = vi
      .fn<(email: string, role: ChannelRole) => Promise<void>>()
      .mockRejectedValueOnce(new ApiError(400, "invalid_request", "x"))
      .mockRejectedValueOnce(new ApiError(429, "rate_limited", "x"));
    show({ addPerson });
    const box = addBox();
    for (const words of [
      "Enter one email address, not your own.",
      "Too many people were added just now; try again later.",
    ]) {
      fireEvent.change(box, { target: { value: "ana@example.invalid" } });
      fireEvent.keyDown(box, { key: "Enter" });
      expect(await screen.findByText(words)).toBeVisible();
    }
  });

  it("offers no email until the gateway adds people by email, and none to anyone but the owner", () => {
    show({ byEmail: false });
    expect(addBox()).toHaveAttribute("placeholder", "Search your tasks");
    cleanup();
    show({ me: BEN, tasks: BENS_TASKS });
    expect(addBox()).toHaveAttribute("placeholder", "Search your tasks");
  });

  it("lets a member add and remove only their own tasks' agents, and leave", async () => {
    const { set, remove } = show({ me: BEN, tasks: BENS_TASKS });
    expect(
      within(row("Coder · Ana")).queryByRole("button", {
        name: /^More actions/,
      }),
    ).toBeNull();
    fireEvent.click(
      within(row("Reviewer · Ben")).getByRole("button", {
        name: "More actions for Reviewer · Ben",
      }),
    );
    // Not the owner: no roles to give, only opening the task and Remove.
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Open task", "Remove"]);
    fireEvent.keyDown(screen.getAllByRole("menuitem")[0] as HTMLElement, {
      key: "Escape",
    });
    const box = addBox();
    fireEvent.click(screen.getByRole("option", { name: "Tests" }));
    await vi.waitFor(() =>
      expect(set).toHaveBeenCalledWith(
        "conversation",
        "0195d8c0-0000-7000-8000-00000000beef",
        "member",
      ),
    );
    expect(box).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Leave channel" }));
    const confirm = screen.getByRole("dialog", { name: "Leave release?" });
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Leave channel" }),
    );
    await vi.waitFor(() => expect(remove).toHaveBeenCalledWith("user", BEN.id));
  });

  it("lets an observer add their own tasks' agents and leave, as the API allows", () => {
    show({ me: WATCHER, tasks: [{ id: "t-1", title: "Notes" }] });
    addBox();
    expect(screen.getByRole("option", { name: "Notes" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Leave channel" })).toBeVisible();
  });

  it("says why no task is offered", () => {
    show({ tasks: [{ id: CODER.id, title: "Coder" }] });
    addBox();
    expect(screen.getByText("All your tasks are already in it.")).toBeVisible();
    cleanup();
    show({ tasks: [] });
    addBox();
    expect(screen.getByText("You have no tasks yet.")).toBeVisible();
  });

  it("offers nothing on an ended channel, or to someone who isn't a member", () => {
    show({ item: { ...RELEASE, state: "ended" } });
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
    cleanup();
    show({
      me: member("user", "Visitor"),
      item: { ...RELEASE, role: null },
    });
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("names each refusal", async () => {
    const set = vi
      .fn<(kind: MemberKind, id: string, role: ChannelRole) => Promise<void>>()
      .mockRejectedValueOnce(new ApiError(409, "channel_full", "x"))
      .mockRejectedValueOnce(new ApiError(403, "not_owner", "x"))
      .mockRejectedValueOnce(new ApiError(404, "not_found", "x"));
    show({ set });
    for (const [item, words] of [
      ["Make read-only", "This channel has as many members as it can take."],
      ["Make lead", "Only the channel's owner can do that."],
      ["Make read-only", "That member or task no longer exists."],
    ] as const) {
      choose("Ben", item);
      expect(await screen.findByRole("alert")).toHaveTextContent(words);
    }
  });
});

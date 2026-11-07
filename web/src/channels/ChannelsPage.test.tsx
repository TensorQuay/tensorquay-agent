import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { ShellContext } from "../shell/shell-context";
import { channel, fakeChannels, member } from "../test/channels";

import { ChannelsPage } from "./ChannelsPage";
import type { ChannelsApi } from "./contract";

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  usePathname: () => "/channels",
}));
afterEach(() => push.mockReset());

function show(api: ChannelsApi) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ChannelsPage api={api} />
    </QueryClientProvider>,
  );
}
const dialog = () => screen.getByRole("dialog", { name: "New channel" });
async function open() {
  fireEvent.click(
    (
      await screen.findAllByRole("button", { name: "New channel" })
    )[0] as HTMLElement,
  );
}
const type = (label: string, value: string) =>
  fireEvent.change(within(dialog()).getByLabelText(label), {
    target: { value },
  });
const create = () =>
  fireEvent.click(
    within(dialog()).getByRole("button", { name: "Create channel" }),
  );

// ADR-0014: channels are shared logs where agents and people post, propose, vote and claim tasks (WEB-U-062).
describe("WEB-U-062 the channels list", () => {
  it("lists each channel with its purpose, rule, state and members, linking to its page", async () => {
    show(
      fakeChannels({
        channels: [
          channel("release", {
            purpose: "Plan the 1.0 release",
            rule: "majority",
            member_count: 4,
          }),
          channel("retro", {
            purpose: "What went wrong last week",
            rule: "human",
            state: "ended",
            member_count: 2,
          }),
        ],
      }).api,
    );
    const list = await screen.findByRole("list", { name: "Channels" });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    const release = items[0] as HTMLElement;
    expect(
      within(release).getByRole("link", { name: "release" }),
    ).toHaveAttribute("href", `/channels/${channel("release").id}`);
    for (const text of [
      "Plan the 1.0 release",
      "Decided by majority",
      "Open",
      "4 members",
      "Led by Ana",
    ])
      expect(within(release).getByText(text)).toBeVisible();
    const retro = items[1] as HTMLElement;
    expect(within(retro).getByText("Decided by a person")).toBeVisible();
    expect(within(retro).getByText("Ended")).toBeVisible();
  });

  // General-purpose finding C1: what a channel is for, in plain words and with an example that isn't only coding.
  it("says what a channel is for, with an example", async () => {
    show(fakeChannels().api);
    expect(
      await screen.findByText(
        "A shared thread for one goal: people and their tasks' agents post updates, agree on decisions and split the work. For example, a weekly report put together by a research task, a data task and you.",
      ),
    ).toBeVisible();
  });

  it("says so when there are none, with the one action", async () => {
    show(fakeChannels().api);
    // A channel's icon, as in the sidebar, not the logo; one plain sentence.
    const sentence = await screen.findByText(
      "No channels yet. Start one when several tasks and people work toward one goal.",
    );
    expect(sentence).toBeVisible();
    const empty = sentence.closest(".empty-state");
    expect(empty?.querySelector("svg.lucide-messages-square")).not.toBeNull();
    expect(empty?.querySelector(".brand-mark")).toBeNull();
    expect(
      screen.getAllByRole("button", { name: "New channel" }).length,
    ).toBeGreaterThan(0);
  });

  it("creates a channel from what it's for alone, with a title suggested from that, then opens it", async () => {
    const fake = fakeChannels();
    show(fake.api);
    await open();
    // One field first (UI-GUIDELINES §0): no Name, Purpose and Decided-by fields side by side.
    expect(within(dialog()).queryByLabelText("Name")).toBeNull();
    expect(within(dialog()).queryByLabelText("Decided by")).toBeNull();
    expect(
      within(dialog()).getByLabelText("What's this channel for?"),
    ).toHaveFocus();
    type(
      "What's this channel for?",
      "Get the 1.0 release out by Friday. The notes are drafted.",
    );
    expect(
      within(dialog()).getByText("Get the 1.0 release out by Friday"),
    ).toBeVisible();
    create();
    await vi.waitFor(() => expect(push).toHaveBeenCalled());
    expect(fake.creates).toEqual([
      {
        name: "Get the 1.0 release out by Friday",
        purpose: "Get the 1.0 release out by Friday. The notes are drafted.",
        rule: "majority",
      },
    ]);
    expect(push).toHaveBeenCalledWith(`/channels/${fake.created()?.id}`);
  });

  it("lets the suggested title be changed, and then no longer follows what the channel is for", async () => {
    const fake = fakeChannels();
    show(fake.api);
    await open();
    type("What's this channel for?", "Plan the release");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Change title" }),
    );
    expect(within(dialog()).getByLabelText("Title")).toHaveValue(
      "Plan the release",
    );
    type("Title", "Release 1.0");
    type("What's this channel for?", "Plan the release and the notes");
    expect(within(dialog()).getByLabelText("Title")).toHaveValue("Release 1.0");
    create();
    await vi.waitFor(() => expect(push).toHaveBeenCalled());
    expect(fake.creates[0]?.name).toBe("Release 1.0");
  });

  it("adds tasks' agents and people from one search box, everyone as a member, the creator as lead", async () => {
    const cy = member("user", "Cy");
    const fake = fakeChannels({
      tasks: [
        { id: "0195d8c0-0000-7000-8000-0000000000c1", title: "Docs" },
        { id: "0195d8c0-0000-7000-8000-0000000000c2", title: "Tests" },
      ],
      people: { "cy@example.invalid": cy },
    });
    show(fake.api);
    await open();
    type("What's this channel for?", "Plan it");
    const chosen = within(dialog()).getByRole("list", { name: "Who's in it" });
    expect(
      within(chosen).getByRole("listitem", { name: "You, lead" }),
    ).toBeVisible();
    const search = within(dialog()).getByRole("combobox", {
      name: "Who's in it",
    });
    fireEvent.focus(search);
    // On focus, your recent tasks are suggested.
    expect(
      (await screen.findAllByRole("option")).map(
        (option) => option.textContent,
      ),
    ).toEqual([
      expect.stringContaining("Docs"),
      expect.stringContaining("Tests"),
    ]);
    fireEvent.change(search, { target: { value: "tes" } });
    fireEvent.click(screen.getByRole("option", { name: /Tests/ }));
    expect(search).toHaveValue("");
    // Closed after a pick, so it covers nothing below it.
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(
      within(chosen).getByRole("listitem", { name: "Tests" }),
    ).toBeVisible();
    // People: by exact email only (the privacy rule); a name or a prefix finds nobody.
    fireEvent.change(search, { target: { value: "cy" } });
    expect(screen.queryByRole("option", { name: /Cy/ })).toBeNull();
    fireEvent.change(search, { target: { value: "cy@example.invalid" } });
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "Enter" });
    expect(
      within(chosen).getByRole("listitem", { name: "cy@example.invalid" }),
    ).toBeVisible();
    fireEvent.focus(search);
    fireEvent.click(await screen.findByRole("option", { name: /Docs/ }));
    fireEvent.click(
      within(chosen).getByRole("button", { name: "Remove Docs" }),
    );
    expect(within(chosen).queryByRole("listitem", { name: "Docs" })).toBeNull();
    // No role to choose while creating: roles change later, in Members.
    expect(
      within(dialog()).queryByRole("button", { name: /^Role/ }),
    ).toBeNull();
    create();
    await vi.waitFor(() => expect(push).toHaveBeenCalled());
    const id = fake.created()?.id ?? "";
    const listed = (await fake.api.members(id)).items.map(
      (item) => `${item.kind}:${item.name}:${item.role}`,
    );
    expect(listed).toEqual(
      expect.arrayContaining(["conversation:Tests:member", "user:Cy:member"]),
    );
    expect(listed).not.toContain("conversation:Docs:member");
  });

  it("says when every task is already in it", async () => {
    const fake = fakeChannels({
      tasks: [{ id: "0195d8c0-0000-7000-8000-0000000000c1", title: "Docs" }],
    });
    show(fake.api);
    await open();
    const search = within(dialog()).getByRole("combobox", {
      name: "Who's in it",
    });
    fireEvent.focus(search);
    fireEvent.click(await screen.findByRole("option", { name: /Docs/ }));
    fireEvent.focus(search);
    expect(
      await within(dialog()).findByText("All your tasks are already in it."),
    ).toBeVisible();
  });

  // One label style in the dialog, and More options reads as a disclosure (review points a and b).
  it("labels every part alike, and shows More options as a disclosure", async () => {
    show(fakeChannels().api);
    await open();
    const labels = ["What's this channel for?", "Who's in it"].map(
      (text) =>
        within(dialog())
          .getAllByText(text)
          .find((element) => element.tagName === "LABEL")?.className,
    );
    expect(labels[0]).toBeTruthy();
    expect(labels[1]).toBe(labels[0]);
    const more = within(dialog()).getByRole("button", { name: "More options" });
    expect(more.querySelector("svg")).not.toBeNull();
    fireEvent.click(more);
    const legend = within(dialog()).getByText("How decisions are made");
    expect(legend.className).toBe(labels[0]);
  });

  it("suggests at most 5 recent tasks and shows at most 8 matches, never every task", async () => {
    const fake = fakeChannels({
      tasks: Array.from({ length: 30 }, (_, index) => ({
        id: `0195d8c0-0000-7000-8000-0000000001${String(index).padStart(2, "0")}`,
        title: `Fix bug ${index + 1}`,
      })),
    });
    show(fake.api);
    await open();
    const search = within(dialog()).getByRole("combobox", {
      name: "Who's in it",
    });
    fireEvent.focus(search);
    expect(await screen.findAllByRole("option")).toHaveLength(5);
    fireEvent.change(search, { target: { value: "bug" } });
    expect(screen.getAllByRole("option")).toHaveLength(8);
    fireEvent.change(search, { target: { value: "bug 27" } });
    expect(
      screen.getAllByRole("option").map((option) => option.textContent),
    ).toEqual([expect.stringContaining("Fix bug 27")]);
    fireEvent.keyDown(search, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(within(dialog()).queryAllByRole("checkbox")).toEqual([]);
  });

  it("keeps how decisions are made under More options, by majority unless changed", async () => {
    const fake = fakeChannels();
    show(fake.api);
    await open();
    type("What's this channel for?", "Plan it");
    const more = within(dialog()).getByRole("button", { name: "More options" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(within(dialog()).queryByRole("radio")).toBeNull();
    fireEvent.click(more);
    expect(
      within(dialog()).getByRole("radio", { name: /Most members agree/ }),
    ).toBeChecked();
    fireEvent.click(
      within(dialog()).getByRole("radio", { name: /A person decides/ }),
    );
    create();
    await vi.waitFor(() => expect(push).toHaveBeenCalled());
    expect(fake.creates[0]?.rule).toBe("human");
  });

  it("names each member it couldn't add, without undoing the channel", async () => {
    const fake = fakeChannels({
      tasks: [{ id: "0195d8c0-0000-7000-8000-0000000000c1", title: "Docs" }],
    });
    show(fake.api);
    await open();
    type("What's this channel for?", "Plan the release");
    const search = within(dialog()).getByRole("combobox", {
      name: "Who's in it",
    });
    fireEvent.focus(search);
    fireEvent.click(await screen.findByRole("option", { name: /Docs/ }));
    fireEvent.change(search, { target: { value: "nobody@example.invalid" } });
    fireEvent.keyDown(search, { key: "Enter" });
    create();
    const result = await within(dialog()).findByRole("list", {
      name: "Not added",
    });
    expect(within(dialog()).getByRole("status")).toHaveTextContent(
      "Created Plan the release.",
    );
    expect(result).toHaveTextContent(
      "nobody@example.invalid: No account uses that email.",
    );
    expect(push).not.toHaveBeenCalled();
    const id = fake.created()?.id ?? "";
    expect(
      (await fake.api.members(id)).items.map((item) => item.name),
    ).toContain("Docs");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Open channel" }),
    );
    expect(push).toHaveBeenCalledWith(`/channels/${id}`);
  });

  it("offers no email until the gateway adds people by email", async () => {
    const fake = fakeChannels();
    delete (fake.api as Partial<ChannelsApi>).addPerson;
    show(fake.api);
    await open();
    const search = within(dialog()).getByRole("combobox", {
      name: "Who's in it",
    });
    expect(search).toHaveAttribute("placeholder", "Search your tasks");
    fireEvent.change(search, { target: { value: "cy@example.invalid" } });
    expect(
      screen.queryByRole("option", { name: /cy@example.invalid/ }),
    ).toBeNull();
    fireEvent.change(search, { target: { value: "" } });
    fireEvent.focus(search);
    // Says why nothing is offered: no tasks yet (F1 review point c).
    expect(
      await within(dialog()).findByText("You have no tasks yet."),
    ).toBeVisible();
  });

  // F1, P1: the errors stayed red after both fields were filled. Each now shows only after Create and goes as soon as
  // its field changes; a gateway refusal of the title opens the title field to fix it.
  it("shows errors only after Create, clears each as its field changes, and names a refused title", async () => {
    const fake = fakeChannels({
      refuse: new ApiError(400, "invalid_request", "x", undefined, [
        { field: "name", reason: "name_invalid" },
      ]),
    });
    show(fake.api);
    await open();
    expect(within(dialog()).queryByRole("alert")).toBeNull();
    create();
    expect(
      within(dialog()).getByText("Say what the channel is for."),
    ).toBeVisible();
    expect(fake.creates).toEqual([]);
    type("What's this channel for?", "Plan it");
    expect(
      within(dialog()).queryByText("Say what the channel is for."),
    ).toBeNull();
    create();
    expect(
      await within(dialog()).findByText(
        "Use 1 to 80 characters, without control characters.",
      ),
    ).toBeVisible();
    expect(within(dialog()).getByLabelText("Title")).toHaveValue("Plan it");
    type("Title", "Plan it well");
    expect(
      within(dialog()).queryByText(
        "Use 1 to 80 characters, without control characters.",
      ),
    ).toBeNull();
    expect(push).not.toHaveBeenCalled();
  });
});

// WEB-U-362 (the 16f page pass, E17): Channels starts its bar with Open navigation, for the drawer below 768 px.
describe("WEB-U-362 Open navigation on Channels", () => {
  it("opens the shell's navigation", async () => {
    const openNavigation = vi.fn<() => void>();
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <ShellContext.Provider value={{ openNavigation }}>
        <QueryClientProvider client={cache}>
          <ChannelsPage api={fakeChannels().api} />
        </QueryClientProvider>
      </ShellContext.Provider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Open navigation" }),
    );
    expect(openNavigation).toHaveBeenCalledTimes(1);
  });
});

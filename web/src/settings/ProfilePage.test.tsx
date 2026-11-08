import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { showProfiles } from "../test/profile-app";
import { fakeProfiles, profile } from "../test/profiles";
import { LIMITS } from "../test/user";

import type { ChoicesApi } from "./profile-queries";

/** Replaced by the harness once it renders (showProfiles). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

const member: User = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "member@example.invalid",
  role: "member",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const choices: ChoicesApi = {
  models: () => Promise.resolve({ items: [] }),
  templates: () => Promise.resolve({ items: [] }),
  connectors: () => Promise.resolve({ items: [] }),
};
const REVIEW = profile("review", { display_name: "Code review" });
const TEAM = profile("team", { display_name: "Team default", shared: true });
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
const tools = () => within(page()).getByRole("region", { name: "Tools" });
const box = (name: string) =>
  within(tools()).getByRole("checkbox", {
    name: (accessible) => accessible === name,
  });
const save = () =>
  fireEvent.click(within(page()).getByRole("button", { name: "Save" }));

// WEB-U-104 (the UI standard's Page): adding, editing or duplicating an agent profile is a page of its own.
describe("WEB-U-104 the Agent profile page", () => {
  it("opens Add, Edit and Duplicate as pages of their own from the list", async () => {
    const at = showProfiles(
      fakeProfiles({ profiles: [REVIEW, TEAM] }).api,
      choices,
      member,
      router,
    );
    expect(
      await screen.findByRole("link", { name: "Add agent profile" }),
    ).toHaveAttribute("href", "/settings/agent-profiles/new");
    expect(
      screen.getByRole("link", { name: "Edit agent profile Code review" }),
    ).toHaveAttribute("href", `/settings/agent-profiles/${REVIEW.id}`);
    expect(
      screen.getByRole("link", {
        name: "Duplicate agent profile Team default as your own",
      }),
    ).toHaveAttribute("href", `/settings/agent-profiles/${TEAM.id}/duplicate`);
    fireEvent.click(
      screen.getByRole("link", { name: "Edit agent profile Code review" }),
    );
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Edit Code review",
      }),
    ).toBeVisible();
    expect(at.path).toBe(`/settings/agent-profiles/${REVIEW.id}`);
    expect(
      screen.getByRole("link", { name: "Agent profiles" }),
    ).toHaveAttribute("href", "/settings/agent-profiles");
    expect(screen.queryByRole("dialog")).toBeNull();
    for (const section of [
      "Basics",
      "Instructions",
      "Model and environment",
      "Tools",
      "Skills",
      "Connectors",
      "Limits",
    ])
      expect(screen.getByRole("region", { name: section })).toBeVisible();
  });

  it("shows a member a shared profile read-only, with only Close", async () => {
    showProfiles(
      fakeProfiles({ profiles: [TEAM] }).api,
      choices,
      member,
      router,
      `/settings/agent-profiles/${TEAM.id}`,
    );
    // WEB-U-359 (a pass over the pages before 17.0): a page that can't be edited is titled by the profile's name, never
    // "Edit".
    await screen.findByRole("heading", { name: "Team default" });
    expect(screen.queryByRole("heading", { name: /^Edit / })).toBeNull();
    expect(page()).toHaveTextContent(
      "This shared profile is the administrators' to change. You can duplicate it to make your own.",
    );
    expect(within(page()).getByLabelText("Display name")).toBeDisabled();
    expect(box("Run commands")).toBeDisabled();
    expect(within(page()).queryByRole("button", { name: "Save" })).toBeNull();
    expect(within(page()).getByRole("button", { name: "Close" })).toBeVisible();
  });

  it("says when the profile no longer exists", async () => {
    showProfiles(
      fakeProfiles({ profiles: [REVIEW] }).api,
      choices,
      member,
      router,
      "/settings/agent-profiles/0195d8c0-0000-7000-8000-00000000ffff",
    );
    expect(
      await screen.findByText("This agent profile no longer exists."),
    ).toBeVisible();
  });

  it("asks before leaving unsaved changes", async () => {
    showProfiles(
      fakeProfiles({ profiles: [REVIEW] }).api,
      choices,
      member,
      router,
      `/settings/agent-profiles/${REVIEW.id}`,
    );
    await screen.findByRole("heading", { name: "Edit Code review" });
    fireEvent.click(box("Run commands"));
    fireEvent.click(screen.getByRole("link", { name: "Agent profiles" }));
    expect(
      screen.getByRole("dialog", { name: "Leave without saving?" }),
    ).toBeVisible();
  });
});

// WEB-U-105: a profile's tools are never hidden. Every tool is listed in its group, ticked by
// default, with one plain line on what it lets the agent do; All ticks or clears them at once.
describe("WEB-U-105 a profile's tools", () => {
  it("lists every tool in its group, ticked, each with what it does", async () => {
    showProfiles(
      fakeProfiles({ profiles: [] }).api,
      choices,
      member,
      router,
      "/settings/agent-profiles/new",
    );
    await screen.findByRole("heading", { name: "Add an agent profile" });
    expect(
      within(tools())
        .getAllByRole("group")
        .map((group) => group.querySelector("legend")?.textContent),
    ).toEqual([
      "Files",
      "Finding",
      "Commands",
      "Notes",
      "Web",
      "Skills",
      "Channels",
    ]);
    const files = within(tools()).getByRole("group", { name: "Files" });
    expect(
      within(files)
        .getAllByRole("checkbox")
        .map((tool) => tool.closest("label")?.textContent),
    ).toEqual(["Read files", "Write files", "Edit files"]);
    expect(box("Run commands")).toHaveAccessibleDescription(
      "Run commands in the task's computer",
    );
    expect(box("Read files")).toHaveAccessibleDescription(
      "Read files in the task's workspace",
    );
    for (const tool of within(tools()).getAllByRole("checkbox"))
      expect(tool).toBeChecked();
    expect(within(tools()).getAllByRole("checkbox")).toHaveLength(15);
  });

  it("marks All as mixed while only some are ticked, and ticks or clears every tool", async () => {
    showProfiles(
      fakeProfiles({ profiles: [] }).api,
      choices,
      member,
      router,
      "/settings/agent-profiles/new",
    );
    await screen.findByRole("heading", { name: "Add an agent profile" });
    const all = box("All");
    fireEvent.click(box("Run commands"));
    expect(all).not.toBeChecked();
    expect(all).toHaveProperty("indeterminate", true);
    fireEvent.click(all);
    for (const tool of within(tools()).getAllByRole("checkbox"))
      expect(tool).toBeChecked();
    expect(all).toHaveProperty("indeterminate", false);
    fireEvent.click(all);
    for (const tool of within(tools()).getAllByRole("checkbox"))
      expect(tool).not.toBeChecked();
  });

  // A save goes back to the list from a promise, outside any act(): React shows the list first and runs the gone page's
  // effect clean-ups a moment later. A click in that moment (a quick reader, or a test on a loaded host: the check's load
  // flake) must reach the list's link, never the gone page's leave guard.
  it("lets the list's links work the moment it shows after a save", async () => {
    const fake = fakeProfiles({ profiles: [REVIEW] });
    showProfiles(
      fake.api,
      choices,
      member,
      router,
      `/settings/agent-profiles/${REVIEW.id}`,
    );
    await screen.findByRole("heading", { name: "Edit Code review" });
    fireEvent.click(box("All"));
    fireEvent.click(box("Read files"));
    // The moment the list is in the page: before React's own work after it runs.
    const listed = new Promise<void>((resolve) => {
      const observer = new MutationObserver(() => {
        if (!screen.queryByText("Saved agent profile Code review.")) return;
        observer.disconnect();
        resolve();
      });
      observer.observe(document.body, {
        childList: true,
        subtree: true,
        characterData: true,
      });
    });
    save();
    const scope = globalThis as {
      IS_REACT_ACT_ENVIRONMENT?: boolean | undefined;
    };
    const acting = scope.IS_REACT_ACT_ENVIRONMENT;
    scope.IS_REACT_ACT_ENVIRONMENT = false;
    try {
      await listed;
    } finally {
      scope.IS_REACT_ACT_ENVIRONMENT = acting;
    }
    fireEvent.click(
      screen.getByRole("link", { name: "Edit agent profile Code review" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Edit Code review" }),
    ).toBeVisible();
  });

  it("saves every tool as no list, some as the list, and refuses none", async () => {
    const fake = fakeProfiles({ profiles: [REVIEW] });
    showProfiles(
      fake.api,
      choices,
      member,
      router,
      `/settings/agent-profiles/${REVIEW.id}`,
    );
    await screen.findByRole("heading", { name: "Edit Code review" });
    fireEvent.click(box("All"));
    save();
    expect(
      await within(page()).findByText(
        "Choose at least one tool, or all of them.",
      ),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    fireEvent.click(box("Read files"));
    fireEvent.click(box("Search file contents"));
    save();
    expect(
      await screen.findByText("Saved agent profile Code review."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields.tools).toEqual(["read_file", "grep"]);
    fireEvent.click(
      screen.getByRole("link", { name: "Edit agent profile Code review" }),
    );
    await screen.findByRole("heading", { name: "Edit Code review" });
    expect(box("Read files")).toBeChecked();
    expect(box("Write files")).not.toBeChecked();
    fireEvent.click(box("All"));
    save();
    expect(
      await screen.findByText("Saved agent profile Code review."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields.tools).toBeNull();
  });
});

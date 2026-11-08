import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Workspace } from "../protocol/generated/api";
import { GB, sharedWorkspace, workspace } from "../test/workspaces";

import { WorkspaceCard } from "./WorkspaceCard";

function show(value: Workspace, source?: string) {
  const onAction = vi.fn();
  render(
    <WorkspaceCard workspace={value} source={source} onAction={onAction} />,
  );
  return { card: screen.getByRole("article"), onAction };
}
const lease = (title: string | null, running: boolean) => ({
  conversation_id: "0195d8c0-0000-7000-8000-0000000000c1",
  title,
  running,
  pinned: false,
});

// UI-GUIDELINES §7 Workspace card: name, owner, size against the quota, last use, the lease, the badge.
describe("WEB-U-025 workspace card", () => {
  it("shows an idle workspace of mine with its size against the quota", () => {
    const { card } = show(workspace({ name: "Website copy" }));
    expect(
      within(card).getByRole("heading", { name: "Website copy" }),
    ).toBeInTheDocument();
    expect(card).toHaveTextContent("Idle · not used yet");
    expect(card).toHaveTextContent("3.1 GB of 10 GB");
    const meter = within(card).getByRole("meter", { name: "Size" });
    expect(meter).toHaveAttribute("aria-valuetext", "3.1 GB of 10 GB");
    expect(meter).toHaveAttribute("data-level", "ok");
    // Nothing shared, so no badge and no owner line.
    expect(card.querySelector(".status-pill")).toBeNull();
    expect(card).not.toHaveTextContent("Owner");
  });

  it("names my own task holding it and pulses only while a run holds it", () => {
    const running = show(
      workspace({ lease: lease("Refactor the report builder", true) }),
    ).card;
    const line = running.querySelector(".workspace-lease");
    expect(line).toHaveTextContent("In use by “Refactor the report builder”");
    expect(line).toHaveAttribute("data-lease", "running");
  });

  it("says another task when the holder is not mine and its run is going", () => {
    const { card } = show(
      sharedWorkspace("write", { lease: lease(null, true) }),
    );
    const line = card.querySelector(".workspace-lease");
    expect(line).toHaveTextContent("In use by another task");
    expect(line).toHaveAttribute("data-lease", "running");
  });

  // WEB-U-199 (a UI review before 17.0): a task that is done but whose environment is kept still holds the lease;
  // "In use" wasn't true there. Only a going run is in use; a kept environment is said quietly, as where it's open.
  it.each([
    ["Hello file", "Open in “Hello file”"],
    [null, "Open in another task"],
  ] as const)(
    "WEB-U-199 says a workspace is open in a task, quietly, once its run is done (%s)",
    (title, words) => {
      const { card } = show(workspace({ lease: lease(title, false) }));
      const line = card.querySelector(".workspace-lease");
      expect(line).toHaveTextContent(words);
      expect(line).not.toHaveTextContent(/In use/);
      expect(line).toHaveAttribute("data-lease", "held");
      expect(line).toHaveClass("muted");
    },
  );

  // WEB-U-200 (a UI review before 17.0): "0 B of 10 GB" for a workspace holding a file was an early measurement, taken
  // during the run before the file existed. A size is a past measurement: it always says when it was taken.
  it("WEB-U-200 says when a workspace's size was measured, with or without a quota", async () => {
    for (const quota of [10 * GB, null]) {
      const { card } = show(
        workspace({
          size_bytes: 0,
          quota_bytes: quota,
          size_measured_at: new Date(Date.now() - 2 * 3_600_000).toISOString(),
        }),
      );
      const measured = card.querySelector(".workspace-measured");
      expect(measured).not.toBeNull();
      await waitFor(() =>
        expect(measured).toHaveTextContent(/^Measured 2 hr\. ago$/),
      );
      cleanup();
    }
    // Never measured: no size and no time.
    const { card } = show(
      workspace({ size_bytes: null, size_measured_at: null }),
    );
    expect(card).toHaveTextContent("Size not measured yet");
    expect(card.querySelector(".workspace-measured")).toBeNull();
  });

  it("badges a shared workspace of mine with its member count", () => {
    const { card } = show(workspace({ share_count: 2 }));
    expect(within(card).getByText("Shared · 2")).toHaveClass("status-pill");
  });

  it("shows the owner and the access of a workspace shared with me", () => {
    const { card } = show(sharedWorkspace("write", { name: "Team handbook" }));
    expect(card).toHaveTextContent("Owner: ana@example.invalid");
    expect(within(card).getByText("Can edit")).toHaveClass("status-pill");
  });

  it("offers a read-only share only Open and Fork", () => {
    const { card } = show(sharedWorkspace("read", { name: "Q3 planning" }));
    expect(within(card).getByText("Can view")).toBeInTheDocument();
    const names = within(card)
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label") ?? button.textContent);
    expect(names).toEqual(["Open Q3 planning", "More actions for Q3 planning"]);
    fireEvent.click(
      within(card).getByRole("button", {
        name: "More actions for Q3 planning",
      }),
    );
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Make a copy"]);
  });

  // UI-GUIDELINES §7: one Open action, the rest in a menu named after the workspace, by access.
  it("keeps Open visible and the owner's other actions in the menu", () => {
    const { card, onAction } = show(workspace({ name: "Sales analysis" }));
    fireEvent.click(
      within(card).getByRole("button", { name: "Open Sales analysis" }),
    );
    expect(onAction).toHaveBeenLastCalledWith("open");
    const actions = [
      ["New task here", "newTask"],
      ["Make a copy", "fork"],
      ["Share", "share"],
      ["Rename", "rename"],
      ["Delete", "delete"],
    ] as const;
    for (const [label, action] of actions) {
      fireEvent.click(
        within(card).getByRole("button", {
          name: "More actions for Sales analysis",
        }),
      );
      expect(
        screen.getAllByRole("menuitem").map((item) => item.textContent),
      ).toEqual(actions.map(([name]) => name));
      fireEvent.click(screen.getByRole("menuitem", { name: label }));
      expect(onAction).toHaveBeenLastCalledWith(action);
      // Focus is back on the menu's button, so a dialog the action opens returns it there (UI-GUIDELINES §9).
      expect(
        within(card).getByRole("button", {
          name: "More actions for Sales analysis",
        }),
      ).toHaveFocus();
    }
  });

  it("warns near the quota and says almost full at the danger line", () => {
    const near = show(workspace({ size_bytes: Math.round(8.6 * GB) })).card;
    expect(within(near).getByRole("meter")).toHaveAttribute(
      "data-level",
      "warn",
    );
    expect(near).not.toHaveTextContent("almost full");
  });

  it("says almost full at 95 %", () => {
    const { card } = show(workspace({ size_bytes: Math.round(9.7 * GB) }));
    expect(within(card).getByRole("meter")).toHaveAttribute(
      "data-level",
      "danger",
    );
    expect(card).toHaveTextContent("9.7 GB of 10 GB · almost full");
  });

  it("shows the size alone without a quota", () => {
    const { card } = show(workspace({ quota_bytes: null }));
    expect(within(card).queryByRole("meter")).toBeNull();
    expect(card).toHaveTextContent("3.1 GB");
  });

  it("shows a fork being copied as a shimmer, not an empty meter, with no actions yet", () => {
    const { card } = show(
      workspace({
        name: "Sales analysis (copy)",
        state: "forking",
        size_bytes: null,
        fork: {
          from: "0195d8c0-0000-7000-8000-0000000000aa",
          consistent: true,
        },
      }),
      "Sales analysis",
    );
    expect(card).toHaveTextContent("Copying “Sales analysis”…");
    expect(within(card).queryByRole("meter")).toBeNull();
    expect(card.querySelector(".skeleton")).not.toBeNull();
    expect(within(card).queryAllByRole("button")).toEqual([]);
    expect(within(card).getByText("Copy")).toHaveClass("status-pill");
  });

  it("offers only delete for a fork that failed", () => {
    const { card } = show(workspace({ name: "Broken", state: "failed" }));
    expect(card).toHaveTextContent("The copy failed.");
    const names = within(card)
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label") ?? button.textContent);
    expect(names).toEqual(["Delete Broken"]);
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { task } from "../test/workspaces";

import { TaskRow, type LeadRow } from "./TaskRow";

afterEach(cleanup);

const id = (n: number) =>
  `0195d8c0-0000-7000-8000-${String(n).padStart(12, "0")}`;

function show(row: ReactNode) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ul>{row}</ul>
    </QueryClientProvider>,
  );
  const link = screen.getByRole("link");
  const item = link.closest("li");
  if (!item) throw new Error("A task row is a list item");
  return { item, link };
}

const lead: LeadRow = {
  count: { words: "2", label: "2 helpers", tone: "muted" },
  waitingForYou: true,
  open: false,
  onToggle: () => undefined,
};

// WEB-U-449 (UI-GUIDELINES §7 Conversation list item): the markup the row's stylesheet relies on. The ⋯ is the row's own
// part, its holder a child of the row after the link, so it can sit inside the row's end; the marks at the link's end
// (status, waiting dot, helper count) share one class, so they give way under the ⋯ together; a row without a ⋯ (a
// search's) has nothing for its title to give way to.
describe("WEB-U-449 the task row's ⋯ is part of the row", () => {
  it("puts the ⋯ in the row, after its link, and marks the link's end", () => {
    const { item, link } = show(
      <TaskRow
        conversation={task(id(1), "w", {
          title: "Fix the date range",
          run_status: "running",
        })}
        current={false}
        workspace=""
        onAction={() => undefined}
      />,
    );
    const parts = [...item.children];
    expect(parts.map((part) => part.className)).toEqual([
      "conversation-link",
      "icon-action task-more",
    ]);
    const more = screen.getByRole("button", {
      name: "More actions for Fix the date range",
    });
    expect(more.parentElement).toBe(parts[1]);
    expect(more).toHaveAttribute("aria-haspopup", "menu");
    // The status is the link's last part, a mark.
    expect(link.lastElementChild).toHaveClass("task-status", "task-mark");
  });

  it("keeps a lead's chevron first and its ⋯ last, with its waiting dot and count as marks", () => {
    const { item, link } = show(
      <TaskRow
        conversation={task(id(2), "w", { title: "Report on Q3" })}
        current
        workspace=""
        lead={lead}
        onAction={() => undefined}
      />,
    );
    expect([...item.children].map((part) => part.className)).toEqual([
      "task-helpers-toggle",
      "conversation-link",
      "icon-action task-more",
    ]);
    expect(
      [...link.querySelectorAll(":scope > .task-mark")].map(
        (mark) => mark.classList[0],
      ),
    ).toEqual(["task-waiting", "type-small"]);
    expect(link.querySelector(":scope > .task-helpers-count")).toHaveClass(
      "task-mark",
    );
  });

  it("gives a search's row no ⋯, so its title never gives way", () => {
    const { item } = show(
      <TaskRow
        conversation={task(id(3), "w", { title: "Fix the date range" })}
        current={false}
        workspace=""
        withMenu={false}
        onAction={() => undefined}
      />,
    );
    expect(item.querySelector(".task-more")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });
});

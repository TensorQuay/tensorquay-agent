import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import en from "../i18n/locales/en.json";
import type { Conversation } from "../protocol/generated/api";
import { fakeGateway, me, task, workspace } from "../test/workspaces";

import { ConversationList } from "./ConversationList";
import { taskSearch } from "./task-search";

const OPEN = "0195d8c0-0000-7000-8000-00000000a0e1";
const home = workspace({ name: "billing-service" });
vi.mock("next/navigation", () => ({ usePathname: () => `/c/${OPEN}` }));

afterEach(() => {
  taskSearch.reset();
  vi.unstubAllGlobals();
});

/** Nine tasks, so the list offers its search; the open one is working. */
function show() {
  const items: Conversation[] = [
    task(OPEN, home.id, { title: "The open task", run_status: "running" }),
    ...Array.from({ length: 8 }, (_, index) =>
      task(`0195d8c0-0000-7000-8000-0000000000${10 + index}`, home.id, {
        title: `Another task ${index + 1}`,
      }),
    ),
  ];
  vi.stubGlobal(
    "fetch",
    fakeGateway({
      "GET /api/v1/conversations": () =>
        Response.json({ items, next_cursor: null }),
      "GET /api/v1/workspaces": () => Response.json({ items: [home] }),
    }).fetch,
  );
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], me);
  render(
    <QueryClientProvider client={cache}>
      <ConversationList />
    </QueryClientProvider>,
  );
}

// WEB-U-435: the collapsed sidebar is a rail of global actions (UI-GUIDELINES §6). The task list gives it two things: the
// Search row, an icon there, named by the app's own tooltip; and the open task, the one link it marks
// current, whose icon carries its run mark. The rail shows that link alone (its style; WEB-E2E-102 in the browser).
describe("WEB-U-435 what the task list gives the rail", () => {
  it("gives the Search row the app's own tooltip where its label is hidden, never a native title", async () => {
    show();
    const row = await screen.findByRole("button", {
      name: en.messages["list.search"],
    });
    expect(row).not.toHaveAttribute("title");
    // In the test's layout nothing has a size, as the label hidden on the rail has none.
    fireEvent.pointerEnter(row);
    expect(document.querySelector(".action-tooltip")).toHaveTextContent(
      en.messages["list.search"] ?? "",
    );
    fireEvent.pointerLeave(row);
  });

  it("marks the open task's link alone as current, its run mark in it", async () => {
    show();
    const tasks = screen.getByRole("region", { name: "Tasks" });
    await within(tasks).findByRole("link", { name: /The open task/ });
    const current = within(tasks)
      .getAllByRole("link")
      .filter((link) => link.getAttribute("aria-current") === "page");
    expect(current).toHaveLength(1);
    expect(current[0]).toHaveAccessibleName(/^The open task/);
    // The one run mark (17.3, WEB-U-441): the same calm dot as the bar's, never a spinner.
    expect(
      current[0]?.querySelector(".task-status > .run-mark"),
    ).not.toBeNull();
    expect(current[0]?.querySelector(".task-status svg")).toBeNull();
  });
});

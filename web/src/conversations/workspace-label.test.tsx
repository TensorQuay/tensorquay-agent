import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fakeGateway, me, task, workspace } from "../test/workspaces";
import { taskFilter } from "../workspaces/task-filter";

import { ConversationList } from "./ConversationList";

vi.mock("next/navigation", () => ({ usePathname: () => "/" }));

const sales = workspace({ name: "Sales analysis" });
const own = workspace({ name: "Summarise the Q3 files" });
const onSales = task("0195d8c0-0000-7000-8000-0000000000e1", sales.id, {
  title: "Refactor the report builder",
});
const onOwn = task("0195d8c0-0000-7000-8000-0000000000e2", own.id, {
  title: "Summarise the Q3 files",
});
function show() {
  const gateway = fakeGateway({
    "GET /api/v1/conversations": () =>
      Response.json({ items: [onSales, onOwn], next_cursor: null }),
    "GET /api/v1/workspaces": () => Response.json({ items: [sales, own] }),
  });
  vi.stubGlobal("fetch", gateway.fetch);
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
beforeEach(() => taskFilter.clear());
afterEach(() => {
  vi.unstubAllGlobals();
});
const rows = () =>
  within(screen.getByRole("region", { name: "Tasks" }))
    .getAllByRole("link")
    // The tooltip's title and workspace; its last part is the full time, which depends on the time zone.
    .map((link) => link.getAttribute("title")?.replace(/ · [^·]+$/, ""));

// Task 076: the task list names a task's workspace and filters by one (answers-076 Q8); since UI-GUIDELINES §7 the name is on
// hover, in the row's title, never a second line.
describe("WEB-U-025 task list and workspaces", () => {
  it("names the workspace on hover when it is not the task's own", async () => {
    show();
    const link = await screen.findByRole("link", {
      name: /Refactor the report builder/,
    });
    await waitFor(() =>
      expect(link).toHaveAttribute(
        "title",
        expect.stringMatching(
          /^Refactor the report builder · Sales analysis · /,
        ),
      ),
    );
    expect(within(link).queryByText("Sales analysis")).toBeNull();
    const other = screen.getByRole("link", { name: /Summarise the Q3 files/ });
    expect(other).toHaveAttribute(
      "title",
      expect.stringMatching(/^Summarise the Q3 files · /),
    );
  });

  it("filters to one workspace with a removable chip", async () => {
    show();
    await screen.findByRole("link", { name: /Refactor/ });
    act(() => taskFilter.set({ id: sales.id, name: "Sales analysis" }));
    expect(rows()).toEqual(["Refactor the report builder · Sales analysis"]);
    const chip = screen.getByRole("button", {
      name: "Remove filter Workspace: Sales analysis",
    });
    expect(chip).toHaveTextContent("Workspace: Sales analysis");
    fireEvent.click(chip);
    expect(taskFilter.current()).toBeNull();
    expect(rows()).toHaveLength(2);
  });

  it("says so when the workspace has no tasks of mine", async () => {
    show();
    await screen.findByRole("link", { name: /Refactor/ });
    act(() => taskFilter.set({ id: "other", name: "Empty" }));
    expect(
      screen.getByText("No tasks on this workspace yet."),
    ).toBeInTheDocument();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { StartProvider } from "../conversations/StartProvider";
import type { Conversation } from "../protocol/generated/api";
import { fakeGateway, me, task, workspace } from "../test/workspaces";

import { Shell } from "./Shell";
import { setSidebar } from "./sidebar";
import { TabTitle } from "./TabTitle";

vi.mock("next/navigation", () => ({
  usePathname: () => "/workspaces",
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
}));

const home = workspace();

/** The list the sidebar loads, already in the cache; anything else the shell asks for is refused. */
function tasks(statuses: NonNullable<Conversation["run_status"]>[]) {
  vi.stubGlobal("fetch", fakeGateway({}).fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], me);
  cache.setQueryData(["conversations"], {
    pages: [
      {
        items: statuses.map((run_status, index) =>
          task(`0195d8c0-0000-7000-8000-00000000000${index}`, home.id, {
            run_status,
          }),
        ),
        next_cursor: null,
      },
    ],
    pageParams: [""],
  });
  return cache;
}
beforeEach(() => {
  delete document.documentElement.dataset.sidebar;
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: false,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  document.title = "";
});

// Nothing waits for the reader (PRD FR-10, superseded: no approvals), so nothing is counted: the tab title is the
// page's own, and the rail's expand button has no badge. A status the gateway no longer sends counts for nothing.
describe("WEB-U-071 no task waits for the reader", () => {
  it("keeps the tab title plain and the rail's expand button without a count, whatever the runs", async () => {
    const cache = tasks([
      "running",
      "failed",
      "waiting" as NonNullable<Conversation["run_status"]>,
      "completed",
    ]);
    render(
      <QueryClientProvider client={cache}>
        <StartProvider>
          <Shell>
            <TabTitle>Workspaces · TensorQuay Agent</TabTitle>
          </Shell>
        </StartProvider>
      </QueryClientProvider>,
    );
    await act(async () => {});
    expect(document.title).toBe("Workspaces · TensorQuay Agent");
    act(() => setSidebar("rail"));
    expect(
      screen.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();
    expect(document.querySelector(".waiting-badge")).toBeNull();
  });
});

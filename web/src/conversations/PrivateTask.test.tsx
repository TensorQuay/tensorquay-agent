import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import { LIMITS } from "../test/user";

import { ConversationPage } from "./ConversationPage";

const stamp = "2026-01-01T00:00:00Z";
const admin = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const owner = "0195d8c0-0000-7000-8000-000000000002";
const task: Conversation = {
  id: "0195d8c0-0000-7000-8000-0000000000a1",
  owner_id: owner,
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "message",
  backend: "docker",
  state: "active",
  created_at: stamp,
  updated_at: stamp,
  last_activity_at: stamp,
  workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
  mcp_servers: [],
  connectors: [],
  team: null,
};
const target = (input: RequestInfo | URL) =>
  typeof input === "string"
    ? input
    : input instanceof URL
      ? input.href
      : input.url;
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  fetch.mockImplementation((input) => {
    const url = target(input);
    if (url === "/api/v1/auth/me") return Promise.resolve(Response.json(admin));
    if (url === `/api/v1/conversations/${task.id}`)
      return Promise.resolve(Response.json(task));
    if (url === "/api/v1/users")
      return Promise.resolve(
        Response.json({
          items: [
            {
              ...admin,
              disabled: false,
              created_at: stamp,
              last_active_at: null,
            },
            {
              id: owner,
              email: "colleague@example.invalid",
              role: "member",
              disabled: false,
              created_at: stamp,
              last_active_at: null,
            },
          ],
        }),
      );
    throw new Error(`Unexpected request ${url}`);
  });
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

// Task 067 / SEC-005: an administrator sees another user's task as metadata only - no content, no actions.
describe("WEB-U-023 private task", () => {
  it("shows the owner and details, and never opens the transcript", async () => {
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={cache}>
        <ConversationPage id={task.id} />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByRole("heading", { level: 1, name: "Private task" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText(
        "This task belongs to colleague@example.invalid. Administrators can see its details, not its content.",
      ),
    ).toBeInTheDocument();
    const details = screen.getByRole("region", { name: "Task details" });
    expect(within(details).getByText("Owner")).toBeInTheDocument();
    expect(within(details).getByText("workstation")).toBeInTheDocument();
    expect(within(details).getByText("local")).toBeInTheDocument();
    expect(within(details).getByText("Active")).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByRole("button", { name: "Rename task" })).toBeNull();
    expect(
      fetch.mock.calls.some(([url]) => target(url).includes("/events")),
    ).toBe(false);
    expect(document.title).toBe("Private task · TensorQuay Agent");
  });
});

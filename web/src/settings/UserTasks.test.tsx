import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation, User } from "../protocol/generated/api";
import { templates } from "../test/templates";
import { LIMITS } from "../test/user";

import { UserTasks } from "./UserTasks";

const stamp = "2026-01-01T00:00:00Z";
const me: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const colleague = "0195d8c0-0000-7000-8000-000000000002";
const task = (id: string, owner = colleague): Conversation => ({
  id,
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
});
const target = (input: RequestInfo | URL) =>
  typeof input === "string"
    ? input
    : input instanceof URL
      ? input.href
      : input.url;
const fetch = vi.fn<typeof globalThis.fetch>();
function serve(tasks: Conversation[]) {
  fetch.mockImplementation((input) => {
    const url = target(input);
    if (url === "/api/v1/users")
      return Promise.resolve(
        Response.json({
          items: [
            { ...me, disabled: false, created_at: stamp, last_active_at: null },
            {
              id: colleague,
              email: "colleague@example.invalid",
              role: "member",
              disabled: false,
              created_at: stamp,
              last_active_at: null,
            },
          ],
        }),
      );
    if (url === "/api/v1/templates")
      return Promise.resolve(Response.json(templates));
    if (url === "/api/v1/models")
      return Promise.resolve(
        Response.json({
          items: [
            {
              name: "local",
              display_name: "Local model",
              description: null,
              verified: true,
              model: "local-model",
              protocol: "openai_chat",
              context_window: null,
              vision: false,
              native_tools: true,
              reasoning: null,
              runs_on: { where: "this_computer", provider: null },
            },
          ],
        }),
      );
    if (url === "/api/v1/conversations?all=true")
      return Promise.resolve(
        Response.json({ items: tasks, next_cursor: null }),
      );
    throw new Error(`Unexpected request ${url}`);
  });
}
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});
function show(user: User = me) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <UserTasks me={user} id={colleague} />
    </QueryClientProvider>,
  );
}

// Task 067 Q4: a person's tasks as metadata rows, each opening the private view; nothing from the tasks' content.
describe("WEB-U-023 a person's tasks", () => {
  it("lists only that person's tasks as metadata with links to the private view", async () => {
    serve([
      task("0195d8c0-0000-7000-8000-0000000000a1"),
      task("0195d8c0-0000-7000-8000-0000000000a2"),
      { ...task("0195d8c0-0000-7000-8000-0000000000a3", me.id), title: "Mine" },
    ]);
    show();
    expect(
      await screen.findByRole("heading", {
        name: "Tasks of colleague@example.invalid",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Administrators see a task’s details, never its content.",
      ),
    ).toBeInTheDocument();
    const table = screen.getByRole("table");
    const links = within(table).getAllByRole("link");
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/c/0195d8c0-0000-7000-8000-0000000000a1",
      "/c/0195d8c0-0000-7000-8000-0000000000a2",
    ]);
    expect(within(table).queryByText("Mine")).toBeNull();
    expect(screen.getByRole("link", { name: "People" })).toHaveAttribute(
      "href",
      "/settings/people",
    );
  });

  // The dev screen review (011e8972), #3: every row was named "Private task", and the environment and model were raw
  // ids. The created date opens the details; the environment and model read as everywhere else.
  it("WEB-U-172 names no row Private task, and shows the environment and model by name", async () => {
    serve([
      task("0195d8c0-0000-7000-8000-0000000000a1"),
      {
        ...task("0195d8c0-0000-7000-8000-0000000000a2"),
        template: "retired-image",
        model_route: "gone",
      },
    ]);
    show();
    const table = await screen.findByRole("table");
    expect(within(table).queryByText("Private task")).toBeNull();
    expect(
      within(table).queryByRole("columnheader", { name: "Name" }),
    ).toBeNull();
    const [first, second] = within(table).getAllByRole("row").slice(1);
    await waitFor(() =>
      expect(within(first!).getByText("Workstation")).toBeVisible(),
    );
    expect(await within(first!).findByText("Local model")).toBeVisible();
    expect(within(first!).queryByText("workstation")).toBeNull();
    // A name the lists no longer have stands as its id, set as one.
    expect(within(second!).getByText("retired-image").tagName).toBe("CODE");
    expect(within(second!).getByText("gone").tagName).toBe("CODE");
    expect(within(first!).getByRole("link")).toHaveAttribute(
      "href",
      "/c/0195d8c0-0000-7000-8000-0000000000a1",
    );
    // The fix's review: same-day tasks must not share one link name (WCAG 2.4.4); the details tell them apart, no title.
    expect(within(first!).getByRole("link")).toHaveAccessibleName(
      "Task created Jan 1, 2026, Workstation, Local model",
    );
  });

  // The fix's review: the ids flashed before the names arrived. The table waits for both lists; a list that fails
  // leaves the ids, set as code, rather than waiting forever.
  it("WEB-U-172 shows no raw id while the names load, and the ids if a list fails", async () => {
    serve([task("0195d8c0-0000-7000-8000-0000000000a1")]);
    const answer = fetch.getMockImplementation() as typeof globalThis.fetch;
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => (release = resolve));
    fetch.mockImplementation(async (input, init) => {
      if (target(input) === "/api/v1/templates") {
        await held;
        return Response.json(
          { error: { code: "unavailable", message: "x", retryable: true } },
          { status: 503 },
        );
      }
      return answer(input, init);
    });
    show();
    await screen.findByRole("heading", {
      name: "Tasks of colleague@example.invalid",
    });
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByText("workstation")).toBeNull();
    release();
    expect((await screen.findByText("workstation")).tagName).toBe("CODE");
  });

  it("says when the person has no tasks", async () => {
    serve([]);
    show();
    expect(await screen.findByText("No tasks.")).toBeInTheDocument();
  });

  it("is for administrators only", () => {
    show({ ...me, role: "member" });
    expect(
      screen.getByText("Only administrators can manage users."),
    ).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalled();
  });
});

// WEB-U-357 (the 16f page pass, E4): an id that isn't one of this installation's people reads as not found, with the
// way back, never an endless skeleton.
describe("WEB-U-357 a person who isn't here", () => {
  it("says so once the people are read", async () => {
    serve([]);
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={cache}>
        <UserTasks me={me} id="0195d8c0-0000-7000-8000-0000000000ff" />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText("This person isn't in this installation."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Loading")).toBeNull();
    expect(screen.getByRole("link", { name: "People" })).toHaveAttribute(
      "href",
      "/settings/people",
    );
  });
});

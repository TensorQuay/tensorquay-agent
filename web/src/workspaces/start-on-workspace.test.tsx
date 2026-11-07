import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NewConversation } from "../conversations/NewConversation";
import type { Workspace } from "../protocol/generated/api";
import { addFromPlus, openChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { fakeGateway, me, task, workspace } from "../test/workspaces";

/** Opens New task's Change panel, then finds a picker in it by name. */
async function pick(name: string) {
  await openChoices();
  return screen.findByRole("button", { name });
}

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  usePathname: () => "/",
}));

const models = {
  items: [
    {
      name: "local",
      model: "local-model",
      protocol: "openai_chat",
      display_name: null,
      verified: null,
      context_window: 32768,
      vision: false,
      native_tools: true,
      runs_on: { where: "hosted", provider: null },
      description: null,
      reasoning: null,
    },
  ],
};
function show(items: Workspace[], chosen?: string) {
  const created = task(
    "0195d8c0-0000-7000-8000-0000000000f1",
    chosen ?? "0195d8c0-0000-7000-8000-0000000000f1",
  );
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json(me),
    "GET /api/v1/models": () => Response.json(models),
    "GET /api/v1/templates": () => Response.json(templates),
    "GET /api/v1/workspaces": () => Response.json({ items }),
    "POST /api/v1/conversations": () => Response.json(created, { status: 201 }),
    [`POST /api/v1/conversations/${created.id}/messages`]: () =>
      Response.json(
        {
          accepted: true,
          conversation_id: created.id,
          message_id: "1",
          held: false,
        },
        { status: 202 },
      ),
  });
  vi.stubGlobal("fetch", gateway.fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <NewConversation workspace={chosen} />
    </QueryClientProvider>,
  );
  return gateway;
}
afterEach(() => {
  vi.unstubAllGlobals();
  push.mockReset();
});
const busy = workspace({
  name: "Data pipeline",
  lease: {
    conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
    title: null,
    running: true,
    pinned: false,
  },
});

// Task 076: a new task on a chosen workspace; a busy one says once that the task will wait (mockup change 3).
describe("WEB-U-025 starting a task on a workspace", () => {
  it("starts on a new workspace by default, with no hint", async () => {
    const { calls } = show([busy]);
    // A new workspace is the default: no workspace chip until "+" adds one (the UI standard's composer).
    await pick("Environment: Workstation");
    expect(screen.queryByRole("button", { name: /^Workspace:/ })).toBeNull();
    expect(
      screen.queryByText("In use: your task will wait for it."),
    ).toBeNull();
    await pick("Environment: Workstation");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Hello" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(
      calls.find((call) => call.url === "/api/v1/conversations")?.body,
    ).not.toHaveProperty("workspace_id");
  });

  it("binds a chosen busy workspace and says the task will wait", async () => {
    const { calls } = show([busy]);
    // "+" adds the workspace chip, its picker open at once.
    expect(await addFromPlus("Workspace…")).toHaveAccessibleName(
      "Workspace: New workspace",
    );
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: /Data pipeline/ }),
    );
    expect(
      screen.getByText("In use: your task will wait for it."),
    ).toBeInTheDocument();
    await pick("Environment: Workstation");
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Hello" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(
      calls.find((call) => call.url === "/api/v1/conversations")?.body,
    ).toMatchObject({ workspace_id: busy.id });
  });

  // WEB-U-107: the status belongs to the chip it is about, not the foot of the page.
  it("says a busy workspace's status on its own chip, and its picker reads it", async () => {
    show([busy], busy.id);
    const picker = await pick("Workspace: Data pipeline");
    const note = screen.getByText("In use: your task will wait for it.");
    expect(note.closest(".composer-chip-item")).toBe(
      picker.closest(".composer-chip-item"),
    );
    expect(note.closest(".composer-chip")).toBeNull();
    expect(picker).toHaveAccessibleDescription(
      expect.stringContaining("In use: your task will wait for it."),
    );
    expect(document.querySelector(".start-hint")).toBeNull();
  });

  // WEB-U-128 (candidate12 F2): with a workspace that holds files, the suggestions work on its files.
  it("suggests work on the files of a chosen workspace that has some", async () => {
    const sales = workspace({ name: "Sales analysis" });
    show([sales], sales.id);
    expect(await pick("Workspace: Sales analysis")).toBeInTheDocument();
    for (const example of [
      "Explain the files in this workspace",
      "Clean up a CSV file and chart its totals",
      "Draft a one-page brief from the notes in this workspace",
    ])
      expect(screen.getByRole("button", { name: example })).toBeVisible();
    expect(
      screen.queryByRole("button", {
        name: "Draft a one-page plan for launching a newsletter",
      }),
    ).toBeNull();
  });

  it("suggests work from nothing with a chosen workspace that is empty", async () => {
    const empty = workspace({ name: "Fresh start", size_bytes: 0 });
    show([empty], empty.id);
    expect(await pick("Workspace: Fresh start")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Draft a one-page plan for launching a newsletter",
      }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", {
        name: "Explain the files in this workspace",
      }),
    ).toBeNull();
  });

  it("preselects the workspace it was started on", async () => {
    const sales = workspace({ name: "Sales analysis" });
    show([sales], sales.id);
    // Started on a workspace, the task shows it as a chip in the composer, with its ×.
    expect(await pick("Workspace: Sales analysis")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Remove workspace Sales analysis" }),
    ).toBeVisible();
  });

  it("waits for a fork being copied before sending", async () => {
    const copy = workspace({ name: "Copy", state: "forking" });
    show([copy], copy.id);
    await pick("Workspace: Copy");
    await pick("Environment: Workstation");
    expect(
      await screen.findByText(
        "The copy is still being made. You can start once it is ready.",
      ),
    ).toBeInTheDocument();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import en from "../i18n/locales/en.json";
import type { Conversation } from "../protocol/generated/api";
import { conversationId, runId } from "../test/frames";
import { openTaskChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { me, shellData, workspace } from "../test/workspaces";
import { emptyTranscript } from "../transcript/reducer";

import { ConversationControls } from "./ConversationControls";
import { TaskChoices } from "./TaskChoices";

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push, replace: vi.fn() }),
  usePathname: () => "/",
}));

const conversation: Conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Workspace review",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "message",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
  mcp_servers: [],
  connectors: [],
  team: null,
};
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
function page(
  working = false,
  shown: Conversation = conversation,
  workspaceName: string | null = null,
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  // The task's workspace, as the gateway lists it: a task's own has the task's id and its title for a name.
  shellData(cache, [
    workspace({
      id: shown.workspace_id,
      name: workspaceName ?? shown.title ?? "Untitled task",
    }),
  ]);
  cache.setQueryData(["templates"], templates);
  cache.setQueryData(["models"], {
    items: ["local", "second"].map((name) => ({
      name,
      model: name === "local" ? "First model" : "Second model",
      protocol: "openai_chat",
      context_window: null,
      runs_on: { where: "this_computer", provider: null },
      vision: false,
      reasoning: null,
    })),
  });
  const transcript = {
    ...emptyTranscript(),
    sandbox: "ready" as const,
    run: {
      id: working ? runId : null,
      status: working ? ("running" as const) : ("idle" as const),
    },
  };
  render(
    <QueryClientProvider client={cache}>
      {/* As the page has them: the task's choices in its header (WEB-U-113). */}
      <ConversationControls
        conversation={shown}
        transcript={transcript}
        choices={<TaskChoices conversation={shown} transcript={transcript} />}
      />
    </QueryClientProvider>,
  );
}

// WEB-U-441 (17.3): the one run mark is in the tab title too, while the task's run is under way.
describe("WEB-U-441 the tab title carries the run mark", () => {
  it("starts with ● while the run works", () => {
    page(true);
    expect(document.title).toBe("● Workspace review · TensorQuay Agent");
    expect(
      document.querySelector(".conversation-status > .run-mark"),
    ).not.toBeNull();
  });

  it("is plain while nothing runs", () => {
    page(false);
    expect(document.title).toBe("Workspace review · TensorQuay Agent");
    expect(document.querySelector(".run-mark")).toBeNull();
  });
});

describe("WEB-U-007 conversation bar", () => {
  // The design direction (task page, P2): an icon in the header says what it does on hover and on keyboard focus, in
  // a tooltip that also names the button, so the name is read once.
  // GW-I-046: an administrator sees another user's task without its title (titles are content).
  it("names a task without a title neutrally and offers no rename", () => {
    const untitled: Conversation = { ...conversation };
    delete untitled.title;
    page(false, untitled);
    expect(
      screen.getByRole("heading", { level: 1, name: "Private task" }),
    ).toBeInTheDocument();
    // Someone else's task: nothing to rename or delete, so no ⋯ at all.
    expect(
      screen.queryByRole("button", { name: /^More actions for/ }),
    ).not.toBeInTheDocument();
    expect(document.title).toBe("Private task · TensorQuay Agent");
  });

  // The owner's correction (WEB-U-113): the header holds the title, the status and the model and environment as two
  // small pickers, with no ⚙ and no Done.
  it("holds the title, the status and the model and environment pickers", async () => {
    page();
    const bar = screen.getByRole("region", { name: "Task header" });
    expect(
      within(bar).queryByRole("button", {
        name: "Change model or environment",
      }),
    ).toBeNull();
    const choices = await openTaskChoices();
    expect(bar).toContainElement(choices);
    expect(
      within(choices).getByRole("button", { name: "Model: First model" }),
    ).toBeVisible();
    expect(
      within(choices).getByRole("button", { name: /^Environment:/ }),
    ).toBeVisible();
    expect(within(choices).queryByRole("button", { name: "Done" })).toBeNull();
  });

  // Candidate12's workspaces suite: a task on a workspace made or shared (not its own) says which, quietly, under its
  // title, beside the agent profile it came from.
  it("names the workspace a task is in, quietly under its title", () => {
    page(false, conversation, "Team handbook");
    const bar = screen.getByRole("region", { name: "Task header" });
    expect(within(bar).getByText("In Team handbook")).toHaveClass("muted");
  });

  it("says nothing of a task's own workspace, named after the task", () => {
    page(false, { ...conversation, workspace_id: conversation.id });
    const bar = screen.getByRole("region", { name: "Task header" });
    expect(within(bar).queryByText(/^In /)).toBeNull();
  });

  it("names both the agent profile and the workspace on one quiet line", () => {
    page(
      false,
      {
        ...conversation,
        profile: {
          id: "p",
          version: 1,
          name: "analyst",
          display_name: "Analyst",
          sets: [],
        },
      },
      "Team handbook",
    );
    const bar = screen.getByRole("region", { name: "Task header" });
    expect(
      within(bar).getByText("Agent: Analyst · In Team handbook"),
    ).toBeVisible();
  });

  it("shows model names and changes the route through one keyed update", async () => {
    fetch.mockResolvedValue(
      Response.json({ ...conversation, model_route: "second" }),
    );
    page();
    const bar = screen.getByRole("region", { name: "Task header" });
    expect(
      within(bar).getByRole("heading", { name: "Workspace review" }),
    ).toBeVisible();
    expect(bar).not.toHaveTextContent(/local|sandbox|idle/i);
    fireEvent.click(
      within(await openTaskChoices()).getByRole("button", {
        name: "Model: First model",
      }),
    );
    fireEvent.click(
      // Its name, then its muted line (WEB-U-103).
      screen.getByRole("menuitemradio", { name: /^Second model/ }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Model: Second model" }),
      ).toBeVisible(),
    );
    const [url, options] = fetch.mock.calls[0] ?? [];
    expect(url).toBe(`/api/v1/conversations/${conversationId}`);
    expect(options?.method).toBe("PATCH");
    expect(options?.body).toBe(JSON.stringify({ route: "second" }));
    expect(new Headers(options?.headers).get("idempotency-key")).toMatch(
      /^[a-f0-9-]{36}$/,
    );
  });
  it("keeps model changes disabled with a reason during work", async () => {
    page(true);
    await openTaskChoices();
    const picker = screen.getByRole("button", { name: "Model: First model" });
    expect(picker).toHaveAttribute("aria-disabled", "true");
    expect(picker).toHaveAccessibleDescription(
      "Stop the current run before changing the model.",
    );
    fireEvent.click(picker);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(fetch).not.toHaveBeenCalled();
  });
  // The owner's "this edit on top sucks": the title itself becomes the field (the UI standard's In-place edit), from a
  // click on the title or the pencil, with no label, Save or Cancel; Esc puts it back and returns to the pencil.
  // The UI standard's header: the title at 16 px weight 600, the status, a quiet Files icon and one ⋯ (Rename, Delete
  // task); no pencil. Renaming still starts from a click on the title.
  it("has a 16/600 title and one ⋯ with Rename and Delete task, and no pencil", () => {
    page();
    expect(
      screen.getByRole("heading", { level: 1, name: "Workspace review" }),
    ).toHaveClass("type-task-title");
    expect(screen.queryByRole("button", { name: "Rename task" })).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Workspace review" }),
    );
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Rename task", "Delete task"]);
  });

  it("deletes the task from its ⋯ after asking, then goes to New task", async () => {
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    page();
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Workspace review" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete task" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete task" });
    expect(dialog).toHaveTextContent("Workspace review");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete task" }),
    );
    await vi.waitFor(() => expect(push).toHaveBeenCalledWith("/"));
    expect(fetch.mock.calls[0]?.[1]?.method).toBe("DELETE");
  });

  it("renames in place from a click on the title, and Esc puts it back", async () => {
    page();
    fireEvent.click(
      await screen.findByRole("heading", { name: "Workspace review" }),
    );
    const field = await screen.findByRole("textbox", { name: "Task title" });
    expect(field).toHaveValue("Workspace review");
    expect(field).toHaveFocus();
    expect(field).toHaveClass("inline-edit", "type-task-title");
    expect(screen.queryByRole("button", { name: "Save title" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
    fireEvent.change(field, { target: { value: "Something else" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(
      await screen.findByRole("heading", { name: "Workspace review" }),
    ).toBeVisible();
    // Back to the ⋯, the keyboard's way to rename.
    expect(
      screen.getByRole("button", { name: "More actions for Workspace review" }),
    ).toHaveFocus();
    expect(fetch).not.toHaveBeenCalledWith(
      expect.stringContaining(conversation.id),
      expect.objectContaining({ method: "PATCH" }),
    );
  });

  it("keeps the old title when the field is left empty", async () => {
    page();
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Workspace review" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename task" }));
    const field = await screen.findByRole("textbox", { name: "Task title" });
    fireEvent.change(field, { target: { value: "   " } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(
      await screen.findByRole("heading", { name: "Workspace review" }),
    ).toBeVisible();
  });

  it("renames with a trimmed title and retains the key after uncertain delivery", async () => {
    fetch
      .mockRejectedValueOnce(new Error("Lost response"))
      .mockResolvedValueOnce(
        Response.json({ ...conversation, title: "A new title" }),
      );
    page();
    fireEvent.click(screen.getByRole("heading", { name: "Workspace review" }));
    const field = await screen.findByRole("textbox", { name: "Task title" });
    fireEvent.change(field, { target: { value: " A new title " } });
    fireEvent.keyDown(field, { key: "Enter" });
    await screen.findByRole("alert");
    // The failed save keeps the text in the field, to try again.
    expect(screen.getByRole("textbox", { name: "Task title" })).toHaveValue(
      " A new title ",
    );
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Task title" }), {
      key: "Enter",
    });
    await screen.findByRole("heading", { name: "A new title" });
    expect(document.title).toBe("A new title · TensorQuay Agent");
    expect(fetch.mock.calls[0]?.[1]?.body).toBe(
      JSON.stringify({ title: "A new title" }),
    );
    expect(
      new Headers(fetch.mock.calls[0]?.[1]?.headers).get("idempotency-key"),
    ).toBe(
      new Headers(fetch.mock.calls[1]?.[1]?.headers).get("idempotency-key"),
    );
  });
});

// WEB-U-204 (FR-33's first UI): the keep-alive pin, which keeps a task's environment on between messages, is the task
// owner's toggle in its ⋯, saying what it costs; before it, only the API could set it.
describe("WEB-U-204 Keep environment on", () => {
  const mine = (pinned: boolean): Conversation => ({
    ...conversation,
    owner_id: me.id,
    pinned,
  });
  const open = () =>
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Workspace review" }),
    );
  const sent = () =>
    fetch.mock.calls.map(([url, init]) => [
      url instanceof Request ? url.url : url.toString(),
      init?.method,
      JSON.parse(
        typeof init?.body === "string" ? init.body : "null",
      ) as unknown,
    ]);

  it("is a checked toggle in the owner's ⋯, between Rename and Delete, saying what it costs", () => {
    page(false, mine(false));
    open();
    const toggle = screen.getByRole("menuitemcheckbox", {
      name: "Keep environment on",
    });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    // What it costs in one short muted line: at most 36 characters, so the menu keeps its 180 to 220 px (17.3).
    const cost = en.messages["task.keepOnNote"];
    expect(cost.length).toBeLessThanOrEqual(36);
    expect(toggle).toHaveAccessibleDescription(cost);
    const items = screen
      .getByRole("menu")
      .querySelectorAll('[role^="menuitem"]');
    expect([...items].map((item) => item.getAttribute("aria-label"))).toEqual([
      "Rename task",
      "Keep environment on",
      "Delete task",
    ]);
  });

  it("turns the pin on, and off again, through one keyed update each", async () => {
    fetch.mockResolvedValueOnce(Response.json(mine(true)));
    page(false, mine(false));
    open();
    fireEvent.click(
      screen.getByRole("menuitemcheckbox", { name: "Keep environment on" }),
    );
    await waitFor(() =>
      expect(sent()).toEqual([
        [`/api/v1/conversations/${conversationId}`, "PATCH", { pinned: true }],
      ]),
    );
    // The menu closed after the choice; reopened, it reads the task as saved.
    await waitFor(() => {
      open();
      expect(
        screen.getByRole("menuitemcheckbox", { name: "Keep environment on" }),
      ).toHaveAttribute("aria-checked", "true");
    });
    fetch.mockResolvedValueOnce(Response.json(mine(false)));
    fireEvent.click(
      screen.getByRole("menuitemcheckbox", { name: "Keep environment on" }),
    );
    await waitFor(() =>
      expect(sent().at(-1)).toEqual([
        `/api/v1/conversations/${conversationId}`,
        "PATCH",
        { pinned: false },
      ]),
    );
  });

  it("says so when the change can't be saved, and keeps the toggle as it was", async () => {
    fetch.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    page(false, mine(false));
    open();
    fireEvent.click(
      screen.getByRole("menuitemcheckbox", { name: "Keep environment on" }),
    );
    expect(
      await screen.findByText(
        "The server could not be reached. Please try again.",
      ),
    ).toBeVisible();
    open();
    expect(
      screen.getByRole("menuitemcheckbox", { name: "Keep environment on" }),
    ).toHaveAttribute("aria-checked", "false");
  });

  it("is chosen from the keyboard like any menu item", async () => {
    fetch.mockResolvedValueOnce(Response.json(mine(true)));
    page(false, mine(false));
    const more = screen.getByRole("button", {
      name: "More actions for Workspace review",
    });
    more.focus();
    fireEvent.keyDown(more, { key: "ArrowDown" });
    fireEvent.click(more);
    const first = screen.getByRole("menuitem", { name: "Rename task" });
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(document.activeElement).toBe(
      screen.getByRole("menuitemcheckbox", { name: "Keep environment on" }),
    );
    fireEvent.click(document.activeElement as HTMLElement);
    await waitFor(() => expect(sent()).toHaveLength(1));
  });

  it("isn't offered to anyone but the task's owner", () => {
    page(false, {
      ...conversation,
      owner_id: "0195d8c0-0000-7000-8000-0000000000aa",
    });
    open();
    expect(screen.queryByRole("menuitemcheckbox")).toBeNull();
  });
});

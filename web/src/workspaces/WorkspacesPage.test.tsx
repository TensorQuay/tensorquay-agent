import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  StartProvider,
  useConversationStart,
} from "../conversations/StartProvider";
import type { Workspace } from "../protocol/generated/api";
import { choose, chosen } from "../test/picker";
import {
  fakeGateway,
  problem,
  sharedWorkspace,
  task,
  workspace,
} from "../test/workspaces";
import { toastStore } from "../ui/toast-store";

import { taskFilter } from "./task-filter";
import { WorkspacesPage } from "./WorkspacesPage";

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  usePathname: () => "/workspaces",
  useSearchParams: () => new URLSearchParams(window.location.search),
}));

let started: string | undefined | null = null;
function StartProbe() {
  const { version, workspace } = useConversationStart();
  useEffect(() => {
    started = version ? workspace : null;
  }, [version, workspace]);
  return null;
}
const LIST = "GET /api/v1/workspaces";
/** The page's first page: 25, most recently used first (UI-GUIDELINES §7, GW-I-117). */
const PAGE = "GET /api/v1/workspaces?limit=25&sort=recent";
const TASKS = "GET /api/v1/conversations";
function show(
  items: Workspace[] | (() => Response | Promise<Response>),
  routes: Parameters<typeof fakeGateway>[0] = {},
) {
  const answer =
    typeof items === "function" ? items : () => Response.json({ items });
  const gateway = fakeGateway({
    // The whole list (the picker and the sidebar's names) and the page's first page answer alike.
    [LIST]: answer,
    [PAGE]: answer,
    [TASKS]: () => Response.json({ items: [], next_cursor: null }),
    ...routes,
  });
  vi.stubGlobal("fetch", gateway.fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], {
    id: "0195d8c0-0000-7000-8000-000000000001",
    email: "me@example.invalid",
    role: "member",
  });
  render(
    <QueryClientProvider client={cache}>
      <StartProvider>
        <StartProbe />
        <WorkspacesPage />
      </StartProvider>
    </QueryClientProvider>,
  );
  return { ...gateway, cache };
}
/** A phone's width, or a laptop's. */
function width(narrow: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: narrow,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
}
beforeEach(() => {
  window.history.replaceState(null, "", "/workspaces");
  width(false);
  push.mockReset();
  started = null;
  taskFilter.clear();
  try {
    localStorage.clear();
  } catch {
    /* no storage */
  }
});
afterEach(() => {
  vi.unstubAllGlobals();
  for (const entry of toastStore.snapshot()) toastStore.dismiss(entry.id);
});

// Task 076: the Workspaces page (UI-GUIDELINES §6 and §7).
describe("WEB-U-025 workspaces page", () => {
  it("loads with skeleton cards, never a spinner", () => {
    show(() => new Promise<Response>(() => undefined));
    expect(
      screen.getByRole("status", { name: "Loading workspaces" }),
    ).toBeInTheDocument();
    expect(document.querySelector(".button-spinner")).toBeNull();
  });

  it("lists every workspace in one table: names first, sharing as a count, the owner as You or by email", async () => {
    show([
      workspace({ name: "Sales analysis", share_count: 2 }),
      workspace({ name: "Website copy" }),
      sharedWorkspace("write", { name: "Team handbook" }),
    ]);
    const table = await screen.findByRole("table", { name: "Workspaces" });
    // Someone else's workspace is listed, so the Owner column is there.
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual(["Name", "Owner", "State", "Size", "Actions"]);
    const rows = within(table).getAllByRole("row").slice(1);
    expect(
      rows.map((row) => row.querySelector(".data-name > span")?.textContent),
    ).toEqual(["Sales analysis", "Website copy", "Team handbook"]);
    // Sharing is a small count after the name, said in words, not a pill.
    expect(within(rows[0]!).getByText("Shared with 2 people")).toHaveClass(
      "sr-only",
    );
    expect(rows[0]!.querySelector(".status-pill")).toBeNull();
    // The owner in every row: "You" for yours, never a blank cell (the review of 047b8713), and someone else's
    // by email with your access.
    const owner = (row: HTMLElement) =>
      within(row).getAllByRole("cell")[1]?.textContent;
    expect(rows.map(owner)).toEqual([
      "You",
      "You",
      "ana@example.invalid · Can edit",
    ]);
    // One main action per view: no Open button on every row.
    expect(
      within(table).queryAllByRole("button", { name: /^Open / }),
    ).toHaveLength(0);
    expect(
      screen.getByRole("heading", { level: 1, name: "Workspaces" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Grid" })).toBeNull();
    expect(screen.queryByRole("button", { name: "List" })).toBeNull();
  });

  // The design direction (page by page): a column appears only when a row fills it, so your own workspaces alone have
  // no Owner column; the name takes the free width.
  it("has no Owner column when every workspace listed is your own", async () => {
    show([
      workspace({ name: "Sales analysis" }),
      workspace({
        name: "A workspace with a long name that still has room in the row",
      }),
    ]);
    const table = await screen.findByRole("table", { name: "Workspaces" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual(["Name", "State", "Size", "Actions"]);
    expect(
      within(table).getAllByRole("row")[1]?.querySelectorAll("td"),
    ).toHaveLength(4);
  });

  it("shows one sentence and one primary button when there are none", async () => {
    show([]);
    expect(
      await screen.findByText(
        "A workspace keeps a task’s files. Share it, copy it or start new tasks on it.",
      ),
    ).toBeInTheDocument();
    const buttons = screen
      .getAllByRole("button", { name: "New workspace" })
      .filter((button) => button.getAttribute("data-variant") === "primary");
    expect(buttons).toHaveLength(1);
    // The workspaces icon, as in the sidebar, not the logo.
    expect(
      document.querySelector(".empty-state svg.lucide-folders"),
    ).not.toBeNull();
    expect(document.querySelector(".empty-state .brand-mark")).toBeNull();
  });

  it("creates a workspace by name", async () => {
    const created = workspace({ name: "Research notes" });
    const { calls } = show([], {
      "POST /api/v1/workspaces": () => Response.json(created, { status: 201 }),
    });
    fireEvent.click(
      (await screen.findAllByRole("button", { name: "New workspace" }))[0]!,
    );
    const dialog = screen.getByRole("dialog", { name: "New workspace" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), {
      target: { value: "Research notes" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create" }));
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      name: "Research notes",
    });
  });

  it("shows cards on a phone", async () => {
    width(true);
    show([
      workspace({ name: "Sales analysis" }),
      workspace({ name: "Website copy" }),
    ]);
    expect(
      (await screen.findAllByRole("article")).map(
        (card) => within(card).getByRole("heading", { level: 3 }).textContent,
      ),
    ).toEqual(["Sales analysis", "Website copy"]);
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("forks with a prefilled name, shows the copy at once and offers a task in it", async () => {
    const sales = workspace({ name: "Sales analysis" });
    const copy = workspace({
      name: "Sales analysis (copy)",
      state: "forking",
      size_bytes: null,
      fork: { from: sales.id, consistent: true },
    });
    let items = [sales];
    const { calls } = show(() => Response.json({ items }), {
      [`POST /api/v1/workspaces/${sales.id}/fork`]: () => {
        items = [copy, sales];
        return Response.json(copy, { status: 202 });
      },
    });
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for Sales analysis",
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Make a copy" }));
    const dialog = screen.getByRole("dialog", {
      name: "Copy “Sales analysis”",
    });
    expect(within(dialog).getByRole("textbox", { name: "Name" })).toHaveValue(
      "Sales analysis (copy)",
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Make a copy" }),
    );
    expect(
      await screen.findByText("Copying “Sales analysis”…"),
    ).toBeInTheDocument();
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      name: "Sales analysis (copy)",
    });
    const toast = toastStore.snapshot().at(-1);
    expect(toast?.message).toBe("Copied into “Sales analysis (copy)”");
    expect(toast?.action?.label).toBe("Start a task in the copy");
    act(() => toast?.action?.onClick());
    expect(started).toBe(copy.id);
    expect(push).toHaveBeenCalledWith("/");
  });

  it("starts a new task on a workspace", async () => {
    const sales = workspace({ name: "Sales analysis" });
    show([sales]);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for Sales analysis",
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "New task here" }));
    expect(started).toBe(sales.id);
    expect(push).toHaveBeenCalledWith("/");
  });

  it("opens a workspace from its row, by click or Enter, or from its menu", async () => {
    const sales = workspace({ name: "Sales analysis" });
    show([sales]);
    const row = (await screen.findByText("Sales analysis")).closest("tr")!;
    expect(row).toHaveAttribute("tabindex", "0");
    fireEvent.keyDown(row, { key: "Enter" });
    expect(taskFilter.current()).toEqual({
      id: sales.id,
      name: "Sales analysis",
    });
    taskFilter.clear();
    fireEvent.click(within(row).getAllByRole("cell")[2]!);
    expect(taskFilter.current()?.id).toBe(sales.id);
    taskFilter.clear();
    // The menu's button doesn't open the row; its first item does.
    fireEvent.click(
      within(row).getByRole("button", {
        name: "More actions for Sales analysis",
      }),
    );
    expect(taskFilter.current()).toBeNull();
    const items = screen.getAllByRole("menuitem");
    expect(items[0]).toHaveTextContent("Open");
    fireEvent.click(items[0]!);
    expect(taskFilter.current()?.id).toBe(sales.id);
  });

  // The UI standard's In-place edit (a workspace name is its example): the name itself becomes the field, no dialog.
  it("renames in place: Enter saves, Esc puts it back", async () => {
    const sales = workspace({ name: "Sales analysis" });
    let listed = sales;
    const { calls } = show(() => Response.json({ items: [listed] }), {
      [`PATCH /api/v1/workspaces/${sales.id}`]: (body) => {
        listed = { ...sales, name: (body as { name: string }).name };
        return Response.json(listed);
      },
    });
    const rename = async (name: string) => {
      fireEvent.click(
        await screen.findByRole("button", { name: `More actions for ${name}` }),
      );
      fireEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
      return screen.getByRole("textbox", { name: "Workspace name" });
    };
    let field = await rename("Sales analysis");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(field).toHaveValue("Sales analysis");
    expect(field).toHaveFocus();
    fireEvent.change(field, { target: { value: "Something else" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(await screen.findByText("Sales analysis")).toBeVisible();
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);

    field = await rename("Sales analysis");
    fireEvent.change(field, { target: { value: " Sales 2026 " } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(await screen.findByText("Sales 2026")).toBeVisible();
    expect(
      screen.queryByRole("textbox", { name: "Workspace name" }),
    ).toBeNull();
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      name: "Sales 2026",
    });
  });

  it("keeps the name in the field when a rename fails, and says why", async () => {
    const sales = workspace({ name: "Sales analysis" });
    show([sales], {
      [`PATCH /api/v1/workspaces/${sales.id}`]: () => problem(500, "internal"),
    });
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for Sales analysis",
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
    const field = screen.getByRole("textbox", { name: "Workspace name" });
    fireEvent.change(field, { target: { value: "Sales 2026" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await vi.waitFor(() =>
      expect(toastStore.snapshot().at(-1)).toMatchObject({
        tone: "error",
        message: "The workspace couldn't be renamed. Try again.",
      }),
    );
    expect(field).toHaveValue("Sales 2026");
  });
});

// Task 076 Q9: the confirmation follows the gateway's delete rules.
describe("WEB-U-025 deleting a workspace", () => {
  const answer = (status: number) =>
    status === 204
      ? new Response(null, { status })
      : problem(status, "workspace_in_use");
  async function remove(
    value: Workspace,
    tasks: ReturnType<typeof task>[] = [],
    status = 204,
  ) {
    const gateway = show([value], {
      [TASKS]: () => Response.json({ items: tasks, next_cursor: null }),
      [`DELETE /api/v1/workspaces/${value.id}`]: () => answer(status),
      [`DELETE /api/v1/workspaces/${value.id}?with_conversations=true`]: () =>
        answer(status),
    });
    await screen.findByRole("table");
    // The count of my tasks comes from the task list; let it load first.
    await vi.waitFor(() =>
      expect(
        gateway.calls.some((call) => call.url === "/api/v1/conversations"),
      ).toBe(true),
    );
    await act(() => Promise.resolve());
    fireEvent.click(
      screen.getByRole("button", {
        name: `More actions for ${value.name ?? ""}`,
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    return { dialog: screen.getByRole("dialog"), ...gateway };
  }

  // WEB-U-275 (the accessibility review of the core screens): the deleted card takes the ⋯ that opened its confirmation
  // with it, so the focus would fall to the page; the page's heading takes it once the card is gone.
  it("gives the focus to the page's heading once the deleted card is gone", async () => {
    const value = workspace({ name: "Old notes" });
    let current = [value];
    const gateway = show(() => Response.json({ items: current }), {
      [`DELETE /api/v1/workspaces/${value.id}`]: () => {
        current = [];
        return new Response(null, { status: 204 });
      },
    });
    await screen.findByRole("table");
    await vi.waitFor(() =>
      expect(
        gateway.calls.some((call) => call.url === "/api/v1/conversations"),
      ).toBe(true),
    );
    await act(() => Promise.resolve());
    const more = screen.getByRole("button", {
      name: "More actions for Old notes",
    });
    more.focus();
    fireEvent.click(more);
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Delete",
      }),
    );
    await vi.waitFor(() =>
      expect(
        screen.getByRole("heading", { level: 1, name: "Workspaces" }),
      ).toHaveFocus(),
    );
  });

  it("names the workspace and removes its files when it has no tasks", async () => {
    const value = workspace({ name: "Old notes" });
    taskFilter.set({ id: value.id, name: "Old notes" });
    const { dialog, calls } = await remove(value);
    expect(dialog).toHaveTextContent(
      "Delete “Old notes”? Its files are removed.",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(
      calls.filter((call) => call.method === "DELETE").map((call) => call.url),
    ).toEqual([`/api/v1/workspaces/${value.id}`]);
    // The task list no longer filters on a workspace that is gone.
    expect(taskFilter.current()).toBeNull();
  });

  it("counts my tasks and deletes them with it", async () => {
    const value = workspace({ name: "Drafts", task_count: 2 });
    const { dialog, calls } = await remove(value, [
      task("0195d8c0-0000-7000-8000-0000000000d1", value.id),
      task("0195d8c0-0000-7000-8000-0000000000d2", value.id),
    ]);
    expect(dialog).toHaveTextContent(
      "Delete “Drafts” and its 2 tasks? The tasks and the files are removed.",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await vi.waitFor(() =>
      expect(
        calls.some(
          (call) =>
            call.method === "DELETE" &&
            call.url ===
              `/api/v1/workspaces/${value.id}?with_conversations=true`,
        ),
      ).toBe(true),
    );
  });

  it("gives only the reason while a task is working in it", async () => {
    const { dialog } = await remove(
      workspace({
        name: "Busy",
        task_count: 1,
        lease: {
          conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
          title: "Mine",
          running: true,
          pinned: false,
        },
      }),
    );
    expect(dialog).toHaveTextContent(
      "A task is working in “Busy”. It can be deleted once that task is done.",
    );
    expect(within(dialog).queryByRole("button", { name: "Delete" })).toBeNull();
  });

  // The 13h UI review (#3): a done task's kept environment doesn't block the delete; the gateway retires it.
  it("offers the delete when a done task's environment is only kept", async () => {
    const value = workspace({
      name: "Kept",
      task_count: 1,
      lease: {
        conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
        title: "Mine",
        running: false,
        pinned: false,
      },
    });
    // The task holding it is the reader's own, so it would go with the workspace.
    const { dialog } = await remove(value, [
      task("0195d8c0-0000-7000-8000-0000000000c9", value.id),
    ]);
    expect(dialog).not.toHaveTextContent(/is working in|is using/);
    expect(
      within(dialog).getByRole("button", { name: "Delete" }),
    ).toBeEnabled();
  });

  // A pinned task keeps its environment on (FR-33), so the delete can't retire it: the reason, and the way out where
  // the reader can take it (their own task's ⋯).
  it.each([
    [
      "Mine",
      "“Mine” keeps its environment on, so “Pinned” can’t be deleted. Turn off Keep environment on in that task’s ⋯ first.",
    ],
    [
      null,
      "Another task keeps its environment on, so “Pinned” can’t be deleted.",
    ],
  ])(
    "gives only the reason while a task keeps its environment on (%s)",
    async (title, words) => {
      const value = workspace({
        name: "Pinned",
        task_count: 1,
        lease: {
          conversation_id: "0195d8c0-0000-7000-8000-0000000000c9",
          title,
          running: false,
          pinned: true,
        },
      });
      const { dialog } = await remove(value, [
        task("0195d8c0-0000-7000-8000-0000000000c9", value.id),
      ]);
      expect(dialog).toHaveTextContent(words);
      expect(
        within(dialog).queryByRole("button", { name: "Delete" }),
      ).toBeNull();
    },
  );

  it("gives only the reason when someone else has tasks on it", async () => {
    const value = workspace({ name: "Team", task_count: 2 });
    const { dialog } = await remove(value, [
      task("0195d8c0-0000-7000-8000-0000000000d3", value.id),
    ]);
    expect(dialog).toHaveTextContent(
      "Other people have tasks on “Team”, so it can’t be deleted.",
    );
    expect(within(dialog).queryByRole("button", { name: "Delete" })).toBeNull();
    // An information dialog closes with Close, never Done (the UI standard's audit).
    expect(within(dialog).getByRole("button", { name: "Close" })).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: "Done" })).toBeNull();
  });

  it("explains the gateway's refusal when it finds the workspace in use", async () => {
    const { dialog } = await remove(workspace({ name: "Race" }), [], 409);
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(
      await within(dialog).findByText(
        "This workspace is in use or has other people’s tasks, so it can’t be deleted now.",
      ),
    ).toBeInTheDocument();
  });
});

/** A page of `count` workspaces named `prefix` 1 to count, and where the next starts. */
function page(prefix: string, count: number, next?: string) {
  return () =>
    Response.json({
      items: Array.from({ length: count }, (_, index) =>
        workspace({ name: `${prefix} ${index + 1}` }),
      ),
      ...(next ? { next_cursor: next } : {}),
    });
}
const names = () =>
  within(screen.getByRole("table", { name: "Workspaces" }))
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.querySelector(".data-name > span")?.textContent);

// UI-GUIDELINES §7: the Workspaces page finds a workspace among many: paged, filtered, sorted and searched on the server.
describe("WEB-U-073 finding a workspace", () => {
  it("shows 25 at a time, most recently used first, and more on request", async () => {
    const { calls } = show([], {
      [PAGE]: page("Recent", 25, "next-1"),
      [`${PAGE}&cursor=next-1`]: page("Older", 5),
    });
    await screen.findByRole("table", { name: "Workspaces" });
    expect(names()).toHaveLength(25);
    fireEvent.click(screen.getByRole("button", { name: "Show more" }));
    await vi.waitFor(() => expect(names()).toHaveLength(30));
    expect(names().at(-1)).toBe("Older 5");
    expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
    expect(calls.map((call) => call.url)).toContain(
      "/api/v1/workspaces?limit=25&sort=recent&cursor=next-1",
    );
  });

  it("filters and sorts on the server and keeps both in the address", async () => {
    window.history.replaceState(
      null,
      "",
      "/workspaces?filter=shared_with_me&sort=name",
    );
    show([], {
      [`${PAGE.replace("sort=recent", "sort=name")}&filter=shared_with_me`]:
        page("Shared", 2),
      [`${PAGE.replace("sort=recent", "sort=name")}&filter=idle`]: page(
        "Idle",
        1,
      ),
      [`${PAGE}&filter=idle`]: page("Idle recent", 1),
    });
    await screen.findByText("Shared 1");
    expect(
      screen.getByRole("button", { name: "Shared with me" }),
    ).toHaveAttribute("aria-pressed", "true");
    // A light segmented control, not a row of bold tabs.
    expect(screen.getByRole("group", { name: "Show" })).toHaveClass(
      "segmented",
    );
    expect(chosen("Sort")).toBe("Name");
    fireEvent.click(screen.getByRole("button", { name: "Idle" }));
    expect(await screen.findByText("Idle 1")).toBeInTheDocument();
    expect(window.location.search).toBe("?filter=idle&sort=name");
    choose("Sort", "Recently used");
    expect(await screen.findByText("Idle recent 1")).toBeInTheDocument();
    expect(window.location.search).toBe("?filter=idle");
  });

  // 17.3: a phone has no room for the segmented filter's four labels; it offers the same choices as the standard picker
  // (the stylesheet shows one or the other by width).
  it("offers the same filters as the standard picker, for a phone", async () => {
    window.history.replaceState(null, "", "/workspaces?filter=shared_with_me");
    show([], {
      [`${PAGE}&filter=shared_with_me`]: page("Shared", 2),
      [`${PAGE}&filter=idle`]: page("Idle", 1),
    });
    await screen.findByText("Shared 1");
    expect(chosen("Show")).toBe("Shared with me");
    choose("Show", "Idle");
    expect(await screen.findByText("Idle 1")).toBeInTheDocument();
    expect(window.location.search).toBe("?filter=idle");
  });

  it("offers no search, filters or sort while every workspace fits on the page (fewer than eight)", async () => {
    show([
      workspace({ name: "Sales analysis" }),
      workspace({ name: "Billing" }),
    ]);
    await screen.findByText("Sales analysis");
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(screen.queryByRole("group", { name: "Show" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Sort: / })).toBeNull();
  });

  it("searches names on the server once typing settles", async () => {
    // Eight or more workspaces: enough to search, filter and sort.
    const { calls } = show(
      [
        workspace({ name: "Sales analysis" }),
        ...Array.from({ length: 7 }, (_, index) =>
          workspace({ name: `Other ${index + 1}` }),
        ),
      ],
      {
        [`${PAGE}&q=web`]: page("Website", 1),
        [`${PAGE}&q=zzz`]: page("None", 0),
      },
    );
    const search = await screen.findByRole("searchbox", {
      name: "Search workspaces",
    });
    await screen.findByText("Sales analysis");
    fireEvent.change(search, { target: { value: "web" } });
    expect(await screen.findByText("Website 1")).toBeInTheDocument();
    expect(screen.queryByText("Sales analysis")).toBeNull();
    fireEvent.change(search, { target: { value: "zzz" } });
    expect(
      await screen.findByText("No workspace matches “zzz”."),
    ).toBeInTheDocument();
    expect(
      calls.filter((call) => call.url.includes("q=")).map((call) => call.url),
    ).toEqual([
      "/api/v1/workspaces?limit=25&sort=recent&q=web",
      "/api/v1/workspaces?limit=25&sort=recent&q=zzz",
    ]);
  });

  it("says when a filter has nothing and offers the way back", async () => {
    window.history.replaceState(null, "", "/workspaces?filter=in_use");
    show([workspace({ name: "Sales analysis" })], {
      [`${PAGE}&filter=in_use`]: page("None", 0),
    });
    expect(
      await screen.findByText("No workspace is in use."),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Show all workspaces" }),
    );
    expect(await screen.findByText("Sales analysis")).toBeInTheDocument();
    expect(window.location.search).toBe("");
  });
});

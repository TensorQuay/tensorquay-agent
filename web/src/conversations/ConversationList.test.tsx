import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import { settleLazy } from "../test/lazy";
import { fakeGateway, me, problem, task, workspace } from "../test/workspaces";
import { toastStore } from "../ui/toast-store";

import { ConversationList } from "./ConversationList";
import { taskSearch } from "./task-search";
import { TaskSearchHost } from "./TaskSearchHost";

vi.mock("next/navigation", () => ({ usePathname: () => "/" }));

const billing = workspace({ name: "billing-service" });
const now = new Date(2026, 8, 28, 21, 40);

/** A task on `billing`, last active `hoursAgo` hours before `now`. */
function ago(
  n: number,
  hoursAgo: number,
  overrides: Partial<Conversation> = {},
) {
  return task(
    `0195d8c0-0000-7000-8000-${String(n).padStart(12, "0")}`,
    billing.id,
    {
      title: `Task ${n}`,
      last_activity_at: new Date(
        now.getTime() - hoursAgo * 3_600_000,
      ).toISOString(),
      ...overrides,
    },
  );
}

/** The list, as many times as the page mounts it (the sidebar, and the phone drawer), beside the shell's one search. */
function show(
  items: Conversation[],
  extra: Record<string, (body: unknown) => Response | Promise<Response>> = {},
  lists = 1,
) {
  const gateway = fakeGateway({
    "GET /api/v1/conversations": () =>
      Response.json({ items, next_cursor: null }),
    "GET /api/v1/workspaces": () => Response.json({ items: [billing] }),
    ...extra,
  });
  vi.stubGlobal("fetch", gateway.fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], me);
  render(
    <QueryClientProvider client={cache}>
      {Array.from({ length: lists }, (_, index) => (
        <ConversationList key={index} />
      ))}
      <TaskSearchHost />
    </QueryClientProvider>,
  );
  return gateway;
}

/** Older tasks, so the list is long enough to offer a search (eight or more). */
const older = (count: number) =>
  Array.from({ length: count }, (_, index) =>
    ago(100 + index, 24 * 40 + index, { title: `Older task ${index + 1}` }),
  );
/** The search overlay, opened from the sidebar's Search row (the design direction, sidebar). */
async function openSearch() {
  fireEvent.click(await screen.findByRole("button", { name: "Search tasks" }));
  await settleLazy();
  return screen.findByRole("dialog", { name: "Search tasks" });
}

const region = () => screen.getByRole("region", { name: "Tasks" });
const group = (name: string) => screen.getByRole("group", { name });

afterEach(() => {
  taskSearch.reset();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  // A test's spies end with it (the Mac test's navigator.platform made every later Ctrl K a Mac's).
  vi.restoreAllMocks();
});

// UI-GUIDELINES §7: the sidebar's task list finds work again: date groups, a status dot, titles, "Show more" and a search.
describe("WEB-U-071 sidebar task list", () => {
  // The sidebar design (WEB-U-071): the date groups say what the list is; a "Tasks" heading above them was noise. The list
  // keeps the name for assistive technology.
  it("has no Tasks heading, and the list is still named Tasks", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(now);
    show([ago(1, 1)]);
    await within(region()).findByRole("link", { name: "Task 1" });
    expect(screen.queryByRole("heading", { name: "Tasks" })).toBeNull();
    expect(
      within(region()).getByRole("heading", { name: "Today" }),
    ).toBeVisible();
  });

  it("groups tasks by date, shows each status in shape and words, and names the workspace on hover", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(now);
    show([
      ago(1, 1, { title: "Fix the date range", run_status: "running" }),
      ago(2, 2, { title: "Add CSV export", run_status: "cancelled" }),
      ago(3, 3, { title: "Rename the parser", run_status: "completed" }),
      ago(4, 25, { title: "Build fails on node 22", run_status: "failed" }),
      ago(5, 24 * 4, { title: "Write the README" }),
      ago(6, 24 * 30, { title: "Old work" }),
    ]);
    await within(region()).findByRole("link", { name: /Fix the date range/ });
    expect(
      within(region())
        .getAllByRole("group")
        .map((element) => element.getAttribute("aria-label")),
    ).toEqual(["Today", "Yesterday", "Previous 7 days", "Older"]);
    // Each row's name is its title and, when it has one, its status in words.
    expect(
      within(group("Today"))
        .getAllByRole("link")
        .map((link) => link.getAttribute("aria-label")),
    ).toEqual([
      "Fix the date range, Working",
      "Add CSV export",
      "Rename the parser",
    ]);
    const failed = within(group("Yesterday")).getByRole("link", {
      name: /Build fails/,
    });
    expect(failed).toHaveAccessibleName("Build fails on node 22, Failed");
    // Each status has its own shape, not only a colour.
    expect(failed.querySelector("[data-status='failed'] svg")).not.toBeNull();
    const quiet = within(group("Today")).getByRole("link", {
      name: /Add CSV export/,
    });
    // PRD FR-10, superseded: no task needs the reader, so a stopped one has no mark.
    expect(quiet.querySelector("[data-status]")).toBeNull();
    // The workspace name is on hover, never a second line.
    expect(quiet).toHaveAttribute(
      "title",
      expect.stringMatching(/^Add CSV export · billing-service · /),
    );
    expect(quiet.querySelector(".conversation-workspace")).toBeNull();
  });

  // A live check before 17.0, revised: a row shows no time (its group heading says when); its tooltip holds the full
  // title, the workspace and the full time.
  it("shows no time on a row, and the full title and time in its tooltip", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(now);
    show([ago(1, 1), ago(2, 2)]);
    await within(region()).findByRole("link", { name: "Task 1" });
    const links = within(group("Today")).getAllByRole("link");
    expect(links.map((link) => link.querySelector(".task-when"))).toEqual([
      null,
      null,
    ]);
    expect(links.map((link) => link.getAttribute("title"))).toEqual([
      "Task 1 · billing-service · Sep 28, 2026, 20:40",
      "Task 2 · billing-service · Sep 28, 2026, 19:40",
    ]);
    expect(links.map((link) => link.getAttribute("aria-label"))).toEqual([
      "Task 1",
      "Task 2",
    ]);
  });

  it("tells identical titles apart by the full time in their names", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(now);
    show([
      ago(1, 1, { title: "Write tests for dates" }),
      ago(2, 5, { title: "Write tests for dates" }),
    ]);
    const links = await within(region()).findAllByRole("link", {
      name: /Write tests for dates/,
    });
    expect(links.map((link) => link.getAttribute("aria-label"))).toEqual([
      "Write tests for dates, Sep 28, 2026, 20:40",
      "Write tests for dates, Sep 28, 2026, 16:40",
    ]);
  });

  it("tells identical titles apart in search results too", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(now);
    const same = [
      ago(1, 1, { title: "Write tests for dates" }),
      ago(2, 5, { title: "Write tests for dates" }),
    ];
    show([...same, ...older(6)], {
      "GET /api/v1/conversations?q=tests": () =>
        Response.json({ items: same, next_cursor: null }),
    });
    await within(region()).findAllByRole("link", { name: /Write tests/ });
    const dialog = await openSearch();
    fireEvent.change(within(dialog).getByRole("searchbox"), {
      target: { value: "tests" },
    });
    await within(dialog).findByText("2 tasks match");
    expect(
      within(dialog)
        .getAllByRole("link")
        .map((link) => link.getAttribute("aria-label")),
    ).toEqual([
      "Write tests for dates, Sep 28, 2026, 20:40",
      "Write tests for dates, Sep 28, 2026, 16:40",
    ]);
  });

  it("shows eight tasks a group, then more on request", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(now);
    show(
      Array.from({ length: 11 }, (_, index) =>
        ago(index + 1, 0.1 * (index + 1)),
      ),
    );
    await within(region()).findByRole("link", { name: "Task 1" });
    expect(within(group("Today")).getAllByRole("link")).toHaveLength(8);
    fireEvent.click(
      within(group("Today")).getByRole("button", { name: "Show more" }),
    );
    expect(within(group("Today")).getAllByRole("link")).toHaveLength(11);
    expect(
      within(group("Today")).queryByRole("button", { name: "Show more" }),
    ).toBeNull();
  });

  // The design direction (sidebar): a Search row and Ctrl/Cmd K open a search overlay; there is no permanent box.
  it("searches every task on the server in an overlay from Ctrl K, lists the matches, and closes with Escape", async () => {
    const found = ago(7, 24 * 60, { title: "Deploy the date fix" });
    const gateway = show(
      [ago(1, 1, { title: "Fix the date range" }), ...older(7)],
      {
        "GET /api/v1/conversations?q=date": () =>
          Response.json({ items: [found], next_cursor: null }),
        "GET /api/v1/conversations?q=nothing": () =>
          Response.json({ items: [], next_cursor: null }),
      },
    );
    await within(region()).findByRole("link", { name: /Fix the date range/ });
    expect(screen.queryByRole("searchbox")).toBeNull();
    const row = screen.getByRole("button", { name: "Search tasks" });
    expect(row).toHaveTextContent("Search");
    expect(row).toHaveTextContent("Ctrl K");
    expect(row).toHaveAttribute("aria-keyshortcuts", "Control+K");
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    await settleLazy();
    const dialog = await screen.findByRole("dialog", { name: "Search tasks" });
    const search = within(dialog).getByRole("searchbox", {
      name: "Search tasks",
    });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "date" } });
    expect(await within(dialog).findByText("1 task matches")).toBeVisible();
    expect(
      within(dialog)
        .getAllByRole("link")
        .map((link) => link.getAttribute("aria-label")),
    ).toEqual(["Deploy the date fix"]);
    fireEvent.change(search, { target: { value: "nothing" } });
    expect(
      await within(dialog).findByText("No tasks match “nothing”."),
    ).toBeVisible();
    fireEvent.keyDown(search, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
    // The list itself never changed.
    expect(
      within(region()).getByRole("link", { name: /Fix the date range/ }),
    ).toBeVisible();
    // Typing is debounced: the partial words were never sent.
    const searched = gateway.calls
      .filter((call) => call.url.includes("q="))
      .map((call) => call.url);
    expect(searched).toEqual([
      "/api/v1/conversations?q=date",
      "/api/v1/conversations?q=nothing",
    ]);
  });

  it("closes the overlay when a match is chosen", async () => {
    const found = ago(7, 24 * 60, { title: "Deploy the date fix" });
    show([...older(8)], {
      "GET /api/v1/conversations?q=date": () =>
        Response.json({ items: [found], next_cursor: null }),
    });
    const dialog = await openSearch();
    fireEvent.change(within(dialog).getByRole("searchbox"), {
      target: { value: "date" },
    });
    fireEvent.click(
      await within(dialog).findByRole("link", { name: "Deploy the date fix" }),
    );
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
  });

  // One Ctrl/Cmd K listener, in the shell, with one overlay, however many lists the page mounts (the sidebar's, and the
  // phone drawer's).
  it("opens exactly one search from Ctrl K with two lists mounted", async () => {
    show([...older(8)], {}, 2);
    await waitFor(() =>
      expect(
        screen.getAllByRole("button", { name: "Search tasks" }),
      ).toHaveLength(2),
    );
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    await settleLazy();
    expect(
      await screen.findAllByRole("dialog", { name: "Search tasks" }),
    ).toHaveLength(1);
    // Rendered by the shell, never inside a list (a hidden sidebar would make it an invisible modal).
    for (const list of screen.getAllByRole("region", { name: "Tasks" }))
      expect(within(list).queryByRole("dialog")).toBeNull();
  });

  // On a Mac Ctrl K is the text fields' delete-to-end-of-line; only Cmd K opens the search there.
  it("opens with Cmd K on a Mac, and leaves Ctrl K to the text fields", async () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    show([...older(8)]);
    const row = await screen.findByRole("button", { name: "Search tasks" });
    expect(row).toHaveTextContent("⌘K");
    expect(row).toHaveAttribute("aria-keyshortcuts", "Meta+K");
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    await settleLazy();
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.keyDown(document.body, { key: "k", metaKey: true });
    await settleLazy();
    expect(
      await screen.findByRole("dialog", { name: "Search tasks" }),
    ).toBeVisible();
  });

  // Nit 5: a second modal never stacks over an open dialog (a rename, a delete).
  it("ignores Ctrl K while another dialog is open", async () => {
    show([...older(8)]);
    await screen.findByRole("button", { name: "Search tasks" });
    const other = document.createElement("dialog");
    other.setAttribute("open", "");
    document.body.append(other);
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    await settleLazy();
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
    other.remove();
  });

  // Enter opens the first match; the arrow keys move from the box through the matches and back.
  it("opens the first match on Enter, and moves through the matches with the arrow keys", async () => {
    const found = [
      ago(7, 24 * 60, { title: "Deploy the date fix" }),
      ago(8, 24 * 61, { title: "Date picker" }),
    ];
    show([...older(8)], {
      "GET /api/v1/conversations?q=date": () =>
        Response.json({ items: found, next_cursor: null }),
    });
    const dialog = await openSearch();
    const box = within(dialog).getByRole("searchbox");
    fireEvent.change(box, { target: { value: "date" } });
    const first = await within(dialog).findByRole("link", {
      name: "Deploy the date fix",
    });
    fireEvent.keyDown(box, { key: "ArrowDown" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(
      within(dialog).getByRole("link", { name: "Date picker" }),
    ).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? first, { key: "ArrowUp" });
    fireEvent.keyDown(first, { key: "ArrowUp" });
    expect(box).toHaveFocus();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
  });

  // Every match, however many: the first page, then Show more, and the count says there are more.
  it("loads more matches on request", async () => {
    const first = Array.from({ length: 3 }, (_, index) =>
      ago(20 + index, 24 * 60 + index, { title: `Report ${index + 1}` }),
    );
    const second = [ago(30, 24 * 70, { title: "Report 4" })];
    show([...older(8)], {
      "GET /api/v1/conversations?q=report": () =>
        Response.json({ items: first, next_cursor: "more" }),
      "GET /api/v1/conversations?cursor=more&q=report": () =>
        Response.json({ items: second, next_cursor: null }),
    });
    const dialog = await openSearch();
    fireEvent.change(within(dialog).getByRole("searchbox"), {
      target: { value: "report" },
    });
    expect(await within(dialog).findByText("3+ tasks match")).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Show more" }));
    expect(await within(dialog).findByText("4 tasks match")).toBeVisible();
    expect(within(dialog).getAllByRole("link")).toHaveLength(4);
  });

  // A search only over a list long enough to need one (eight or more, or more to load): an empty list says so in one
  // quiet line.
  it("offers no search and says so in one quiet line when there are no tasks", async () => {
    const gateway = show([]);
    await vi.waitFor(() =>
      expect(
        gateway.calls.some((call) => call.url === "/api/v1/conversations"),
      ).toBe(true),
    );
    expect(
      await within(region()).findByText("Your tasks will appear here."),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Search tasks" })).toBeNull();
    expect(region().querySelector(".brand-mark")).toBeNull();
  });

  it("offers no search while every task fits without scrolling (fewer than eight)", async () => {
    show([1, 2, 3].map((n) => ago(n, n)));
    await within(region()).findByRole("link", { name: /Task 1/ });
    expect(screen.queryByRole("button", { name: "Search tasks" })).toBeNull();
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    await settleLazy();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps a task's actions in a menu: rename and delete", async () => {
    show([ago(1, 1, { title: "Fix the date range" })]);
    const more = await screen.findByRole("button", {
      name: "More actions for Fix the date range",
    });
    expect(
      within(region()).queryByRole("button", { name: /^Delete/ }),
    ).toBeNull();
    fireEvent.click(more);
    const menu = await screen.findByRole("menu", {
      name: "More actions for Fix the date range",
    });
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Rename task", "Delete task"]);
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "Rename task" }),
    );
    // Renamed in place (the UI standard's In-place edit), never in a dialog.
    const field = await within(region()).findByRole("textbox", {
      name: "Task title",
    });
    expect(field).toHaveValue("Fix the date range");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // WEB-U-275: deleting a task from its row's ⋯ removes the row and its ⋯, so the list takes the focus once it's gone.
  it("gives the focus to the task list once the deleted row is gone", async () => {
    const items = [
      ago(1, 1, { title: "Fix the date range" }),
      ago(2, 2, { title: "Keep me" }),
    ];
    const id = items[0]?.id ?? "";
    show(items, {
      [`DELETE /api/v1/conversations/${id}`]: () => {
        items.splice(0, 1);
        return new Response(null, { status: 204 });
      },
    });
    const more = await screen.findByRole("button", {
      name: "More actions for Fix the date range",
    });
    more.focus();
    fireEvent.click(more);
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Delete task" }),
    );
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Delete task",
      }),
    );
    await vi.waitFor(() => expect(region()).toHaveFocus());
    expect(screen.queryByText("Fix the date range")).toBeNull();
  });

  // The owner's "this edit on top sucks", in the sidebar too: Rename turns the row's title into the field.
  it("renames a row in place: Enter saves, Esc puts it back", async () => {
    const items = [ago(1, 1, { title: "Fix the date range" })];
    const id = items[0]?.id ?? "";
    const gateway = show(items, {
      [`PATCH /api/v1/conversations/${id}`]: (body) => {
        const title = (body as { title: string }).title;
        items[0] = { ...(items[0] as Conversation), title };
        return Response.json(items[0]);
      },
    });
    const rename = async () => {
      fireEvent.click(
        await screen.findByRole("button", {
          name: /^More actions for /,
        }),
      );
      fireEvent.click(
        await screen.findByRole("menuitem", { name: "Rename task" }),
      );
      return within(region()).findByRole("textbox", { name: "Task title" });
    };
    let field = await rename();
    expect(field).toHaveFocus();
    fireEvent.change(field, { target: { value: "Something else" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(
      await within(region()).findByRole("link", {
        name: /^Fix the date range/,
      }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", {
        name: "More actions for Fix the date range",
      }),
    ).toHaveFocus();
    expect(gateway.calls.some((call) => call.method === "PATCH")).toBe(false);

    field = await rename();
    fireEvent.change(field, { target: { value: " Date range fix " } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(
      await within(region()).findByRole("link", { name: /^Date range fix/ }),
    ).toBeVisible();
    expect(gateway.calls.find((call) => call.method === "PATCH")?.body).toEqual(
      { title: "Date range fix" },
    );
  });

  it("keeps the text in the row when a rename fails, and says why", async () => {
    const items = [ago(1, 1, { title: "Fix the date range" })];
    const id = items[0]?.id ?? "";
    show(items, {
      [`PATCH /api/v1/conversations/${id}`]: () => problem(500, "internal"),
    });
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for Fix the date range",
      }),
    );
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Rename task" }),
    );
    const field = await within(region()).findByRole("textbox", {
      name: "Task title",
    });
    fireEvent.change(field, { target: { value: "Date range fix" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await vi.waitFor(() =>
      expect(toastStore.snapshot().at(-1)).toMatchObject({ tone: "error" }),
    );
    expect(field).toHaveValue("Date range fix");
    expect(field).not.toHaveAttribute("aria-busy");
  });
});

// WEB-U-218 (UI-GUIDELINES: nothing fails silently): Ctrl/Cmd K pressed before the task list is known did nothing at
// all, since the shortcut listened only once a long enough list had loaded. It is remembered now: the search opens as
// soon as a long list arrives, and a list too short to search, or one that couldn't be read, says so.
describe("WEB-U-218 Ctrl K before the task list is known", () => {
  const SHORT = "All your tasks fit in the list, so there's no search yet.";
  const FAILED = "Search isn't available: the task list couldn't be read.";
  const toasts = () => toastStore.snapshot().map((entry) => entry.message);
  const clearToasts = () => {
    for (const entry of toastStore.snapshot()) toastStore.dismiss(entry.id);
  };
  // Other tests' toasts last five seconds; each case starts and ends without any.
  beforeEach(clearToasts);
  afterEach(clearToasts);
  /** The list's answer held until `release`, so the press comes first. */
  function held(answer: () => Response) {
    let release: () => void = () => undefined;
    show([], {
      "GET /api/v1/conversations": () =>
        new Promise<Response>((resolve) => {
          release = () => resolve(answer());
        }),
    });
    return () => release();
  }
  /** One press of the shortcut; whether the page took it from the browser. */
  function press() {
    const event = new KeyboardEvent("keydown", {
      key: "k",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    document.body.dispatchEvent(event);
    return event.defaultPrevented;
  }

  it("opens the search once a long enough list arrives", async () => {
    const release = held(() =>
      Response.json({ items: older(8), next_cursor: null }),
    );
    expect(press()).toBe(true);
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
    release();
    await settleLazy();
    expect(
      await screen.findByRole("dialog", { name: "Search tasks" }),
    ).toBeVisible();
    expect(toasts()).toEqual([]);
  });

  it("says so once the list turns out too short, and opens nothing later", async () => {
    const release = held(() =>
      Response.json({
        items: [ago(1, 1, { title: "Only task" })],
        next_cursor: null,
      }),
    );
    press();
    release();
    await waitFor(() => expect(toasts()).toEqual([SHORT]));
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
    // The one press was answered: a later, longer list doesn't open a search nobody asked for again.
    act(() => taskSearch.available(true));
    await settleLazy();
    expect(screen.queryByRole("dialog", { name: "Search tasks" })).toBeNull();
  });

  it("says why at once when the list is known to be too short, or couldn't be read", async () => {
    show([ago(1, 1, { title: "Only task" })]);
    await within(region()).findByRole("link", { name: /Only task/ });
    expect(press()).toBe(true);
    expect(toasts()).toEqual([SHORT]);
    cleanup();
    clearToasts();
    taskSearch.reset();
    const release = held(() => new Response("broken", { status: 500 }));
    press();
    release();
    await waitFor(() => expect(toasts()).toEqual([FAILED]));
  });
});

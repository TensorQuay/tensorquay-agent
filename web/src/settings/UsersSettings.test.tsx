import {
  notifyManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  Account,
  Conversation,
  Invitation,
  User,
} from "../protocol/generated/api";
import { LIMITS } from "../test/user";

import { UsersSettings } from "./UsersSettings";

const stamp = "2026-01-01T00:00:00Z";
const me: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const colleague: Account = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "colleague@example.invalid",
  role: "member",
  disabled: false,
  created_at: stamp,
  last_active_at: null,
};
const accounts: Account[] = [
  { ...me, disabled: false, created_at: stamp, last_active_at: null },
  colleague,
];
const task = (owner: string, id: string): Conversation => ({
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
const pending: Invitation = {
  id: "0195d8c0-0000-7000-8000-00000000000c",
  email: "new@example.invalid",
  role: "member",
  status: "pending",
  created_by: me.id,
  created_at: stamp,
  expires_at: "2026-01-08T00:00:00Z",
};

type Route = (body: unknown) => Response;
const routes = new Map<string, Route[]>();
const calls: { key: string; body: unknown }[] = [];
function on(key: string, ...responses: Route[]) {
  routes.set(key, responses);
}
const json =
  (value: unknown, status = 200): Route =>
  () =>
    Response.json(value, { status });
const empty: Route = () => new Response(null, { status: 204 });
const problem =
  (status: number, code: string): Route =>
  () =>
    Response.json(
      { error: { code, message: "diagnostic", retryable: false } },
      { status },
    );
const target = (input: RequestInfo | URL) =>
  typeof input === "string"
    ? input
    : input instanceof URL
      ? input.href
      : input.url;
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  routes.clear();
  calls.length = 0;
  fetch.mockReset();
  fetch.mockImplementation((input, init) => {
    const key = `${init?.method ?? "GET"} ${target(input)}`;
    const body: unknown =
      typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
    calls.push({ key, body });
    const queue = routes.get(key);
    const next = queue && (queue.length > 1 ? queue.shift() : queue[0]);
    if (!next) throw new Error(`Unexpected request ${key}`);
    return Promise.resolve(next(body));
  });
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});
function standard() {
  on("GET /api/v1/users", json({ items: accounts }));
  on("GET /api/v1/invitations", json({ items: [] }));
  on(
    "GET /api/v1/conversations?all=true",
    json({
      items: [
        task(colleague.id, "0195d8c0-0000-7000-8000-0000000000a1"),
        task(colleague.id, "0195d8c0-0000-7000-8000-0000000000a2"),
      ],
      next_cursor: null,
    }),
  );
}
// A fixed clock, a day after the pending invitation was made and before it expires.
const today = () => new Date("2026-01-02T12:00:00Z");
function show(user: User = me, clock: () => Date = today) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <UsersSettings me={user} clock={clock} />
    </QueryClientProvider>,
  );
  return cache;
}
const row = async (email: string) => {
  const cell = await screen.findByText(email);
  const found = cell.closest("tr");
  if (!found) throw new Error(`${email} must be a table row`);
  return within(found);
};

// Task 067 / GW-I-046: user administration for administrators, in plain words, with the gateway's guards.
describe("WEB-U-023 users", () => {
  it("tells members this is for administrators and asks nothing", () => {
    show({ ...me, role: "member" });
    expect(
      screen.getByText("Only administrators can manage users."),
    ).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("lists people with your badge, task counts and no Disable on your own row", async () => {
    standard();
    show();
    const mine = await row(me.email);
    expect(mine.getByText("You")).toBeInTheDocument();
    expect(mine.queryByRole("button", { name: /^Disable/ })).toBeNull();
    const theirs = await row(colleague.email);
    expect(theirs.getByRole("link", { name: "2 tasks" })).toHaveAttribute(
      "href",
      `/settings/people/${colleague.id}`,
    );
    expect(theirs.getByText("Active")).toBeInTheDocument();
    // A long address wraps rather than being cut off, since the table has room (finding 12).
    expect(theirs.getByText(colleague.email).closest(".data-name")).toHaveClass(
      "data-name-wrap",
    );
  });

  it("disables after a named confirmation and enables without one", async () => {
    standard();
    on(
      "PATCH /api/v1/users/" + colleague.id,
      json({ ...colleague, disabled: true }),
    );
    show();
    const theirs = await row(colleague.email);
    fireEvent.click(
      theirs.getByRole("button", {
        name: `More actions for ${colleague.email}`,
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: `Disable ${colleague.email}` }),
    );
    // The confirmation names the account it disables.
    const dialog = screen.getByRole("dialog", {
      name: `Disable ${colleague.email}?`,
    });
    expect(dialog).toHaveTextContent(
      "They are signed out at once and their API tokens stop working. Their tasks stay.",
    );
    on(
      "GET /api/v1/users",
      json({ items: [accounts[0], { ...colleague, disabled: true }] }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Disable account" }),
    );
    expect(
      await (await row(colleague.email)).findByText("Disabled"),
    ).toBeInTheDocument();
    expect(calls.find((call) => call.key.startsWith("PATCH"))?.body).toEqual({
      disabled: true,
    });
    fireEvent.click(
      (await row(colleague.email)).getByRole("button", {
        name: `More actions for ${colleague.email}`,
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: `Enable ${colleague.email}` }),
    );
    expect(
      calls.filter((call) => call.key.startsWith("PATCH")).at(-1)?.body,
    ).toEqual({
      disabled: false,
    });
  });

  // A screen review before 17.0: Disable was a bare red button on every row, one click from a destructive
  // action. It is a danger item under the row's ⋯, as on the other admin tables; your own row has no ⋯.
  it("WEB-U-169 keeps Disable and Enable under the row's ⋯, Disable as danger", async () => {
    standard();
    show();
    const theirs = await row(colleague.email);
    expect(
      theirs.queryByRole("button", { name: /^(Disable|Enable)\b/ }),
    ).toBeNull();
    fireEvent.click(
      theirs.getByRole("button", {
        name: `More actions for ${colleague.email}`,
      }),
    );
    const item = screen.getByRole("menuitem", {
      name: `Disable ${colleague.email}`,
    });
    expect(item).toHaveTextContent("Disable…");
    expect(item).toHaveAttribute("data-danger", "true");
    const mine = await row(me.email);
    expect(mine.queryByRole("button", { name: /^More actions/ })).toBeNull();
  });

  // The fix's review: the row's ⋯ kept the pending guard the bare buttons had; while a change is saved,
  // Disable and Enable say so instead of acting twice.
  it("WEB-U-169 holds Disable while another change is being saved", async () => {
    standard();
    const answer = fetch.getMockImplementation();
    fetch.mockImplementation((input, init) =>
      init?.method === "PATCH"
        ? new Promise<Response>(() => undefined)
        : (answer as typeof globalThis.fetch)(input, init),
    );
    show();
    const theirs = await row(colleague.email);
    fireEvent.click(
      theirs.getByRole("button", {
        name: `Role for ${colleague.email}: Member`,
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /Administrator/ }),
    );
    fireEvent.click(
      theirs.getByRole("button", {
        name: `More actions for ${colleague.email}`,
      }),
    );
    const item = screen.getByRole("menuitem", {
      name: new RegExp(`^Disable ${colleague.email}`),
    });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveTextContent("Saving the last change…");
  });

  it("WEB-U-169 holds Enable while another change is being saved", async () => {
    standard();
    on(
      "GET /api/v1/users",
      json({ items: [accounts[0], { ...colleague, disabled: true }] }),
    );
    const answer = fetch.getMockImplementation();
    fetch.mockImplementation((input, init) =>
      init?.method === "PATCH"
        ? new Promise<Response>(() => undefined)
        : (answer as typeof globalThis.fetch)(input, init),
    );
    show();
    const theirs = await row(colleague.email);
    fireEvent.click(
      theirs.getByRole("button", {
        name: `Role for ${colleague.email}: Member`,
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /Administrator/ }),
    );
    fireEvent.click(
      theirs.getByRole("button", {
        name: `More actions for ${colleague.email}`,
      }),
    );
    const item = screen.getByRole("menuitem", {
      name: new RegExp(`^Enable ${colleague.email}`),
    });
    expect(item).toHaveAttribute("aria-disabled", "true");
  });

  // WEB-U-117 (a first-hour check): a role is chosen with the standard picker, at once; the gateway's
  // last-administrator rule still reads plainly if a race brings it.
  it("changes a role at once with the standard picker, and shows the last-administrator rule plainly", async () => {
    standard();
    on("PATCH /api/v1/users/" + colleague.id, problem(409, "last_admin"));
    show();
    const theirs = await row(colleague.email);
    fireEvent.click(
      theirs.getByRole("button", {
        name: `Role for ${colleague.email}: Member`,
      }),
    );
    expect(screen.queryByRole("combobox")).toBeNull();
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: "Administrator" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "At least one administrator must stay active.",
    );
    expect(calls.find((call) => call.key.startsWith("PATCH"))?.body).toEqual({
      role: "admin",
    });
  });

  // WEB-U-117: you can't change your own role here (demoting yourself, perhaps the last administrator, is never one
  // pick away); the row says so.
  it("shows your own role, fixed, with why", async () => {
    standard();
    show();
    const mine = await row(me.email);
    const role = mine.getByRole("button", {
      name: `Role for ${me.email}: Administrator`,
    });
    expect(role).toHaveAttribute("aria-disabled", "true");
    expect(role).toHaveAccessibleDescription(
      "You can't change your own role. Another administrator can.",
    );
    fireEvent.click(role);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  // WEB-U-117: an address is read whole: on one line when there is room, and never broken inside a word.
  it("breaks an email only after its @ or a dot", async () => {
    standard();
    show();
    const cell = (await row(colleague.email)).getByText(colleague.email, {
      selector: "span",
    });
    expect(cell.innerHTML).toBe("colleague@<wbr>example.<wbr>invalid");
  });

  it("invites by email and shows the link once with its expiry", async () => {
    standard();
    on(
      "POST /api/v1/invitations",
      json({ ...pending, token: "synthetic-invitation-token" }, 201),
    );
    show();
    await row(me.email);
    fireEvent.change(screen.getByRole("textbox", { name: "Email" }), {
      target: { value: "new@example.invalid" },
    });
    on("GET /api/v1/invitations", json({ items: [pending] }));
    fireEvent.click(screen.getByRole("button", { name: "Invite" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Invitation created",
    });
    expect(
      within(dialog).getByRole("textbox", { name: "Invitation link" }),
    ).toHaveValue(`${window.location.origin}/join/synthetic-invitation-token`);
    expect(dialog).toHaveTextContent(
      "Send this link to new@example.invalid yourself; nothing is emailed.",
    );
    expect(
      calls.find((call) => call.key === "POST /api/v1/invitations")?.body,
    ).toEqual({
      email: "new@example.invalid",
      role: "member",
    });
  });

  it("says when the email already has an account", async () => {
    standard();
    on("POST /api/v1/invitations", problem(409, "email_taken"));
    show();
    await row(me.email);
    fireEvent.change(screen.getByRole("textbox", { name: "Email" }), {
      target: { value: colleague.email },
    });
    fireEvent.click(screen.getByRole("button", { name: "Invite" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This email already has an account.",
    );
  });

  it("shows invitation status and revokes only pending ones after a confirm", async () => {
    standard();
    const accepted = {
      ...pending,
      id: "0195d8c0-0000-7000-8000-00000000000d",
      email: "joined@example.invalid",
      status: "accepted" as const,
    };
    on("GET /api/v1/invitations", json({ items: [pending, accepted] }));
    on("DELETE /api/v1/invitations/" + pending.id, empty);
    show();
    const open = await row(pending.email);
    expect(open.getByText("Pending")).toBeInTheDocument();
    // An accepted invitation's person is under People now, so the invitation is no longer listed.
    expect(screen.queryByText(accepted.email)).toBeNull();
    // WEB-U-191: Revoke is under the row's ⋯ as a danger item, as in every settings table, never a bare button.
    expect(open.queryByRole("button", { name: /^Revoke/ })).toBeNull();
    fireEvent.click(
      open.getByRole("button", {
        name: `More actions for the invitation to ${pending.email}`,
      }),
    );
    const revoke = screen.getByRole("menuitem", {
      name: `Revoke invitation for ${pending.email}`,
    });
    expect(revoke).toHaveTextContent("Revoke…");
    expect(revoke).toHaveAttribute("data-danger");
    fireEvent.click(revoke);
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "Revoke this invitation?" }),
      ).getByRole("button", { name: "Revoke invitation" }),
    );
    await vi.waitFor(() =>
      expect(calls.some((call) => call.key.startsWith("DELETE"))).toBe(true),
    );
  });

  // Adversarial review (Users P3): closed invitations stay listed forever. Pending ones stay, and an expired one for a
  // week so it can be sent again; accepted and revoked ones go.
  it("lists pending invitations, and expired ones for a week only", async () => {
    standard();
    const invitation = (email: string, extra: Partial<Invitation>) => ({
      ...pending,
      id: `0195d8c0-0000-7000-8000-${email.length.toString().padStart(12, "0")}`,
      email,
      ...extra,
    });
    on(
      "GET /api/v1/invitations",
      json({
        items: [
          pending,
          invitation("lapsed@example.invalid", {
            status: "expired",
            expires_at: "2026-01-20T00:00:00Z",
          }),
          invitation("old@example.invalid", {
            status: "expired",
            expires_at: "2026-01-01T00:00:00Z",
          }),
          invitation("gone@example.invalid", { status: "revoked" }),
        ],
      }),
    );
    show(me, () => new Date("2026-01-25T12:00:00Z"));
    expect((await row(pending.email)).getByText("Pending")).toBeInTheDocument();
    expect(
      (await row("lapsed@example.invalid")).getByText("Expired"),
    ).toBeInTheDocument();
    expect(screen.queryByText("old@example.invalid")).toBeNull();
    expect(screen.queryByText("gone@example.invalid")).toBeNull();
  });

  it("drops the Invitations heading when none are left to show", async () => {
    standard();
    on(
      "GET /api/v1/invitations",
      json({ items: [{ ...pending, status: "accepted" }] }),
    );
    // Query notifications run at once here (by default they wait a timer turn), so once the list has answered the
    // page has rendered it: no waiting on a clock (§2.3).
    notifyManager.setScheduler((callback) => callback());
    try {
      const cache = show();
      await waitFor(() =>
        expect(cache.getQueryState(["invitations"])?.status).toBe("success"),
      );
      await screen.findByText(me.email);
      await act(async () => {});
      // The list has answered and the people are shown: nothing is left under Invitations.
      expect(screen.queryByRole("heading", { name: "Invitations" })).toBeNull();
    } finally {
      notifyManager.setScheduler((callback) => setTimeout(callback, 0));
    }
  });

  it("invites the first colleague when you are alone", async () => {
    on("GET /api/v1/users", json({ items: [accounts[0]] }));
    on("GET /api/v1/invitations", json({ items: [] }));
    on(
      "GET /api/v1/conversations?all=true",
      json({ items: [], next_cursor: null }),
    );
    show();
    expect(
      await screen.findByText(
        "Only you so far. Invite someone to work with you.",
      ),
    ).toBeInTheDocument();
  });
});

// The 13f screen review, #7: People opened on an empty Invite form, the people below it. The people come first, then the
// pending invitations, then Invite someone.
// 17.3 (UI-GUIDELINES §7 Table): a table's numbers align on the right, in tabular figures; names and words on the left.
describe("WEB-U-443 People's counts on the right", () => {
  it("aligns the task counts and their column's label on the right", async () => {
    standard();
    show();
    await row(colleague.email);
    expect(screen.getByRole("columnheader", { name: "Tasks" })).toHaveClass(
      "numeric",
    );
    const counts = await screen.findAllByRole("link", {
      name: /^\d+ tasks?$/,
    });
    expect(counts.length).toBeGreaterThan(0);
    for (const count of counts)
      expect(count.closest("td")).toHaveClass("numeric");
  });
});

describe("WEB-U-156 People lists the people first", () => {
  it("puts the people before invitations and the invite form", async () => {
    standard();
    show();
    await row(colleague.email);
    // The section's heading, People, is the settings shell's (17.3): here its list, the first table, comes first.
    const people = screen.getAllByRole("table")[0] as HTMLElement;
    const invite = screen.getByRole("heading", {
      level: 2,
      name: "Invite someone",
    });
    expect(
      people.compareDocumentPosition(invite) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    const headings = screen
      .getAllByRole("heading", { level: 2 })
      .map((heading) => heading.textContent);
    expect(headings.at(-1)).toBe("Invite someone");
  });
});

// WEB-U-355 (a pass over the pages before 17.0): Invite says an empty or malformed email under the field in the app's
// words, never the browser's own bubble, and sends nothing.
describe("WEB-U-355 the invitee's email, said by the page", () => {
  it.each([
    ["", "Enter the person's email."],
    ["ana", "Enter an email address, such as ana@example.com."],
  ])("%j", async (typed, words) => {
    show();
    const invite = await screen.findByRole("button", { name: "Invite" });
    expect(invite.closest("form")).toHaveAttribute("novalidate");
    const email = screen.getByRole("textbox", { name: "Email" });
    fireEvent.change(email, { target: { value: typed } });
    fireEvent.click(invite);
    expect(await screen.findByText(words)).toBeInTheDocument();
    expect(email).toHaveAttribute("aria-invalid", "true");
    expect(
      fetch.mock.calls.filter(
        ([input, init]) =>
          target(input).includes("/invitations") && init?.method === "POST",
      ),
    ).toEqual([]);
  });
});

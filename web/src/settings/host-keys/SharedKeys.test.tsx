import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/error";
import {
  fakeAdminHostKeys,
  fakeHostKeys,
  hostKey,
  personalRow,
} from "../../test/host-keys";

import { AdminSharedKeys, MemberSharedKeys, SharedKeyPage } from "./SharedKeys";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  usePathname: () => "/settings/keys/shared",
}));
afterEach(() => vi.restoreAllMocks());

const now = () => new Date("2026-09-30T21:00:00Z");
/**
 * A member's own key as an administrator lists it, with the member's name for it included as a gateway might still
 * send it: the page must never show it (the owner's rule, least exposure: a member's key is exposed only to them).
 */
const memberKey = () => ({
  ...personalRow({ host: "github.com", last_use: null }),
  name: "GITHUB_WORK",
});
function wrap(children: ReactNode) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={cache}>{children}</QueryClientProvider>,
  );
}
const field = (name: string) => screen.getByLabelText(name, { exact: true });
const type = (name: string, value: string) =>
  fireEvent.change(field(name), { target: { value } });
const save = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

const pypi = hostKey({
  name: "PYPI_MIRROR",
  host: "pypi.acme.lan",
  path_prefix: null,
  shared: true,
  users: "all",
  last_use: {
    at: "2026-09-30T09:40:00Z",
    count: 3,
    conversation_id: "c1",
    task_title: "Build wheels",
    outcome: "ok",
    user: { id: "u-sam", email: "sam@acme.test" },
  },
});
const jira = hostKey({
  name: "JIRA_BOT",
  host: "acme.atlassian.net",
  path_prefix: null,
  shared: true,
  users: [
    { id: "u-ana", email: "ana@acme.test" },
    { id: "u-sam", email: "sam@acme.test" },
  ],
});

// The reviewed Keys sketch §5 (ADR-0037 §1, §9): administrators make shared keys and choose who may use them; they see
// members' own keys only to delete them.
describe("WEB-U-145 Keys › Shared for administrators", () => {
  it("heads the shared keys at the page's second level, for administrators and members", async () => {
    const admin = wrap(
      <AdminSharedKeys
        api={fakeAdminHostKeys().api}
        now={now}
        timeZone="UTC"
      />,
    );
    expect(
      await screen.findByRole("heading", {
        name: "Shared keys for web services",
        level: 2,
      }),
    ).toBeVisible();
    admin.unmount();
    wrap(<MemberSharedKeys api={fakeHostKeys().api} now={now} />);
    expect(
      await screen.findByRole("heading", {
        name: "Shared keys for web services",
        level: 2,
      }),
    ).toBeVisible();
  });

  it("lists the shared keys with who may use them and their last use, by whom on hover", async () => {
    wrap(
      <AdminSharedKeys
        api={fakeAdminHostKeys({ items: [pypi, jira] }).api}
        now={now}
        timeZone="UTC"
      />,
    );
    const table = await screen.findByRole("table", {
      name: "Shared keys for web services",
    });
    const [first, second] = within(table).getAllByRole("row").slice(1);
    expect(first).toHaveTextContent(
      /PYPI_MIRROR.*pypi\.acme\.lan.*Everyone.*Today, 09:40 · Build wheels/,
    );
    expect(
      within(first as HTMLElement).getByText(/Today, 09:40/),
    ).toHaveAttribute(
      "title",
      "Used 3 times in Build wheels, last on Sep 30, 2026, 09:40, by sam@acme.test",
    );
    expect(second).toHaveTextContent(
      /JIRA_BOT.*acme\.atlassian\.net.*2 people.*Never used/,
    );
    // Not on hover only: a button that keyboard and touch can open too (the review of 938fa83c, item 17).
    fireEvent.click(
      within(second as HTMLElement).getByRole("button", { name: "2 people" }),
    );
    const who = screen.getByRole("region", { name: "Who may use JIRA_BOT" });
    expect(
      within(who)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["ana@acme.test", "sam@acme.test"]);
    expect(
      within(table).getByRole("link", { name: "PYPI_MIRROR" }),
    ).toHaveAttribute("href", `/settings/keys/shared/${pypi.id}`);
    expect(
      screen.getByRole("link", { name: "Add a shared key for a web service" }),
    ).toHaveAttribute("href", "/settings/keys/shared/new");
  });

  it("shows members' own keys only to delete, after asking", async () => {
    const own = memberKey();
    const { api, list } = fakeAdminHostKeys({ items: [pypi, own] });
    wrap(<AdminSharedKeys api={api} now={now} timeZone="UTC" />);
    const table = await screen.findByRole("table", {
      name: "Members' own keys for web services",
    });
    const [row] = within(table).getAllByRole("row").slice(1);
    expect(row).toHaveTextContent(/ana@acme\.test.*github\.com.*Never used/);
    // Its owner, web address and last use; never the member's own name for it, nor its value.
    expect(document.body).not.toHaveTextContent("GITHUB_WORK");
    // Never a link to change it: an administrator may only delete it.
    expect(within(row as HTMLElement).queryByRole("link")).toBeNull();
    // The row's ⋯ with one danger item, as every settings table (the review of 938fa83c, item 16).
    fireEvent.click(
      within(row as HTMLElement).getByRole("button", {
        name: "More actions for ana@acme.test's key for github.com",
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete…" }));
    const dialog = screen.getByRole("dialog", {
      name: "Delete ana@acme.test's key for github.com?",
    });
    expect(dialog).toHaveTextContent(
      "Their tasks can't use github.com with it any more. Only ana@acme.test can add it again.",
    );
    expect(document.body).not.toHaveTextContent("GITHUB_WORK");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    expect(
      await screen.findByText("Deleted ana@acme.test's key for github.com."),
    ).toBeVisible();
    expect(list.map((item) => item.id)).toEqual([pypi.id]);
  });

  // Least exposure (the review of 5121d0d0, item 23): a member's key's last use is its time alone for an administrator,
  // never the member's task, nor whether it was refused and why.
  it("shows when a member's key was last used, and nothing about the task", async () => {
    const used = {
      ...memberKey(),
      last_use: {
        at: "2026-09-30T09:40:00Z",
        count: 3,
        conversation_id: "c9",
        task_title: "Sales report",
        outcome: "ok" as const,
      },
    };
    const refused = {
      ...personalRow({ id: "k-2", host: "gitlab.com" }),
      owner: { id: "u-sam", email: "sam@acme.test" },
      last_use: {
        at: "2026-09-28T08:00:00Z",
        count: 1,
        conversation_id: null,
        task_title: null,
        outcome: "expired" as const,
      },
    };
    wrap(
      <AdminSharedKeys
        api={fakeAdminHostKeys({ items: [used, refused] }).api}
        now={now}
        timeZone="UTC"
      />,
    );
    const table = await screen.findByRole("table", {
      name: "Members' own keys for web services",
    });
    const [first, second] = within(table).getAllByRole("row").slice(1);
    expect(first).toHaveTextContent(
      /ana@acme\.test.*github\.com.*Today, 09:40/,
    );
    expect(second).toHaveTextContent(/sam@acme\.test.*gitlab\.com.*Sep 28/);
    expect(table).not.toHaveTextContent(/Sales report|task|refused|ended/i);
    expect(table.querySelector("[title]")).toBeNull();
  });

  it("deletes a member's key that changed meanwhile after saying so, as it is now", async () => {
    const own = memberKey();
    const { api, list, calls } = fakeAdminHostKeys({ items: [own] });
    wrap(<AdminSharedKeys api={api} now={now} timeZone="UTC" />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for ana@acme.test's key for github.com",
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete…" }));
    list[0] = { ...own, version: own.version + 1 };
    const dialog = screen.getByRole("dialog", {
      name: "Delete ana@acme.test's key for github.com?",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    expect(
      await within(dialog).findByText(
        "Someone changed this key since you opened it. Delete again to delete it as it is now.",
      ),
    ).toBeVisible();
    await waitFor(() => expect(calls.keys).toHaveBeenCalledTimes(2));
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    expect(
      await screen.findByText("Deleted ana@acme.test's key for github.com."),
    ).toBeVisible();
    expect(calls.deleteKey).toHaveBeenLastCalledWith(own.id, own.version + 1);
    expect(list).toEqual([]);
  });

  it("leaves itself out on a gateway without host keys", async () => {
    const { api, calls } = fakeAdminHostKeys({ missing: true });
    const { container } = wrap(<AdminSharedKeys api={api} now={now} />);
    await waitFor(() => expect(calls.keys).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container).toBeEmptyDOMElement();
  });
});

// The reviewed Keys sketch §6 and decision 2: members see the shared keys they may use, read-only, with only their own
// last use, and no one else's names.
describe("WEB-U-146 Keys › Shared for members", () => {
  it("lists the shared keys they may use, read-only, with their own last use", async () => {
    const mine = {
      ...pypi,
      users: null,
      last_use: {
        ...(pypi.last_use ?? {
          at: "",
          count: 0,
          conversation_id: null,
          task_title: null,
          outcome: "ok" as const,
        }),
        user: null,
      },
    };
    wrap(
      <MemberSharedKeys
        api={fakeHostKeys({ shared: [mine] }).api}
        now={now}
        timeZone="UTC"
      />,
    );
    const table = await screen.findByRole("table", {
      name: "Shared keys you may use",
    });
    const [row] = within(table).getAllByRole("row").slice(1);
    expect(row).toHaveTextContent(
      /PYPI_MIRROR.*pypi\.acme\.lan.*Today, 09:40 · Build wheels/,
    );
    // No column that says the same on every row (the review of 938fa83c, item 18).
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual(["Name", "Web address", "Last used"]);
    expect(within(table).queryByRole("button")).toBeNull();
    expect(within(table).queryByRole("link")).toBeNull();
    expect(table).not.toHaveTextContent("@");
  });

  it("says so when there are none", async () => {
    wrap(<MemberSharedKeys api={fakeHostKeys().api} now={now} />);
    expect(
      await screen.findByText("No shared keys for you yet."),
    ).toBeVisible();
  });
});

// ADR-0037 §1: who may use a shared key, everyone or chosen people; on its page, added or changed.
describe("WEB-U-144 who may use a shared key", () => {
  it("adds a shared key for everyone by default, sent as all", async () => {
    const { api, sent } = fakeAdminHostKeys();
    const onDone = vi.fn();
    wrap(<SharedKeyPage api={api} onDone={onDone} />);
    expect(
      await screen.findByRole("heading", { name: "Add a shared key" }),
    ).toBeVisible();
    expect(
      screen.getByRole("region", { name: "Who may use it" }),
    ).toBeVisible();
    expect(
      screen.getByRole("radio", { name: "Everyone in this installation" }),
    ).toBeChecked();
    type("Web address", "pypi.acme.lan");
    type("Name", "PYPI_MIRROR");
    type("Key", "pypi-secret");
    save();
    await waitFor(() =>
      expect(onDone).toHaveBeenCalledWith("Saved PYPI_MIRROR."),
    );
    expect(sent).toEqual([
      expect.objectContaining({ name: "PYPI_MIRROR", users: "all" }),
    ]);
    expect(screen.getByRole("link", { name: "Shared keys" })).toHaveAttribute(
      "href",
      "/settings/keys/shared",
    );
  });

  it("chooses people, shows them as chips with ×, and sends their ids", async () => {
    const { api, sent } = fakeAdminHostKeys();
    const onDone = vi.fn();
    wrap(<SharedKeyPage api={api} onDone={onDone} />);
    fireEvent.click(
      await screen.findByRole("radio", { name: "Chosen people" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: /^Add a person/ }),
    );
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: "ana@acme.test" }),
    );
    fireEvent.click(screen.getByRole("button", { name: /^Add a person/ }));
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: "sam@acme.test" }),
    );
    const chosen = screen.getByRole("list", { name: "People who may use it" });
    expect(
      within(chosen)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["ana@acme.test", "sam@acme.test"]);
    fireEvent.click(
      screen.getByRole("button", { name: "Remove sam@acme.test" }),
    );
    type("Web address", "acme.atlassian.net");
    type("Name", "JIRA_BOT");
    type("Key", "jira-secret");
    save();
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(sent[0]).toMatchObject({ users: ["u-ana"] });
  });

  it("refuses Chosen people with nobody chosen", async () => {
    const { api, calls } = fakeAdminHostKeys();
    wrap(<SharedKeyPage api={api} onDone={vi.fn()} />);
    fireEvent.click(
      await screen.findByRole("radio", { name: "Chosen people" }),
    );
    type("Web address", "acme.atlassian.net");
    type("Name", "JIRA_BOT");
    type("Key", "jira-secret");
    save();
    expect(
      await screen.findByText("Choose at least one person, or Everyone."),
    ).toBeVisible();
    // The picker can take the focus; the choice's fieldset can't (the review of 938fa83c, item 14).
    const picker = screen.getByRole("button", { name: /^Add a person/ });
    expect(picker).toHaveAttribute("aria-invalid", "true");
    expect(picker).toHaveAccessibleDescription(
      "Choose at least one person, or Everyone.",
    );
    await waitFor(() => expect(picker).toHaveFocus());
    expect(
      screen.getByRole("radiogroup", { name: "Who may use it" }),
    ).not.toHaveAttribute("aria-invalid");
    expect(calls.addShared).not.toHaveBeenCalled();
  });

  it("opens a shared key with its people, and saves them with its version", async () => {
    const { api, sent } = fakeAdminHostKeys({ items: [jira] });
    const onDone = vi.fn();
    wrap(
      <SharedKeyPage
        api={api}
        id={jira.id}
        onDone={onDone}
        now={now}
        timeZone="UTC"
      />,
    );
    expect(
      await screen.findByRole("heading", { name: "JIRA_BOT" }),
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "Chosen people" })).toBeChecked();
    fireEvent.click(
      screen.getByRole("radio", { name: "Everyone in this installation" }),
    );
    save();
    await waitFor(() => expect(onDone).toHaveBeenCalledWith("Saved JIRA_BOT."));
    expect(sent[0]).toMatchObject({ version: 1, users: "all" });
  });

  // ADR-0037 §1: names are unique across a member's own keys and the shared keys they may use. The gateway refuses
  // without naming whose key it is, so an administrator never sees members' key names that way (GW-I-170).
  it.each([
    [
      "the code and its detail",
      new ApiError(409, "name_taken_by_user", "Taken.", undefined, [
        { field: "name", reason: "name_taken_by_user" },
      ]),
    ],
    ["the code alone", new ApiError(409, "name_taken_by_user", "Taken.")],
  ])(
    "says at Name when someone who may use it has their own key of that name (%s)",
    async (_, refusal) => {
      const { api } = fakeAdminHostKeys({ refuse: refusal });
      wrap(<SharedKeyPage api={api} onDone={vi.fn()} />);
      await screen.findByLabelText("Web address", { exact: true });
      type("Web address", "github.com");
      type("Name", "GITHUB_TOKEN");
      type("Key", "secret");
      save();
      const words =
        "Someone who may use this key already has their own key named GITHUB_TOKEN. Choose another name.";
      expect(await screen.findByText(words)).toBeVisible();
      expect(field("Name")).toHaveAttribute("aria-invalid", "true");
      expect(field("Name")).toHaveAccessibleDescription(
        expect.stringContaining(words),
      );
      expect(screen.queryByText(/ana@|sam@/)).toBeNull();
    },
  );

  it("names a chosen person no longer in the installation as a removed account", async () => {
    const gone = {
      ...jira,
      users: [
        { id: "u-ana", email: "ana@acme.test" },
        { id: "u-gone", email: "gone@acme.test" },
      ],
    };
    const { api } = fakeAdminHostKeys({ items: [gone] });
    wrap(<SharedKeyPage api={api} id={gone.id} onDone={vi.fn()} />);
    const chosen = await screen.findByRole("list", {
      name: "People who may use it",
    });
    expect(
      within(chosen)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["ana@acme.test", "Removed account"]);
    expect(chosen).not.toHaveTextContent("u-gone");
  });

  it("checks a shared key's name against the other shared keys", async () => {
    const { api, calls } = fakeAdminHostKeys({ items: [pypi] });
    wrap(<SharedKeyPage api={api} onDone={vi.fn()} />);
    await waitFor(() => expect(calls.keys).toHaveBeenCalled());
    type("Web address", "pypi.org");
    type("Name", "PYPI_MIRROR");
    type("Key", "secret");
    save();
    expect(
      await screen.findByText(
        "PYPI_MIRROR is already the shared key for pypi.acme.lan. Pick another name.",
      ),
    ).toBeVisible();
    expect(calls.addShared).not.toHaveBeenCalled();
  });
});

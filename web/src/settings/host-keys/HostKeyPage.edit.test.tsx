import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeHostKeys, hostKey } from "../../test/host-keys";

import type { HostKey, HostKeysApi } from "./contract";
import { HostKeyPage } from "./HostKeyPage";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  usePathname: () => "/settings/keys/k",
}));
afterEach(() => vi.restoreAllMocks());

const key = (overrides: Partial<HostKey> = {}) =>
  hostKey({
    name: "GITHUB_WORK",
    host: "github.com",
    path_prefix: "/acme",
    last_use: {
      at: "2026-09-30T14:02:00Z",
      count: 12,
      conversation_id: "c1",
      task_title: "Sales report",
      user: null,
      outcome: "ok",
    },
    ...overrides,
  });

function show(api: HostKeysApi, id: string) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const onDone = vi.fn();
  render(
    <QueryClientProvider client={cache}>
      <HostKeyPage
        api={api}
        id={id}
        onDone={onDone}
        now={() => new Date("2026-09-30T21:00:00Z")}
        timeZone="UTC"
      />
    </QueryClientProvider>,
  );
  return { onDone };
}
const field = (name: string) => screen.getByLabelText(name, { exact: true });
const save = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

// ADR-0037 §9 and the reviewed Keys sketch §4: a key's own page shows what it is and when it was used, never its value;
// its fields change and save as a whole; the value is replaced in place; deleting asks first.
describe("WEB-U-143 a key's own page", () => {
  it("shows the key: its name, address and way, its last use, and when its value was set", async () => {
    const item = key();
    show(fakeHostKeys({ keys: [item] }).api, item.id);
    expect(
      await screen.findByRole("heading", { name: "GITHUB_WORK" }),
    ).toBeVisible();
    expect(screen.getByText("github.com/acme · as a token")).toBeVisible();
    expect(screen.getByText("Today, 14:02 · Sales report")).toBeVisible();
    expect(screen.getByText("Set on Sep 12, 2026")).toBeVisible();
    // Its fields, as it is.
    expect(field("Web address")).toHaveValue("github.com/acme");
    expect(field("Name")).toHaveValue("GITHUB_WORK");
    expect(screen.getByRole("radio", { name: /^As a token/ })).toBeChecked();
    // Never a value: no field holds one until Replace.
    expect(screen.queryByLabelText("New key")).toBeNull();
  });

  it("opens all its uses", async () => {
    const item = key();
    const { api, calls } = fakeHostKeys({ keys: [item] });
    show(api, item.id);
    fireEvent.click(
      await screen.findByRole("button", { name: "Show all uses" }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Uses of GITHUB_WORK" }),
    ).toBeVisible();
    expect(calls.reads).toHaveBeenCalledWith(
      item.id,
      undefined,
      expect.anything(),
    );
  });

  it("saves its fields as a whole, with the version it read, and no value", async () => {
    const item = key();
    const { api, updated } = fakeHostKeys({ keys: [item] });
    const { onDone } = show(api, item.id);
    fireEvent.change(await screen.findByLabelText("Name", { exact: true }), {
      target: { value: "GITHUB_ACME" },
    });
    save();
    await waitFor(() =>
      expect(onDone).toHaveBeenCalledWith("Saved GITHUB_ACME."),
    );
    expect(updated).toEqual([
      {
        version: 1,
        name: "GITHUB_ACME",
        address: "https://github.com/acme",
        scheme: "bearer",
      },
    ]);
  });

  it("replaces the value in place, and sends it only then", async () => {
    const item = key();
    const { api, updated } = fakeHostKeys({ keys: [item] });
    const { onDone } = show(api, item.id);
    fireEvent.click(await screen.findByRole("button", { name: "Replace…" }));
    const fresh = field("New key");
    expect(fresh).toHaveAttribute("type", "password");
    expect(fresh).toHaveFocus();
    // Empty, it asks for the key.
    save();
    expect(await screen.findByText("Paste the key.")).toBeVisible();
    fireEvent.change(fresh, { target: { value: "ghp_new" } });
    save();
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(updated.at(-1)).toMatchObject({ value: "ghp_new", version: 1 });
  });

  it("keeps the current value when replacing is called off", async () => {
    const item = key();
    const { api, updated } = fakeHostKeys({ keys: [item] });
    show(api, item.id);
    fireEvent.click(await screen.findByRole("button", { name: "Replace…" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Keep the current key" }),
    );
    expect(screen.queryByLabelText("New key")).toBeNull();
    save();
    await waitFor(() => expect(updated).toHaveLength(1));
    expect(updated[0]).not.toHaveProperty("value");
  });

  it("lets it keep its own name, and refuses another key's", async () => {
    const item = key();
    const other = key({
      name: "NOTION_KEY",
      host: "api.notion.com",
      path_prefix: null,
    });
    const { api, calls } = fakeHostKeys({ keys: [item, other] });
    show(api, item.id);
    fireEvent.change(await screen.findByLabelText("Name", { exact: true }), {
      target: { value: "NOTION_KEY" },
    });
    save();
    expect(
      await screen.findByText(
        "NOTION_KEY is already your key for api.notion.com. Pick another name.",
      ),
    ).toBeVisible();
    expect(calls.updateKey).not.toHaveBeenCalled();
  });

  it("asks before deleting, then goes back saying so", async () => {
    const item = key();
    const { api, items } = fakeHostKeys({ keys: [item] });
    const { onDone } = show(api, item.id);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for GITHUB_WORK",
      }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete…" }));
    const dialog = screen.getByRole("dialog", { name: "Delete GITHUB_WORK?" });
    expect(dialog).toHaveTextContent(
      "Tasks can't use github.com/acme with it any more. A run that's using it now loses it at once.",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    await waitFor(() =>
      expect(onDone).toHaveBeenCalledWith("Deleted GITHUB_WORK."),
    );
    expect(items).toEqual([]);
  });

  // The review of b608b50f, item 10: as every settings form, a conflict keeps the person's edits and takes the new
  // version, so saving again replaces the other change.
  it("keeps the person's edits when someone changed the key meanwhile, and saves them over it next time", async () => {
    const item = key();
    const { api, items, updated } = fakeHostKeys({ keys: [item] });
    const { onDone } = show(api, item.id);
    await screen.findByRole("heading", { name: "GITHUB_WORK" });
    // Another tab renamed it.
    items[0] = { ...item, name: "GITHUB_OTHER", version: 2 };
    fireEvent.change(field("Name"), { target: { value: "GITHUB_ACME" } });
    fireEvent.click(screen.getByRole("button", { name: "Replace…" }));
    fireEvent.change(field("New key"), { target: { value: "ghp_pasted" } });
    save();
    expect(
      await screen.findByText(
        "Someone changed this key since you opened it. Your edits are kept: save again to replace their change with yours.",
      ),
    ).toBeVisible();
    expect(field("Name")).toHaveValue("GITHUB_ACME");
    expect(field("New key")).toHaveValue("ghp_pasted");
    expect(onDone).not.toHaveBeenCalled();
    save();
    await waitFor(() =>
      expect(onDone).toHaveBeenCalledWith("Saved GITHUB_ACME."),
    );
    expect(updated.at(-1)).toMatchObject({
      version: 2,
      name: "GITHUB_ACME",
      value: "ghp_pasted",
    });
  });

  // The review of b608b50f, item 11: a delete that a change beat takes the new version, so deleting again works.
  it("takes the key's new version when a delete was refused as changed, and deletes on the next try", async () => {
    const item = key();
    const { api, items } = fakeHostKeys({ keys: [item] });
    const { onDone } = show(api, item.id);
    await screen.findByRole("heading", { name: "GITHUB_WORK" });
    items[0] = { ...item, version: 2 };
    const menu = () =>
      fireEvent.click(
        screen.getByRole("button", { name: "More actions for GITHUB_WORK" }),
      );
    menu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete…" }));
    const dialog = screen.getByRole("dialog", { name: "Delete GITHUB_WORK?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    expect(
      await within(dialog).findByText(
        "Someone changed this key since you opened it. Delete again to delete it as it is now.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    await waitFor(() =>
      expect(onDone).toHaveBeenCalledWith("Deleted GITHUB_WORK."),
    );
    expect(items).toEqual([]);
  });

  it("says plainly when the key no longer exists", async () => {
    show(fakeHostKeys({ keys: [] }).api, "gone");
    expect(await screen.findByText("This key no longer exists.")).toBeVisible();
    expect(screen.getByRole("link", { name: "Keys" })).toHaveAttribute(
      "href",
      "/settings/keys",
    );
  });

  it("asks once to check a migrated key's name, and Save confirms it", async () => {
    const item = key({ check_name: true });
    const { api, updated } = fakeHostKeys({ keys: [item] });
    const { onDone } = show(api, item.id);
    expect(
      await screen.findByText(
        "This key's name was chosen for you from its address. Check it, then Save.",
      ),
    ).toBeVisible();
    save();
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(updated).toHaveLength(1);
  });
});

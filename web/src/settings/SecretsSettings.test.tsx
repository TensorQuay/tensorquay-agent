import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError, failure } from "../api/error";
import type { User } from "../protocol/generated/api";
import { fakeAdmin, secretItem } from "../test/admin";
import { LIMITS } from "../test/user";

import type { AdminApi } from "./admin/contract";
import { SecretsSettings } from "./SecretsSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const secret = secretItem;
const seeded = [
  secret("deepseek", {
    seed: "current",
    last_set_by: "seed",
    routes: ["deepseek-flash", "deepseek-v4-pro"],
  }),
  secret("old-provider", { seed: "removed", last_set_by: "seed" }),
  secret("spare"),
];
const many = secret("gateway", {
  routes: ["gateway-fast", "gateway-long", "gateway-vision"],
});
// A synthetic marker, never a real credential: the page must never show it back.
const VALUE = "fixture-secret-value-0001";

function show(api: AdminApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <SecretsSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
const row = (name: string) =>
  screen.getByRole("row", { name: new RegExp(`^${name}(?![\\w-])`) });
/** Opens a shared key's "…" menu and chooses one of its items (UI-GUIDELINES §7). */
function menuItem(ref: string, item: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `More actions for ${ref}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
}
/** Adds a shared key in its dialog (opening it when a refusal has not kept it open), as an administrator would. */
async function add(name: string, value: string) {
  if (!screen.queryByRole("dialog", { name: "Add a shared key" }))
    fireEvent.click(screen.getByRole("button", { name: "Add a shared key" }));
  const dialog = screen.getByRole("dialog", { name: "Add a shared key" });
  fireEvent.change(within(dialog).getByLabelText("Name"), {
    target: { value: name },
  });
  fireEvent.change(within(dialog).getByLabelText("Value"), {
    target: { value },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Add key" }));
  await Promise.resolve();
}

// FR-22, FR-28: shared secrets for administrators, write-only (WEB-U-035).
// One word on the page, "key", never "secret": names without the secrets/ reference, which Who may use states.
describe("WEB-U-035 shared keys", () => {
  // As on the Models page, a file-managed list still says where its keys come from, and when every key
  // has left the file, its marker is what says to delete them.
  it.each([
    ["current", "Configuration file"],
    ["removed", "No longer in the configuration file"],
  ] as const)(
    "notes a key's source when every key is %s in the configuration file",
    async (seed, words) => {
      show(
        fakeAdmin({
          secrets: [
            secret("deepseek", { seed, last_set_by: "seed" }),
            secret("openai", { seed, last_set_by: "seed" }),
          ],
        }).api,
      );
      await screen.findByRole("table", { name: "Shared keys" });
      expect(within(row("deepseek")).getByText(words)).toBeVisible();
      expect(within(row("openai")).getByText(words)).toBeVisible();
    },
  );

  it("lists names, users and sources without any value, and explains a blocked delete", async () => {
    show(fakeAdmin({ secrets: seeded }).api);
    const table = await screen.findByRole("table", { name: "Shared keys" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["Name", "Used by", "Last changed", "Actions"]);
    expect(
      within(row("deepseek")).getByText("deepseek-flash, deepseek-v4-pro"),
    ).toBeVisible();
    expect(
      within(row("deepseek")).getByText("Configuration file"),
    ).toBeVisible();
    expect(
      within(row("old-provider")).getByText(
        "No longer in the configuration file",
      ),
    ).toBeVisible();
    // Where a key came from only when it isn't the usual (added here): a small note under the name, no column.
    expect(within(row("spare")).queryByText("Added here")).toBeNull();
    expect(within(row("spare")).getByText("Not used")).toBeVisible();
    // The name alone: secrets/ is the reference, not the name, and "secret" is not the page's word.
    const name = within(row("deepseek")).getByText("deepseek");
    expect(name).toBeVisible();
    // The name in the page's sans type, not code.
    expect(name.closest("code, .type-code")).toBeNull();
    expect(table).not.toHaveTextContent(/secrets\/|[Ss]ecret/);
    // A key in use keeps Delete in its menu, reachable and saying why it can't run (UI-GUIDELINES §5b).
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for shared key deepseek",
      }),
    );
    const blocked = screen.getByRole("menuitem", {
      name: "Delete shared key deepseek",
    });
    expect(blocked).toHaveAttribute("aria-disabled", "true");
    expect(blocked).toHaveAccessibleDescription(
      "A model route uses it; change the route first.",
    );
    expect(within(blocked).getByText(/change the route first/)).toBeVisible();
    fireEvent.click(blocked);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // One actions column (UI-GUIDELINES §7): Replace, and a menu with the rest; no actions in the data cells.
  it("gives each key Replace and a menu, keeps names on one line and counts more than two models", async () => {
    show(fakeAdmin({ secrets: [...seeded, many] }).api);
    await screen.findByRole("table", { name: "Shared keys" });
    for (const name of ["deepseek", "spare"])
      expect(
        within(row(name))
          .getAllByRole("button")
          .map((button) => button.getAttribute("aria-label")),
      ).toEqual([
        `Replace shared key ${name}`,
        `More actions for shared key ${name}`,
      ]);
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for shared key spare" }),
    );
    expect(
      screen
        .getAllByRole("menuitem")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual([
      "Show reads of shared key spare",
      "Change who may use shared key spare",
      "Set the reminder for shared key spare",
      "Delete shared key spare",
    ]);
    expect(
      screen.getByRole("menuitem", { name: "Delete shared key spare" }),
    ).not.toHaveAttribute("aria-disabled");
    // A long name truncates on one line; the full reference is on hover (WEB-U-181, as on Models) and the name in the
    // row's accessible name.
    const name = within(row("gateway")).getByText("gateway");
    expect(name).toHaveClass("secret-ref-line");
    expect(name).toHaveAttribute("title", "secrets/gateway");
    // Up to two models are named; more read as a count with the list on hover and for screen readers.
    const count = within(row("gateway")).getByText("3 models");
    expect(count).toHaveAttribute(
      "title",
      "gateway-fast, gateway-long, gateway-vision",
    );
    expect(row("gateway")).toHaveAccessibleName(
      /gateway-fast, gateway-long, gateway-vision/,
    );
    expect(
      within(row("deepseek")).getByText("deepseek-flash, deepseek-v4-pro"),
    ).toBeVisible();
  });

  it("says shared keys serve connectors as well as model routes, and gives the reference in Who may use", async () => {
    show(fakeAdmin({ secrets: seeded }).api);
    await screen.findByRole("table", { name: "Shared keys" });
    expect(
      screen.getByText(/reach model providers and connectors\./),
    ).toBeVisible();
    menuItem("shared key deepseek", "Change who may use shared key deepseek");
    const dialog = screen.getByRole("dialog", {
      name: "Who may use shared key deepseek",
    });
    expect(
      within(dialog).getByText(
        "Configuration files refer to it as secrets/deepseek.",
      ),
    ).toBeVisible();
  });

  // One main action, as on Mine: a button opens the add dialog; no form sits under the table.
  it("adds from an Add a shared key dialog, not a form under the table", async () => {
    show(fakeAdmin({ secrets: seeded }).api);
    await screen.findByRole("table", { name: "Shared keys" });
    expect(screen.queryByLabelText("Name")).toBeNull();
    expect(screen.queryByLabelText("Value")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add a shared key" }));
    const dialog = screen.getByRole("dialog", { name: "Add a shared key" });
    expect(
      within(dialog).getByText(
        "Letters, digits, - and _, up to 120 characters.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("checks a new key before sending it, then adds it without keeping the value", async () => {
    const admin = fakeAdmin({ secrets: seeded });
    show(admin.api);
    await screen.findByRole("table", { name: "Shared keys" });
    await add("has space", VALUE);
    expect(
      screen.getByText("Use letters, digits, - and _, up to 120 characters."),
    ).toBeVisible();
    await add("git-mine", VALUE);
    expect(
      screen.getByText(
        "Names starting with git- or connector- are reserved for people's own credentials.",
      ),
    ).toBeVisible();
    await add("openai", "");
    expect(screen.getByText("Enter a value.")).toBeVisible();
    expect(admin.writes).toEqual([]);
    await add("openai", VALUE);
    expect(await screen.findByText("Added shared key openai.")).toBeVisible();
    expect(row("openai")).toBeVisible();
    expect(admin.writes).toEqual([
      { name: "openai", key: expect.any(String) as string },
    ]);
    // The dialog closes on success and keeps nothing.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.body.textContent).not.toContain(VALUE);
  });

  // Who may use a new key is chosen as it is added, in the same dialog, not in a second one afterwards.
  it("adds a key and who may use it in one step", async () => {
    const admin = fakeAdmin({
      secrets: seeded,
      choices: [
        { kind: "route", name: "local" },
        { kind: "connector", name: "tracker" },
      ],
    });
    show(admin.api);
    await screen.findByRole("table", { name: "Shared keys" });
    fireEvent.click(screen.getByRole("button", { name: "Add a shared key" }));
    const dialog = screen.getByRole("dialog", { name: "Add a shared key" });
    const routes = await within(dialog).findByRole("group", {
      name: "Model routes",
    });
    expect(
      within(dialog).getByText(
        "Routes and connectors that already name this key may use it anyway.",
      ),
    ).toBeVisible();
    fireEvent.click(within(routes).getByRole("checkbox", { name: "local" }));
    await add("openai", VALUE);
    expect(await screen.findByText("Added shared key openai.")).toBeVisible();
    expect(admin.consumerWrites).toEqual([
      {
        name: "openai",
        consumers: [{ kind: "route", name: "local" }],
        version: 1,
      },
    ]);
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows the gateway's named problems at their fields", async () => {
    const admin = fakeAdmin({ secrets: seeded });
    const api: AdminApi = {
      ...admin.api,
      putSecret: vi.fn(() =>
        Promise.reject(
          new ApiError(400, "invalid_request", "x", undefined, [
            { field: "value", reason: "value_too_long" },
          ]),
        ),
      ),
    };
    show(api);
    await screen.findByRole("table", { name: "Shared keys" });
    await add("openai", VALUE);
    expect(await screen.findByText("The value is too long.")).toBeVisible();
    expect(screen.getByLabelText("Value")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  // details[].reason is open in the browser: a reason this web does not know keeps the refusal and reads generically.
  it("shows a reason this web does not know as the field's generic problem", async () => {
    const admin = fakeAdmin({ secrets: seeded });
    const body = {
      error: {
        code: "invalid_request",
        message: "The secret was refused.",
        retryable: false,
        details: [{ field: "value", reason: "value_from_a_newer_gateway" }],
      },
    };
    const refusal = failure(400, new Headers(), body);
    expect(refusal.code).toBe("invalid_request");
    const api: AdminApi = {
      ...admin.api,
      putSecret: vi.fn(() => Promise.reject(refusal)),
    };
    show(api);
    await screen.findByRole("table", { name: "Shared keys" });
    await add("openai", VALUE);
    expect(await screen.findByText("Check this field.")).toBeVisible();
    expect(screen.getByLabelText("Value")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("replaces with the version it read, and recovers from another admin's change", async () => {
    const admin = fakeAdmin({ secrets: seeded });
    show(admin.api);
    await screen.findByRole("table", { name: "Shared keys" });
    fireEvent.click(
      screen.getByRole("button", { name: "Replace shared key deepseek" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Replace shared key deepseek",
    });
    expect(
      within(dialog).getByText(
        "This key comes from the configuration file. Your value stays until the file's value for it changes.",
      ),
    ).toBeVisible();
    admin.touch("secrets/deepseek");
    fireEvent.change(within(dialog).getByLabelText("New value"), {
      target: { value: VALUE },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Replace key" }),
    );
    expect(
      await within(dialog).findByText(
        "Someone changed this key since you opened the page. The list is now current; try again.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Replace key" }),
    );
    await screen.findByText("Updated shared key deepseek.");
    expect(admin.writes.map((write) => write.version)).toEqual([1, 2]);
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("deletes an unused key after a named confirmation", async () => {
    const admin = fakeAdmin({ secrets: seeded });
    show(admin.api);
    await screen.findByRole("table", { name: "Shared keys" });
    menuItem("shared key spare", "Delete shared key spare");
    const dialog = screen.getByRole("dialog", {
      name: "Delete shared key spare?",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete key" }));
    await screen.findByText("Deleted shared key spare.");
    expect(screen.queryByRole("row", { name: /^spare(?![\w-])/ })).toBeNull();
    expect(admin.writes).toEqual([
      { name: "spare", version: 1, key: expect.any(String) as string },
    ]);
  });

  it.each([
    [{ limit: 3 }, "The store already holds the most keys it allows (3)."],
    [
      { session: false },
      "Changing shared keys needs a browser session. Sign in again.",
    ],
  ])("names a refused write plainly", async (options, copy) => {
    show(fakeAdmin({ secrets: seeded, ...options }).api);
    await screen.findByRole("table", { name: "Shared keys" });
    await add("openai", VALUE);
    expect(await screen.findByText(copy)).toBeVisible();
  });

  it("says plainly when the list cannot be loaded, and is for administrators only", async () => {
    const api: AdminApi = {
      ...fakeAdmin().api,
      secrets: vi.fn(() =>
        Promise.reject(new ApiError(503, "store_unavailable", "x")),
      ),
    };
    show(api);
    expect(
      await screen.findByText("The shared keys could not be loaded."),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  });

  it("is for administrators only", () => {
    const secrets = vi.fn<AdminApi["secrets"]>();
    const api: AdminApi = { ...fakeAdmin().api, secrets };
    show(api, { ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can manage shared keys."),
    ).toBeVisible();
    expect(secrets).not.toHaveBeenCalled();
  });
});

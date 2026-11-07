import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { fakeAdmin, secretItem } from "../test/admin";
import { fakeConnectors } from "../test/connectors";
import { showModels } from "../test/models-app";
import { choose, picker } from "../test/picker";
import { fakeRoutes, route } from "../test/routes";
import { LIMITS } from "../test/user";

import { ConnectorsSettings } from "./ConnectorsSettings";
import { SecretsSettings } from "./SecretsSettings";

/** Replaced by the harness once it renders (showModels). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const KEY = "fake-provider-key-0002";

function wrap(children: React.ReactNode) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(<QueryClientProvider client={cache}>{children}</QueryClientProvider>);
}
const dialog = () => screen.getByRole("dialog");
/** A route's own page (WEB-U-115): its form, from the title down to Save and Cancel. */
const routePage = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** A route, connector or engine's box, under its kind's group: the kind is the group's name, not each box's (F8). */
const KINDS = {
  route: "Model routes",
  connector: "Connectors",
  engine: "Decision engines",
} as const;
const box = (kind: keyof typeof KINDS, name: string) =>
  within(within(dialog()).getByRole("group", { name: KINDS[kind] })).getByRole(
    "checkbox",
    { name },
  );
/** Opens a shared key's "…" menu and chooses one of its items (UI-GUIDELINES §7). */
function menuItem(ref: string, item: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `More actions for ${ref}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
}
async function openBindings(name: string) {
  await screen.findByRole("table", { name: "Shared keys" });
  menuItem(`shared key ${name}`, `Change who may use shared key ${name}`);
  return screen.getByRole("dialog", { name: `Who may use shared key ${name}` });
}
const saveBindings = () =>
  fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));

// ADR-0033: each shared secret names the routes and connectors that may use it; a save naming
// a secret not allowed for it is refused and offers to allow it (WEB-U-054).
describe("WEB-U-054 who may use a secret", () => {
  // The design direction: who may use a key is in its Who may use dialog, not a second line under Used by.
  it("says who may use a key in its dialog, not under its row", async () => {
    wrap(
      <SecretsSettings
        me={admin}
        api={
          fakeAdmin({
            secrets: [
              secretItem("local", {
                consumers: [
                  { kind: "connector", name: "tracker" },
                  { kind: "route", name: "local" },
                ],
              }),
              secretItem("spare"),
            ],
            choices: [
              { kind: "route", name: "local" },
              { kind: "connector", name: "tracker" },
            ],
          }).api
        }
      />,
    );
    await screen.findByRole("table", { name: "Shared keys" });
    expect(screen.queryByText(/^Allowed for/)).toBeNull();
    await openBindings("local");
    await within(dialog()).findByRole("group", { name: "Model routes" });
    expect(box("route", "local")).toBeChecked();
    expect(box("connector", "tracker")).toBeChecked();
  });

  it("changes who may use a secret with the version read, keeping one that no longer exists until unticked", async () => {
    const fake = fakeAdmin({
      secrets: [
        secretItem("spare", {
          consumers: [{ kind: "route", name: "retired" }],
        }),
      ],
      choices: [
        { kind: "route", name: "local" },
        { kind: "route", name: "deepseek-flash" },
        { kind: "connector", name: "tracker" },
      ],
    });
    wrap(<SecretsSettings me={admin} api={fake.api} />);
    await openBindings("spare");
    await within(dialog()).findByRole("group", { name: "Model routes" });
    const gone = box("route", "retired (no longer exists)");
    // No box repeats its kind: the group says it.
    expect(
      within(dialog())
        .getAllByRole("checkbox")
        .some((checkbox) =>
          /^(Route|Connector|Decision engine) /.test(
            checkbox.getAttribute("aria-label") ??
              checkbox.closest("label")?.textContent ??
              "",
          ),
        ),
    ).toBe(false);
    expect(gone).toBeChecked();
    expect(box("route", "local")).not.toBeChecked();
    fireEvent.click(box("route", "local"));
    fireEvent.click(box("connector", "tracker"));
    fireEvent.click(gone);
    saveBindings();
    expect(
      await screen.findByText("Saved who may use shared key spare."),
    ).toBeVisible();
    expect(fake.consumerWrites).toEqual([
      {
        name: "spare",
        consumers: [
          { kind: "connector", name: "tracker" },
          { kind: "route", name: "local" },
        ],
        version: 1,
      },
    ]);
    await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("keeps the choice after another admin's change, and reads the gateway's refusals", async () => {
    const fake = fakeAdmin({
      secrets: [secretItem("spare")],
      choices: [{ kind: "route", name: "local" }],
    });
    wrap(<SecretsSettings me={admin} api={fake.api} />);
    await openBindings("spare");
    await within(dialog()).findByRole("group", { name: "Model routes" });
    fireEvent.click(box("route", "local"));
    fake.touch("secrets/spare");
    saveBindings();
    expect(
      await within(dialog()).findByText(
        "Someone changed this key since you opened it. Your choice is kept: save again to replace their change.",
      ),
    ).toBeVisible();
    expect(box("route", "local")).toBeChecked();
    fake.refuseConsumers();
    saveBindings();
    expect(
      await within(dialog()).findByText(
        "One of these can't use a key. Refresh the page and try again.",
      ),
    ).toBeVisible();
    expect(fake.consumerWrites.map((write) => write.version)).toEqual([1, 2]);
  });

  it("WEB-U-291: offers to allow a route's secret when the save is refused, then saves", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      secrets: ["secrets/shared"],
      bindings: { "secrets/shared": [] },
    });
    showModels(fake.api, router);
    await screen.findByRole("table", { name: "Model routes" });
    fireEvent.click(
      screen.getByRole("link", { name: "Edit route Local model" }),
    );
    await screen.findByRole("group", { name: "Save or cancel" });
    fireEvent.click(
      within(routePage()).getByRole("button", {
        name: "Use another shared key",
      }),
    );
    fireEvent.change(
      within(routePage()).getByLabelText("Shared key", { exact: true }),
      { target: { value: "secrets/shared" } },
    );
    // Display name is required before a save (F4).
    fireEvent.change(within(routePage()).getByLabelText("Display name"), {
      target: { value: "Local" },
    });
    fireEvent.click(within(routePage()).getByRole("button", { name: "Save" }));
    expect(
      await within(routePage()).findByText(
        "shared isn't allowed for this route. Allow it first.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(routePage()).getByRole("button", { name: "Allow it for local" }),
    );
    expect(
      await within(routePage()).findByText("Allowed. Save again."),
    ).toBeVisible();
    expect(fake.allowCalls).toEqual([
      { ref: "secrets/shared", consumer: { kind: "route", name: "local" } },
    ]);
    // Display name is required before a save (F4).
    fireEvent.change(within(routePage()).getByLabelText("Display name"), {
      target: { value: "Local" },
    });
    fireEvent.click(within(routePage()).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved route Local.")).toBeVisible();
  });

  it("WEB-U-291: offers the same for a connector's shared credential", async () => {
    const fake = fakeConnectors({ bindings: { "secrets/tracker": [] } });
    wrap(<ConnectorsSettings me={admin} api={fake.api} />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Add connector" }),
    );
    const set = (label: string, value: string) => {
      const box = within(dialog()).queryByLabelText(label);
      if (box) fireEvent.change(box, { target: { value } });
      else choose(label, value, dialog());
    };
    set("Name", "tracker");
    set("Server address", "https://mcp.tracker.example/mcp");
    set("Send a credential", "As a bearer token");
    fireEvent.click(picker("Shared key", dialog()));
    await screen.findByRole("menuitemradio", { name: /^tracker/ });
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    set("Shared key", "tracker");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save connector" }),
    );
    expect(
      await within(dialog()).findByText(
        "tracker isn't allowed for this connector. Allow it first.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Allow it for tracker" }),
    );
    expect(
      await within(dialog()).findByText("Allowed. Save again."),
    ).toBeVisible();
    expect(fake.allowCalls).toEqual([
      {
        ref: "secrets/tracker",
        consumer: { kind: "connector", name: "tracker" },
      },
    ]);
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save connector" }),
    );
    expect(await screen.findByText("Saved connector tracker.")).toBeVisible();
  });

  it("allows a pasted key for its route at once, and names it if that fails", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    showModels(fake.api, router);
    await screen.findByRole("table", { name: "Model routes" });
    fireEvent.click(
      screen.getByRole("link", { name: "Edit route Local model" }),
    );
    await screen.findByRole("group", { name: "Save or cancel" });
    const paste = (name: string) => {
      // Paste key under the Secret field, or Replace key once the key reads as one line (F4).
      const button =
        within(routePage()).queryByRole("button", { name: "Paste key" }) ??
        within(routePage()).queryByRole("button", { name: "Replace key" });
      if (button) fireEvent.click(button);
      const panel = within(routePage()).getByRole("group", {
        name: "Paste a key",
      });
      fireEvent.change(within(panel).getByLabelText("Secret name"), {
        target: { value: name },
      });
      fireEvent.change(within(panel).getByLabelText("Key"), {
        target: { value: KEY },
      });
      fireEvent.click(within(panel).getByRole("button", { name: "Store key" }));
    };
    paste("local-key");
    await within(routePage()).findByText(/^Stored the key as local-key\./);
    expect(fake.allowCalls).toEqual([
      { ref: "secrets/local-key", consumer: { kind: "route", name: "local" } },
    ]);
    fake.refuseAllow();
    paste("local-key-2");
    expect(
      await within(routePage()).findByText(
        "Stored the key as local-key-2, but it could not be allowed for this route yet. Saving offers to allow it.",
      ),
    ).toBeVisible();
    expect(
      within(routePage()).getByText("Uses the shared key local-key-2."),
    ).toBeVisible();
  });
});

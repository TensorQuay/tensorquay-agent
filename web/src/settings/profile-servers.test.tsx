import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { McpServers, User } from "../protocol/generated/api";
import { showProfiles } from "../test/profile-app";
import { fakeProfiles, profile } from "../test/profiles";
import { LIMITS } from "../test/user";

import type { ProfilesApi } from "./profiles";
import type { ChoicesApi } from "./ProfilesSettings";

/** Replaced by the harness once it renders (showProfiles). */
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
const member: User = { ...admin, role: "member" };
const SERVERS: McpServers = {
  items: [
    { name: "search", templates: [], enabled_by_default: true },
    { name: "tickets", templates: ["workstation"], enabled_by_default: false },
  ],
};
function choices(servers = vi.fn(() => Promise.resolve(SERVERS))): ChoicesApi {
  return {
    models: () => Promise.resolve({ items: [] }),
    templates: () =>
      Promise.resolve({
        items: [
          {
            name: "workstation",
            display_name: "Workstation",
            description: null,
            capabilities: ["shell"],
            is_default: true,
            cpus: 1,
            memory_mib: 512,
          },
        ],
      }),
    connectors: () => Promise.resolve({ items: [] }),
    servers,
  };
}
function show(api: ProfilesApi, me: User, options: ChoicesApi = choices()) {
  showProfiles(api, options, me, router);
}
/** The profile's page (WEB-U-104): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** Follows a link on the list to a profile's page, and waits for it. */
async function go(link: string) {
  fireEvent.click(await screen.findByRole("link", { name: link }));
  await screen.findByRole("group", { name: "Save or cancel" });
}
const box = (name: string) => within(page()).getByRole("checkbox", { name });
const save = () =>
  fireEvent.click(within(page()).getByRole("button", { name: "Save" }));

// Shared agent profiles may bring the administrator's tool servers (GET /api/v1/admin/mcp/servers); members' own profiles
// still set none (ADR-0012, Gate 6; WEB-U-059).
describe("WEB-U-059 tool servers in shared profiles", () => {
  it("lets an administrator choose a shared profile's servers, keeping one that no longer exists until unticked", async () => {
    const shared = profile("ops", {
      display_name: "Ops",
      shared: true,
      mcp_servers: ["search", "retired"],
    });
    const fake = fakeProfiles({ profiles: [shared], admin: true });
    show(fake.api, admin);
    await go("Edit agent profile Ops");
    const servers = await within(page()).findByRole("region", {
      name: "Tool servers",
    });
    expect(
      within(servers)
        .getAllByRole("checkbox")
        .map((item) => item.closest("label")?.textContent),
    ).toEqual([
      "search (on by default)",
      "tickets (only in Workstation)",
      "retired (no longer exists)",
    ]);
    expect(box("search (on by default)")).toBeChecked();
    expect(box("retired (no longer exists)")).toBeChecked();
    fireEvent.click(box("tickets (only in Workstation)"));
    fireEvent.click(box("retired (no longer exists)"));
    save();
    expect(await screen.findByText("Saved agent profile Ops.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields.mcp_servers).toEqual([
      "search",
      "tickets",
    ]);
  });

  it("offers servers on a new profile only while it is shared", async () => {
    const fake = fakeProfiles({ admin: true });
    show(fake.api, admin);
    await go("Add agent profile");
    fireEvent.change(
      within(page()).getByLabelText("Display name", { exact: true }),
      { target: { value: "Team search" } },
    );
    // The ID follows the display name (WEB-U-104).
    expect(page()).toHaveTextContent("ID: team-search · Change");
    expect(
      within(page()).queryByRole("region", { name: "Tool servers" }),
    ).toBeNull();
    expect(page()).toHaveTextContent("Tool servers: none.");
    fireEvent.click(within(page()).getByRole("checkbox", { name: /^Shared/ }));
    fireEvent.click(
      await within(page()).findByRole("checkbox", {
        name: "search (on by default)",
      }),
    );
    // Back to personal: the servers go with it, since a person's profile sets none.
    fireEvent.click(within(page()).getByRole("checkbox", { name: /^Shared/ }));
    fireEvent.click(within(page()).getByRole("checkbox", { name: /^Shared/ }));
    expect(box("search (on by default)")).not.toBeChecked();
    fireEvent.click(box("search (on by default)"));
    save();
    expect(
      await screen.findByText("Saved agent profile Team search."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields).toMatchObject({
      shared: true,
      mcp_servers: ["search"],
    });
  });

  it("never asks for the servers for a member, whose profiles set none", async () => {
    const servers = vi.fn(() => Promise.resolve(SERVERS));
    const fake = fakeProfiles();
    show(fake.api, member, choices(servers));
    await go("Add agent profile");
    expect(page()).toHaveTextContent("Tool servers: none.");
    expect(servers).not.toHaveBeenCalled();
  });

  it("says so when the servers cannot be loaded, and keeps the profile's own", async () => {
    const shared = profile("ops", {
      display_name: "Ops",
      shared: true,
      mcp_servers: ["search"],
    });
    const fake = fakeProfiles({ profiles: [shared], admin: true });
    show(
      fake.api,
      admin,
      choices(vi.fn(() => Promise.reject(new Error("offline")))),
    );
    await go("Edit agent profile Ops");
    expect(
      await within(page()).findByText(
        "The tool servers could not be loaded, so this agent profile keeps its own: search.",
      ),
    ).toBeVisible();
    save();
    expect(await screen.findByText("Saved agent profile Ops.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields.mcp_servers).toEqual(["search"]);
  });

  it("drops the servers from a personal copy of a shared profile", async () => {
    const shared = profile("ops", {
      display_name: "Ops",
      shared: true,
      mcp_servers: ["search"],
    });
    const fake = fakeProfiles({ profiles: [shared] });
    show(fake.api, member);
    await go("Duplicate agent profile Ops as your own");
    expect(page()).toHaveTextContent("Tool servers: none.");
    fireEvent.click(
      within(page()).getByRole("button", { name: "Change the ID" }),
    );
    fireEvent.change(within(page()).getByLabelText("ID", { exact: true }), {
      target: { value: "my-ops" },
    });
    save();
    expect(await screen.findByText("Saved agent profile Ops.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields.mcp_servers).toEqual([]);
  });
});

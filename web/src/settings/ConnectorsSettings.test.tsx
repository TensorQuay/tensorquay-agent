import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { User } from "../protocol/generated/api";
import { connector, fakeConnectors } from "../test/connectors";
import { LIMITS } from "../test/user";

import type { ConnectorsApi } from "./admin/connectors";
import { ConnectorsSettings } from "./ConnectorsSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const listed = [
  connector("tracker", {
    display_name: "Issue tracker",
    auth: { scheme: "bearer", required: true },
    secret_ref: "secrets/gone",
    key_missing: true,
    tools: { allow: ["create_issue", "list_issues"] },
    enabled_by_default: true,
    seed: "current",
  }),
  connector("wiki", { enabled: false, tools: { deny: ["delete_page"] } }),
];
function show(api: ConnectorsApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ConnectorsSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
const row = (name: RegExp) => screen.getByRole("row", { name });

// ADR-0031: the connector registry for administrators; credentials are named, never shown (WEB-U-043).
describe("WEB-U-043 the connectors list", () => {
  it("names each connector with its host, tools, status and source", async () => {
    show(fakeConnectors({ connectors: listed }).api);
    await screen.findByRole("table", { name: "Connectors" });
    expect(
      screen.getAllByRole("columnheader").map((cell) => cell.textContent),
    ).toEqual(["Connector", "Tools", "Status", "Actions"]);
    const tracker = row(/Issue tracker/);
    for (const text of [
      "mcp.tracker.example · Configuration file",
      "2 tools allowed",
      "In new tasks",
      "Key missing",
    ])
      expect(within(tracker).getByText(text)).toBeVisible();
    expect(
      within(tracker).getByRole("link", { name: "Add it under Keys › Shared" }),
    ).toHaveAttribute("href", "/settings/keys/shared");
    const wiki = row(/wiki/);
    for (const text of [
      "All but 1 tool",
      "Off",
      "mcp.wiki.example · Added here",
    ])
      expect(within(wiki).getByText(text)).toBeVisible();
    expect(screen.queryByText(/secrets\/gone/)).toBeNull();
    // PRD FR-10, superseded: no kind of action in the list.
    expect(
      screen.queryByText(
        /Acts on the service|Changes data|Only looks|Sends messages/,
      ),
    ).toBeNull();
    // The display name only (UI-GUIDELINES §10): the internal name shows only when there is no display name.
    expect(within(tracker).queryByText("tracker")).toBeNull();
    expect(within(wiki).getByText("wiki")).toBeVisible();
  });

  // One actions column (UI-GUIDELINES §7, as on Models and Keys): Edit, and Delete in a menu named after the row.
  it("gives each connector Edit and a menu with Delete", async () => {
    show(fakeConnectors({ connectors: listed }).api);
    await screen.findByRole("table", { name: "Connectors" });
    expect(
      within(row(/Issue tracker/))
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "Edit connector Issue tracker",
      "More actions for connector Issue tracker",
    ]);
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for connector wiki" }),
    );
    const remove = screen.getByRole("menuitem", {
      name: "Delete connector wiki",
    });
    expect(remove).toHaveAttribute("data-danger", "true");
  });

  it("says what an empty registry means, and shows members nothing", async () => {
    show(fakeConnectors().api);
    // Finding 20: what a connector is for, with a concrete example that isn't only about code.
    expect(
      await screen.findByText(
        "Connectors give the agent tools from other services, such as searching your team's documents or filing an issue in your tracker. The gateway holds their credentials and sends them; the agent never sees one. A change applies from each task’s next connection.",
      ),
    ).toBeVisible();
    expect(
      await screen.findByText(
        "No connectors yet. Add one to let the agent use a service, such as searching your team's documents or filing an issue in your tracker.",
      ),
    ).toBeVisible();
    show(fakeConnectors({ connectors: listed }).api, {
      ...admin,
      role: "member",
    });
    expect(
      screen.getByText("Only administrators can manage connectors."),
    ).toBeVisible();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { User } from "../protocol/generated/api";
import { connector, fakeConnectors } from "../test/connectors";
import { choose, picker } from "../test/picker";
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
function show(api: ConnectorsApi) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ConnectorsSettings me={admin} api={api} />
    </QueryClientProvider>,
  );
}
const dialog = () => screen.getByRole("dialog");
/** Types into the field labelled `label`, or, for a standard picker (WEB-U-134), chooses the choice so worded. */
const set = (label: string, value: string) => {
  const box = within(dialog()).queryByLabelText(label);
  if (box) fireEvent.change(box, { target: { value } });
  else choose(label, value, dialog());
};
/** Waits until a picker offers a choice (its list loads with the form), and closes it again. */
async function offered(label: string, option: string) {
  fireEvent.click(picker(label, dialog()));
  await screen.findByRole("menuitemradio", { name: new RegExp(`^${option}`) });
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
}
const save = () =>
  fireEvent.click(
    within(dialog()).getByRole("button", { name: "Save connector" }),
  );

// Adding and editing a connector: the credential by reference and the tool filter (WEB-U-043). There is no kind of
// action (PRD FR-10, superseded): no permission rules apply to a connector's tools.
describe("WEB-U-043 the connector form", () => {
  it("adds a connector with a header credential and a tool list, and no kind of action", async () => {
    const fake = fakeConnectors();
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", { name: "Add connector" }),
    );
    set("Name", "tracker");
    set("Server address", "https://mcp.tracker.example/mcp");
    set("Send a credential", "In a header");
    set("Header name", "X-Api-Key");
    // WEB-U-300: each shared key by its name, never the configuration's secrets/ notation; what's saved is still the
    // reference (secret_ref below).
    await offered("Shared key", "tracker");
    fireEvent.click(picker("Shared key", dialog()));
    expect(
      screen
        .getAllByRole("menuitemradio")
        .map((option) => option.textContent ?? ""),
    ).not.toContainEqual(expect.stringMatching(/secrets\//));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    set("Shared key", "tracker");
    // Where the shared key lives and where people add their own: the Keys page's two tabs, in one word, "key".
    expect(
      within(dialog()).getByText(
        "A shared key (Keys › Shared), never shown here. People can add their own under Keys › Mine; theirs is used first.",
      ),
    ).toBeVisible();
    set("Tools the agent may use", "Only these");
    set("Tool names, one per line", "create_issue\nlist_issues");
    expect(within(dialog()).queryByLabelText("Kind of action")).toBeNull();
    expect(dialog()).not.toHaveTextContent(/permission rules/);
    save();
    expect(await screen.findByText("Saved connector tracker.")).toBeVisible();
    expect(fake.writes).toHaveLength(1);
    expect(fake.writes[0]?.name).toBe("tracker");
    expect(fake.writes[0]?.fields).toMatchObject({
      auth: { scheme: "header", header: "X-Api-Key", required: false },
      secret_ref: "secrets/tracker",
      tools: { allow: ["create_issue", "list_issues"] },
    });
    expect(fake.writes[0]).not.toHaveProperty("version");
    expect(fake.writes[0]?.fields).not.toHaveProperty("class");
  });

  it("refuses bad input at its field before sending, and shows the gateway's reason after", async () => {
    const fake = fakeConnectors({ reserved: ["files"] });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", { name: "Add connector" }),
    );
    set("Name", "Tracker");
    set("Server address", "http://mcp.tracker.example/mcp");
    save();
    expect(within(dialog()).getByText(/^Use lower-case letters/)).toBeVisible();
    expect(
      within(dialog()).getByText(
        "Use the server’s https:// address, without a name, password or fragment.",
      ),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    set("Name", "tracker");
    set("Server address", "https://mcp.tracker.example/mcp?token=1");
    save();
    expect(
      await within(dialog()).findByText(
        "Put credentials in a secret, not in the address.",
      ),
    ).toBeVisible();
    set("Name", "files");
    set("Server address", "https://mcp.files.example/mcp");
    save();
    expect(
      await within(dialog()).findByText(
        "A connector or tool server already uses this name.",
      ),
    ).toBeVisible();
  });

  it("keeps the edits on a conflict and saves them over the newer version", async () => {
    const fake = fakeConnectors({ connectors: [connector("wiki")] });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", { name: "Edit connector wiki" }),
    );
    expect(within(dialog()).queryByLabelText("Name")).toBeNull();
    set("Call timeout (seconds)", "120");
    fake.touch("wiki");
    save();
    expect(
      await within(dialog()).findByText(/^Someone changed this connector/),
    ).toBeVisible();
    expect(
      within(dialog()).getByLabelText("Call timeout (seconds)"),
    ).toHaveValue("120");
    save();
    expect(await screen.findByText("Saved connector wiki.")).toBeVisible();
    expect(fake.writes.map((write) => write.version)).toEqual([1, 2]);
  });

  it("deletes after a named confirmation", async () => {
    const fake = fakeConnectors({ connectors: [connector("wiki")] });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for connector wiki",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Delete connector wiki" }),
    );
    expect(dialog()).toHaveTextContent(
      "Tasks lose this connector’s tools from their next connection.",
    );
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Delete connector" }),
    );
    expect(await screen.findByText("Deleted connector wiki.")).toBeVisible();
    await waitFor(() =>
      expect(
        screen.getByText(
          "No connectors yet. Add one to let the agent use a service, such as searching your team's documents or filing an issue in your tracker.",
        ),
      ).toBeVisible(),
    );
  });
});

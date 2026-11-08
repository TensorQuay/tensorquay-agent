import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import type { SecretRead, SecretReadPage } from "../protocol/generated/api";
import { fakeAdmin, secretItem } from "../test/admin";
import { fakeCredentials, userConnector } from "../test/connector-credentials";
import { LIMITS } from "../test/user";

import type { AdminApi } from "./admin/contract";
import { KeysSettings } from "./KeysSettings";
import { SecretReads } from "./SecretReads";
import { SecretsSettings } from "./SecretsSettings";

const TASK = "0195d8c0-0000-7000-8000-0000000000c1";
const ALICE = "0195d8c0-0000-7000-8000-00000000a11c";
const GONE = "0195d8c0-0000-7000-8000-00000000dead";
const read = (id: string, extra: Partial<SecretRead> = {}): SecretRead => ({
  id,
  consumer_kind: "route",
  consumer: "local",
  conversation_id: TASK,
  user_id: ALICE,
  run_id: "r-1",
  outcome: "ok",
  count: 1,
  first_at: "2026-09-26T09:00:00Z",
  last_at: "2026-09-26T09:00:00Z",
  ...extra,
});
const first: SecretReadPage = {
  items: [
    read("0195d8c0-0000-7000-8000-000000000e03", {
      count: 12,
      first_at: "2026-09-20T09:00:00Z",
      last_at: "2026-09-27T09:00:00Z",
    }),
    read("0195d8c0-0000-7000-8000-000000000e02", {
      consumer_kind: "connector",
      consumer: "tracker",
      conversation_id: null,
      run_id: null,
      user_id: GONE,
      outcome: "absent",
    }),
  ],
  next: "0195d8c0-0000-7000-8000-000000000e02",
};
const older: SecretReadPage = {
  items: [
    read("0195d8c0-0000-7000-8000-000000000e01", { outcome: "unavailable" }),
  ],
  next: null,
};

function wrap(children: React.ReactNode) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={cache}>{children}</QueryClientProvider>,
  );
}
const cells = (row: HTMLElement) =>
  within(row)
    .getAllByRole("cell")
    .map((cell) => cell.textContent);

afterEach(() => vi.unstubAllGlobals());

// ADR-0033: who read a secret, for which task and with what outcome; never a value (WEB-U-053).
describe("WEB-U-053 the reads of a secret", () => {
  it("lists reads newest first with their consumer, task, person and outcome, and loads older ones", async () => {
    const load = vi
      .fn<(before?: string) => Promise<SecretReadPage>>()
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(older);
    wrap(
      <SecretReads
        title="Reads of shared key local"
        load={load}
        emails={{ [ALICE]: "alice@example.invalid" }}
        onClose={() => undefined}
      />,
    );
    const dialog = screen.getByRole("dialog", {
      name: "Reads of shared key local",
    });
    const table = await within(dialog).findByRole("table", { name: "Reads" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["When", "Used by", "Task", "Person", "Outcome"]);
    let rows = within(table).getAllByRole("row").slice(1);
    expect(cells(rows[0] as HTMLElement).slice(1)).toEqual([
      "Route local",
      "Open the task",
      "alice@example.invalid",
      "Read",
    ]);
    expect(rows[0]).toHaveTextContent(/12 times since /);
    expect(
      within(rows[0] as HTMLElement).getByRole("link", {
        name: "Open the task",
      }),
    ).toHaveAttribute("href", `/c/${TASK}`);
    expect(cells(rows[1] as HTMLElement).slice(1)).toEqual([
      "Connector tracker",
      "No task",
      "A removed user",
      "No value at the time",
    ]);
    expect(load).toHaveBeenLastCalledWith(undefined, expect.anything());
    fireEvent.click(within(dialog).getByRole("button", { name: "Load older" }));
    expect(await within(table).findByText("Store unavailable")).toBeVisible();
    expect(load).toHaveBeenLastCalledWith(first.next, expect.anything());
    rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(3);
    expect(
      within(dialog).queryByRole("button", { name: "Load older" }),
    ).toBeNull();
  });

  it("shows no Person column in a person's own view, and says when nothing was read", async () => {
    wrap(
      <SecretReads
        title="Reads of your tracker credential"
        load={() => Promise.resolve({ items: [], next: null })}
        onClose={() => undefined}
      />,
    );
    expect(await screen.findByText("Not read yet.")).toBeVisible();
    wrap(
      <SecretReads
        title="Reads of your wiki credential"
        load={() =>
          Promise.resolve({
            items: [
              read("0195d8c0-0000-7000-8000-000000000e09", {
                user_id: null,
              }),
            ],
            next: null,
          })
        }
        onClose={() => undefined}
      />,
    );
    const table = await screen.findByRole("table", { name: "Reads" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["When", "Used by", "Task", "Outcome"]);
  });

  // A screen review before 17.0: a Task column with "No task" on every row, and "Not set then", which made
  // an administrator guess. A column shows only when a row fills it (UI-GUIDELINES §7); the outcome says what happened.
  it("WEB-U-183 hides the Task column when no read has a task, and words a missing value plainly", async () => {
    wrap(
      <SecretReads
        title="Reads of shared key local"
        load={() =>
          Promise.resolve({
            items: [
              read("0195d8c0-0000-7000-8000-000000000e10", {
                conversation_id: null,
                consumer_kind: "connector",
                consumer: "tracker",
                run_id: null,
                outcome: "absent",
              }),
              read("0195d8c0-0000-7000-8000-000000000e11", {
                conversation_id: null,
              }),
            ],
            next: null,
          })
        }
        emails={{ [ALICE]: "alice@example.invalid" }}
        onClose={() => undefined}
      />,
    );
    const table = await screen.findByRole("table", { name: "Reads" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["When", "Used by", "Person", "Outcome"]);
    expect(within(table).queryByText("No task")).toBeNull();
    expect(within(table).queryByText("Run unknown")).toBeNull();
    expect(within(table).getByText("No value at the time")).toBeVisible();
  });

  it("keeps the rows when older reads fail, and explains a missing credential", async () => {
    const load = vi
      .fn<(before?: string) => Promise<SecretReadPage>>()
      .mockResolvedValueOnce(first)
      .mockRejectedValueOnce(new ApiError(503, "store_unavailable", "x"));
    wrap(
      <SecretReads
        title="Reads of shared key local"
        load={load}
        emails={{}}
        onClose={() => undefined}
      />,
    );
    const table = await screen.findByRole("table", { name: "Reads" });
    fireEvent.click(screen.getByRole("button", { name: "Load older" }));
    expect(
      await screen.findByText("Older reads could not be loaded."),
    ).toBeVisible();
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    wrap(
      <SecretReads
        title="Reads of your mail credential"
        load={() => Promise.reject(new ApiError(404, "not_found", "x"))}
        missing="reads.noConnectorCredential"
        onClose={() => undefined}
      />,
    );
    expect(
      await screen.findByText("You have no key for this connector."),
    ).toBeVisible();
  });

  it("opens from the Secrets page with people's emails for administrators", async () => {
    const fake = fakeAdmin({
      secrets: [secretItem("local")],
      reads: { local: first },
      people: { [ALICE]: "alice@example.invalid" },
    });
    const api: AdminApi = fake.api;
    wrap(
      <SecretsSettings
        me={{
          id: "0195d8c0-0000-7000-8000-000000000001",
          email: "admin@example.invalid",
          role: "admin",
          internet: "none",
          limits: LIMITS,
          features: { agent_teams: false },
        }}
        api={api}
      />,
    );
    await screen.findByRole("table", { name: "Shared keys" });
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for shared key local" }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Show reads of shared key local" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Reads of shared key local",
    });
    expect(
      await within(dialog).findByText("alice@example.invalid"),
    ).toBeVisible();
    expect(fake.readCalls).toEqual([{ name: "local", before: undefined }]);
  });

  it("opens from a person's connector key, on Keys", async () => {
    const fake = fakeCredentials(
      [userConnector("wiki", { has_credential: true })],
      { reads: { wiki: first } },
    );
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof globalThis.fetch>(() =>
        Promise.resolve(Response.json({ items: [] })),
      ),
    );
    wrap(
      <KeysSettings
        me={{
          id: "0195d8c0-0000-7000-8000-000000000002",
          email: "member@example.invalid",
          role: "member",
          internet: "none",
          limits: LIMITS,
          features: { agent_teams: false },
        }}
        connectors={fake.api}
      />,
    );
    await screen.findByRole("table", { name: "Your keys" });
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for your wiki key",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", {
        name: "Show reads of your wiki key",
      }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Reads of your wiki key",
    });
    expect(
      await within(dialog).findByRole("table", { name: "Reads" }),
    ).toBeVisible();
    expect(within(dialog).queryByText("Person")).toBeNull();
  });
});

// Run-ID (2), agreed with the lead: a read whose run is unknown says so quietly in its Task cell, so a key read can be
// tied to its run or is plainly not; a connector's read never carries a run (the MCP proxy takes none, ADR-0035), so
// it says nothing of it; a read with its run says nothing more.
describe("WEB-U-130 a key read with no run says Run unknown", () => {
  async function table(items: SecretRead[]) {
    wrap(
      <SecretReads
        title="Reads of shared key local"
        load={() => Promise.resolve({ items, next: null })}
        onClose={() => undefined}
      />,
    );
    return screen.findByRole("table", { name: "Reads" });
  }
  const taskCell = (row: HTMLElement) => within(row).getAllByRole("cell")[2];

  it.each([
    ["a model call that named no run", { consumer_kind: "route" as const }],
    ["a decision engine's call", { consumer_kind: "engine" as const }],
    ["a push", { consumer_kind: "git" as const, consumer: "github.com" }],
  ])("says Run unknown for %s", async (_, extra) => {
    const rows = within(await table([read("e1", { ...extra, run_id: null })]))
      .getAllByRole("row")
      .slice(1);
    const cell = taskCell(rows[0] as HTMLElement);
    expect(cell).toHaveTextContent("Open the task");
    const words = within(cell as HTMLElement).getByText("Run unknown");
    expect(words).toHaveClass("muted");
  });

  it("says Run unknown under No task too", async () => {
    const rows = within(
      await table([
        read("e1", { conversation_id: null, run_id: null }),
        read("e2"),
      ]),
    )
      .getAllByRole("row")
      .slice(1);
    expect(taskCell(rows[0] as HTMLElement)).toHaveTextContent(
      /^No task\s*Run unknown$/,
    );
  });

  // WEB-U-183: with no task in any read, the column stays for an unknown run, and says only that.
  it("WEB-U-183 keeps the column for an unknown run without repeating No task", async () => {
    const found = await table([
      read("e1", { conversation_id: null, run_id: null }),
      read("e2", {
        conversation_id: null,
        consumer_kind: "connector",
        consumer: "tracker",
        run_id: null,
      }),
    ]);
    const rows = within(found).getAllByRole("row").slice(1);
    expect(taskCell(rows[0] as HTMLElement)).toHaveTextContent(/^Run unknown$/);
    expect(taskCell(rows[1] as HTMLElement)).toHaveTextContent(/^$/);
    expect(within(found).queryByText("No task")).toBeNull();
  });

  it("says nothing of runs for a connector's read, or a read with its run", async () => {
    const found = await table([
      read("e1", {
        consumer_kind: "connector",
        consumer: "tracker",
        run_id: null,
      }),
      read("e2"),
    ]);
    expect(within(found).queryByText("Run unknown")).toBeNull();
  });
});

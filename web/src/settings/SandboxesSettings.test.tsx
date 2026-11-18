import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import type { User } from "../protocol/generated/api";
import { LIMITS } from "../test/user";

import type {
  SandboxListing,
  SandboxRow,
  SandboxesApi,
} from "./admin/sandboxes";
import { SandboxesSettings } from "./SandboxesSettings";

// The Sandboxes page (the Sandboxes page design note, §4): its states, each row's resources, Stop now and its results,
// and the privacy rule (nothing of a task but its id reaches the page): WEB-U-486, 487 and 488.

const NOW = Date.parse("2026-11-17T01:26:00Z");
const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const member: User = {
  ...admin,
  id: "0195d8c0-0000-7000-8000-000000000009",
  role: "member",
};

const GIB = 2 ** 30;

function row(
  overrides: Partial<SandboxRow> & { conversation_id: string },
): SandboxRow {
  return {
    owner: {
      id: "0195d8c0-0000-7000-8000-0000000000b1",
      email: "ben@example.org",
    },
    environment: { name: "minimal", display_name: "Minimal", desktop: false },
    state: "idle",
    started_at: "2026-11-17T00:12:00Z",
    last_activity_at: "2026-11-17T01:20:00Z",
    stops_at: "2026-11-17T01:30:00Z",
    kept_on: false,
    size: { cpu_millicores: 2000, memory_bytes: 4 * GIB, pids: 512 },
    usage: { cpu_millicores: 200, memory_bytes: 0.4 * GIB, pids: 7 },
    can_stop: true,
    stop_refusal: null,
    ...overrides,
  };
}

/** The design note's fixture: four sandboxes of three people, one working, one starting, one idle, one kept on. */
const WORKING = row({
  conversation_id: "0195d8c0-0000-7000-8000-00000000c001",
  state: "working",
  stops_at: null,
  can_stop: false,
  stop_refusal: "working",
  usage: { cpu_millicores: 1400, memory_bytes: 3.1 * GIB, pids: 40 },
  size: { cpu_millicores: 2000, memory_bytes: 4 * GIB, pids: 512 },
});
const STARTING = row({
  conversation_id: "0195d8c0-0000-7000-8000-00000000c002",
  owner: {
    id: "0195d8c0-0000-7000-8000-0000000000b2",
    email: "cho@example.org",
  },
  state: "starting",
  stops_at: null,
  usage: null,
  can_stop: false,
  stop_refusal: "starting",
});
const IDLE = row({ conversation_id: "0195d8c0-0000-7000-8000-00000000c003" });
const KEPT = row({
  conversation_id: "0195d8c0-0000-7000-8000-00000000c004",
  owner: { id: admin.id, email: admin.email },
  environment: { name: "desktop", display_name: "Desktop", desktop: true },
  kept_on: true,
  stops_at: "2026-11-17T08:12:00Z",
});

function listing(overrides: Partial<SandboxListing> = {}): SandboxListing {
  const items = overrides.items ?? [WORKING, STARTING, IDLE, KEPT];
  return {
    backend: "docker",
    limits: {
      slots: 6,
      per_user: 4,
      desktops: 2,
      idle_seconds: 600,
      max_lifetime_seconds: 28_800,
    },
    totals: {
      in_use: items.length,
      starting: items.filter((item) => item.state === "starting").length,
      working: items.filter((item) => item.state === "working").length,
      idle: items.filter((item) => item.state === "idle").length,
      stopping: 0,
      desktops: items.filter((item) => item.environment.desktop).length,
      waiting: 0,
      reserved: { cpu_millicores: 8000, memory_bytes: 16 * GIB },
      usage: {
        cpu_millicores: 1800,
        memory_bytes: 3.9 * GIB,
        measured_at: "2026-11-17T01:26:05Z",
      },
    },
    items,
    next_after: null,
    left_behind: [],
    ...overrides,
  };
}

function fake(
  answers: SandboxListing[] | Error,
  stop = vi.fn<SandboxesApi["stop"]>(),
) {
  const pages = Array.isArray(answers) ? [...answers] : [];
  const api: SandboxesApi = {
    sandboxes: vi.fn(() => {
      if (!Array.isArray(answers)) return Promise.reject(answers);
      const next = pages.length > 1 ? pages.shift() : pages[0];
      return next
        ? Promise.resolve(next)
        : Promise.reject(new Error("no answer"));
    }),
    stop,
  };
  return api;
}

function show(api: SandboxesApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <SandboxesSettings me={me} api={api} clock={() => NOW} />
    </QueryClientProvider>,
  );
  return cache;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("WEB-U-486 the Sandboxes page's states", () => {
  // The real-stack run's numbers (18 Nov 2026): a small use never rounds to nothing, and one core is one core.
  it("says a small use in megabytes, and one core as one core", async () => {
    const MIB = 2 ** 20;
    const small = row({
      conversation_id: "0195d8c0-0000-7000-8000-00000000c00a",
      size: { cpu_millicores: 1000, memory_bytes: 512 * MIB, pids: 512 },
      usage: { cpu_millicores: 0, memory_bytes: 8.7 * MIB, pids: 3 },
    });
    show(
      fake([
        listing({
          items: [small],
          totals: {
            ...listing().totals,
            in_use: 1,
            working: 0,
            starting: 0,
            idle: 1,
            desktops: 0,
            reserved: { cpu_millicores: 3000, memory_bytes: 1.5 * GIB },
            usage: {
              cpu_millicores: 0,
              memory_bytes: 15.2 * MIB,
              measured_at: "2026-11-17T01:26:05Z",
            },
          },
        }),
      ]),
    );
    expect(
      await screen.findByText(
        "In use: 0 of 3 cores · 15.2 MB of 1.5 GB set aside",
      ),
    ).toBeVisible();
    const table = screen.getByRole("table", { name: "Running sandboxes" });
    expect(table).toHaveTextContent("0 of 1 core");
    expect(table).not.toHaveTextContent("1 cores");
    expect(table).toHaveTextContent("8.7 MB of 512 MB");
  });

  it("shows the headline, the lines under it, and every row in order with its resources", async () => {
    show(fake([listing()]));
    expect(await screen.findByText("4 of 6 running")).toBeVisible();
    expect(screen.getByText("Desktops: 1 of 2")).toBeVisible();
    expect(
      screen.getByText("In use: 1.8 of 8 cores · 3.9 GB of 16 GB set aside"),
    ).toBeVisible();
    expect(
      screen.getByText("Sandboxes run as Docker containers."),
    ).toBeVisible();
    expect(screen.queryByText(/waiting for a sandbox/)).toBeNull();
    const rows = within(
      screen.getByRole("table", { name: "Running sandboxes" }),
    ).getAllByRole("row");
    // The header, then the rows as the gateway ordered them.
    expect(rows).toHaveLength(5);
    expect(rows[1]).toHaveTextContent("ben@example.org");
    expect(rows[1]).toHaveTextContent("Working");
    expect(rows[1]).toHaveTextContent("1.4 of 2 cores");
    expect(rows[1]).toHaveTextContent("3.1 GB of 4 GB");
    expect(rows[2]).toHaveTextContent("Starting");
    // Measured elsewhere but not here: not available, with the size muted under it.
    expect(rows[2]).toHaveTextContent("Not available");
    expect(rows[2]).toHaveTextContent("2 cores");
    expect(rows[3]).toHaveTextContent("Idle");
    expect(rows[3]).toHaveTextContent("Stops in about 4 min");
    expect(rows[4]).toHaveTextContent("You");
    expect(rows[4]).toHaveTextContent("Kept on");
    expect(rows[4]).toHaveTextContent("Desktop");
    expect(screen.queryByRole("region", { name: "Left behind" })).toBeNull();
  });

  it("says the install is full, and how many wait", async () => {
    const data = listing();
    show(
      fake([
        {
          ...data,
          limits: { ...data.limits, slots: 4 },
          totals: { ...data.totals, waiting: 2 },
        },
      ]),
    );
    expect(await screen.findByText("4 of 4 running · full")).toBeVisible();
    expect(
      screen.getByText("2 tasks are waiting for a sandbox."),
    ).toBeVisible();
  });

  it("leaves Desktops out without a Desktop environment, and says Measuring… before the first sample", async () => {
    const data = listing({ items: [{ ...IDLE, usage: null }] });
    show(fake([{ ...data, totals: { ...data.totals, usage: null } }]));
    expect(await screen.findByText("1 of 6 running")).toBeVisible();
    expect(screen.queryByText(/^Desktops:/)).toBeNull();
    expect(screen.getByText("Set aside: 8 cores · 16 GB")).toBeVisible();
    expect(screen.getAllByText("Measuring…")).toHaveLength(2);
  });

  it("names the microVM backend's memory for what it is", async () => {
    show(fake([listing({ backend: "firecracker" })]));
    expect(
      await screen.findByText(
        "Sandboxes run as microVMs. Memory is what each microVM holds.",
      ),
    ).toBeVisible();
  });

  it("shows the empty state with no action", async () => {
    const data = listing({ items: [] });
    show(fake([{ ...data, totals: { ...data.totals, usage: null } }]));
    expect(
      await screen.findByText(
        "No sandbox is running. One starts when someone sends a message.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("says plainly when the sandboxes can't be read, and tries again", async () => {
    const api = fake(new Error("down"));
    show(api);
    expect(
      await screen.findByText("Couldn't read the sandboxes."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(api.sandboxes).toHaveBeenCalledTimes(2);
  });

  it("lists what was left behind above the table, with a copyable remove command", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    show(
      fake([
        listing({
          left_behind: [
            {
              kind: "network",
              name: "tqa-sbx-18e7c62210a67d70",
              age_seconds: 660,
              reason: "sandbox_failed",
              remove: "docker network rm tqa-sbx-18e7c62210a67d70",
            },
          ],
        }),
      ]),
    );
    const section = await screen.findByRole("region", { name: "Left behind" });
    expect(section).toHaveTextContent(
      "A network tqa-sbx-18e7c62210a67d70, 11 minutes old: the engine wouldn't remove it",
    );
    expect(
      within(section).getByText("docker network rm tqa-sbx-18e7c62210a67d70"),
    ).toBeVisible();
    fireEvent.click(within(section).getByRole("button", { name: "Copy" }));
    expect(writeText).toHaveBeenCalledWith(
      "docker network rm tqa-sbx-18e7c62210a67d70",
    );
    expect(
      await within(section).findByRole("button", { name: "Copied" }),
    ).toBeVisible();
    // Above the table, in reading order.
    const table = screen.getByRole("table", { name: "Running sandboxes" });
    expect(
      section.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    vi.unstubAllGlobals();
  });

  it("shows 200 of 201 and reads the next page on Show more", async () => {
    const many = Array.from({ length: 200 }, (_, index) =>
      row({
        conversation_id: `0195d8c0-0000-7000-8000-${String(index).padStart(12, "0")}`,
      }),
    );
    const first = listing({
      items: many,
      next_after: many[199]?.conversation_id ?? null,
    });
    const second = listing({ items: [IDLE] });
    const api = fake([first, second]);
    show(api);
    expect(await screen.findByText("Showing 200 of 200")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show more" }));
    expect(
      await screen.findByText("ben@example.org", {
        exact: false,
        selector: "tr:nth-child(201) *",
      }),
    ).toBeVisible();
    expect(api.sandboxes).toHaveBeenLastCalledWith(many[199]?.conversation_id);
    expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
  });

  it("is for administrators only", () => {
    show(fake([listing()]), member);
    expect(
      screen.getByText("Only administrators can see the sandboxes."),
    ).toBeVisible();
    expect(screen.queryByRole("table")).toBeNull();
  });
});

describe("WEB-U-487 Stop now: the menu, the confirm and every result's words", () => {
  // ben@ owns the working and the idle sandbox: `which` is the row among his, in the listing's order.
  async function openStop(name: string, which = 0) {
    const menus = await screen.findAllByRole("button", {
      name: `More actions for ${name}'s sandbox`,
    });
    fireEvent.click(menus[which] as HTMLElement);
    return screen.getByRole("menuitem", { name: `Stop ${name}'s sandbox now` });
  }

  it("is enabled on an idle row and dimmed with its reason on a working one", async () => {
    show(fake([listing()]));
    const working = await openStop("ben@example.org");
    expect(working).toHaveAttribute("aria-disabled", "true");
    expect(working).toHaveAccessibleDescription(
      "It is working now; it can be stopped once the run ends.",
    );
  });

  it("asks once, naming the owner, and the kept-on and kept sign-in lines only where due", async () => {
    const stop = vi.fn<SandboxesApi["stop"]>(() => Promise.resolve());
    const api = fake(
      [listing(), listing({ items: [WORKING, STARTING, KEPT] })],
      stop,
    );
    show(api);
    fireEvent.click(await openStop("ben@example.org", 1));
    const dialog = await screen.findByRole("dialog", {
      name: "Stop ben@example.org's sandbox?",
    });
    expect(dialog).toHaveTextContent(
      "The task keeps its files and history. Its next message starts a new sandbox, and its owner sees a note in the task.",
    );
    expect(dialog).not.toHaveTextContent("kept on");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Stop sandbox" }),
    );
    // The mutation starts on the next tick.
    await waitFor(() =>
      expect(stop).toHaveBeenCalledWith(
        IDLE.conversation_id,
        expect.any(String),
      ),
    );
    expect(
      await screen.findByText("Stopping ben@example.org's sandbox."),
    ).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    // The list is read again; the stopped row is gone.
    expect(api.sandboxes).toHaveBeenCalledTimes(2);
  });

  it("warns on a kept-on Desktop, your own named as yours", async () => {
    show(fake([listing()]));
    fireEvent.click(await openStop("admin@example.invalid"));
    const dialog = await screen.findByRole("dialog", {
      name: "Stop your sandbox?",
    });
    expect(dialog).toHaveTextContent(
      "It is kept on: its owner chose to keep it running.",
    );
    expect(dialog).toHaveTextContent("Its desktop sign-in ends with it.");
  });

  it.each([
    [
      "working",
      "ben@example.org's sandbox is working now; it can't be stopped until the run ends.",
    ],
    [
      "work_waiting",
      "A message is waiting for ben@example.org's sandbox; it can't be stopped until that run ends.",
    ],
    ["not_found", "That sandbox has already stopped."],
  ])("says what a %s refusal means", async (code, sentence) => {
    const stop = vi.fn<SandboxesApi["stop"]>(() =>
      Promise.reject(
        new ApiError(code === "not_found" ? 404 : 409, code, "refused"),
      ),
    );
    show(fake([listing()], stop));
    fireEvent.click(await openStop("ben@example.org", 1));
    const dialog = await screen.findByRole("dialog", {
      name: "Stop ben@example.org's sandbox?",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Stop sandbox" }),
    );
    expect(await within(dialog).findByText(sentence)).toBeVisible();
  });

  it("shows a failed request's own words", async () => {
    const stop = vi.fn<SandboxesApi["stop"]>(() =>
      Promise.reject(new Error("The request failed.")),
    );
    show(fake([listing()], stop));
    fireEvent.click(await openStop("ben@example.org", 1));
    const dialog = await screen.findByRole("dialog", {
      name: "Stop ben@example.org's sandbox?",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Stop sandbox" }),
    );
    expect(
      await within(dialog).findByText("The request failed."),
    ).toBeVisible();
  });
});

describe("WEB-U-488 the page never renders a task's title, link or count", () => {
  it("shows the owner's email and the environment, and no planted string nor any link on a row", async () => {
    const planted = "Quarterly figures for ACME-7731";
    const data = listing();
    // A gateway that leaked a title would be refused by the validator; here the page itself is the subject: it
    // reads only the documented fields, so extra ones never reach the screen.
    const leaky = {
      ...data,
      items: data.items.map((item) => ({ ...item, title: planted })),
    } as SandboxListing;
    show(fake([leaky]));
    const table = await screen.findByRole("table", {
      name: "Running sandboxes",
    });
    expect(table).not.toHaveTextContent(planted);
    expect(table).not.toHaveTextContent("ACME-7731");
    expect(within(table).queryByRole("link")).toBeNull();
    expect(table).toHaveTextContent("ben@example.org");
    expect(table).toHaveTextContent("Minimal");
    // The administrator's own row reads "You" and nothing more than the others.
    const own = within(table).getAllByRole("row").at(-1);
    expect(own).toHaveTextContent("You");
    expect(own).not.toHaveTextContent(admin.email);
  });
});

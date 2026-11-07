import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { environment, fakeTemplates } from "../test/templates-admin";
import { LIMITS } from "../test/user";

import type { TemplatesApi } from "./admin/templates";
import { STATUS_REFRESH_MS, StatusSettings } from "./StatusSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const ready = {
  ready: true,
  checks: {
    backend_preflight: true,
    image_present: true,
    runtime_revision: true,
    database: true,
    secret_store: true,
  },
  sandbox: { backend: "docker", network_mode: "bridge" },
  templates: {
    default: "workstation",
    default_offered: true,
    default_reason: null,
    checks_pending: 0,
  },
  git_credentials_kept: { not_web_address: 0, waiting: 0 },
  runs_active: 0,
};
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  vi.unstubAllGlobals();
});
const json = (value: unknown, status = 200) =>
  Promise.resolve(Response.json(value, { status }));

function show(me: User = admin, templates?: TemplatesApi) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <StatusSettings
        me={me}
        {...(templates ? { environments: templates } : {})}
      />
    </QueryClientProvider>,
  );
}
const failing = (checks: Partial<typeof ready.checks>, extra = {}) => ({
  ...ready,
  ...extra,
  ready: false,
  checks: { ...ready.checks, ...checks },
});

// Task 090 (FR-22) and the owner's review: can new tasks start, and if not, what to do; passing checks in one line.
describe("WEB-U-033 status", () => {
  it("says in one line that new tasks can start, with the passing checks quietly under it", async () => {
    fetch.mockImplementationOnce(() => json(ready));
    show();
    expect(await screen.findByText("New tasks can start.")).toBeVisible();
    expect(fetch.mock.calls[0]?.[0]).toBe("/readyz?detail=1");
    expect(
      screen.getByText(
        "Passing: Database, Key store, Environment engine, Agent image, Agent version.",
      ),
    ).toBeVisible();
    expect(
      screen.getByText("Environments run as Docker containers."),
    ).toBeVisible();
    expect(screen.getByText(/^Checked at /)).toBeVisible();
    // No table of OK badges, and no engineer terms.
    expect(screen.queryByRole("table")).toBeNull();
    expect(
      screen.queryByText(/network bridge|Secret store|Runtime/),
    ).toBeNull();
  });

  it("explains a failing check and says what to do", async () => {
    fetch.mockImplementationOnce(() =>
      json(failing({ image_present: false }), 503),
    );
    show();
    expect(
      await screen.findByText("New tasks can't start: 1 problem below."),
    ).toBeVisible();
    const problem = screen.getByRole("region", { name: "Agent image" });
    expect(problem).toHaveTextContent(
      "The agent's image is missing on the engine host. Build or load it there, then check again.",
    );
    expect(
      screen.getByText(
        "Passing: Database, Key store, Environment engine, Agent version.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("names every failing check, in reading order, with the microVM engine's own words", async () => {
    fetch.mockImplementationOnce(() =>
      json(
        failing(
          { backend_preflight: false, image_present: false },
          { sandbox: { backend: "firecracker", network_mode: "none" } },
        ),
        503,
      ),
    );
    show();
    expect(
      await screen.findByText("New tasks can't start: 2 problems below."),
    ).toBeVisible();
    expect(
      screen
        .getAllByRole("heading", { level: 2 })
        .map((heading) => heading.textContent),
    ).toEqual([
      "Environment engine",
      "Agent image",
      "Default environment",
      "Before a restart",
    ]);
    expect(
      screen.getByRole("region", { name: "Environment engine" }),
    ).toHaveTextContent(
      "The gateway can't reach the microVM engine, which runs environments. Check that it is running and reachable from the gateway.",
    );
    expect(
      screen.getByRole("region", { name: "Agent image" }),
    ).toHaveTextContent("or its VM disk image isn't built");
    expect(screen.getByText("Environments run as microVMs.")).toBeVisible();
  });

  it.each([
    [
      "database",
      "Database",
      "The gateway can't reach its database. Check that the database is running and the gateway's database address is right.",
    ],
    [
      "secret_store",
      "Key store",
      "The gateway can't open its key store, so tasks can't get their keys. Check the store's files and permissions on the gateway host.",
    ],
    [
      "runtime_revision",
      "Agent version",
      "The agent's image is older than this gateway. Rebuild it from this release, then check again.",
    ],
  ])("says what to do when %s fails", async (key, name, advice) => {
    fetch.mockImplementationOnce(() => json(failing({ [key]: false }), 503));
    show();
    expect(await screen.findByRole("region", { name })).toHaveTextContent(
      advice,
    );
  });

  it("says plainly when the status cannot be loaded, and tries again", async () => {
    fetch
      .mockImplementationOnce(() =>
        json(
          { error: { code: "internal", message: "x", retryable: true } },
          500,
        ),
      )
      .mockImplementationOnce(() => json(ready));
    show();
    expect(
      await screen.findByText("The status could not be loaded."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("New tasks can start.")).toBeVisible();
  });

  it("refuses a body that is not the readiness contract", async () => {
    fetch.mockImplementationOnce(() => json({ ready: true }));
    show();
    expect(
      await screen.findByText("The status could not be loaded."),
    ).toBeVisible();
  });

  it("checks again on request", async () => {
    fetch
      .mockImplementationOnce(() => json(failing({ database: false }), 503))
      .mockImplementationOnce(() => json(ready));
    show();
    expect(
      await screen.findByText("New tasks can't start: 1 problem below."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByText("New tasks can start.")).toBeVisible();
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("is for administrators only", () => {
    show({ ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can see the status."),
    ).toBeVisible();
    expect(fetch).not.toHaveBeenCalled();
  });
});

// ADR-0032 (D1): the default environment's state from readiness, with why people cannot pick it (WEB-U-047).
describe("WEB-U-047 the default environment in status", () => {
  const section = () =>
    screen.getByRole("region", { name: "Default environment" });

  it("says the default is available", async () => {
    fetch.mockImplementationOnce(() => json(ready));
    show();
    await screen.findByText("New tasks can start.");
    expect(section()).toHaveTextContent("workstation");
    expect(within(section()).getByText("Available")).toBeVisible();
    expect(within(section()).queryByRole("link")).toBeNull();
    expect(within(section()).queryByText(/waiting for their check/)).toBeNull();
  });

  it.each([
    ["rootfs_missing", /No VM disk image yet/],
    ["disabled", /It is turned off\./],
    ["unverified", /It has not passed its check yet\./],
    ["refused", /a reason this page does not know yet/],
  ])(
    "says why it is not available (%s) and links to Environments",
    async (reason, sentence) => {
      fetch.mockImplementationOnce(() =>
        json({
          ...ready,
          templates: {
            ...ready.templates,
            default_offered: false,
            default_reason: reason,
            checks_pending: 2,
          },
        }),
      );
      show();
      await screen.findByText("New tasks can start.");
      expect(within(section()).getByText("Not available")).toBeVisible();
      expect(section()).toHaveTextContent(sentence);
      expect(
        within(section()).getByRole("link", { name: "Open Environments" }),
      ).toHaveAttribute("href", "/settings/environments");
      expect(section()).toHaveTextContent(
        "2 environments are waiting for their check.",
      );
    },
  );

  it("names the default by its display name, with the link set apart from the reason (UX review 0927)", async () => {
    fetch.mockImplementationOnce(() =>
      json({
        ...ready,
        templates: {
          ...ready.templates,
          default: "legacy",
          default_offered: false,
          default_reason: "image_not_found",
        },
      }),
    );
    show(
      admin,
      fakeTemplates({
        environments: [
          environment("legacy", {
            display_name: "Legacy tools",
            is_default: true,
            check: {
              state: "refused",
              reason: "image_not_found",
              checked_version: 1,
              secret_files: null,
              scan_limit: null,
            },
          }),
        ],
      }).api,
    );
    await screen.findByText("New tasks can start.");
    expect(await within(section()).findByText("Legacy tools")).toBeVisible();
    const link = within(section()).getByRole("link", {
      name: "Open Environments",
    });
    // Its own line: the reason sentence does not run into it.
    expect(link.parentElement).not.toHaveTextContent(/digest/);
  });
});

// The dev screen review (011e8972), #8: while new tasks couldn't start, the default environment still wore a green
// "Available". Its pill is muted then, and says it is waiting on the problem above.
describe("WEB-U-174 the default environment while tasks can't start", () => {
  const section = () =>
    screen.getByRole("region", { name: "Default environment" });

  it("shows no green Available, and says it waits on the problem above", async () => {
    fetch.mockImplementationOnce(() => json(failing({ image_present: false })));
    show();
    await screen.findByText(/New tasks can.t start/);
    expect(within(section()).queryByText("Available")).toBeNull();
    const pill = within(section()).getByText(
      "Ready once the problem above is fixed",
    );
    expect(pill).toHaveAttribute("data-tone", "muted");
    expect(section().querySelector('[data-tone="ok"]')).toBeNull();
  });

  it("names the default's own reason first when it isn't offered either", async () => {
    fetch.mockImplementationOnce(() =>
      json(
        failing(
          { image_present: false },
          {
            templates: {
              ...ready.templates,
              default_offered: false,
              default_reason: "disabled",
            },
          },
        ),
      ),
    );
    show();
    await screen.findByText(/New tasks can.t start/);
    expect(within(section()).getByText("Not available")).toHaveAttribute(
      "data-tone",
      "danger",
    );
  });
});

// The fix's review: with no problem listed (a check this page doesn't name), "the problem above" points at nothing.
describe("WEB-U-174 the default environment's words when no problem is listed", () => {
  it("says it is ready once new tasks can start", async () => {
    fetch.mockImplementationOnce(() => json(failing({})));
    show();
    await screen.findByText(/New tasks can.t start/);
    const section = screen.getByRole("region", { name: "Default environment" });
    expect(section).toHaveTextContent("Ready once new tasks can start");
    expect(section).not.toHaveTextContent(/above/);
  });
});

// ADR-0037 §7's second release: Git credentials that couldn't become keys are kept, and the administrator is told how
// many and what to do. Diagnostics: tasks can still start.
describe("WEB-U-265 Git credentials kept in status", () => {
  it("says how many couldn't become keys and what to do, beside a ready gateway", async () => {
    fetch.mockImplementationOnce(() =>
      json({
        ...ready,
        git_credentials_kept: { not_web_address: 2, waiting: 1 },
      }),
    );
    show();
    expect(await screen.findByText("New tasks can start.")).toBeVisible();
    const kept = screen.getByRole("region", { name: "Git credentials" });
    expect(kept).toHaveTextContent(
      "2 Git credentials couldn't become keys: their address isn't a web address. Add them under Keys by hand.",
    );
    expect(kept).toHaveTextContent(
      "1 Git credential is waiting to become a key: its owner already has the most keys.",
    );
  });

  it("says nothing when there are none, or the count couldn't be read", async () => {
    for (const kept of [{ not_web_address: 0, waiting: 0 }, null]) {
      fetch.mockImplementationOnce(() =>
        json({ ...ready, git_credentials_kept: kept }),
      );
      show();
      expect(await screen.findByText("New tasks can start.")).toBeVisible();
      expect(
        screen.queryByRole("region", { name: "Git credentials" }),
      ).toBeNull();
      document.body.innerHTML = "";
    }
  });
});

// First-hour F7's follow-up (GW-I-229): how many tasks are working on a step, a count only, so a restart can wait for a
// gap; the page reads it again every 30 s while it is open.
describe("WEB-U-292 tasks working on a step in status", () => {
  it("says no task is working on a step, so restarting now starts no step over", async () => {
    fetch.mockImplementationOnce(() => json(ready));
    show();
    const before = await screen.findByRole("region", {
      name: "Before a restart",
    });
    expect(before).toHaveTextContent(
      "No task is working on a step, so restarting now wouldn't start any step over.",
    );
  });

  it("counts the tasks working on a step and says to wait or tell people, never naming them", async () => {
    for (const [count, words] of [
      [
        1,
        "1 task is working on a step. Restarting now would start its step over: wait until none is, or tell people first.",
      ],
      [
        3,
        "3 tasks are working on a step. Restarting now would start their steps over: wait until none is, or tell people first.",
      ],
    ] as const) {
      fetch.mockImplementationOnce(() =>
        json({ ...ready, runs_active: count }),
      );
      show();
      const before = await screen.findByRole("region", {
        name: "Before a restart",
      });
      expect(before).toHaveTextContent(words);
      expect(within(before).queryAllByRole("link")).toHaveLength(0);
      document.body.innerHTML = "";
    }
  });

  it("says nothing when the count couldn't be read", async () => {
    fetch.mockImplementationOnce(() => json({ ...ready, runs_active: null }));
    show();
    expect(await screen.findByText("New tasks can start.")).toBeVisible();
    expect(
      screen.queryByRole("region", { name: "Before a restart" }),
    ).toBeNull();
  });

  it("reads the count again every 30 s while the page is open", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      fetch
        .mockImplementationOnce(() => json({ ...ready, runs_active: 2 }))
        .mockImplementationOnce(() => json({ ...ready, runs_active: 0 }));
      show();
      const before = await screen.findByRole("region", {
        name: "Before a restart",
      });
      expect(before).toHaveTextContent("2 tasks are working on a step.");
      await vi.advanceTimersByTimeAsync(STATUS_REFRESH_MS);
      expect(
        await within(before).findByText(
          "No task is working on a step, so restarting now wouldn't start any step over.",
        ),
      ).toBeVisible();
      expect(fetch).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  // WEB-U-297: the count is a live line that stays the same element across reads, so a change is announced and an
  // unchanged read says nothing; "Checked at", which changes on every read, isn't live.
  it("WEB-U-297: announces the count when it changes, and never the time of each read", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      fetch
        .mockImplementationOnce(() => json({ ...ready, runs_active: 1 }))
        .mockImplementationOnce(() => json({ ...ready, runs_active: 0 }));
      show();
      const before = await screen.findByRole("region", {
        name: "Before a restart",
      });
      const line = within(before).getByRole("status");
      expect(line).toHaveTextContent(/^1 task is working on a step\./);
      await vi.advanceTimersByTimeAsync(STATUS_REFRESH_MS);
      await within(before).findByText(/^No task is working on a step/);
      // The same live element, its text replaced: what a screen reader announces.
      expect(within(before).getByRole("status")).toBe(line);
      expect(
        screen
          .getByText(/^Checked at /)
          .closest('[role="status"], [aria-live]'),
      ).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});

// WEB-U-299 (the c15 accessibility review): each Status section is named once, by its heading, and the verdict is a
// live line, so a change after Check again is heard.
describe("WEB-U-299 Status's sections and verdict", () => {
  it("names each section by its heading, and the verdict is a live line", async () => {
    fetch.mockImplementationOnce(() => json({ ...ready, runs_active: 0 }));
    show();
    const before = await screen.findByRole("region", {
      name: "Before a restart",
    });
    const heading = within(before).getByRole("heading", {
      name: "Before a restart",
    });
    expect(before).toHaveAttribute("aria-labelledby", heading.id);
    expect(before).not.toHaveAttribute("aria-label");
    expect(screen.getByText("New tasks can start.")).toHaveAttribute(
      "role",
      "status",
    );
  });

  // The 7 Oct review, P3-2: the claim itself, as WEB-U-297 does for the count. After Check again the verdict is the
  // same live element with its words replaced, which is what a screen reader announces.
  it("WEB-U-299: a changed verdict after Check again is the same live line, its words replaced", async () => {
    fetch
      .mockImplementationOnce(() => json(ready))
      .mockImplementationOnce(() =>
        json({
          ...ready,
          ready: false,
          checks: { ...ready.checks, database: false },
        }),
      );
    show();
    const verdict = await screen.findByText("New tasks can start.");
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(
      await screen.findByText("New tasks can't start: 1 problem below."),
    ).toBe(verdict);
    expect(verdict).toHaveAttribute("role", "status");
  });

  // The 7 Oct review, P3-3: a failed read after a good one keeps the page and its live lines, says so in a live line of
  // its own, and the next good read is heard in the same elements.
  it("keeps the last read and its live lines when a 30 s read fails", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      fetch
        .mockImplementationOnce(() => json({ ...ready, runs_active: 1 }))
        .mockImplementationOnce(() => json({ error: "internal" }, 500))
        .mockImplementationOnce(() => json({ ...ready, runs_active: 0 }));
      show();
      const verdict = await screen.findByText("New tasks can start.");
      const before = screen.getByRole("region", { name: "Before a restart" });
      const count = within(before).getByRole("status");
      const failed =
        "The latest read failed, so this shows the one before. It's read again in 30 s, or choose Check again.";
      expect(screen.queryByText(failed)).toBeNull();

      await vi.advanceTimersByTimeAsync(STATUS_REFRESH_MS);
      const line = await screen.findByText(failed);
      expect(line).toHaveAttribute("role", "status");
      // WEB-U-361 (the 16f page pass, E15): an older read shown is quiet, never the danger tone under a green answer.
      expect(line).toHaveClass("muted");
      expect(line).not.toHaveClass("field-error");
      expect(screen.queryByText("The status could not be loaded.")).toBeNull();
      expect(screen.getByText("New tasks can start.")).toBe(verdict);
      expect(within(before).getByRole("status")).toBe(count);
      expect(count).toHaveTextContent(/^1 task is working on a step\./);

      await vi.advanceTimersByTimeAsync(STATUS_REFRESH_MS);
      await within(before).findByText(/^No task is working on a step/);
      expect(within(before).getByRole("status")).toBe(count);
      expect(screen.getByText("New tasks can start.")).toBe(verdict);
      expect(line).toBeEmptyDOMElement();
    } finally {
      vi.useRealTimers();
    }
  });
});

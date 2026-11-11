import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { modelName } from "../conversations/model-name";
import type { User } from "../protocol/generated/api";
import { choose } from "../test/picker";
import { LIMITS } from "../test/user";

import type {
  AdminApi,
  UsageQuery,
  UsageReport,
  UsageRow,
} from "./admin/contract";
import { UsageSettings } from "./UsageSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
// A fixed clock: the page's period ends on this UTC day.
const clock = () => new Date("2026-09-26T15:00:00Z");
const figures = {
  requests: 12,
  reported: 12,
  estimated: 0,
  prompt_tokens: 30363,
  completion_tokens: 2920,
  cached_tokens: 1024,
  reasoning_tokens: null,
};
const row = (key: string, extra: Partial<UsageRow> = {}): UsageRow => ({
  key,
  label: null,
  ...figures,
  ...extra,
});
const report = (extra: Partial<UsageReport> = {}): UsageReport => ({
  from: "2026-08-28",
  to: "2026-09-26",
  group: "day",
  rows: [
    row("2026-09-24", {
      requests: 0,
      reported: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      cached_tokens: null,
    }),
    row("2026-09-25", { prompt_tokens: 20000 }),
    row("2026-09-26"),
  ],
  totals: {
    ...figures,
    requests: 24,
    reported: 24,
    prompt_tokens: 60726,
    completion_tokens: 5840,
    cached_tokens: 2048,
  },
  truncated: false,
  ...extra,
});
function fake(answer: (query: UsageQuery) => Promise<UsageReport>) {
  const usage = vi.fn<AdminApi["usage"]>(answer);
  const api: AdminApi = {
    usage,
    secrets: vi.fn(),
    putSecret: vi.fn(),
    deleteSecret: vi.fn(),
    secretReads: vi.fn(),
    people: vi.fn(),
    setConsumers: vi.fn(),
    consumerChoices: vi.fn(),
    setReminder: vi.fn(),
  };
  return { api, usage };
}
/** The routes the model picker knows: "local" has no display name (the quick-start route, found before 17.0). */
const catalogue = {
  items: [
    {
      name: "local",
      model: "qwen3-4b",
      display_name: null,
      description: null,
      verified: null,
      protocol: "openai_chat",
      context_window: null,
      vision: false,
      native_tools: true,
      reasoning: null,
      runs_on: { where: "this_computer", provider: null },
    },
  ],
};
function show(api: AdminApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["models"], catalogue);
  render(
    <QueryClientProvider client={cache}>
      <UsageSettings me={me} api={api} clock={clock} />
    </QueryClientProvider>,
  );
}
const GROUP = "\u202f";
// Thousands take a narrow no-break space (UI-GUIDELINES §10): matched as written, since the default matcher folds it to a space.
const exactly = { normalizer: (text: string) => text };
const zero = {
  requests: 0,
  reported: 0,
  estimated: 0,
  prompt_tokens: 0,
  completion_tokens: 0,
  cached_tokens: null,
  reasoning_tokens: null,
};
const people = report({
  group: "user",
  rows: [
    row("0195d8c0-0000-7000-8000-000000000003", {
      requests: 4,
      prompt_tokens: 10726,
      completion_tokens: 840,
    }),
    row("0195d8c0-0000-7000-8000-000000000002", {
      label: "member@example.invalid",
      requests: 20,
      prompt_tokens: 50000,
      completion_tokens: 5000,
    }),
  ],
});
const models = report({
  group: "route",
  rows: [
    row("deepseek-flash", {
      prompt_tokens: 50000,
      completion_tokens: 5000,
    }),
    row("local", { prompt_tokens: 10726, completion_tokens: 840 }),
  ],
});
/** The gateway's answer per query: this period by day, person and model, and the period it is compared with. */
function answers(
  compared: UsageReport["totals"] = {
    ...figures,
    requests: 20,
    reported: 20,
    prompt_tokens: 50000,
    completion_tokens: 6410,
  },
) {
  return (query: UsageQuery) =>
    Promise.resolve(
      query.group === "user"
        ? people
        : query.group === "route"
          ? models
          : query.from.slice(0, 7) === "2026-09"
            ? report()
            : report({ rows: [], totals: compared }),
    );
}

// FR-21: the period's total and its change first, then who and which model use it, largest first; each day only on
// demand, newest first (WEB-U-034).
describe("WEB-U-034 usage", () => {
  it("leads with this month's tokens and their change against the same days of last month", async () => {
    const { api, usage } = fake(answers());
    show(api);
    expect(await screen.findByText("66.6K tokens", exactly)).toBeVisible();
    // WEB-U-461: the exact figure stays for screen readers; the short one is what shows.
    expect(screen.getByText(`66${GROUP}566 tokens`, exactly)).toHaveClass(
      "sr-only",
    );
    // 66 566 against 56 410: 18 % more.
    expect(screen.getByText("+18% vs the same days of August")).toBeVisible();
    expect(
      screen.getByText(
        "24 requests · 60.7K tokens in · 5.8K tokens out",
        exactly,
      ),
    ).toBeVisible();
    expect(
      screen.getByText("Cached: 2K · Reasoning: not reported", exactly),
    ).toBeVisible();
    expect(usage.mock.calls.map(([query]) => query)).toEqual(
      expect.arrayContaining([
        { from: "2026-09-01", to: "2026-09-26", group: "day" },
        { from: "2026-09-01", to: "2026-09-26", group: "user" },
        { from: "2026-09-01", to: "2026-09-26", group: "route" },
        { from: "2026-08-01", to: "2026-08-26", group: "day" },
      ]),
    );
    // Decision engines count against their own budgets, on their page: the sentence's own words are the link.
    const engines = screen.getByRole("link", { name: "Decision engines" });
    expect(engines).toHaveAttribute("href", "/settings/decision-engines");
    expect(engines.parentElement).toHaveTextContent(
      /^Decision engines have their own budgets\.$/,
    );
  });

  it("draws each day as a bar with zero days as gaps, named by its busiest day", async () => {
    const { api } = fake(answers());
    show(api);
    const chart = await screen.findByRole("img", { name: /Tokens per day/ });
    expect(chart).toHaveAccessibleName(
      `Tokens per day. Busiest: Sep 26, 2026, 33${GROUP}283 tokens.`,
    );
    expect(chart.querySelectorAll("[data-day]")).toHaveLength(26);
    expect(chart.querySelectorAll("[data-day][data-empty]")).toHaveLength(24);
  });

  it("ranks people and models by tokens, largest first, with their share", async () => {
    const { api } = fake(answers());
    show(api);
    const byPerson = await screen.findByRole("table", {
      name: "Usage by person",
    });
    expect(
      within(byPerson)
        .getAllByRole("row")
        .slice(1)
        .map((line) =>
          within(line)
            .getAllByRole("cell")
            .map(
              (cell) =>
                (cell.querySelector('[aria-hidden="true"]') ?? cell)
                  .textContent,
            ),
        ),
    ).toEqual([
      ["member@example.invalid", "55K", "83%"],
      ["Removed account", "11.6K", "17%"],
    ]);
    const byModel = screen.getByRole("table", { name: "Usage by model" });
    expect(within(byModel).getAllByRole("row")[1]).toHaveTextContent(
      "deepseek-flash",
    );
  });

  it("shows the top five and the rest on request", async () => {
    const many = report({
      group: "user",
      rows: Array.from({ length: 7 }, (_, index) =>
        row(`person-${index}`, {
          label: `p${index}@example.invalid`,
          prompt_tokens: 1000 * (index + 1),
          completion_tokens: 0,
        }),
      ),
    });
    const { api } = fake((query) =>
      query.group === "user" ? Promise.resolve(many) : answers()(query),
    );
    show(api);
    const byPerson = await screen.findByRole("table", {
      name: "Usage by person",
    });
    expect(within(byPerson).getAllByRole("row")).toHaveLength(6);
    expect(within(byPerson).getAllByRole("row")[1]).toHaveTextContent(
      "p6@example.invalid",
    );
    fireEvent.click(screen.getByRole("button", { name: "Show all 7 people" }));
    expect(within(byPerson).getAllByRole("row")).toHaveLength(8);
  });

  it("lists each day only on request, newest first and without the empty ones", async () => {
    const { api } = fake(answers());
    show(api);
    await screen.findByText(`66${GROUP}566 tokens`, exactly);
    expect(screen.queryByRole("table", { name: "Usage by day" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show each day" }));
    const table = screen.getByRole("table", { name: "Usage by day" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["Day", "Requests", "Tokens in", "Tokens out"]);
    expect(
      within(table)
        .getAllByRole("row")
        .slice(1)
        .map((line) => within(line).getAllByRole("cell")[0]?.textContent),
    ).toEqual(["Sep 26, 2026", "Sep 25, 2026"]);
  });

  // UI-GUIDELINES §10, one way per kind of count on a page: requests read exactly, in the summary (whose plural needs
  // the number itself) and on each day alike; only the token counts read short.
  it("reads each day's requests exactly, as the summary reads the period's", async () => {
    const busy = report({
      rows: [row("2026-09-26", { requests: 1234, reported: 1234 })],
      totals: { ...figures, requests: 1234, reported: 1234 },
    });
    const { api } = fake((query) =>
      Promise.resolve(
        query.group === "user"
          ? people
          : query.group === "route"
            ? models
            : busy,
      ),
    );
    show(api);
    expect(
      await screen.findByText(
        `1${GROUP}234 requests · 30.4K tokens in · 2.9K tokens out`,
        exactly,
      ),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show each day" }));
    const table = screen.getByRole("table", { name: "Usage by day" });
    const [, day] = within(table).getAllByRole("row");
    if (!day) throw new Error("the table has the day's row");
    expect(within(day).getAllByRole("cell")[1]?.textContent).toBe(
      `1${GROUP}234`,
    );
  });

  it.each([
    [
      "lastMonth",
      { from: "2026-08-01", to: "2026-08-31" },
      { from: "2026-07-01", to: "2026-07-31" },
      "+18% vs July",
    ],
    [
      "7",
      { from: "2026-09-20", to: "2026-09-26" },
      { from: "2026-09-13", to: "2026-09-19" },
      "+18% vs the 7 days before",
    ],
  ])(
    "asks for the chosen period (%s) and compares it with the one before",
    async (value, range, before, change) => {
      const { api, usage } = fake((query) =>
        Promise.resolve(
          query.group !== "day"
            ? query.group === "user"
              ? people
              : models
            : query.from === range.from
              ? report()
              : report({
                  rows: [],
                  totals: {
                    ...figures,
                    requests: 20,
                    reported: 20,
                    prompt_tokens: 50000,
                    completion_tokens: 6410,
                  },
                }),
        ),
      );
      show(api);
      await screen.findByRole("button", { name: "Show each day" });
      choose(
        "Period",
        value === "lastMonth"
          ? "Last month"
          : value === "thisMonth"
            ? "This month"
            : `Last ${value} days`,
      );
      expect(await screen.findByText(change)).toBeVisible();
      expect(usage.mock.calls.map(([query]) => query)).toEqual(
        expect.arrayContaining([
          { ...range, group: "day" },
          { ...before, group: "day" },
        ]),
      );
    },
  );

  it("leaves out the cached and reasoning line when the model servers reported neither", async () => {
    const quiet = {
      ...report().totals,
      cached_tokens: null,
      reasoning_tokens: null,
    };
    const { api } = fake((query) =>
      query.group === "day" && query.from === "2026-09-01"
        ? Promise.resolve(report({ totals: quiet }))
        : answers()(query),
    );
    show(api);
    await screen.findByText(`66${GROUP}566 tokens`, exactly);
    expect(screen.queryByText(/^Cached:/)).toBeNull();
  });

  it("says when there is nothing to compare with", async () => {
    const { api } = fake(answers(zero));
    show(api);
    expect(
      await screen.findByText(
        "Nothing to compare with in the same days of August",
      ),
    ).toBeVisible();
  });

  it("says how many requests reported no usage, and notes a shortened list", async () => {
    const { api } = fake((query) =>
      query.group === "user"
        ? Promise.resolve({ ...people, truncated: true })
        : query.group === "day" && query.from === "2026-09-01"
          ? Promise.resolve(
              report({ totals: { ...report().totals, reported: 21 } }),
            )
          : answers()(query),
    );
    show(api);
    expect(
      await screen.findByText(
        "3 requests reported no token usage; their tokens are not counted.",
      ),
    ).toBeVisible();
    expect(
      screen.getByText("Showing the 2 largest; the totals include the rest."),
    ).toBeVisible();
  });

  // WEB-U-337: a request stopped mid-reply never got its server's usage; the gateway estimates it from what streamed
  // and says so, and only the rest are "not counted".
  it("WEB-U-337 says how many requests are estimated, apart from those not counted", async () => {
    const { api } = fake((query) =>
      query.group === "day" && query.from === "2026-09-01"
        ? Promise.resolve(
            report({
              totals: { ...report().totals, reported: 20, estimated: 2 },
            }),
          )
        : answers()(query),
    );
    show(api);
    expect(
      await screen.findByText(
        "2 requests ended before their model server reported usage; their tokens are estimated from what was streamed.",
      ),
    ).toBeVisible();
    expect(
      screen.getByText(
        "2 requests reported no token usage; their tokens are not counted.",
      ),
    ).toBeVisible();
  });

  it("says so when the period has no model requests", async () => {
    const { api } = fake(() =>
      Promise.resolve(report({ rows: [], totals: zero })),
    );
    show(api);
    expect(
      await screen.findByText("No model requests in this period."),
    ).toBeVisible();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("says plainly when usage cannot be loaded, and tries again", async () => {
    let failing = true;
    const { api } = fake((query) =>
      failing
        ? Promise.reject(new ApiError(503, "store_unavailable", "x"))
        : answers()(query),
    );
    show(api);
    expect(await screen.findByText("Usage could not be loaded.")).toBeVisible();
    failing = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(
      await screen.findByText(`66${GROUP}566 tokens`, exactly),
    ).toBeVisible();
  });

  it("is for administrators only", () => {
    const { api, usage } = fake(() => Promise.resolve(report()));
    show(api, { ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can see usage."),
    ).toBeVisible();
    expect(usage).not.toHaveBeenCalled();
  });
});

// Found before 17.0: on day one the chart was one unlabelled bar at the right edge, the Model column read "local", and
// "59.2K" sat above "59 200".
describe("WEB-U-127 usage reads the same on day one as later", () => {
  it("dates the chart at both ends and says its busiest day in words", async () => {
    const { api } = fake(answers());
    show(api);
    const chart = await screen.findByRole("img", { name: /^Tokens per day/ });
    const figure = chart.closest("figure");
    if (!figure) throw new Error("The chart is a figure");
    expect(within(figure).getByText("Sep 1, 2026")).toBeVisible();
    expect(within(figure).getByText("Sep 26, 2026")).toBeVisible();
    // The busiest day, visible in short form (WEB-U-461) and the chart's name with the exact figure.
    const caption = figure.querySelector("figcaption");
    expect(caption?.querySelector('[aria-hidden="true"]')).toHaveTextContent(
      "Tokens per day. Busiest: Sep 26, 2026, 33.3K tokens.",
    );
    expect(chart).toHaveAccessibleName(
      `Tokens per day. Busiest: Sep 26, 2026, 33${GROUP}283 tokens.`,
    );
  });

  it("names a route without a display name as the model picker does, and one with its display name", async () => {
    const { api } = fake((query) =>
      Promise.resolve(
        query.group === "route"
          ? report({
              group: "route",
              rows: [
                row("deepseek-flash", {
                  label: "DeepSeek Flash",
                  prompt_tokens: 50000,
                }),
                row("local", { prompt_tokens: 10726 }),
              ],
            })
          : answers()(query),
      ),
    );
    show(api);
    const table = await screen.findByRole("table", { name: "Usage by model" });
    expect(within(table).getByText("DeepSeek Flash")).toBeVisible();
    const local = modelName(catalogue.items[0] as { model: string });
    expect(local).not.toBe("local");
    expect(within(table).getByText(local)).toBeVisible();
    expect(within(table).queryByText("local")).toBeNull();
  });

  it("WEB-U-461 writes every token count the same way: short on screen, never a long row of digits", async () => {
    const { api } = fake(answers());
    show(api);
    await screen.findByText("66.6K tokens", exactly);
    // What shows on screen: everything but the screen-reader twins.
    const page = document.body.cloneNode(true) as HTMLElement;
    for (const hidden of page.querySelectorAll(".sr-only")) hidden.remove();
    const long = page.textContent?.match(
      new RegExp(`.{0,30}\\d${GROUP}\\d{3}.{0,30}`, "g"),
    );
    expect(long).toBeNull();
    expect(page.textContent).toMatch(/\d(\.\d)?K\b/);
  });
});

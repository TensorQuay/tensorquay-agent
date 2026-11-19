import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import { edgeLead, events, stopStarted, writing } from "./agents-stack-api";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// E2E-AGENTS-040 (18.1, ADR-0034's rounds amendment): a team in two rounds on the agents stack suite, with ROUNDS-A's
// script in deploy/tests/fixtures/agents.json. Round 1: the lead starts a tallier and a checker, reads their reports
// and ends at done. The person writes again: that run opens round 2, the lead brings the tallier back by its name (its
// copy kept), reads its report and ends again. The person's API says two rounds and every round's tokens; the page
// says Round 2 in the strip, "Round 2 started", round 2's end note, and that the checker isn't in this round. It runs
// before agents-stack.spec.ts (name order), so E2E-AGENTS-020 stays last.
test.skip(
  process.env.TQA_STACK_SUITE !== "agents",
  "Requires the agents stack suite",
);

test.afterEach(async ({ page, baseURL }) => {
  await stopStarted(page, baseURL);
});

type RoundsTeam = {
  state: string;
  round: number;
  rounds: {
    round: number;
    ended_at: string | null;
    stop_reason: string | null;
    budget: { tokens: number; spent: number };
  }[];
  tokens_all_rounds: number;
  helpers: { name: string; round: number; state: string }[];
};

async function rounds(page: Page, leadId: string): Promise<RoundsTeam | null> {
  const response = await page.request.get(
    `/api/v1/conversations/${leadId}/team`,
  );
  if (response.status() === 404) return null;
  expect(response.status()).toBe(200);
  return (await response.json()) as RoundsTeam;
}

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({
    path: join(directory, `${name}.png`),
    fullPage: true,
  });
}

test("E2E-AGENTS-040 a person's follow-up opens round 2, which brings a helper back, and the page says so", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(4, 300_000));
  await bootstrap(page);
  const leadId = await edgeLead(
    page,
    baseURL,
    "ROUNDS-A count the regions over two rounds",
  );

  // Round 1 ends at the lead's done.
  await expect
    .poll(
      async () => {
        const now = await rounds(page, leadId);
        return now ? `${now.round} ${now.state}` : null;
      },
      { timeout: afterStart(180_000) },
    )
    .toBe("1 ended");

  // The person writes again: that run opens round 2, and the tallier comes back.
  const sent = await page.request.post(
    `/api/v1/conversations/${leadId}/messages`,
    {
      headers: writing(baseURL),
      data: { text: "Count them once more.", attachments: [] },
    },
  );
  expect(sent.ok()).toBe(true);
  await expect
    .poll(
      async () => {
        const now = await rounds(page, leadId);
        return now ? `${now.round} ${now.state}` : null;
      },
      { timeout: afterStart(180_000) },
    )
    .toBe("2 ended");

  // The person's API: two rounds, each ended at done, every round's tokens; the tallier in round 2, the checker not.
  const team = (await rounds(page, leadId))!;
  expect(team.rounds.map((each) => [each.round, each.stop_reason])).toEqual([
    [1, "done"],
    [2, "done"],
  ]);
  expect(team.rounds.every((each) => each.ended_at !== null)).toBe(true);
  expect(team.tokens_all_rounds).toBe(
    team.rounds.reduce((sum, each) => sum + each.budget.spent, 0),
  );
  const round = Object.fromEntries(
    team.helpers.map((each) => [each.name, each.round]),
  );
  expect(round).toEqual({ tallier: 2, "checker-r": 1 });

  // The log has one round_started, for round 2, to the person.
  const log = await page.request.get(
    `/api/v1/conversations/${leadId}/team/messages?after=0&limit=200`,
  );
  expect(log.status()).toBe(200);
  const { items } = (await log.json()) as {
    items: { type: string; round?: number; to: { kind: string } }[];
  };
  const started = items.filter((each) => each.type === "round_started");
  expect(started.map((each) => [each.round, each.to.kind])).toEqual([
    [2, "person"],
  ]);

  // What the run left, for a failure's reading: the team, its log, and each helper's events by type.
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (directory) {
    const helpers = await page.request.get(
      `/api/v1/conversations/${leadId}/team`,
    );
    const each = (
      (await helpers.json()) as {
        helpers: { conversation: string; name: string }[];
      }
    ).helpers;
    const journals: Record<string, string[]> = {};
    for (const helper of each)
      journals[helper.name] = (await events(page, helper.conversation)).map(
        (event) => event.type,
      );
    await mkdir(directory, { recursive: true });
    await writeFile(
      join(directory, "rounds-diagnostics.json"),
      JSON.stringify({ team, log: items, journals }, null, 1),
    );
  }

  // The page: the strip names the round, the transcript its start and round 2's end note.
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const strip = page.getByRole("region", { name: "Helpers" });
  await expect(strip).toContainText("Round 2");
  await expect(
    page.getByText("Round 2 started", { exact: true }),
  ).toBeVisible();
  const roundTwo = page.getByRole("region", { name: /^Round 2: / });
  await expect(roundTwo).toBeVisible();
  // Round 2's note counts round 2 only: the tallier's one report, and no share for the checker, not in this round.
  await expect(roundTwo).toContainText("1 report from tallier");
  await expect(roundTwo).not.toContainText("checker-r");
  await capture(page, "rounds-lead");
  // All helpers: the checker wasn't brought back.
  await strip.getByRole("button", { name: "All helpers" }).click();
  const sheet = page.getByRole("dialog", { name: "All helpers" });
  const checker = sheet.getByRole("listitem").filter({ hasText: "checker-r" });
  await expect(checker).toContainText("Not in this round");
  await expect(
    sheet.getByRole("listitem").filter({ hasText: "tallier" }),
  ).not.toContainText("Not in this round");
  await capture(page, "rounds-all-helpers");
});

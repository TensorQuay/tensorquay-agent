import { expect, test } from "@playwright/test";

import { edgeLead, helperStates, stopStarted, team } from "./agents-stack-api";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// E2E-AGENTS-039 (c17 3a; GW-I-319, WEB-U-423): a lead woken by its helper's report and stopped by its person before
// it read it isn't woken again for that report (teams::wake's rule), so the report waited unread and nothing said so
// (the c16k wake review's S2). The lead's strip now says how many of the team's messages wait unread for it, and the
// count follows a later report live, without a reload. On the real stack with the scripted model: EDGE-U's lead starts
// "reporter-u", ends its turn, and holds its woken turn; reporter-u reports after 5 s and again 90 s later.
// The file sorts before agents-stack.spec.ts, so E2E-AGENTS-020 stays last.
test.skip(
  process.env.TQA_STACK_SUITE !== "agents",
  "Requires the agents stack suite",
);

test.afterEach(async ({ page, baseURL }) => {
  await stopStarted(page, baseURL);
});

test("E2E-AGENTS-039 a lead stopped before it read its helper's report: the strip says what waits unread, live", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 360_000));
  await bootstrap(page);
  const phrase = "EDGE-U stop the lead before it reads its helper's report";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => (await team(page, leadId))?.lead.unread ?? -1, {
      timeout: afterStart(120_000),
    })
    .toBeGreaterThanOrEqual(1);
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  // The report woke the lead; its turn is held. The person stops it before it reads, and before reporter-u's second
  // report (90 s after its first): a slow stack that misses that shows here as setup, not as the strip's fault.
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toBeVisible({ timeout: afterStart(60_000) });
  expect((await team(page, leadId))?.lead.unread).toBe(1);
  await stop.click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/, {
    timeout: afterStart(60_000),
  });
  expect((await team(page, leadId))?.state).toBe("running");
  // The line sits in the strip's live region, beside "Helpers are still working." while reporter-u works.
  const region = page.getByRole("status").filter({ hasText: "unread" });
  await expect(
    region.getByText("1 team message waits unread for the lead.", {
      exact: true,
    }),
  ).toBeVisible({ timeout: afterStart(30_000) });
  // reporter-u's second report, 90 s after its first: the count follows without a reload, and the stopped lead
  // isn't woken for it (its place hasn't moved).
  await expect(
    region.getByText("2 team messages wait unread for the lead.", {
      exact: true,
    }),
  ).toBeVisible({ timeout: afterStart(120_000) });
  // reporter-u ends with one more model turn after its report.
  await expect
    .poll(async () => helperStates(page, leadId), {
      timeout: afterStart(60_000),
    })
    .toEqual({ "reporter-u": "done" });
  await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/);
});

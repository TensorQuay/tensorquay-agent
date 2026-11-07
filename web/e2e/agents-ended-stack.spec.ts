import { expect, test } from "@playwright/test";

import {
  edgeLead,
  helperStates,
  leadRow,
  stopIfRunning,
  stopStarted,
  team,
} from "./agents-stack-api";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-062: an ended team reads as ended (the c16 UX review, #1-#3; WEB-U-310, GW-U-169), on the agents stack
// suite with EDGE-A's script in deploy/tests/fixtures/agents.json, whose helper "ghost" has no script and fails. It
// runs between agents-edges-stack.spec.ts and agents-stack.spec.ts (name order), so E2E-AGENTS-020 stays last.
test.skip(
  process.env.TQA_STACK_SUITE !== "agents",
  "Requires the agents stack suite",
);

test.afterEach(async ({ page, baseURL }) => {
  await stopStarted(page, baseURL);
});

const NOTICE = "The lead task sees what you send here.";

test("WEB-E2E-062 an ended team reads as ended: the strip says how, the count is quiet, the helper's notice goes", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 300_000));
  await bootstrap(page);
  const phrase = "EDGE-A brief a helper that has no script";
  const leadId = await edgeLead(page, baseURL, phrase);
  await expect
    .poll(async () => (await helperStates(page, leadId)).ghost, {
      timeout: afterStart(120_000),
    })
    .toBe("failed");
  const ghost = (await team(page, leadId))?.helpers.find(
    (helper) => helper.name === "ghost",
  );
  if (!ghost) throw new Error("The team lists no ghost");

  // While the team runs: the lead's strip has its meter, the list's count is the failure in danger, and the helper's
  // page says the lead sees what its person sends.
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  const strip = page.getByRole("region", { name: "Helpers" });
  await expect(strip.getByRole("meter", { name: "Budget used" })).toBeVisible({
    timeout: afterStart(30_000),
  });
  const count = leadRow(page, leadId).locator(".task-helpers-count");
  await expect(count).toHaveAttribute("data-tone", "danger", {
    timeout: afterStart(45_000),
  });
  await expect(count).toHaveAttribute("title", "1 helper, 1 failed");
  await page.goto(`/c/${encodeURIComponent(ghost.conversation)}`);
  await expect(page.getByText(NOTICE)).toBeVisible({
    timeout: afterStart(30_000),
  });

  // The person stops the team: stop_reason "stopped", which the strip says as Stopped.
  await stopIfRunning(page, baseURL, leadId);
  expect((await team(page, leadId))?.stop_reason).toBe("stopped");

  // The helper is an ordinary task again: no notice (the gateway copies nothing to an ended team's lead).
  await page.reload();
  await expect(page.getByRole("textbox").first()).toBeVisible({
    timeout: afterStart(30_000),
  });
  await expect(page.getByText(NOTICE)).toHaveCount(0);

  // The lead's strip says how it ended and what share it used, with no meter and no time left.
  await page.goto(`/c/${encodeURIComponent(leadId)}`);
  await expect(strip.locator(".helpers-budget")).toHaveText(
    /^Stopped · \d+ % of the budget used$/,
    { timeout: afterStart(30_000) },
  );
  await expect(strip.locator(".helpers-budget")).toHaveAttribute(
    "title",
    /^[\d.]+ % of the team's budget used$/,
  );
  await expect(strip.getByRole("meter")).toHaveCount(0);
  await expect(strip).not.toContainText("Ends at");

  // The list's row is quiet: the count in muted, its tooltip "1 helper", the row's name with no failure in it.
  await expect(count).toHaveAttribute("data-tone", "muted", {
    timeout: afterStart(45_000),
  });
  await expect(count).toHaveAttribute("title", "1 helper");
  await expect(leadRow(page, leadId)).toHaveAccessibleName(/1 helper$/);
});

// WEB-E2E-091 (c17 U9 on the real stack; WEB-U-414): a run the task's record says ended never reads Working while the
// page's stream replays it (16u live L2: "Working… · 4 min 46 s" for a moment on an ended lead's fresh page). It reads
// WEB-E2E-062's ended team, so it runs after it (this file's order): the lead, stopped, and its helper "ghost",
// failed. A small stack replays too fast to catch that moment, so the test holds it: it reads each task's real
// replay from the gateway's stream with the page's own session, and serves the page that replay cut just after the
// last run's start (no reconnect for 10 min), the moment 16u's page was in. Every text the header holds is recorded
// from before the first paint; none may say Working, while the task list's record says the run ended.
test("WEB-E2E-091 an ended team's pages opened fresh never read Working while they replay", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 180_000));
  await bootstrap(page);
  const phrase = "EDGE-A brief a helper that has no script";
  const listed = (await (
    await page.request.get("/api/v1/conversations?scope=top")
  ).json()) as { items: { id: string; title?: string | null }[] };
  const leadId = listed.items.find((item) => item.title === phrase)?.id;
  if (!leadId) throw new Error("WEB-E2E-062's lead isn't listed: run it first");
  const ended = await team(page, leadId);
  expect(ended?.state, "WEB-E2E-062 left its team ended").toBe("ended");
  const ghost = ended?.helpers.find((helper) => helper.name === "ghost");
  if (!ghost) throw new Error("The team lists no ghost");
  const cookie = (await page.context().cookies(baseURL))
    .map(({ name, value }) => `${name}=${value}`)
    .join("; ");
  /** The task's replay as the gateway streams it, read for a few seconds, cut just after its last run's start. */
  const cutReplay = async (task: string) => {
    const stop = new AbortController();
    const timer = setTimeout(() => stop.abort(), 4_000);
    let text = "";
    try {
      const response = await fetch(
        `${baseURL}/api/v1/conversations/${task}/events?stream=1&protocol=1`,
        {
          headers: { cookie, accept: "text/event-stream" },
          signal: stop.signal,
        },
      );
      expect(response.ok, "the stream opens").toBe(true);
      const reader = response.body!.getReader();
      const decoder = new TextDecoder();
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        text += decoder.decode(value, { stream: true });
      }
    } catch (error) {
      if (!stop.signal.aborted) throw error;
    } finally {
      clearTimeout(timer);
    }
    const frames = text.split("\n\n").filter((block) => /^data:/m.test(block));
    const start = frames.findLastIndex((block) =>
      /"type"\s*:\s*"run\.started"/.test(block),
    );
    expect(start, `${task}'s replay has a run start`).toBeGreaterThan(-1);
    expect(
      frames.length,
      `${task}'s replay goes on after its last run's start`,
    ).toBeGreaterThan(start + 1);
    return `retry: 600000\n\n${frames.slice(0, start + 1).join("\n\n")}\n\n`;
  };
  await page.addInitScript(() => {
    const seen: string[] = [];
    (window as unknown as { headerTexts: string[] }).headerTexts = seen;
    const note = (words: string | null | undefined) => {
      const trimmed = (words ?? "").trim();
      if (trimmed && seen.at(-1) !== trimmed) seen.push(trimmed);
    };
    new MutationObserver(() =>
      note(document.querySelector('[aria-label="Run status"]')?.textContent),
    ).observe(document, {
      subtree: true,
      childList: true,
      characterData: true,
    });
  });
  for (const [name, task, end] of [
    ["the lead", leadId, /^(Stopped|Done)/],
    ["ghost", ghost.conversation, /^Failed/],
  ] as const) {
    const body = await cutReplay(task);
    const stream = `**/api/v1/conversations/${task}/events?*stream=1*`;
    await page.route(stream, (route) =>
      route.fulfill({
        status: 200,
        headers: { "content-type": "text/event-stream" },
        body,
      }),
    );
    for (const visit of ["opened", "reloaded"]) {
      if (visit === "opened") await page.goto(`/c/${encodeURIComponent(task)}`);
      else await page.reload();
      const status = page.getByLabel("Run status");
      await expect(status, `${name} ${visit}`).toHaveText(/\S/, {
        timeout: afterStart(30_000),
      });
      // The page has read its record and the cut replay by now; the header must have held no Working at any time.
      await page.waitForTimeout(3_000);
      const texts = await page.evaluate(
        () => (window as unknown as { headerTexts: string[] }).headerTexts,
      );
      expect(
        texts.filter((text) => /Working/.test(text)),
        `${name} ${visit}: ${texts.join(" | ")}`,
      ).toEqual([]);
      // And it settled on the run's end as the record says it, not on some other state.
      await expect(status, `${name} ${visit}`).toHaveText(end);
    }
    await page.unroute(stream);
  }
});

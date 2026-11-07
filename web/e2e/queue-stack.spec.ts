import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import {
  answered,
  clearTasks,
  holdSlot,
  observeQueue,
  queueCapture,
  queueEvidence,
  queuedPrompt,
  release,
  stop,
} from "./queue-stack";
import { afterStart } from "./stack-start";
import { snapshot } from "./ux/assertions";
import { bootstrap, signIn } from "./ux/flow";

test.skip(
  !process.env.TQA_STACK_URL,
  "Requires the owned one-slot admission stack",
);
// One slot for all four: a failed test's holder would queue the rest, so a failure skips them instead.
test.describe.configure({ mode: "serial" });
const results: { conversationId: string; outcome: string }[] = [];

/** Each task's run status as the gateway's task list reports it (the list carries run_status, GW-I-120). */
async function runStatuses(page: Page, ids: string[]) {
  const response = await page.request.get("/api/v1/conversations?limit=100");
  expect(response.status()).toBe(200);
  const { items } = (await response.json()) as {
    items: { id: string; run_status?: string }[];
  };
  return ids.map((id) => {
    const status = items.find((item) => item.id === id)?.run_status;
    if (!status) throw new Error(`No run status for task ${id}`);
    return { conversationId: id, outcome: status };
  });
}
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});
test.afterAll(async () => {
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({ runs: results }),
      { mode: 0o600 },
    );
});

for (const width of [1024, 1440])
  for (const colorScheme of ["light", "dark"] as const)
    // The holder's scripted model call keeps the one slot until the holder presses Stop (the queue fixture).
    test(`WEB-E2E-018 real queued replay, waking and sleeping ${width} ${colorScheme}`, async ({
      page,
      context,
    }) => {
      // Two turns, each with an environment start, a held call and a queued task: past the stack's default 60 s.
      test.setTimeout(140_000);
      const suffix = `${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 1000 });
      await page.emulateMedia({ colorScheme });
      const blocker = await context.newPage();
      await blocker.setViewportSize({ width, height: 1000 });
      await blocker.emulateMedia({ colorScheme });
      const stream = await observeQueue(page);
      const holding = await observeQueue(blocker);
      try {
        await signIn(page);
        await clearTasks(page);
        await blocker.goto("/");
        const first = await holdSlot(blocker);
        let second = "";
        for (const turn of [1, 2]) {
          if (turn === 2) await holdSlot(blocker);
          const before = stream.snapshots.length;
          second = await queuedPrompt(page, `${suffix}-${turn}`);
          await stop(blocker);
          await answered(page, turn);
          expect(
            stream.snapshots
              .slice(before)
              .some((frame) => frame.sandbox.state === "creating"),
          ).toBe(true);
          expect(await page.evaluate(() => window.__queueStatus)).toContain(
            "Starting…",
          );
          expect(
            stream.snapshots
              .slice(before)
              .filter((frame) => frame.sandbox.state === "queued")
              .every((frame) => frame.queue?.ahead === 0),
          ).toBe(true);
          await expect(
            page.getByRole("article", { name: "Your message" }),
          ).toHaveCount(turn);
          await queueCapture(page, `${suffix}-${turn}-asleep`);
          // The answer can show before the run ends; the snapshot waits for Done, as the reloaded page does. The task
          // was asleep, so its environment starts again first (e2e/stack-start.ts).
          await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
            timeout: afterStart(30_000),
          });
          const completed = await snapshot(page);
          await page.reload();
          await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
          await expect.poll(() => snapshot(page)).toBe(completed);
        }
        expect(
          await page.evaluate(() => ({
            width: document.documentElement.scrollWidth,
            height: document.documentElement.scrollHeight,
          })),
        ).toEqual(page.viewportSize());
        expect(stream.invalid).toEqual([]);
        expect(holding.invalid).toEqual([]);
        expect([...stream.errors, ...holding.errors]).toEqual([]);
        // The outcomes come from the gateway, not from the spec: the holder was stopped, the queued task completed.
        results.push(...(await runStatuses(page, [first, second])));
      } finally {
        try {
          await queueEvidence(page, suffix, stream);
          await queueEvidence(blocker, `${suffix}-holding`, holding);
        } finally {
          await release(blocker);
          await stream.session.detach();
          await holding.session.detach();
          await blocker.close();
        }
      }
    });

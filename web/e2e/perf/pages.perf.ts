import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import { join } from "node:path";

import {
  expect,
  test,
  type Browser,
  type BrowserContext,
  type Page,
} from "@playwright/test";

import { createConversation, signIn } from "../session";

// WEB-E-010 (Phase 9 performance budgets, approved 2026-09-28, about twice the review stack's p95): each main page is
// ready and paints within its budget from a cold load, and typing in a long transcript stays quick. On the fake front
// with generated data (the 100-turn transcript PERF-040 uses, a 30-post channel), 1 warm-up and 10 measured loads per
// page. The scroll budget is WEB-E-009's (transcript.perf.ts). TQA_PERF_CPU_THROTTLE slows the CPU, to show the
// budgets fail when rendering is slow.
const WARM_UP = 1;
const MEASURED = 10;
const LCP_P50_MS = 1500;
const TYPING = { p95Ms: 100, maxMs: 200 };
// The fake front's seeded channel has 9 posts; the run adds posts up to 30, as measured on the review stack.
const SEEDED_CHANNEL = "0195d8c0-0000-7000-8000-00000000c4a1";
const CHANNEL_POSTS = 30;

type Target = {
  name: string;
  path: string;
  signedIn: boolean;
  readyP95Ms: number;
  /** Evaluated in the page: true once its own content is there. */
  ready: string;
};

/** In the page, from navigation start: the largest paint, and when the page's ready condition first holds. */
function instrument(ready: string) {
  const state = window as unknown as {
    __perf: { lcp: number; ready: number | null };
  };
  state.__perf = { lcp: 0, ready: null };
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) state.__perf.lcp = entry.startTime;
  }).observe({ type: "largest-contentful-paint", buffered: true });
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const holds = new Function(`return (${ready})`) as () => boolean;
  const check = () => {
    if (state.__perf.ready === null && holds())
      state.__perf.ready = performance.now();
  };
  new MutationObserver(check).observe(document, {
    subtree: true,
    childList: true,
    characterData: true,
  });
}

const THROTTLE = Number(process.env.TQA_PERF_CPU_THROTTLE ?? 1);

/** A new page, its CPU slowed when TQA_PERF_CPU_THROTTLE asks. */
async function slowed(context: BrowserContext) {
  const page = await context.newPage();
  if (THROTTLE > 1)
    await (
      await context.newCDPSession(page)
    ).send("Emulation.setCPUThrottlingRate", { rate: THROTTLE });
  return page;
}

type Session = Awaited<ReturnType<ReturnType<Page["context"]>["storageState"]>>;

async function coldLoad(
  browser: Browser,
  target: Target,
  session: Session,
): Promise<{ readyMs: number; lcpMs: number }> {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    reducedMotion: "reduce",
    ...(target.signedIn ? { storageState: session } : {}),
  });
  const page = await slowed(context);
  await page.addInitScript(instrument, target.ready);
  await page.goto(target.path);
  await page.waitForFunction(
    () =>
      (window as unknown as { __perf: { ready: number | null } }).__perf
        .ready !== null,
    null,
    { timeout: 60_000 },
  );
  await page.waitForTimeout(200);
  const perf = await page.evaluate(
    () =>
      (window as unknown as { __perf: { lcp: number; ready: number } }).__perf,
  );
  await context.close();
  return { readyMs: perf.ready, lcpMs: perf.lcp };
}

/** Typing into the long transcript's message box: each key event's duration (input delay, handlers, next paint). */
async function typing(browser: Browser, url: string, session: Session) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    storageState: session,
  });
  const page = await slowed(context);
  await page.goto(url);
  await expect(page.getByRole("heading", { name: "Report 100" })).toBeAttached({
    timeout: 60_000,
  });
  await page.evaluate(() => {
    const state = window as unknown as { __keys: number[] };
    state.__keys = [];
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) state.__keys.push(entry.duration);
    }).observe({
      type: "event",
      durationThreshold: 16,
      buffered: true,
    } as PerformanceObserverInit);
  });
  const box = page.getByLabel("Message", { exact: true });
  await box.click();
  await page.keyboard.type(
    "Please summarise the ledger totals again, part by part, in one table.",
    { delay: 40 },
  );
  await page.waitForTimeout(500);
  const durations = await page.evaluate(
    () => (window as unknown as { __keys: number[] }).__keys,
  );
  await context.close();
  return durations;
}

const percentile = (values: number[], share: number) => {
  const sorted = [...values].sort((a, b) => a - b);
  return Math.round(
    sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))] ?? 0,
  );
};

test("WEB-E-010 the main pages load and respond within their budgets", async ({
  page,
  browser,
}) => {
  const load = os.loadavg();
  await signIn(page);
  const url = await createConversation(page);
  await page.goto(url);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Load the long transcript");
  await input.press("Enter");
  await expect(page.getByRole("heading", { name: "Report 100" })).toBeAttached({
    timeout: 60_000,
  });
  const seeded = await page.request.get(
    `/api/v1/channels/${SEEDED_CHANNEL}/messages?limit=100`,
  );
  const { messages } = (await seeded.json()) as { messages: unknown[] };
  for (let post = messages.length; post < CHANNEL_POSTS; post++) {
    const posted = await page.request.post(
      `/api/v1/channels/${SEEDED_CHANNEL}/messages`,
      {
        headers: { "Idempotency-Key": randomUUID() },
        data: { body: { type: "text", text: `Generated post ${post + 1}.` } },
      },
    );
    expect(posted.status()).toBe(201);
  }
  const session = await page.context().storageState();
  const table = `!!document.querySelector('table.data-table') && !document.querySelector('.skeleton')`;
  const targets: Target[] = [
    {
      name: "sign-in",
      path: "/login",
      signedIn: false,
      readyP95Ms: 500,
      ready: `!!document.querySelector('button[type=submit]')`,
    },
    {
      name: "task list",
      path: "/",
      signedIn: true,
      readyP95Ms: 1000,
      ready: `!!document.querySelector('textarea') && !document.querySelector('.skeleton')`,
    },
    {
      name: "a 100-turn conversation",
      path: url,
      signedIn: true,
      readyP95Ms: 2000,
      ready: `[...document.querySelectorAll('[data-testid="transcript"] h2')].some((h) => h.textContent === 'Report 100') && !document.querySelector('[data-testid="transcript"] .skeleton')`,
    },
    {
      name: "channels list",
      path: "/channels",
      signedIn: true,
      readyP95Ms: 1000,
      ready: `!!document.querySelector('h1') && !document.querySelector('.skeleton')`,
    },
    {
      name: "a channel",
      path: `/channels/${SEEDED_CHANNEL}`,
      signedIn: true,
      readyP95Ms: 1000,
      ready: `document.querySelectorAll('.channel-log .channel-post').length >= ${CHANNEL_POSTS}`,
    },
    {
      name: "settings: models",
      path: "/settings/models",
      signedIn: true,
      readyP95Ms: 1000,
      ready: table,
    },
    {
      name: "settings: secrets",
      path: "/settings/secrets",
      signedIn: true,
      readyP95Ms: 1000,
      ready: table,
    },
  ];
  const results: Record<
    string,
    { readyP95Ms: number; lcpP50Ms: number; budgetMs: number }
  > = {};
  for (const target of targets) {
    for (let run = 0; run < WARM_UP; run++)
      await coldLoad(browser, target, session);
    const samples = [];
    for (let run = 0; run < MEASURED; run++)
      samples.push(await coldLoad(browser, target, session));
    results[target.name] = {
      readyP95Ms: percentile(
        samples.map((sample) => sample.readyMs),
        0.95,
      ),
      lcpP50Ms: percentile(
        samples.map((sample) => sample.lcpMs),
        0.5,
      ),
      budgetMs: target.readyP95Ms,
    };
  }
  const keys = await typing(browser, url, session);
  const summary = {
    pages: results,
    typing: {
      events: keys.length,
      p95Ms: percentile(keys, 0.95),
      maxMs: Math.round(Math.max(0, ...keys)),
    },
    noisy:
      Math.max(load[0] ?? 0, os.loadavg()[0] ?? 0) > os.cpus().length * 0.75,
  };
  const day = new Date().toISOString().slice(0, 10);
  const directory = join("..", "tests", "perf", "results", day);
  await mkdir(directory, { recursive: true });
  await writeFile(
    join(directory, `web-e-010-${Date.now()}.json`),
    JSON.stringify(
      {
        id: "WEB-E-010",
        environment: {
          cpus: os.cpus().length,
          chromium: browser.version(),
          commit: execFileSync("git", ["rev-parse", "--short", "HEAD"], {
            encoding: "utf8",
          }).trim(),
          load,
          throttle: THROTTLE,
        },
        summary,
      },
      null,
      1,
    ),
  );
  console.log(`WEB-E-010: ${JSON.stringify(summary)}`);
  // Soft, so one run names every measure over its budget.
  for (const [name, result] of Object.entries(results)) {
    expect
      .soft(result.readyP95Ms, `${name}: ready p95`)
      .toBeLessThanOrEqual(result.budgetMs);
    expect
      .soft(result.lcpP50Ms, `${name}: LCP p50`)
      .toBeLessThanOrEqual(LCP_P50_MS);
  }
  expect
    .soft(summary.typing.p95Ms, "typing: key event p95")
    .toBeLessThanOrEqual(TYPING.p95Ms);
  expect
    .soft(summary.typing.maxMs, "typing: slowest key event")
    .toBeLessThanOrEqual(TYPING.maxMs);
});

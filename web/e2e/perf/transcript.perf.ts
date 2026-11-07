import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import { join } from "node:path";

import { expect, test, type Browser, type Page } from "@playwright/test";

import { createConversation, signIn } from "../session";

// PERF-040 (Gate 5) and WEB-E-009 (Gate 4): a 10,000-event transcript is interactive within 2 s, and scrolling it stays
// responsive (p95 frame <= 50 ms, no long task > 200 ms). TEST-PLAN §15.1: 5 warm-up and 50 measured cold loads,
// p50 and p99, the environment recorded, raw samples as JSON under tests/perf/results/<date>/ (git-ignored).
const WARM_UP = 5;
const MEASURED = 50;
const TARGET = { ttiMs: 2000, frameP95Ms: 50, longTaskMs: 200 };
const LAST_ANSWER = "Report 100";

type Sample = {
  ttiMs: number;
  renderedMs: number;
  frameP95Ms: number;
  longestTaskMs: number;
};

/** Records in the page when the long transcript is rendered, and the longest task, from navigation start. */
function instrument() {
  const state = window as unknown as {
    __perf: { rendered?: number; longest: number };
  };
  state.__perf = { longest: 0 };
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries())
      state.__perf.longest = Math.max(state.__perf.longest, entry.duration);
  }).observe({ type: "longtask", buffered: true });
  const check = () => {
    if (state.__perf.rendered !== undefined) return;
    const transcript = document.querySelector('[data-testid="transcript"]');
    const done = [...(transcript?.querySelectorAll("h2") ?? [])].some(
      (heading) => heading.textContent === "Report 100",
    );
    if (done && !transcript?.querySelector(".skeleton"))
      state.__perf.rendered = performance.now();
  };
  new MutationObserver(check).observe(document, {
    subtree: true,
    childList: true,
    characterData: true,
  });
}

type Session = Awaited<ReturnType<ReturnType<Page["context"]>["storageState"]>>;

async function measure(
  browser: Browser,
  url: string,
  session: Session,
): Promise<Sample> {
  // A cold page in the setup's session: the fake front keeps conversations per session.
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    reducedMotion: "reduce",
    storageState: session,
  });
  const page = await context.newPage();
  // Optional CPU slowdown, as a check that the harness fails when rendering is slow, or to model a slower machine.
  const throttle = Number(process.env.TQA_PERF_CPU_THROTTLE ?? 1);
  if (throttle > 1)
    await (
      await context.newCDPSession(page)
    ).send("Emulation.setCPUThrottlingRate", { rate: throttle });
  await page.addInitScript(instrument);
  await page.goto(url);
  await page.waitForFunction(
    () =>
      (window as unknown as { __perf: { rendered?: number } }).__perf
        .rendered !== undefined,
    null,
    { timeout: 30_000 },
  );
  // Interactive: the composer answers a keystroke with a painted frame.
  const composer = page.getByLabel("Message", { exact: true });
  const probe = page.evaluate(
    () =>
      new Promise<number>((resolve) => {
        document.addEventListener(
          "input",
          () => requestAnimationFrame(() => resolve(performance.now())),
          { once: true, capture: true },
        );
      }),
  );
  await composer.press("x");
  const ttiMs = await probe;
  const scroll = await scrollFrames(page);
  const perf = await page.evaluate(
    () =>
      (window as unknown as { __perf: { rendered: number; longest: number } })
        .__perf,
  );
  await context.close();
  return {
    ttiMs,
    renderedMs: perf.rendered,
    ...scroll,
    longestTaskMs: Math.max(perf.longest, scroll.longestTaskMs),
  };
}

/** Scrolls the transcript from the bottom to the top in half-screen steps, timing each painted frame. */
async function scrollFrames(page: Page) {
  return page.evaluate(async () => {
    const region = document.querySelector<HTMLElement>(".transcript-scroll");
    if (!region) throw new Error("The transcript region is missing");
    let longest = 0;
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries())
        longest = Math.max(longest, entry.duration);
    });
    observer.observe({ type: "longtask" });
    const frames: number[] = [];
    let last = await new Promise<number>(requestAnimationFrame);
    while (region.scrollTop > 0) {
      region.scrollBy(0, -region.clientHeight / 2);
      const now = await new Promise<number>(requestAnimationFrame);
      frames.push(now - last);
      last = now;
    }
    observer.disconnect();
    frames.sort((a, b) => a - b);
    return {
      frameP95Ms: frames[Math.floor(frames.length * 0.95)] ?? 0,
      longestTaskMs: longest,
    };
  });
}

const percentile = (values: number[], share: number) => {
  const sorted = [...values].sort((a, b) => a - b);
  return (
    sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * share))] ?? 0
  );
};

function environment(browser: Browser) {
  const cpus = os.cpus();
  return {
    cpu: cpus[0]?.model ?? "unknown",
    cpus: cpus.length,
    memoryBytes: os.totalmem(),
    kernel: `${os.type()} ${os.release()}`,
    chromium: browser.version(),
    commit: execFileSync("git", ["rev-parse", "--short", "HEAD"], {
      encoding: "utf8",
    }).trim(),
    load: os.loadavg(),
  };
}

test("PERF-040 / WEB-E-009 a 10,000-event transcript renders and scrolls within budget", async ({
  page,
  browser,
}) => {
  const before = environment(browser);
  await signIn(page);
  const url = await createConversation(page);
  const input = page.getByLabel("Message", { exact: true });
  await page.goto(url);
  await input.fill("Load the long transcript");
  await input.press("Enter");
  await expect(page.getByRole("heading", { name: LAST_ANSWER })).toBeAttached({
    timeout: 60_000,
  });
  const session = await page.context().storageState();
  for (let run = 0; run < WARM_UP; run++) await measure(browser, url, session);
  const samples: Sample[] = [];
  for (let run = 0; run < MEASURED; run++)
    samples.push(await measure(browser, url, session));
  const after = os.loadavg();
  const tti = samples.map((sample) => sample.ttiMs);
  const summary = {
    ttiP50Ms: percentile(tti, 0.5),
    ttiP99Ms: percentile(tti, 0.99),
    frameP95Ms: Math.max(...samples.map((sample) => sample.frameP95Ms)),
    longestTaskMs: Math.max(...samples.map((sample) => sample.longestTaskMs)),
    noisy: Math.max(before.load[0] ?? 0, after[0] ?? 0) > before.cpus * 0.75,
  };
  const day = new Date().toISOString().slice(0, 10);
  const directory = join("..", "tests", "perf", "results", day);
  await mkdir(directory, { recursive: true });
  const file = join(directory, `perf-040-${Date.now()}.json`);
  await writeFile(
    file,
    JSON.stringify(
      {
        id: "PERF-040",
        target: TARGET,
        environment: { ...before, loadAfter: after },
        summary,
        samples,
      },
      null,
      2,
    ),
  );
  console.log(`PERF-040 ${JSON.stringify(summary)} -> ${file}`);
  expect(summary.ttiP50Ms, "time to interactive, p50").toBeLessThanOrEqual(
    TARGET.ttiMs,
  );
  expect(
    summary.frameP95Ms,
    "scroll frame p95 (worst run)",
  ).toBeLessThanOrEqual(TARGET.frameP95Ms);
  expect(summary.longestTaskMs, "longest task").toBeLessThanOrEqual(
    TARGET.longTaskMs,
  );
});

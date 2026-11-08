import { randomUUID } from "node:crypto";
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type BrowserContext, type Page } from "@playwright/test";

import { openCard } from "./cards";
import { chooseEnvironment } from "./environment";
import { afterStart, budget, scaled } from "./stack-start";
import { bootstrap } from "./ux/flow";

// The Desktop environment end to end on the real stack (ADR-0039; WEB-E2E-071..077), on the desktop suite's stack:
// the Desktop built-in is the image under test, and the scripted model (deploy/tests/fixtures/desktop.json) answers
// each case's DESK- phrase. Its desktop actions are keys, typing, waits and `observe` only (a click needs a ref the
// script can't know). The fake front's screen.spec.ts (WEB-E2E-037) covers the panel's every state; this suite covers
// what only a real desktop, gateway and runtime give: a live screen, Take over's stop and control, stored pictures, a
// restart, the browser's sign-out, the per-run caps and the desktop slots.
/** WEB-E2E-077's idle limit: short, so the first Desktop idles out within the test. */
const IDLE_SECONDS = 20;

test.skip(
  process.env.TQA_STACK_SUITE !== "desktop",
  "Requires the desktop stack suite",
);
// One sign-in for the whole file, its cookies reused by each test's own browser context, as the c15 stack specs do:
// each test gets a fresh context, so without them every case met the sign-in page (WEB-E2E-071 timed out on it, 12 Oct
// 2026), and a sign-in per test would pass the gateway's 10 a minute from an address (auth/rate.rs).
let session: Awaited<ReturnType<BrowserContext["storageState"]>>;
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const context = await browser.newContext({ baseURL });
  try {
    await bootstrap(await context.newPage());
    session = await context.storageState();
  } finally {
    await context.close();
  }
});
test.beforeEach(async ({ page }) => {
  expect(session, "beforeAll's sign-in, reused by every test").toBeDefined();
  await page.context().addCookies(session.cookies);
});

const writing = (baseURL: string) => ({
  "Idempotency-Key": randomUUID(),
  Origin: baseURL,
});

// Each test's tasks go with it: deleting a task stops its environment, so its Desktop no longer counts against
// `desktop.max_sessions` (2 by default) and the next test starts one at once, whatever ran before it. Every task of
// the suite's one account is this suite's (the stack is its own).
test.afterEach(async ({ page, baseURL }) => {
  if (!baseURL) return;
  const listed = await page.request.get("/api/v1/conversations?limit=100");
  expect(listed.ok()).toBe(true);
  const { items } = (await listed.json()) as { items: { id: string }[] };
  for (const { id } of items) {
    const deleted = await page.request.delete(`/api/v1/conversations/${id}`, {
      headers: writing(baseURL),
    });
    expect(deleted.status(), `delete task ${id}`).toBe(204);
  }
});

/** A new task on `template` whose first message holds `phrase`, sent by the API; returns its id. */
async function task(
  page: Page,
  baseURL: string,
  phrase: string,
  template = "desktop",
) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: writing(baseURL),
    data: { title: phrase, model_route: "local", template },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  const sent = await page.request.post(`/api/v1/conversations/${id}/messages`, {
    headers: writing(baseURL),
    data: { text: `${phrase}.`, attachments: [] },
  });
  expect(sent.ok()).toBe(true);
  return id;
}

/** Sets an administrator's limit and returns how to put it back (a reset names the version it read). */
async function limit(page: Page, baseURL: string, key: string, value: number) {
  const listed = await page.request.get("/api/v1/admin/limits");
  expect(listed.ok()).toBe(true);
  const found = (
    (await listed.json()) as { items: { key: string; version: number }[] }
  ).items.find((item) => item.key === key);
  if (!found) throw new Error(`The gateway lists no ${key}`);
  const set = await page.request.put(`/api/v1/admin/limits/${key}`, {
    headers: writing(baseURL),
    data: { value, version: found.version },
  });
  expect(set.ok(), `set ${key} to ${value}`).toBe(true);
  const { version } = (await set.json()) as { version: number };
  return async () => {
    const reset = await page.request.delete(
      `/api/v1/admin/limits/${key}?version=${version}`,
      { headers: writing(baseURL) },
    );
    expect(reset.ok(), `reset ${key}`).toBe(true);
  };
}

/** Asks the driver to restart the gateway (demo_agents.Restarter, started for this suite) and waits until it has. */
async function restartGateway(reason: string) {
  const markers = process.env.TQA_DESKTOP_MARKERS ?? "";
  if (!markers) throw new Error("The driver must supply TQA_DESKTOP_MARKERS");
  await rm(join(markers, "restarted"), { force: true });
  await writeFile(join(markers, "restart-wanted"), reason);
  const bound = scaled(240_000);
  const started = Date.now();
  for (;;) {
    const failed = await readFile(
      join(markers, "restart-failed"),
      "utf8",
    ).catch(() => null);
    if (failed !== null)
      throw new Error(`The driver's restart failed: ${failed}`);
    if (await readFile(join(markers, "restarted"), "utf8").catch(() => null))
      return;
    if (Date.now() - started > bound)
      throw new Error(`No restart after ${bound / 1000} s`);
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

/**
 * Asks the driver to check this task's pictures on disk now, while the task exists (demo_desktop.PictureWatch; the
 * afterEach deletes every task, and its pictures with it), and waits for its verdict: a copy takes a second or two.
 */
async function picturesChecked(id: string) {
  const markers = process.env.TQA_DESKTOP_MARKERS ?? "";
  if (!markers) throw new Error("The driver must supply TQA_DESKTOP_MARKERS");
  await rm(join(markers, "pictures-checked"), { force: true });
  await rm(join(markers, "pictures-failed"), { force: true });
  // Whole or not at all: the driver reads it as soon as it exists.
  await writeFile(join(markers, "pictures-wanted.partial"), id);
  await rename(
    join(markers, "pictures-wanted.partial"),
    join(markers, "pictures-wanted"),
  );
  const bound = scaled(30_000);
  const started = Date.now();
  for (;;) {
    const failed = await readFile(
      join(markers, "pictures-failed"),
      "utf8",
    ).catch(() => null);
    if (failed !== null)
      throw new Error(
        `The driver's check of task ${id}'s pictures failed: ${failed}`,
      );
    if (
      await readFile(join(markers, "pictures-checked"), "utf8").catch(
        () => null,
      )
    )
      return;
    if (Date.now() - started > bound)
      throw new Error(
        `The driver didn't check task ${id}'s pictures within ${bound / 1000} s (demo_desktop.PictureWatch, started by demo.py --suite desktop)`,
      );
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

const panel = (page: Page) => page.getByRole("region", { name: "Screen" });
const status = (page: Page) => panel(page).getByRole("status").first();
const CONTROL = /^You have control\./;

/**
 * Opens the task's Screen panel from its toggle, unless it is open, and waits for the screen to say who holds it:
 * watching, or, after a hand-back, that the desktop was handed back (WEB-U-242: the page says so after a reload too).
 */
async function openScreen(page: Page) {
  if (!(await panel(page).isVisible()))
    await page.getByRole("button", { name: /^Screen/ }).click();
  await expect(status(page)).toHaveText(
    /^(Watching\.|You handed back the desktop\.)/,
    { timeout: afterStart(120_000) },
  );
}

/**
 * The Live picture's mean brightness (0 black to 255 white) over the whole frame, read from its canvas as the page
 * draws it, with the frame's size: an empty canvas reads as size 0.
 */
async function liveBrightness(page: Page) {
  return panel(page)
    .locator("canvas")
    .first()
    .evaluate((canvas: HTMLCanvasElement) => {
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context || canvas.width === 0 || canvas.height === 0)
        return { width: canvas.width, height: canvas.height, mean: 0 };
      const pixels = context.getImageData(
        0,
        0,
        canvas.width,
        canvas.height,
      ).data;
      let sum = 0;
      for (let i = 0; i < pixels.length; i += 4)
        sum += (pixels[i] ?? 0) + (pixels[i + 1] ?? 0) + (pixels[i + 2] ?? 0);
      return {
        width: canvas.width,
        height: canvas.height,
        mean: Math.round(sum / (pixels.length / 4) / 3),
      };
    });
}

/**
 * Brighter than this isn't the black frame: the one real-world use met before 17.0 read about 0, and the desktop's
 * white or a page reads far above. Low on purpose: a browser with a dark toolbar, or a busy page, can read well under
 * 200 without being black.
 */
const NOT_BLACK = 60;

/**
 * Takes over and waits for control, on time or late (a late grant says to wait, then changes). `atGrant` runs the
 * moment control is granted, before anything else is awaited.
 */
async function takeOver(page: Page, atGrant?: () => Promise<void>) {
  await panel(page).getByRole("button", { name: "Take over" }).click();
  await expect(status(page)).toHaveText(CONTROL, {
    timeout: afterStart(60_000),
  });
  if (atGrant) await atGrant();
  await expect(status(page)).toHaveText(
    "You have control. When you're done, hand back, then send a message to let the agent carry on.",
    { timeout: scaled(30_000) },
  );
}

const HANDED_BACK =
  "You handed back the desktop. Send a message to let the agent carry on.";

// U1, U2, U8, U9 and the toggle (#14; WEB-U-241/243/246/247/282): Desktop chosen on New task, the agent's desktop
// actions with the panel opening by itself, the toggle saying the agent is on the desktop, each action's line in words,
// its pictures stored and shown, and Replay.
test("WEB-E2E-071 a Desktop task: the live screen, its step lines and pictures, and Replay", async ({
  page,
}) => {
  test.setTimeout(budget(2, 360_000));
  await page.goto("/");
  await chooseEnvironment(page, "Desktop");
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("DESK-A look at a page.");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/([0-9a-f-]{36})$/);
  const id = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  // The 8-second wait keeps a desktop action running: the toggle says so, and the panel has opened by itself.
  await expect(
    page.getByRole("button", {
      name: "Screen: the agent is using the desktop",
    }),
  ).toBeVisible({ timeout: afterStart(180_000) });
  await expect(panel(page)).toBeVisible();
  await expect(status(page)).toHaveText("Watching. The agent is working.");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(120_000),
  });
  await expect(status(page)).toHaveText(/^Watching\.$/);
  await expect(
    page.getByRole("button", { name: "Screen", exact: true }),
  ).toBeVisible();
  // The step's lines in words: never the tool's name, the text typed, or coordinates.
  const step = page.getByRole("group", { name: "Activity" }).last();
  const toggle = step.getByRole("button", { name: /Worked for/ });
  if ((await toggle.getAttribute("aria-expanded")) !== "true")
    await toggle.click();
  for (const line of [
    "Pressed ctrl+l",
    "Typed 27 characters",
    "Pressed Return",
    /^Waited 8/,
    "Looked at the screen",
  ])
    await expect(step.getByText(line).first()).toBeVisible();
  await expect(step).not.toContainText("file:///workspace/page.html");
  // A key's card: its pictures first, before and after.
  await openCard(step.getByRole("button", { name: /Pressed Return/ }));
  await expect(step.getByText("Before", { exact: true }).first()).toBeVisible();
  await expect(step.getByText("After", { exact: true }).first()).toBeVisible();
  // Replay: the run's desktop steps, one picture at a time.
  await panel(page).getByRole("button", { name: "Replay" }).click();
  await expect(panel(page)).toContainText(/Step \d+ of \d+ · /);
  await expect(
    panel(page).getByRole("button", { name: "Previous step" }),
  ).toBeVisible();
  await expect(
    panel(page).getByRole("button", { name: "Next step" }),
  ).toBeVisible();
  // The driver checks the pictures on disk now, before the afterEach deletes the task, and its verdict names the task
  // looked at (demo_desktop.verify_result).
  const result = process.env.TQA_STACK_RESULT;
  if (!result) throw new Error("The driver must supply TQA_STACK_RESULT");
  await writeFile(result, JSON.stringify({ lookedAt: id }));
  await picturesChecked(id);
});

// U3, U4, U11, #19, #22, #27 (WEB-U-242/270/306): Take over stops the held run, control's lines, the expanded screen
// with its controls and the desktop's Escape, Hand back and its line after a reload, then a message held during a second
// Take over that goes at the hand-back.
test("WEB-E2E-072 Take over and Hand back: control, Expand, the keyboard, a reload and a held message", async ({
  page,
  baseURL,
}, testInfo) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 360_000));
  const id = await task(page, baseURL, "DESK-B hold for a take over");
  await page.goto(`/c/${encodeURIComponent(id)}`);
  await expect(page.getByLabel("Run status")).toHaveText(/^Working/, {
    timeout: afterStart(120_000),
  });
  await openScreen(page);
  await expect(
    panel(page).getByText(
      "Taking over stops the agent. After you hand back, send a message to let it carry on.",
    ),
  ).toBeVisible();
  // SBX-C-024 (real-world use before 17.0): the desktop's browser restarts at the grant, and the Live picture is never
  // black meanwhile: at the grant, then 1 s and 2 s after it. One sample each, so a shorter black gap passes unseen.
  const seen: { at: number; width: number; height: number; mean: number }[] =
    [];
  await takeOver(page, async () => {
    const granted = Date.now();
    for (const at of [0, 1, 2]) {
      await page.waitForTimeout(Math.max(0, granted + at * 1_000 - Date.now()));
      seen.push({ at, ...(await liveBrightness(page)) });
    }
  });
  // The samples as evidence, kept with the run whether it passes or fails.
  await testInfo.attach("live-brightness-at-grant.json", {
    body: JSON.stringify(seen, null, 2),
    contentType: "application/json",
  });
  console.log(
    `WEB-E2E-072 Live brightness at the grant: ${JSON.stringify(seen)}`,
  );
  for (const sample of seen) {
    expect(
      sample.width > 0 && sample.height > 0,
      `the Live picture has a frame ${sample.at} s after the grant (${JSON.stringify(sample)})`,
    ).toBe(true);
    expect(
      sample.mean,
      `the Live picture isn't black ${sample.at} s after the grant (${JSON.stringify(seen)})`,
    ).toBeGreaterThan(NOT_BLACK);
  }
  await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/, {
    timeout: scaled(30_000),
  });
  await expect(
    panel(page).getByText(
      "Keys go to the desktop while it has focus; press Escape twice to give the keyboard back to the page. You can paste in; nothing is copied out.",
    ),
  ).toBeVisible();
  await expect(
    panel(page).getByText(
      "A sign-in here lasts for the agent's next run only. Each run after that starts signed out.",
    ),
  ).toBeVisible();
  // Expand carries the status and the controls; one Escape is the desktop's, twice gives the keyboard back, and then
  // one more closes it.
  await panel(page).getByRole("button", { name: "Expand" }).click();
  const dialog = page.getByRole("dialog", { name: "Screen" });
  await expect(dialog.getByRole("status").first()).toHaveText(CONTROL);
  await expect(dialog.getByRole("button", { name: "Hand back" })).toBeVisible();
  await dialog
    // By its name: an image while watching, an application in control (the c16 a11y pass), whichever this tree has.
    .getByLabel("The agent's desktop, live, you have control", { exact: true })
    .click();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(
    panel(page).getByRole("button", { name: "Expand" }),
  ).toBeFocused();
  // Hand back resumes nothing, and the page still says so after a reload.
  await panel(page).getByRole("button", { name: "Hand back" }).click();
  await expect(status(page)).toHaveText(HANDED_BACK);
  await expect(
    panel(page).getByRole("button", { name: "Take over" }),
  ).toBeFocused();
  await page.reload();
  await openScreen(page);
  await expect(status(page)).toHaveText(HANDED_BACK);
  // A message sent during a second Take over waits for the hand-back, then goes.
  await takeOver(page);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("I signed in; carry on.");
  await input.press("Enter");
  await expect(
    page.getByText(
      "Sends when you hand back the desktop: I signed in; carry on.",
    ),
  ).toBeVisible();
  await panel(page).getByRole("button", { name: "Hand back" }).click();
  await expect(page.getByText("Carried on after the hand-back.")).toBeVisible({
    timeout: afterStart(120_000),
  });
  await expect(
    page.getByText("You took over the desktop.").first(),
  ).toBeVisible();
  await expect(
    page.getByText(/^You handed back the desktop after /).first(),
  ).toBeVisible();
});

// U5 (WEB-U-279): a gateway restart during Take over hands the desktop back, and the transcript says why, with no time.
test("WEB-E2E-073 a gateway restart during Take over hands the desktop back, and says so", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 480_000));
  const id = await task(page, baseURL, "DESK-C hold for a restart");
  await page.goto(`/c/${encodeURIComponent(id)}`);
  await openScreen(page);
  await takeOver(page);
  await restartGateway("WEB-E2E-073: a restart during Take over");
  await expect(
    page.getByText("The desktop was handed back when the server restarted."),
  ).toBeVisible({ timeout: afterStart(120_000) });
  await expect(status(page)).toHaveText(/^Watching\.$/, {
    timeout: afterStart(120_000),
  });
});

// U6 (WEB-U-244/259; #6, #16): Clear the browser's sign-ins: unavailable while the agent works; after a Take over and
// Hand back, asked, then cleared by the desktop, said in the panel and the transcript.
test("WEB-E2E-074 Clear the browser's sign-ins after a Take over, not while the agent works", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 360_000));
  const id = await task(page, baseURL, "DESK-D hold before clearing");
  await page.goto(`/c/${encodeURIComponent(id)}`);
  await openScreen(page);
  await panel(page)
    .getByRole("button", { name: "More actions for the screen" })
    .click();
  const item = page.getByRole("menuitem", {
    name: /Clear the browser's sign-ins/,
  });
  await expect(item).toHaveAttribute("aria-disabled", "true");
  await expect(item).toHaveAccessibleDescription(
    /Available when the agent isn't working\./,
  );
  await page.keyboard.press("Escape");
  await takeOver(page);
  await panel(page).getByRole("button", { name: "Hand back" }).click();
  await expect(status(page)).toHaveText(HANDED_BACK);
  await panel(page)
    .getByRole("button", { name: "More actions for the screen" })
    .click();
  await page
    .getByRole("menuitem", { name: "Clear the browser's sign-ins" })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Clear the desktop browser's sign-ins?",
  });
  await dialog.getByRole("button", { name: "Clear sign-ins" }).click();
  await expect(
    panel(page).getByText("The desktop browser's sign-ins were cleared."),
  ).toBeVisible({ timeout: afterStart(60_000) });
  await expect(
    page
      .getByRole("region", { name: "Transcript" })
      .getByText("The desktop browser's sign-ins were cleared."),
  ).toBeVisible();
});

// U13 (WEB-U-241, GW-I-228): the per-run cap on desktop actions, at 1: the second action is refused and its line says
// so in words; the run carries on.
test("WEB-E2E-075 a run past its desktop actions cap is told so, in words", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 300_000));
  const reset = await limit(page, baseURL, "computer.max_actions_per_run", 1);
  try {
    const id = await task(page, baseURL, "DESK-E two desktop actions");
    await page.goto(`/c/${encodeURIComponent(id)}`);
    await expect(page.getByText("Stopped at the limit.")).toBeVisible({
      timeout: afterStart(180_000),
    });
    const step = page.getByRole("group", { name: "Activity" }).last();
    const toggle = step.getByRole("button", { name: /Worked for/ });
    if ((await toggle.getAttribute("aria-expanded")) !== "true")
      await toggle.click();
    await expect(
      step.getByText(
        "No more desktop actions this run: the action limit was reached",
      ),
    ).toBeVisible();
  } finally {
    await reset();
  }
});

// #13/#31 (WEB-U-001; ADR-0039 §10, amended 12 Oct 2026): with one Desktop at once, a second desktop task waits for
// it and says so; stopping the first's run doesn't free it (an idle Desktop is never retired for a waiter: its
// browser's sign-ins would go), and the second starts once the first idles out, here after a short idle limit.
test("WEB-E2E-077 a desktop task waits for a free desktop, says so, and starts when one idles out", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 480_000));
  // Set before the first task starts: a sandbox keeps the idle limit it started with.
  const idle = await limit(page, baseURL, "sandbox.idle_seconds", IDLE_SECONDS);
  const reset = await limit(page, baseURL, "desktop.max_sessions", 1);
  try {
    const first = await task(page, baseURL, "DESK-G hold a desktop first");
    await page.goto(`/c/${encodeURIComponent(first)}`);
    await expect(page.getByLabel("Run status")).toHaveText(/^Working/, {
      timeout: afterStart(180_000),
    });
    const second = await task(page, baseURL, "DESK-G hold a desktop second");
    await page.goto(`/c/${encodeURIComponent(second)}`);
    await expect(page.getByLabel("Run status")).toHaveText(
      "Waiting for a free desktop: other tasks are using them all. One frees soon after another task finishes.",
      { timeout: afterStart(60_000) },
    );
    await page.goto(`/c/${encodeURIComponent(first)}`);
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/, {
      timeout: scaled(60_000),
    });
    // Stopped, the first Desktop is idle but still up: the second still waits.
    await page.goto(`/c/${encodeURIComponent(second)}`);
    await expect(page.getByLabel("Run status")).toHaveText(
      /^Waiting for a free desktop/,
    );
    // Once the first has been idle for its limit its sandbox ends, and the second starts.
    await expect(page.getByLabel("Run status")).toHaveText(/^Working/, {
      timeout: afterStart(180_000 + IDLE_SECONDS * 1000),
    });
    await page.getByRole("button", { name: "Stop", exact: true }).click();
  } finally {
    await reset();
    await idle();
  }
});

// A first-hour check before 17.0: a Desktop task's transcript filled with
// "Blocked network access to accounts.google.com …" and the like, the desktop browser's own background calls, before
// the model answered anything. The browser now makes none (images/runtime: the managed policy and the switches file),
// so a fresh Desktop's first minute, its browser up and idle, adds no such notice on a stack whose allow-list is empty.
test("WEB-E2E-082 a fresh Desktop's first minute adds no blocked-network notice", async ({
  page,
}) => {
  test.setTimeout(budget(2, 300_000));
  await page.goto("/");
  await chooseEnvironment(page, "Desktop");
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("DESK-H a quiet first minute.");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(180_000),
  });
  // The desktop is up and its browser running.
  await openScreen(page);
  const started = Date.now();
  const blocked = page.getByText(/^Blocked network access/);
  while (Date.now() - started < scaled(60_000)) {
    await expect(blocked).toHaveCount(0);
    await page.waitForTimeout(5_000);
  }
  // And after a reload, from the stored history.
  await page.reload();
  await expect(page.getByText("Ready.", { exact: true })).toBeVisible();
  await expect(blocked).toHaveCount(0);
});

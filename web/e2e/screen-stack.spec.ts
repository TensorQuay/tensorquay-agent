import { readFileSync } from "node:fs";
import { writeFile } from "node:fs/promises";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { axe } from "./settings-page";
import { afterStart, budget } from "./stack-start";

// WEB-E2E-037 and SEC-059 on the real stack (ADR-0039 §5, §6 and §9), and slice 3's record on the page: the browser
// half of crates/tqa-sandbox/tests/egress/desktop_live.py, which owns the stack, a real Desktop sandbox, the scripted
// model's runs and every check of what reached the desktop. Each phase is one test the driver runs in turn.
test.skip(
  process.env.TQA_STACK_SUITE !== "screen",
  "Requires the owned desktop stack",
);

// Read, not imported (Playwright loads specs as ES modules), as e2e/screen.spec.ts does: the panel's words are the
// merged wording, so a reworded line can't leave this spec waiting on copy the page no longer shows.
const en = JSON.parse(readFileSync("src/i18n/locales/en.json", "utf8")) as {
  messages: Record<string, string>;
};
const words = (key: string) => {
  const found = en.messages[key];
  if (found === undefined) throw new Error(`en.json has no ${key}`);
  return found;
};

const PHASE = process.env.TQA_SCREEN_PHASE;
const TASK = process.env.TQA_SCREEN_TASK ?? "";

async function signIn(page: Page) {
  await page.goto("/login");
  await page
    .getByLabel("Email", { exact: true })
    .fill(process.env.TQA_SCREEN_EMAIL ?? "");
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.TQA_STACK_PASSWORD ?? "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
}

/** Opens every finished run's steps, which fold under "Worked for". */
async function openSteps(page: Page) {
  const worked = page.getByRole("button", { name: /^Worked for/ });
  for (const each of await worked.all())
    if ((await each.getAttribute("aria-expanded")) === "false")
      await each.click();
}

/** Opens the click on Save among the run's steps, bounded on its own with the reason: a run that never reached the
 * form has no such step, and the click alone waited out the test's whole budget with nothing said (c16's sign-in
 * rerun, 12 Oct 2026). */
async function openSaved(page: Page) {
  const saved = page.getByRole("button", { name: /^Clicked “Save”/ });
  // Each try opens any run's steps still folded: under load a run's group can arrive after the first openSteps, and
  // stay folded with the step inside it (the 16d integration, 12 Oct 2026).
  await expect(async () => {
    await openSteps(page);
    await expect(saved).toBeVisible({ timeout: 2_000 });
  }, "the run's click on Save is among its steps (did the run reach the form?)").toPass(
    { timeout: 30_000 },
  );
  await saved.click();
}

/** Expects `line` among the run's steps, opening any still folded on each try, as {@link openSaved} does: under load a
 * run's group can render after one openSteps and stay folded with the line inside it (the 16u integration's caps
 * check, 18 Oct 2026). */
async function inSteps(page: Page, line: Locator, what: string) {
  await expect(async () => {
    await openSteps(page);
    await expect(line).toBeVisible({ timeout: 2_000 });
  }, what).toPass({ timeout: afterStart(60_000) });
}

async function send(page: Page, text: string) {
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(text);
  await input.press("Enter");
  // Done again: the status first leaves Done for the new run.
  await expect(page.getByLabel("Run status")).not.toHaveText("Done");
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(90_000),
  });
}

/** A picture the gateway served, loaded in the room its size reserves. */
async function loaded(picture: Locator) {
  await expect(picture).toBeVisible();
  await expect
    .poll(() =>
      picture.evaluate((image: HTMLImageElement) => image.naturalWidth),
    )
    .toBeGreaterThan(0);
}

/** The Screen panel, opened from the task's bar when it isn't open already. */
async function screenPanel(page: Page) {
  const panel = page.getByRole("region", { name: "Screen" });
  if ((await panel.count()) === 0)
    await page.getByRole("button", { name: "Screen", exact: true }).click();
  await expect(panel).toBeVisible();
  return panel;
}

/** Take over, then sign in to the fixture site by hand in the desktop's browser, then Hand back. */
async function signInByHand(page: Page, panel: Locator) {
  await panel.getByRole("button", { name: "Take over" }).click();
  // Typed only under plain control: after a run the desktop's browser starts again, and the line says a sign-in may
  // not last until the runtime says it is up and focused (RT-I-055, GW-U-203; 16r's sec_059_a typed into a desktop
  // with no browser yet). The runtime waits up to 20 s for it, past the gateway's 10 s, so this waits longer.
  await expect(panel.getByRole("status").first()).toHaveText(
    words("screen.control"),
    { timeout: afterStart(45_000) },
  );
  // Under control the picture takes keys and pointer, so it is an application, not an image (screen.test.tsx).
  const picture = panel.getByRole("application", {
    name: words("screen.pictureControl"),
  });
  // The runtime's browser is open on the desktop, blank (ADR-0039 §6): focus the picture, then its address bar.
  await picture.click();
  await page.keyboard.press("Control+l");
  // A person's pause before typing: Chromium gives ctrl+L to the page first and focuses its address bar only once the
  // page passes it on, so a letter sent within that round trip lands in the page. Typed at once, the URL lost its first
  // letters and "ttp://…" asked to open xdg-open (c16, 12 Oct 2026).
  await page.waitForTimeout(500);
  await page.keyboard.type(process.env.TQA_SCREEN_LOGIN ?? "", { delay: 20 });
  await page.keyboard.press("Enter");
  // The site's page loads and sets its cookie; the driver checks where it landed.
  await page.waitForTimeout(4_000);
  await panel.getByRole("button", { name: "Hand back" }).click();
  await expect(panel.getByRole("status").first()).toHaveText(
    words("screen.handedBackIdle"),
  );
}

async function result(checked: string[]) {
  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify({ checked }));
}

test("WEB-E2E-037 a real desktop run watched, its pictures and replay, then a Take over to sign in", async ({
  page,
}) => {
  test.skip(PHASE !== "watch", "The driver's first sign-in phase");
  test.setTimeout(budget(1, 600_000));
  const checked: string[] = [];
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto(`/c/${TASK}`);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Open the form and save it");
  await input.press("Enter");
  // The first desktop action opens the Screen tab by itself (WEB-U-243), watching the real desktop.
  const panel = page.getByRole("region", { name: "Screen" });
  await expect(panel).toBeVisible({ timeout: afterStart(60_000) });
  await expect(
    panel.getByRole("img", { name: words("screen.picture") }),
  ).toBeVisible();
  await expect(panel.locator("canvas")).toBeVisible();
  checked.push("watching");
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(120_000),
  });
  await axe(page);

  await openSteps(page);
  await openSaved(page);
  await loaded(page.getByRole("img", { name: "Before" }).first());
  await loaded(page.getByRole("img", { name: "After" }).first());
  checked.push("pictures");

  await panel.getByRole("button", { name: "Replay" }).click();
  const step = panel.getByText(/^Step \d+ of \d+ · /);
  await expect(step).toBeVisible();
  const last = await step.textContent();
  await panel.getByRole("button", { name: "Previous step" }).click();
  await expect(step).not.toHaveText(last ?? "");
  await panel.getByRole("button", { name: "Live" }).click();
  checked.push("replay");

  await expect(page.getByText(words("tool.computerStale"))).toBeVisible();
  checked.push("stale");

  await signInByHand(page, panel);
  checked.push("take-over");
  await expect(page.getByText(words("screen.noticeTakenOver"))).toBeVisible();
  await expect(
    page.getByText(words("screen.noticeHandedBack").replace(/\{time\}.*$/, "")),
  ).toBeVisible();
  await axe(page);
  checked.push("handed-back");

  // The run that follows the sign-in, then the one after it: the driver reads what the site said to each.
  await send(page, "Check who the site thinks you are");
  await send(page, "Check again");
  await result(checked);
});

test("SEC-059 a sign-in by hand with no run after it", async ({ page }) => {
  test.skip(PHASE !== "sign-in", "The driver's second sign-in phase");
  test.setTimeout(budget(0, 120_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto(`/c/${TASK}`);
  await signInByHand(page, await screenPanel(page));
  await result(["signed-in"]);
});

test("SEC-059 Sign out everywhere from the Screen panel", async ({ page }) => {
  test.skip(PHASE !== "sign-out", "The driver's last sign-in phase");
  test.setTimeout(budget(0, 120_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto(`/c/${TASK}`);
  const panel = await screenPanel(page);
  await panel
    .getByRole("button", { name: "More actions for the screen" })
    .click();
  await page
    .getByRole("menuitem", { name: words("screen.signOutEverywhere") })
    .click();
  const dialog = page.getByRole("dialog", {
    name: words("screen.signOutTitle"),
  });
  await dialog.getByRole("button", { name: words("screen.signOut") }).click();
  // Said under the panel and kept in the transcript: the panel's own line.
  await expect(panel.getByText(words("screen.signedOut"))).toBeVisible();
  await axe(page);
  await result(["signed-out"]);
});

test("Slice 3's record on the page: evidence, refusals and a full task stopped", async ({
  page,
}) => {
  test.skip(PHASE !== "record", "The driver's record phase");
  test.setTimeout(budget(0, 180_000));
  const checked: string[] = [];
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);

  await page.goto(`/c/${TASK}`);
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await openSteps(page);
  await openSaved(page);
  await loaded(page.getByRole("img", { name: "Before" }).first());
  await loaded(page.getByRole("img", { name: "After" }).first());
  checked.push("acted");
  await expect(
    page.getByText(words("tool.computerStale")).first(),
  ).toBeVisible();
  checked.push("stale");
  await expect(
    page.getByText(words("tool.computerScreenshotTooLarge")),
  ).toBeVisible();
  await axe(page);
  checked.push("too-detailed");

  await page.goto(`/c/${process.env.TQA_SCREEN_CAPPED ?? ""}`);
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await inSteps(
    page,
    page.getByText(words("tool.computerScreenshotLimit")),
    "the run's screenshot limit is among its steps",
  );
  await inSteps(
    page,
    page.getByText(words("tool.computerActionLimit")),
    "the run's action limit is among its steps",
  );
  checked.push("caps");

  // The full task: the gateway stopped its run at its state bound before any picture (GW-I-091), so the page says
  // Stopped and shows none.
  await page.goto(`/c/${process.env.TQA_SCREEN_FULL ?? ""}`);
  await expect(page.getByLabel("Run status")).toHaveText(words("run.stopped"));
  await expect(
    page.getByRole("img", { name: words("screen.after") }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("img", { name: words("screen.before") }),
  ).toHaveCount(0);
  await axe(page);
  checked.push("state-full");
  await result(checked);
});

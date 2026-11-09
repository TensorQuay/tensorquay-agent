import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

const LONG_TITLE =
  "A quarterly report on every region's figures, with notes on each team's targets, its hires and the year ahead";

/** The page for the reviewers, at its 2x scale, once its finite motion has ended. */
async function shot(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter(
            (motion) =>
              motion.effect?.getComputedTiming().iterations !== Infinity,
          )
          .map((motion) => motion.finished.catch(() => undefined)),
      ),
      new Promise((resolve) => setTimeout(resolve, 5_000)),
    ]),
  );
  await page.screenshot({ path: join(directory, `${name}.png`) });
}

/** A task with this title, opened. */
async function task(page: Page, title = "Workspace review") {
  const made = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title, model_route: "local" },
  });
  expect(made.status()).toBe(201);
  await page.goto(`/c/${((await made.json()) as { id: string }).id}`);
  const header = page.getByRole("region", { name: "Task header" });
  await expect(header.getByRole("heading", { level: 1 })).toBeVisible();
  return header;
}

/** Opens or closes the Files panel from the header's Files icon, in any language (its name is translated). */
async function panel(page: Page, open: boolean) {
  const files = page.locator(
    '.conversation-bar button[aria-controls="files-panel"]',
  );
  if ((await files.getAttribute("aria-expanded")) !== String(open))
    await files.click();
  await expect(files).toHaveAttribute("aria-expanded", String(open));
  if (open) await expect(page.locator(".files-panel")).toBeVisible();
}

/** Where the main area's content starts and ends (inside its side padding), and the docked Files panel's start. */
function main(page: Page) {
  return page.evaluate(() => {
    const content = document.querySelector(".app-content");
    if (!content) throw new Error("The main area must be there");
    const box = content.getBoundingClientRect();
    const style = getComputedStyle(content);
    const files = document.querySelector(".files-panel");
    const docked =
      files && getComputedStyle(files).position !== "absolute"
        ? files.getBoundingClientRect().left
        : null;
    return {
      left: box.left + parseFloat(style.paddingLeft),
      right: box.right - parseFloat(style.paddingRight),
      panel: docked,
      gap: parseFloat(
        getComputedStyle(content).getPropertyValue("--space-6") || "24",
      ),
    };
  });
}

const box = async (locator: Locator) => {
  const found = await locator.boundingBox();
  if (!found) throw new Error("The part must be laid out");
  return {
    left: found.x,
    right: found.x + found.width,
    top: found.y,
    bottom: found.y + found.height,
    width: found.width,
    height: found.height,
    centre: found.x + found.width / 2,
  };
};
const near = (a: number, b: number, by = 1) => Math.abs(a - b) <= by;

/**
 * The header across the main area (inside its side padding, or up to the Files panel's gap): the title at its start,
 * the ⋯ at its end, everything on one line inside it; the transcript and the composer in their centred column.
 */
async function spans(page: Page, header: Locator, at: string) {
  const area = await main(page);
  const end = area.panel === null ? area.right : area.panel - area.gap;
  const bar = await box(header);
  expect
    .soft(near(bar.left, area.left), `${at}: the header's start`)
    .toBe(true);
  expect
    .soft(
      near(bar.right, end),
      `${at}: the header's end (${bar.right} vs ${end})`,
    )
    .toBe(true);
  const title = await box(header.getByRole("heading", { level: 1 }));
  expect
    .soft(title.left - bar.left <= 8, `${at}: the title at the start`)
    .toBe(true);
  const more = await box(
    header.getByRole("button", { name: /^More actions for / }),
  );
  expect
    .soft(near(more.right, bar.right, 2), `${at}: the ⋯ at the end`)
    .toBe(true);
  // One line: the title, the status, the pickers, Files and the ⋯ share the bar's row.
  for (const part of [
    header.getByLabel("Run status"),
    header.getByRole("button", { name: /^Model: / }),
    header.getByRole("button", { name: /^Environment: / }),
    header.getByRole("button", { name: "Files", exact: true }),
  ]) {
    const placed = await box(part);
    expect
      .soft(
        placed.top >= bar.top && placed.bottom <= bar.bottom,
        `${at}: one line`,
      )
      .toBe(true);
  }
  expect
    .soft(
      await header.evaluate(
        (element) => element.scrollWidth <= element.clientWidth + 1,
      ),
      `${at}: nothing past it`,
    )
    .toBe(true);
  // The reading column stays where it was: at most 720 px, centred under the header.
  for (const part of [
    page.getByRole("region", { name: "Transcript" }),
    page.getByRole("form", { name: "Message composer" }),
  ]) {
    const placed = await box(part);
    expect.soft(placed.width <= 721, `${at}: the column's width`).toBe(true);
    expect
      .soft(
        near(placed.centre, (bar.left + bar.right) / 2, 2),
        `${at}: the column centred`,
      )
      .toBe(true);
  }
}

// WEB-E2E-117 (UI-GUIDELINES §7 Conversation bar, decided on 9 Nov 2026): the task header spans the main area, inside
// its side padding: the title and its state at the start, the model and environment pickers, Files and the ⋯ at the
// end; with the Files panel open it ends at the panel's gap. Only the header widens: the transcript and the composer
// keep their centred column. A long title gives way first; nothing wraps to a second line.
for (const width of [1280, 1600] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-117 the task header spans the main area at ${width} ${colorScheme}`, async ({
      browser,
    }) => {
      test.setTimeout(120_000);
      const context = await browser.newContext({
        viewport: { width, height: 900 },
        deviceScaleFactor: 2,
        colorScheme,
        reducedMotion: "reduce",
      });
      const page = await context.newPage();
      await signIn(page);
      const header = await task(page);
      await panel(page, false);
      await page.mouse.move(width / 2, 600);
      await shot(page, `task-header-${width}-closed-${colorScheme}`);
      await spans(page, header, `${width} closed`);
      await panel(page, true);
      await page.mouse.move(width / 2, 600);
      await shot(page, `task-header-${width}-open-${colorScheme}`);
      await spans(page, header, `${width} open`);
      await context.close();
    });

// WEB-E2E-117 with a long title, at 1280: beside the Files panel the title is cut with an ellipsis (whole in its
// tooltip and to a screen reader) while the pickers keep their names; on the rail the header still spans; in Tamil and
// Thai, with their longer words, nothing wraps or runs past it either.
test("WEB-E2E-117 a long title gives way first, on the rail and in Tamil and Thai", async ({
  browser,
}) => {
  test.setTimeout(150_000);
  const context = await browser.newContext({
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 2,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  await signIn(page);
  const header = await task(page, LONG_TITLE);
  await panel(page, true);
  const title = header.getByRole("heading", { level: 1 });
  await expect.soft(title).toHaveAttribute("title", LONG_TITLE);
  await expect.soft(title).toHaveText(LONG_TITLE);
  expect
    .soft(
      await title.evaluate(
        (heading) => heading.scrollWidth > heading.clientWidth,
      ),
    )
    .toBe(true);
  for (const name of [/^Model: /, /^Environment: /])
    await expect
      .soft(header.getByRole("button", { name }).locator(".picker-name"))
      .toBeVisible();
  await spans(page, header, "long title, open");
  await shot(page, "task-header-long-title-1280");

  await panel(page, false);
  await page.getByRole("button", { name: /^Collapse sidebar/ }).click();
  await expect(page.locator(".app-sidebar")).toHaveCSS("width", "56px");
  await page.mouse.move(640, 600);
  await spans(page, header, "rail");
  await shot(page, "task-header-rail-1280");
  await page.getByRole("button", { name: /^Expand sidebar/ }).click();

  for (const language of ["ta", "th"]) {
    await page.evaluate(
      (code) => localStorage.setItem("tensorquay-language", code),
      language,
    );
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("lang", language);
    const local = page.locator(".conversation-bar");
    await expect(local.getByRole("heading", { level: 1 })).toBeVisible();
    await panel(page, true);
    const bar = await box(local);
    expect
      .soft(bar.height <= 57, `${language}: one line (${bar.height})`)
      .toBe(true);
    expect
      .soft(
        await local.evaluate(
          (element) => element.scrollWidth <= element.clientWidth + 1,
        ),
        `${language}: nothing past it`,
      )
      .toBe(true);
    await shot(page, `task-header-${language}-1280`);
  }
  await page.evaluate(() => localStorage.removeItem("tensorquay-language"));
  await context.close();
});

// WEB-E2E-117 on a phone: the header keeps its phone layout (the pickers on the line under the title), inside the window.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-117 the task header on a phone ${colorScheme}`, async ({
    browser,
  }) => {
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      deviceScaleFactor: 2,
      isMobile: true,
      hasTouch: true,
      colorScheme,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await signIn(page);
    const header = await task(page);
    const bar = await box(header);
    expect.soft(bar.left >= 0 && bar.right <= 390).toBe(true);
    const title = await box(header.getByRole("heading", { level: 1 }));
    const model = await box(header.getByRole("button", { name: /^Model: / }));
    expect
      .soft(model.top >= title.bottom - 1, "the pickers under the title")
      .toBe(true);
    expect
      .soft(
        await page.evaluate(() => document.documentElement.scrollWidth <= 390),
      )
      .toBe(true);
    await shot(page, `task-header-390-${colorScheme}`);
    await context.close();
  });

// WEB-E2E-117, the same rule for the other top bars over a centred column, at 1600: New task's header, a channel's
// header, and the Workspaces and Channels bars each span the main area inside its side padding.
test("WEB-E2E-117 the other top bars span the main area at 1600", async ({
  browser,
}) => {
  test.setTimeout(120_000);
  const context = await browser.newContext({
    viewport: { width: 1600, height: 900 },
    deviceScaleFactor: 2,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  await signIn(page);
  const bars: [string, string, string][] = [
    ["new-task", "/", ".conversation-bar"],
    ["workspaces", "/workspaces", ".workspaces-bar"],
    ["channels", "/channels", ".channels-bar"],
  ];
  for (const [name, path, selector] of bars) {
    await page.goto(path);
    const bar = page.locator(selector).first();
    await expect(bar).toBeVisible();
    const area = await main(page);
    const placed = await box(bar);
    expect
      .soft(near(placed.left, area.left), `${name}: the bar's start`)
      .toBe(true);
    expect
      .soft(
        near(placed.right, area.right),
        `${name}: the bar's end (${placed.right} vs ${area.right})`,
      )
      .toBe(true);
    await shot(page, `bar-${name}-1600`);
  }
  // Settings sits over no centred column: its title already starts at the main area's start, with nothing at its end.
  await page.goto("/settings/account");
  const settings = page.locator(".settings-title");
  await expect(settings).toBeVisible();
  expect
    .soft(
      near((await box(settings)).left, (await main(page)).left),
      "settings: the title's start",
    )
    .toBe(true);
  await shot(page, "bar-settings-1600");
  await page.goto("/channels");
  await page
    .getByRole("list", { name: "Channels" })
    .getByRole("link", { name: "release", exact: true })
    .click();
  const channel = page.getByRole("region", { name: "Channel header" });
  await expect(channel).toBeVisible();
  const area = await main(page);
  const placed = await box(channel);
  expect
    .soft(near(placed.left, area.left), "channel: the bar's start")
    .toBe(true);
  expect
    .soft(
      near(placed.right, area.right),
      `channel: the bar's end (${placed.right} vs ${area.right})`,
    )
    .toBe(true);
  const log = await box(page.getByRole("list", { name: "Channel log" }));
  expect.soft(log.width <= 721, "channel: the log keeps its column").toBe(true);
  // Everything else on the page but its side reads in the log's column (its idle agents, its composer).
  const strays: string[] = await page
    .locator(".channel-page > :not(.channel-bar):not(.channel-side)")
    .evaluateAll(
      (parts: Element[], column: { left: number; right: number }) =>
        parts
          .filter((part) => {
            const rect = part.getBoundingClientRect();
            return (
              rect.width > 0 &&
              (rect.left < column.left - 1 || rect.right > column.right + 1)
            );
          })
          .map((part) => part.getAttribute("class") ?? part.tagName),
      { left: log.left, right: log.right },
    );
  expect.soft(strays, "channel: in the log's column").toEqual([]);
  await shot(page, "bar-channel-1600");
  await context.close();
});

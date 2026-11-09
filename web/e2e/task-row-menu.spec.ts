import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { signIn } from "./session";
import { expect, resetFront, test, type Locator, type Page } from "./test";

/** The even inset between the ⋯ and its row's edges (`--space-half`). */
const INSET = 2;
/** The row's corner radius (`--control-radius`); the ⋯'s corners are concentric with it. */
const ROW_RADIUS = 8;
const LONG_TITLE =
  "Compare the quarterly supplier invoices with the purchase orders";

/**
 * The sidebar for the reviewers, at the page's own scale (the tests run at 2x, so a shot is the designer's 2x view),
 * when TQA_WEB_EVIDENCE_DIR names a folder.
 */
async function shot(sidebar: Locator, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  // Once every finite fade or slide has ended (a drawer opening), never mid-way; bounded, as e2e/evidence.ts does.
  await sidebar.evaluate(() =>
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
  await sidebar.screenshot({ path: join(directory, `${name}.png`) });
}

async function makeTask(page: Page, title: string) {
  const made = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title, model_route: "local" },
  });
  expect(made.status()).toBe(201);
  return ((await made.json()) as { id: string }).id;
}

/** A row's box, its ⋯'s box and what the ⋯ draws, as the browser lays them out now. */
function measure(row: Locator) {
  return row.evaluate((node) => {
    const button = node.querySelector(":scope > .task-more > .button");
    if (!(button instanceof HTMLElement)) throw new Error("The row has a ⋯");
    const outer = node.getBoundingClientRect();
    const inner = button.getBoundingClientRect();
    const style = getComputedStyle(button);
    // The row's own line: an open lead's row also holds its helpers below it.
    const line = node.querySelector(":scope > .conversation-link");
    const own = (line ?? node).getBoundingClientRect();
    return {
      top: inner.top - own.top,
      bottom: own.bottom - inner.bottom,
      end: outer.right - inner.right,
      width: inner.width,
      height: inner.height,
      lineHeight: own.height,
      radius: parseFloat(style.borderTopLeftRadius),
      rowRadius: parseFloat(getComputedStyle(node).borderTopRightRadius),
      fill: style.backgroundColor,
      image: style.backgroundImage,
      opacity: style.opacity,
    };
  });
}

/** Whether the title (and what follows it) fades out under the ⋯: the mask the title wears while the ⋯ shows. */
const masked = (row: Locator) =>
  row
    .locator(":scope > .conversation-link .conversation-link-text")
    .evaluate((node) => getComputedStyle(node).maskImage !== "none");

/** A computed colour's alpha: `rgba(r, g, b, a)`, or `color(srgb r g b / a)` as a `color-mix()` tint computes. */
function alphaOf(colour: string) {
  const match =
    /rgba\([^)]*,\s*([\d.]+)\)/.exec(colour) ??
    /color\([^)]*\/\s*([\d.]+)\)/.exec(colour);
  return Number(match?.[1]);
}

const away = (page: Page) => page.mouse.move(1100, 700);

// WEB-E2E-114 (UI-GUIDELINES §7 Conversation list item): a task row's ⋯ is part of the row. Under the pointer it sits
// inside the row's backdrop at the row's end: square, one even inset above, below and at the end, its corners
// concentric with the row's (the row's radius less the inset), with no fill of its own; its own hover is a quiet tint
// inside that backdrop; the title fades out under it while it shows and keeps the full width without hover. The same
// on the open task's row and on a lead's row with its chevron, in both themes. A click leaves no ⋯ behind: the
// clicked row's link keeps its focus, and only the row under the pointer, or with keyboard focus, shows one.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-114 the task row's ⋯ is part of the row at 1280 ${colorScheme}`, async ({
    browser,
  }) => {
    test.setTimeout(90_000);
    const context = await browser.newContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
      colorScheme,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await resetFront(page.request, ["agent-teams"]);
    await signIn(page);
    await makeTask(page, LONG_TITLE);
    const open = await makeTask(page, "Weekly sales summary");
    await page.goto(`/c/${open}`);
    const sidebar = page.locator(".app-sidebar");
    const rows = sidebar.locator(".conversation-row");
    const rowOf = (name: RegExp) =>
      rows.filter({
        has: page.locator(":scope > .conversation-link", { hasText: name }),
      });
    const cases = [
      { name: "row", row: rowOf(/^Compare the quarterly/) },
      { name: "current", row: rowOf(/^Weekly sales summary/) },
      { name: "chevron", row: rowOf(/^Report on Q3/) },
      { name: "chevron-open", row: rowOf(/^Report on Q3/) },
    ];
    for (const { name, row } of cases)
      await test.step(`the ${name}'s ⋯ on hover`, async () => {
        await expect(row).toHaveCount(1);
        if (name === "chevron-open") {
          await page
            .getByRole("button", { name: "Helpers of Report on Q3" })
            .click();
          await expect(
            row.getByRole("list", { name: "Helpers" }),
          ).toBeVisible();
          // The chevron keeps the focus a click gave it; the row is looked at without it.
          await page.evaluate(() => {
            if (document.activeElement instanceof HTMLElement)
              document.activeElement.blur();
          });
        }
        await away(page);
        // Without the pointer, the ⋯ is hidden and the title keeps the full width.
        expect.soft((await measure(row)).opacity).toBe("0");
        expect.soft(await masked(row)).toBe(false);
        await row.hover({ position: { x: 40, y: 10 } });
        await shot(sidebar, `row-menu-${name}-${colorScheme}`);
        const box = await measure(row);
        expect.soft(box.opacity).toBe("1");
        // One shape: the ⋯ adds no surface of its own over the row's backdrop.
        expect.soft(box.fill).toBe("rgba(0, 0, 0, 0)");
        expect.soft(box.image).toBe("none");
        // Inside the row's end, vertically centred, one even inset.
        for (const inset of [box.top, box.bottom, box.end])
          expect.soft(Math.abs(inset - INSET)).toBeLessThan(0.6);
        expect.soft(Math.abs(box.width - box.height)).toBeLessThan(0.6);
        expect
          .soft(Math.abs(box.height - (box.lineHeight - 2 * INSET)))
          .toBeLessThan(0.6);
        // Corners concentric with the row's.
        expect.soft(box.rowRadius).toBe(ROW_RADIUS);
        expect.soft(box.radius).toBe(ROW_RADIUS - INSET);
        // The title, where it reaches the ⋯, fades out under it.
        if (name === "row") expect.soft(await masked(row)).toBe(true);
        // The ⋯'s own hover: a quiet tint inside the backdrop, never an opaque chip.
        await row.locator(":scope > .task-more > .button").hover();
        await shot(sidebar, `row-menu-${name}-button-hover-${colorScheme}`);
        const hovered = await measure(row);
        const alpha = alphaOf(hovered.fill);
        expect.soft(alpha).toBeGreaterThan(0);
        expect.soft(alpha).toBeLessThan(0.2);
        expect.soft(hovered.image).toBe("none");
      });
    await test.step("a click leaves no ⋯ behind: only the row under the pointer shows one", async () => {
      const opened = rowOf(/^Report on Q3/);
      const other = rowOf(/^Compare the quarterly/);
      const link = opened.locator(":scope > .conversation-link");
      await link.click();
      await expect(link).toHaveAttribute("aria-current", "page");
      await expect(link).toBeFocused();
      await other.hover({ position: { x: 40, y: 10 } });
      await shot(sidebar, `row-menu-after-click-${colorScheme}`);
      const showing = await rows.evaluateAll(
        (all) =>
          all.filter((node) => {
            const button = node.querySelector(":scope > .task-more > .button");
            return button && getComputedStyle(button).opacity === "1";
          }).length,
      );
      expect.soft(showing).toBe(1);
      expect.soft((await measure(other)).opacity).toBe("1");
      expect.soft((await measure(opened)).opacity).toBe("0");
      // The lead as the open task, under the pointer: its ⋯ and its chevron in the selected fill.
      await opened.hover({ position: { x: 60, y: 10 } });
      await shot(sidebar, `row-menu-chevron-current-${colorScheme}`);
      // Keyboard focus shows it: onto the lead's ⋯, then back to its link.
      await away(page);
      await page.keyboard.press("Tab");
      await page.keyboard.press("Shift+Tab");
      await expect(link).toBeFocused();
      expect.soft((await measure(opened)).opacity).toBe("1");
    });
    await test.step("keyboard focus keeps a visible ring", async () => {
      await away(page);
      const button = cases[0]?.row.locator(":scope > .task-more > .button");
      if (!button) throw new Error("The first row has a ⋯");
      await button.focus();
      await page.keyboard.press("Shift+Tab");
      await page.keyboard.press("Tab");
      await expect(button).toBeFocused();
      await expect(button).toHaveCSS("outline-style", "solid");
      await expect(button).toHaveCSS("outline-width", "2px");
      expect((await measure(cases[0]?.row ?? rows.first())).opacity).toBe("1");
      await shot(sidebar, `row-menu-focus-${colorScheme}`);
    });
    await context.close();
  });

// WEB-E2E-114 on a phone (hover: none): the ⋯ always shows, calm, with no fill of its own, and the title ends before it
// instead of under it, so the row's status stays readable.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-114 the task row's ⋯ in the phone drawer at 390 ${colorScheme}`, async ({
    browser,
  }) => {
    test.setTimeout(90_000);
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      deviceScaleFactor: 2,
      isMobile: true,
      hasTouch: true,
      colorScheme,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await resetFront(page.request, ["agent-teams"]);
    await signIn(page);
    await makeTask(page, LONG_TITLE);
    const open = await makeTask(page, "Weekly sales summary");
    await page.goto(`/c/${open}`);
    await page.getByRole("button", { name: "Open navigation" }).click();
    const drawer = page.getByRole("dialog", { name: "Navigation" });
    await expect(drawer).toBeVisible();
    const rows = drawer.locator(".conversation-row");
    await expect(rows.first()).toBeVisible();
    await shot(drawer, `row-menu-phone-${colorScheme}`);
    for (const title of [
      /^Compare the quarterly/,
      /^Weekly sales/,
      /^Report on Q3/,
    ]) {
      const row = rows.filter({
        has: page.locator(":scope > .conversation-link", { hasText: title }),
      });
      const box = await measure(row);
      expect.soft(box.opacity).toBe("1");
      expect.soft(box.fill).toBe("rgba(0, 0, 0, 0)");
      for (const inset of [box.top, box.bottom, box.end])
        expect.soft(Math.abs(inset - INSET)).toBeLessThan(0.6);
      expect.soft(box.radius).toBe(ROW_RADIUS - INSET);
      // The title and the row's marks end before the ⋯, never under it.
      const clear = await row.evaluate((node) => {
        const button = node.querySelector(":scope > .task-more");
        const text = node.querySelector(".conversation-link-text");
        const marks = [
          ...node.querySelectorAll(
            ":scope > .conversation-link > :not(.task-icon)",
          ),
        ];
        if (!button || !text) return false;
        const edge = button.getBoundingClientRect().left;
        return [text, ...marks].every(
          (part) => part.getBoundingClientRect().right <= edge + 0.5,
        );
      });
      expect.soft(clear).toBe(true);
    }
    await context.close();
  });

import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, resetFront, test, type Locator, type Page } from "./test";

// WEB-E2E-128 (18.0.1): what opens in the side panel fits it at desktop widths. The Screen tab's ⋯ menu is wide
// enough for its items, inside the panel (WEB-U-483); at 390 it stays as it was, aligned to its ⋯'s end. A PDF's
// page pictures fit the Files panel's width, its scrollbar included (WEB-U-484). Light and dark.

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `panel-fit-${name}.png`) });
}

/** A new task on the Desktop environment, which the "desktop" scenario offers. */
async function desktopTask(page: Page): Promise<string> {
  const response = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: {
      title: "Fill in the form",
      model_route: "local",
      template: "desktop",
    },
  });
  expect(response.status()).toBe(201);
  const { id } = (await response.json()) as { id: string };
  return id;
}

/** The panel's content box: its border box less its side padding. */
async function contentBox(panel: Locator) {
  return panel.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return {
      left: box.left + parseFloat(style.paddingLeft),
      right: box.right - parseFloat(style.paddingRight),
    };
  });
}

/** Opens the Screen tab's ⋯ menu and checks it fits the panel, nothing in it cut. Returns the menu's and ⋯'s boxes. */
async function openMenu(page: Page) {
  const bounds = page.locator("aside[data-menu-bounds]");
  const more = page.getByRole("button", {
    name: "More actions for the screen",
  });
  await more.click();
  const surface = page.getByRole("region", {
    name: "More actions for the screen",
  });
  const keep = surface.getByRole("menuitemcheckbox", {
    name: "Keep the sign-in for this task",
  });
  await expect(keep).toBeVisible();
  await settled(page);
  const inside = await contentBox(bounds);
  const box = await surface.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      width: rect.width,
      cut: element.scrollWidth > element.clientWidth,
    };
  });
  expect(box.left).toBeGreaterThanOrEqual(inside.left - 0.5);
  expect(box.right).toBeLessThanOrEqual(inside.right + 0.5);
  expect(box.cut).toBe(false);
  // Its first item's word reads on one line, never cut short.
  const lines = await keep.evaluate((item) => {
    const word = item.querySelector(".action-menu-word > :first-child");
    const range = document.createRange();
    if (word) range.selectNodeContents(word);
    const tops = new Set(
      Array.from(range.getClientRects(), (rect) => Math.round(rect.top)),
    );
    return { lines: tops.size, cut: item.scrollWidth > item.clientWidth };
  });
  expect(lines).toEqual({ lines: 1, cut: false });
  return { box, more: (await more.boundingBox())! };
}

// A desktop browser draws classic scrollbars, which take the column's width (the livepass's ~10 px); Playwright hides
// them unless told not to.
test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

test.beforeEach(async ({ request }) => {
  await resetFront(request, ["desktop"]);
});

for (const width of [1280, 1440] as const)
  for (const colorScheme of ["light", "dark"] as const) {
    test(`WEB-E2E-128 the Screen menu fits its panel (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      const id = await desktopTask(page);
      await page.goto(`/c/${encodeURIComponent(id)}`);
      await page.getByRole("button", { name: "Screen", exact: true }).click();
      const panel = page.getByRole("region", { name: "Screen" });
      await expect(panel.getByRole("status").first()).toHaveText("Watching.");
      // Watching: the ⋯ after Take over.
      await openMenu(page);
      await capture(page, `screen-menu-live-${width}-${colorScheme}`);
      await page.keyboard.press("Escape");
      // Stopped: the ⋯ alone at the panel's start, the livepass's "Keep the sign-i…".
      await page.request.post(
        `/__fake/screen?conversation=${encodeURIComponent(id)}`,
        { data: { refuse: "sandbox_not_running" } },
      );
      await expect(panel.getByRole("status").first()).toHaveText(
        "The desktop isn't running. It starts when you send a message.",
      );
      await openMenu(page);
      await capture(page, `screen-menu-stopped-${width}-${colorScheme}`);
    });

    test(`WEB-E2E-128 a PDF's pages fit the Files panel (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const panel = await openFilesPanel(page);
      await panel.getByRole("button", { name: "reports" }).click();
      await panel.getByRole("button", { name: /^long\.pdf/ }).click();
      const pages = panel.getByRole("region", { name: "Pages of long.pdf" });
      await expect(pages.getByRole("group", { name: "Page 3" })).toBeVisible();
      await settled(page);
      const fit = await pages.evaluate((column: HTMLElement) => {
        const box = column.getBoundingClientRect();
        const right = box.left + column.clientLeft + column.clientWidth;
        return {
          scrolls: column.scrollHeight > column.clientHeight,
          // The scrollbar takes room: the case the pages' width must leave for it.
          scrollbar:
            column.offsetWidth - column.clientLeft * 2 > column.clientWidth,
          sideways: column.scrollWidth > column.clientWidth,
          over: Array.from(
            column.querySelectorAll(".file-pdf-page"),
            (page) => page.getBoundingClientRect().right - right,
          ).filter((past) => past > 0.5),
        };
      });
      expect(fit).toEqual({
        scrolls: true,
        scrollbar: true,
        sideways: false,
        over: [],
      });
      await capture(page, `pdf-pages-${width}-${colorScheme}`);
    });
  }

test("WEB-E2E-128 the Screen menu at 390 stays aligned to its ⋯", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  const id = await desktopTask(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  await page.getByRole("button", { name: "Screen", exact: true }).click();
  const panel = page.getByRole("region", { name: "Screen" });
  await expect(panel.getByRole("status").first()).toHaveText("Watching.");
  const { box, more } = await openMenu(page);
  expect(Math.abs(box.right - (more.x + more.width))).toBeLessThanOrEqual(1);
  await capture(page, "screen-menu-live-390");
});

// The Screen's Expand: its dialog sits in the panel's markup, not its box; its ⋯ menu opens under the ⋯, in the window.
test("WEB-E2E-128 the Screen menu in the expanded dialog opens under its ⋯", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await signIn(page);
  const id = await desktopTask(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  await page.getByRole("button", { name: "Screen", exact: true }).click();
  const panel = page.getByRole("region", { name: "Screen" });
  await panel.getByRole("button", { name: "Take over" }).click();
  await expect(panel.getByRole("status").first()).toHaveText(
    /^You have control\./,
  );
  await panel.getByRole("button", { name: "Expand" }).click();
  const dialog = page.getByRole("dialog", { name: "Screen" });
  const more = dialog.getByRole("button", {
    name: "More actions for the screen",
  });
  await more.click();
  const surface = page.getByRole("region", {
    name: "More actions for the screen",
  });
  await expect(
    surface.getByRole("menuitemcheckbox", {
      name: "Keep the sign-in for this task",
    }),
  ).toBeVisible();
  await settled(page);
  const trigger = (await more.boundingBox())!;
  const box = (await surface.boundingBox())!;
  expect(box.x).toBeLessThanOrEqual(trigger.x + trigger.width);
  expect(box.x + box.width).toBeGreaterThanOrEqual(trigger.x);
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(1280);
  expect(box.width).toBeGreaterThanOrEqual(192);
  await capture(page, "screen-menu-expanded-1280");
});

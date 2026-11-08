import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `layout-${name}.png`) });
}

const box = async (page: Page, selector: string) => {
  const rectangle = await page.locator(selector).first().boundingBox();
  if (!rectangle) throw new Error(`${selector} must be visible`);
  return rectangle;
};

// UI-GUIDELINES §6 layouts A (expanded) and C (rail): no header, a one-line bar, and work starting near the top.
for (const width of [1024, 1440])
  for (const colorScheme of ["light", "dark"] as const)
    for (const sidebar of ["expanded", "rail"] as const)
      test(`WEB-U-007 layout ${sidebar} ${width} ${colorScheme}`, async ({
        page,
      }) => {
        await page.setViewportSize({ width, height: 900 });
        await page.emulateMedia({ colorScheme });
        // Seed the starting state once; later loads keep whatever the page remembered.
        await page.addInitScript((state) => {
          if (!localStorage.getItem("tensorquay-sidebar"))
            localStorage.setItem("tensorquay-sidebar", state);
        }, sidebar);
        await signIn(page);
        await page.goto(await createConversation(page));
        const input = page.getByRole("textbox", { name: "Message" });
        await input.fill("Summarize the workspace.");
        await input.press("Enter");
        await expect(page.getByLabel("Run status")).toHaveText(/Done/);

        await expect(page.getByRole("banner")).toHaveCount(0);
        expect((await box(page, ".app-sidebar")).width).toBe(
          sidebar === "rail" ? 56 : 260,
        );
        await expect(
          page.getByRole("button", {
            name: sidebar === "rail" ? "Expand sidebar" : "Collapse sidebar",
          }),
        ).toBeVisible();

        const bar = await box(page, ".conversation-bar");
        expect(bar.y).toBe(0);
        expect(bar.height).toBeLessThanOrEqual(57);
        for (const part of [
          "h1",
          '[aria-label="Run status"]',
          // The quiet Files icon and the ⋯ (the UI standard's header).
          ".files-toggle",
          ".conversation-pickers .icon-action",
        ]) {
          const piece = await box(page, `.conversation-bar ${part}`);
          expect(
            piece.y,
            `${part} sits on the bar's one line`,
          ).toBeGreaterThanOrEqual(bar.y);
          expect(piece.y + piece.height).toBeLessThanOrEqual(
            bar.y + bar.height,
          );
        }
        await page.locator(".transcript-scroll").evaluate((element) => {
          element.scrollTop = 0;
        });
        const first = await box(page, ".transcript-items > *");
        expect(
          first.y,
          "first transcript line within 64 px",
        ).toBeLessThanOrEqual(64);
        const column = await box(page, ".conversation-page");
        // The UI standard: one conversation width, 720 px, whether the sidebar is expanded or a rail.
        expect(column.width).toBeLessThanOrEqual(720);
        await expect(page.locator(".conversation-page")).toHaveAttribute(
          "data-scrolled",
          "false",
        );
        expect((await new AxeBuilder({ page }).analyze()).violations).toEqual(
          [],
        );
        await capture(page, `${sidebar}-${width}-${colorScheme}`);

        await page.locator("body").press("[");
        expect((await box(page, ".app-sidebar")).width).toBe(
          sidebar === "rail" ? 260 : 56,
        );
        await page.reload();
        await expect(page.getByLabel("Run status")).toHaveText(/Done/);
        expect(
          (await box(page, ".app-sidebar")).width,
          "the choice is remembered",
        ).toBe(sidebar === "rail" ? 260 : 56);
      });

// WEB-U-007: before the app's scripts run, the toggle's name already matches the layout the stylesheet shows (the
// stored choice, else the width), and it stays the same once they run.
for (const [stored, width, layout] of [
  [null, 1024, "rail"],
  [null, 1280, "expanded"],
  ["rail", 1280, "rail"],
  ["expanded", 1024, "expanded"],
] as const)
  test(`WEB-U-007 the sidebar toggle's name before hydration: ${stored ?? "nothing"} stored at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.addInitScript((state) => {
      localStorage.removeItem("tensorquay-sidebar");
      if (state) localStorage.setItem("tensorquay-sidebar", state);
    }, stored);
    await signIn(page);
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route("**/_next/static/**/*.js", async (route) => {
      await held;
      await route.continue();
    });
    const name = layout === "rail" ? "Expand sidebar" : "Collapse sidebar";
    const toggle = page.getByRole("button", { name, exact: true });
    try {
      await page.goto("/", { waitUntil: "domcontentloaded" });
      await expect(toggle).toBeVisible();
      await expect(toggle).not.toHaveAttribute("title");
      expect((await box(page, ".app-sidebar")).width).toBe(
        layout === "rail" ? 56 : 260,
      );
    } finally {
      release();
    }
    // Hydrated: its tooltip appears, and its name hasn't changed.
    await expect(toggle).toHaveAttribute("title", `${name} ([)`);
  });

// UI-GUIDELINES §7: the account row sits at the bottom of the sidebar and its menu opens upward.
for (const sidebar of ["expanded", "rail"] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-007 account row ${sidebar} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1440, height: 900 });
      await page.emulateMedia({ colorScheme });
      await page.addInitScript((state) => {
        localStorage.setItem("tensorquay-sidebar", state);
      }, sidebar);
      await signIn(page);
      await page.goto("/");
      const row = page.getByRole("button", { name: "Account" });
      const rowBox = await row.boundingBox();
      if (!rowBox) throw new Error("The account row must be visible");
      expect(rowBox.y + rowBox.height).toBeGreaterThan(900 - 64);
      await expect(row.getByText("member@example.invalid")).toBeVisible({
        visible: sidebar === "expanded",
      });
      await row.click();
      const menu = page.getByRole("menu", { name: "Account" });
      // The menu's actions load on demand: measure once they are in, not the smaller "Opening" placeholder the
      // popover shows first and re-places when it grows.
      await expect(
        menu.getByRole("menuitem", { name: "Sign out" }),
      ).toBeVisible();
      const menuBox = await menu.boundingBox();
      if (!menuBox) throw new Error("The account menu must be visible");
      expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(rowBox.y);
      expect(menuBox.y).toBeGreaterThanOrEqual(0);
      await capture(page, `account-${sidebar}-${colorScheme}`);
      await page.keyboard.press("Escape");
      await expect(row).toBeFocused();
    });

const opacity = (page: Page, selector: string) =>
  page
    .locator(selector)
    .first()
    .evaluate((element) => getComputedStyle(element).opacity);

// The task row's actions menu sits inside the row, 8 px from its right edge, and shows on hover or keyboard focus.
test("WEB-U-007 task row actions appear on hover and focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await createConversation(page);
  await page.goto("/");
  const row = page.locator(".conversation-row").first();
  const remove = row.getByRole("button", { name: /^More actions for / });
  const rowBox = await row.boundingBox();
  const removeBox = await remove.boundingBox();
  if (!rowBox || !removeBox) throw new Error("The task row must be visible");
  expect(rowBox.x + rowBox.width - (removeBox.x + removeBox.width)).toBe(8);
  expect(await opacity(page, ".conversation-row > .task-more > .button")).toBe(
    "0",
  );
  await row.hover();
  await expect
    .poll(() => opacity(page, ".conversation-row > .task-more > .button"))
    .toBe("1");
  await page.mouse.move(720, 450);
  await expect
    .poll(() => opacity(page, ".conversation-row > .task-more > .button"))
    .toBe("0");
  await row.getByRole("link").focus();
  await expect
    .poll(() => opacity(page, ".conversation-row > .task-more > .button"))
    .toBe("1");
});

test.describe("touch", () => {
  test.use({ hasTouch: true, isMobile: true });
  // Touch screens have no hover, so the actions menu stays visible.
  test("WEB-U-007 task row actions are always visible on touch", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await signIn(page);
    await createConversation(page);
    await page.goto("/");
    await page.getByRole("button", { name: "Open navigation" }).click();
    const remove = page
      .getByRole("dialog", { name: "Navigation" })
      .getByRole("button", { name: /^More actions for / });
    await expect(remove).toBeVisible();
    expect(
      await remove.evaluate((element) => getComputedStyle(element).opacity),
    ).toBe("1");
  });
});

/** How the bar spends its width: the run status is whole, never cut, and the title truncates instead. */
async function barState(page: Page) {
  return page.evaluate(() => {
    const clause = document.querySelector(
      ".conversation-bar .conversation-status [role='group']",
    );
    const title = document.querySelector(".conversation-bar h1");
    if (!clause || !title) throw new Error("The bar needs a title and status");
    const range = document.createRange();
    range.selectNodeContents(clause);
    const text = range.getBoundingClientRect();
    const box = clause.getBoundingClientRect();
    // Text below the one-line box is out of sight; text on the line must fit it horizontally.
    const outside = text.top >= box.bottom - 0.5 || box.width < 0.5;
    const inside =
      !outside && text.left >= box.left - 0.5 && text.right <= box.right + 0.5;
    const word = document
      .querySelector(".conversation-bar [role='status']")
      ?.getBoundingClientRect();
    const heading = title.getBoundingClientRect();
    return {
      clause: inside ? "whole" : outside ? "hidden" : "cut",
      titleTruncated: title.scrollWidth > title.clientWidth,
      // The run word sits on the title's line, centred with it.
      aligned: word
        ? Math.abs(
            word.top + word.height / 2 - (heading.top + heading.height / 2),
          ) <= 1.5
        : false,
    };
  });
}

// At 1024 the bar cut the status mid-word. Now the status is never cut; the title truncates.
test("WEB-U-007 the bar keeps the status whole and truncates the title", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 900 });
  await page.addInitScript(() => {
    localStorage.setItem("tensorquay-sidebar", "expanded");
  });
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Summarize the workspace.");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/Done/);
  // A sleeping environment isn't news in the bar.
  await expect(page.locator(".environment-status")).toHaveCount(0);
  for (const title of [
    "Short",
    "Workspace review",
    "A deliberately long task title that cannot fit beside the status and the pickers",
  ]) {
    await page.locator(".conversation-bar h1").click();
    const field = page.getByRole("textbox", { name: "Task title" });
    await field.fill(title);
    await field.press("Enter");
    await expect(page.locator(".conversation-bar h1")).toHaveText(title);
    const state = await barState(page);
    expect(state.clause, `${title}: status whole`).toBe("whole");
    expect(state.aligned, `${title}: status centred with the title`).toBe(true);
    if (title.startsWith("A deliberately"))
      expect(state.titleTruncated, `${title}: the title gives way`).toBe(true);
  }
  // With the room of the rail, a short title isn't truncated.
  await page.locator(".conversation-bar h1").click();
  const field = page.getByRole("textbox", { name: "Task title" });
  await field.fill("Short");
  await field.press("Enter");
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  expect(await barState(page)).toEqual({
    clause: "whole",
    titleTruncated: false,
    aligned: true,
  });
});

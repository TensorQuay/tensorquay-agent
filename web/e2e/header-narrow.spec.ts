import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { addFromPlus } from "./choices";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

// WEB-U-106 on the page: where the header is narrow its model and environment pickers show their icons, never a cut
// name; the chosen name is in the tooltip and the menu's title. At 1280 the names show whole. WEB-U-107: New task's
// workspace chip carries its own status, right under it.
async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== "running"),
  );
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

/** Whether any shown name in the header's pickers is cut short. */
function cut(header: Locator) {
  return header
    .locator(".header-choices .picker-name")
    .evaluateAll((names) =>
      names.some(
        (name) =>
          name instanceof HTMLElement &&
          name.offsetParent !== null &&
          name.scrollWidth > name.clientWidth,
      ),
    );
}

/** The header's pickers, checked: icons alone where it is narrow, unless `named` (a task's own, WEB-U-154). */
async function pickers(page: Page, width: number, named = width >= 768) {
  const header = page.getByRole("region", { name: "Task header" });
  const choices = header.getByRole("group", {
    name: "Model and environment",
  });
  const model = choices.getByRole("button", { name: /^Model: / });
  const environment = choices.getByRole("button", { name: /^Environment: / });
  // Named once their lists have come (not "Loading…" or "Unavailable").
  await expect(model).toHaveAccessibleName(/^Model: (?!Loading|Unavailable)/);
  await expect(environment).toHaveAccessibleName(
    /^Environment: (?!Loading|Unavailable)/,
  );
  for (const picker of [model, environment]) {
    await expect(picker).toBeVisible();
    await expect(picker.locator(".picker-icon")).toBeVisible({
      visible: !named,
    });
    await expect(picker.locator(".picker-name")).toBeVisible({
      visible: named,
    });
  }
  expect(await cut(header)).toBe(false);
  return { header, model, environment };
}

for (const width of [1280, 390])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-106/107 the header's pickers and the workspace chip at ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto("/");
      const { header, model, environment } = await pickers(page, width);
      await capture(page, `narrow-new-task-${width}-${theme}`);

      if (width < 768) {
        // The chosen name, whole, in its tooltip on hover and atop its menu when open.
        const name = await environment.getAttribute("aria-label");
        if (!name) throw new Error("The picker must be named");
        await environment.hover();
        const tip = header.locator(".picker-name-tip", { hasText: name });
        await expect(tip).toBeVisible();
        const tipBox = await tip.boundingBox();
        if (!tipBox) throw new Error("The tooltip must be visible");
        expect(tipBox.x).toBeGreaterThanOrEqual(0);
        expect(tipBox.x + tipBox.width).toBeLessThanOrEqual(width);
        await capture(page, `narrow-tip-${width}-${theme}`);
        await environment.click();
        await expect(tip).toBeHidden();
        const menu = page.getByRole("region", { name: "Environment choices" });
        await expect(menu.locator(".picker-title")).toHaveText(name);
        // WEB-U-185: each size reads as everywhere else ("2 CPUs · 4 GB") and stays on one line.
        for (const size of await menu.locator(".nowrap").all()) {
          await expect(size).toHaveText(/^\d+ CPUs? · [\d.]+ (GB|MB)$/);
          expect(
            await size.evaluate((element) => {
              const range = document.createRange();
              range.selectNodeContents(element);
              return new Set(
                [...range.getClientRects()].map((rect) => Math.round(rect.top)),
              ).size;
            }),
          ).toBe(1);
        }
        await capture(page, `narrow-menu-${width}-${theme}`);
        await page.keyboard.press("Escape");
        await expect(menu).toHaveCount(0);
        await expect(model).toBeVisible();
      } else {
        // With room for the names, the menu needs no title.
        await environment.click();
        const menu = page.getByRole("region", { name: "Environment choices" });
        await expect(menu.getByRole("menu")).toBeVisible();
        await expect(menu.locator(".picker-title")).toHaveCount(0);
        await page.keyboard.press("Escape");
      }

      // WEB-U-107: a busy workspace says so on its own chip, not below the composer.
      const composer = page.getByRole("form", { name: "Message composer" });
      await addFromPlus(page, "Workspace…");
      await page.getByRole("menuitemradio", { name: /^Team handbook/ }).click();
      const chip = composer.locator(".composer-chip", {
        has: page.getByRole("button", { name: "Workspace: Team handbook" }),
      });
      const note = composer.getByText("In use: your task will wait for it.");
      await expect(note).toBeVisible();
      const chipBox = await chip.boundingBox();
      const noteBox = await note.boundingBox();
      if (!chipBox || !noteBox) throw new Error("Both must be visible");
      expect(noteBox.y - (chipBox.y + chipBox.height)).toBeGreaterThanOrEqual(
        0,
      );
      expect(noteBox.y - (chipBox.y + chipBox.height)).toBeLessThanOrEqual(8);
      expect(Math.abs(noteBox.x - chipBox.x)).toBeLessThanOrEqual(16);
      expect(noteBox.x + noteBox.width).toBeLessThanOrEqual(width);
      await capture(page, `narrow-chip-${width}-${theme}`);
    });

// A task's header has the same pickers, beside its status, Files icon and ⋯.
for (const width of [1280, 390])
  test(`WEB-U-106 a task header's pickers at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    // With the Files panel closed (beside it, see below).
    const files = page
      .getByRole("region", { name: "Task header" })
      .getByRole("button", { name: "Files" });
    if ((await files.getAttribute("aria-expanded")) === "true")
      await files.click();
    await expect(files).toHaveAttribute("aria-expanded", "false");
    // WEB-U-154 (a screen review before 17.0): on a phone a task's title comes first, whole, and its model and
    // environment take a quiet line under it, with their names.
    const { header, model } = await pickers(page, width, true);
    const title = header.getByRole("heading", { level: 1 });
    expect(
      await title.evaluate(
        (heading) => heading.scrollWidth <= heading.clientWidth,
      ),
    ).toBe(true);
    if (width < 768) {
      const [titleBox, modelBox] = await Promise.all([
        title.boundingBox(),
        model.boundingBox(),
      ]);
      if (!titleBox || !modelBox)
        throw new Error("The header must be laid out");
      expect(modelBox.y).toBeGreaterThanOrEqual(
        titleBox.y + titleBox.height - 1,
      );
    }
    await capture(page, `narrow-task-${width}`);
  });

// WEB-U-106: at 390 a title too long for the first line truncates, and the status, the Files
// icon and ⋯ stay beside it, never on a line of their own; the pickers keep the line under it.
test("WEB-U-106 a long task title at 390 truncates beside ⋯", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": crypto.randomUUID() },
    data: {
      title: "A quarterly report on every region's figures, with notes",
      model_route: "local",
    },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${id}`);
  const header = page.getByRole("region", { name: "Task header" });
  const title = header.getByRole("heading", { level: 1 });
  const more = header.getByRole("button", { name: /^More actions for / });
  const status = page.getByLabel("Run status");
  await expect(more).toBeVisible();
  expect(
    await title.evaluate(
      (heading) => heading.scrollWidth > heading.clientWidth,
    ),
  ).toBe(true);
  const [titleBox, moreBox, statusBox, modelBox] = await Promise.all([
    title.boundingBox(),
    more.boundingBox(),
    status.boundingBox(),
    header.getByRole("button", { name: /^Model: / }).boundingBox(),
  ]);
  if (!titleBox || !moreBox || !statusBox || !modelBox)
    throw new Error("The header must be laid out");
  // One row: ⋯ and the status overlap the title's line; the pickers sit under it.
  for (const box of [moreBox, statusBox]) {
    expect(box.y).toBeLessThan(titleBox.y + titleBox.height);
    expect(box.y + box.height).toBeGreaterThan(titleBox.y);
  }
  expect(modelBox.y).toBeGreaterThanOrEqual(titleBox.y + titleBox.height - 1);
  expect(moreBox.x + moreBox.width).toBeLessThanOrEqual(390);
  await capture(page, "narrow-task-long-title-390");
});

// WEB-U-125 (the lead's F6 note): beside the Files panel at 1280 the names still fit, so they show, whole.
test("WEB-U-125 a task header beside the Files panel at 1280 keeps its names", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await signIn(page);
  await page.goto(await createConversation(page));
  const header = page.getByRole("region", { name: "Task header" });
  const files = header.getByRole("button", { name: "Files" });
  if ((await files.getAttribute("aria-expanded")) !== "true")
    await files.click();
  await expect(files).toHaveAttribute("aria-expanded", "true");
  for (const name of [/^Model: /, /^Environment: /]) {
    const picker = header.getByRole("button", { name });
    await expect(picker.locator(".picker-name")).toBeVisible();
    await expect(picker.locator(".picker-icon")).toBeHidden();
  }
  expect(await cut(header)).toBe(false);
  const more = header.getByRole("button", { name: /^More actions for / });
  const moreBox = await more.boundingBox();
  const headerBox = await header.boundingBox();
  if (!moreBox || !headerBox) throw new Error("Both must be visible");
  expect(moreBox.x + moreBox.width).toBeLessThanOrEqual(
    headerBox.x + headerBox.width,
  );
  await capture(page, "narrow-task-files-1280");
});

// WEB-U-154 (a screen review before 17.0): a touch keyboard has no Shift+Enter, so a phone isn't told to use it.
test.describe("on a touch screen", () => {
  test.use({
    isMobile: true,
    hasTouch: true,
    viewport: { width: 390, height: 800 },
  });
  test("WEB-U-154 the composer gives no keyboard hint", async ({ page }) => {
    await signIn(page);
    await page.goto(await createConversation(page));
    await expect(page.getByRole("textbox", { name: "Message" })).toBeVisible();
    await expect(page.getByText(/Shift\+Enter/)).toHaveCount(0);
  });
});

import { randomUUID } from "node:crypto";

import { signIn } from "./session";
import { expect, test, type Page } from "./test";
import { auditView } from "./ux/polish-audit";

// WEB-U-021 for the sidebar on its own (the workspaces audit of UI-GUIDELINES §5b found its paddings off the scale):
// the expanded sidebar, the rail and the phone drawer, with enough tasks for Show more and the Search row, keep every
// padding, gap and radius on the scale; and its navigation rows are 32 px with a mouse, 44 px on a touch screen (§5c).
async function tasks(page: Page) {
  await signIn(page);
  for (let n = 1; n <= 10; n += 1) {
    const made = await page.request.post("/api/v1/conversations", {
      headers: { "Idempotency-Key": randomUUID() },
      data: { title: `Sidebar audit task ${n}`, model_route: "local" },
    });
    expect(made.status()).toBe(201);
  }
  await page.goto("/");
}
/** The list inside `root` is loaded and long enough to offer Show more and the Search row. */
async function loaded(page: Page, root: string) {
  await expect(
    page.locator(root).getByRole("button", { name: "Show more" }).first(),
  ).toBeVisible();
}
/** The heights of the navigation rows inside `root`: the nav links and the task rows. */
async function rowHeights(page: Page, root: string) {
  return page
    .locator(`${root} .nav-item, ${root} .conversation-link`)
    .evaluateAll((rows) =>
      rows
        .filter((row) => row.checkVisibility())
        .map((row) => Math.round(row.getBoundingClientRect().height)),
    );
}

for (const width of [1024, 1440, 1920])
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-021 sidebar audit ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      // Below 1440 the sidebar may start as the rail. Audit it expanded, as a person who chose it: the setting is in
      // place before the page loads. (Clicking Expand once if visible raced the page's hydration: before it, the
      // server's expanded toggle shows; after, the rail's. The 3 Oct 2026 hand-in caught it at 1024 dark.)
      await page.addInitScript(() => {
        localStorage.setItem("tensorquay-sidebar", "expanded");
      });
      await tasks(page);
      const sidebar = ".app-sidebar";
      await loaded(page, sidebar);
      expect(await auditView(page, `sidebar ${width}`, sidebar)).toEqual([]);
      const heights = await rowHeights(page, sidebar);
      expect(heights.length).toBeGreaterThan(5);
      expect(new Set(heights)).toEqual(new Set([32]));
      await page.getByRole("button", { name: "Collapse sidebar" }).click();
      expect(await auditView(page, `rail ${width}`, sidebar)).toEqual([]);
    });

test("WEB-U-021 sidebar audit: the phone drawer", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await tasks(page);
  await page.getByRole("button", { name: "Open navigation" }).click();
  await expect(page.getByRole("dialog", { name: "Navigation" })).toBeVisible();
  await loaded(page, ".navigation-drawer");
  expect(await auditView(page, "drawer 390", ".navigation-drawer")).toEqual([]);
});

test("WEB-U-080 navigation rows are 44 px on a touch screen", async ({
  browser,
  baseURL,
}) => {
  const touch = await browser.newContext({
    ...(baseURL ? { baseURL } : {}),
    hasTouch: true,
    isMobile: true,
    viewport: { width: 390, height: 844 },
  });
  const page = await touch.newPage();
  await tasks(page);
  await page.getByRole("button", { name: "Open navigation" }).click();
  await loaded(page, ".navigation-drawer");
  const heights = await rowHeights(page, ".navigation-drawer");
  expect(heights.length).toBeGreaterThan(5);
  expect(new Set(heights)).toEqual(new Set([44]));
  await touch.close();
});

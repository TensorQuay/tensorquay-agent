import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { signIn } from "./session";
import { expect, resetFront, test, type Locator, type Page } from "./test";

/** The sidebar (or the drawer) for the reviewers, at the page's 2x scale, once its finite motion has ended. */
async function shot(area: Locator, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await area.evaluate(() =>
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
  await area.screenshot({ path: join(directory, `${name}.png`) });
}

async function tasks(page: Page) {
  await resetFront(page.request, ["agent-teams"]);
  await signIn(page);
  let open = "";
  for (const title of ["Weekly sales summary", "Draft the onboarding guide"]) {
    const made = await page.request.post("/api/v1/conversations", {
      headers: { "Idempotency-Key": randomUUID() },
      data: { title, model_route: "local" },
    });
    expect(made.status()).toBe(201);
    open ||= ((await made.json()) as { id: string }).id;
  }
  return open;
}

/** The sidebar's parts, laid out as the browser draws them, relative to the area's left edge. */
function parts(area: Locator) {
  return area.evaluate((root) => {
    const edge = root.getBoundingClientRect().left;
    const box = (node: Element | null) => {
      const rect = node?.getBoundingClientRect();
      return rect
        ? {
            left: rect.left - edge,
            right: rect.right - edge,
            top: rect.top,
            bottom: rect.bottom,
            centre: rect.left - edge + rect.width / 2,
          }
        : null;
    };
    const items = [
      ...root.querySelectorAll<HTMLElement>(
        ".navigation > .nav-item, .navigation > .nav-items > .nav-item, .sidebar-footer .nav-item",
      ),
    ];
    const label = root.querySelector(".task-group .sidebar-label");
    const labels = [...root.querySelectorAll(".task-group")];
    const normal = [...root.querySelectorAll(".conversation-row")].find(
      (row) => !row.querySelector(".task-helpers-toggle"),
    );
    const lead = [...root.querySelectorAll(".conversation-row")].find((row) =>
      row.querySelector(".task-helpers-toggle"),
    );
    return {
      names: items.map((item) => item.textContent?.trim() ?? ""),
      titled: items
        .filter((item) => item.hasAttribute("title"))
        .map((item) => item.textContent?.trim()),
      brandTitled: root.querySelector(".product-name")?.hasAttribute("title"),
      mark: box(root.querySelector(".brand-mark")),
      brand: box(root.querySelector(".sidebar-brand")),
      icons: items.map((item) => box(item.querySelector("svg"))),
      words: items.map((item) => box(item.querySelector(".nav-label"))),
      rows: items.map((item) => box(item)),
      label: label
        ? (box(label)?.left ?? 0) +
          parseFloat(getComputedStyle(label).paddingInlineStart)
        : null,
      labelTop: box(label)?.top ?? null,
      groups: labels.map((group) => box(group)),
      title: box(normal?.querySelector(".task-title") ?? null),
      leadTitle: box(lead?.querySelector(".task-title") ?? null),
      chevron: box(lead?.querySelector(".task-helpers-toggle svg") ?? null),
      toggle: box(root.querySelector(".sidebar-toggle")),
      avatar: box(root.querySelector(".account-avatar")),
    };
  });
}

const near = (a: number | undefined, b: number | undefined, by = 1) =>
  Math.abs((a ?? NaN) - (b ?? NaN)) <= by;

// WEB-E2E-115 (UI-GUIDELINES §6; decided on 9 Nov 2026): the sidebar's top block is New task, Workspaces and Channels,
// in that order and in the same row style, then the task groups; the bottom keeps only the account. One left edge for
// the logo, every item's icon, the group labels and the task titles (a lead's chevron, its title just after it); the
// rows at the scale's height, 8 px under the header, one group gap before the first group and between groups; the toggle on
// the ⋯ column. No native tooltip where a label shows; on the rail its icons share one axis and the app's own tooltip
// names each item.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-115 the sidebar's top block and one left edge at 1280 ${colorScheme}`, async ({
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
    const open = await tasks(page);
    await page.goto(`/c/${open}`);
    const sidebar = page.locator(".app-sidebar");
    await expect(sidebar.locator(".conversation-row").first()).toBeVisible();
    await page.mouse.move(900, 600);
    await shot(sidebar, `sidebar-expanded-${colorScheme}`);
    const at = await parts(sidebar);
    expect.soft(at.names).toEqual(["New task", "Workspaces", "Channels"]);
    expect.soft(at.titled).toEqual([]);
    expect.soft(at.brandTitled).toBe(false);
    // The top block comes before the task groups, one row after another, 2 px apart.
    const [first, second, third] = at.rows;
    expect.soft((third?.bottom ?? Infinity) <= (at.labelTop ?? 0)).toBe(true);
    expect.soft(near((second?.top ?? 0) - (first?.bottom ?? 0), 2)).toBe(true);
    expect.soft(near((third?.top ?? 0) - (second?.bottom ?? 0), 2)).toBe(true);
    for (const row of at.rows)
      expect.soft(near((row?.bottom ?? 0) - (row?.top ?? 0), 36)).toBe(true);
    // One left edge: the logo, each item's icon and the group labels.
    for (const icon of at.icons)
      expect.soft(near(icon?.left, at.mark?.left)).toBe(true);
    expect.soft(near(at.label ?? undefined, at.mark?.left)).toBe(true);
    // The task titles start on the same edge, with no icon column; a lead's chevron icon too, its title just after it.
    expect.soft(near(at.title?.left, at.mark?.left)).toBe(true);
    expect.soft(near(at.chevron?.left, at.mark?.left)).toBe(true);
    expect
      .soft(near((at.leadTitle?.left ?? 0) - (at.chevron?.right ?? 0), 4))
      .toBe(true);
    // 8 px under the header; one group gap (16 px) before the first group and between groups.
    expect
      .soft(near((first?.top ?? 0) - (at.brand?.bottom ?? 0), 8))
      .toBe(true);
    const before = (at.labelTop ?? 0) - (third?.bottom ?? 0);
    expect.soft(before >= 15 && before <= 21).toBe(true);
    // The toggle sits on the ⋯ column at the rows' end.
    const row = sidebar.locator(".conversation-row").first();
    await row.hover();
    const more = await row
      .locator(":scope > .task-more > .button")
      .evaluate((button) => {
        const rect = button.getBoundingClientRect();
        const side = button.closest(".app-sidebar")?.getBoundingClientRect();
        return rect.left - (side?.left ?? 0) + rect.width / 2;
      });
    expect.soft(near(at.toggle?.centre, more)).toBe(true);

    // The rail: the icons on one axis, each named by the app's own tooltip.
    await page.getByRole("button", { name: /^Collapse sidebar/ }).click();
    await expect(sidebar).toHaveCSS("width", "56px");
    await page.mouse.move(900, 600);
    const rail = await parts(sidebar);
    for (const icon of rail.icons)
      expect.soft(near(icon?.centre, rail.mark?.centre)).toBe(true);
    expect.soft(near(rail.avatar?.centre, rail.mark?.centre)).toBe(true);
    await sidebar.getByRole("link", { name: "Workspaces" }).hover();
    const tip = page.locator(".action-tooltip", { hasText: "Workspaces" });
    await expect.soft(tip).toBeVisible();
    await shot(page.locator("body"), `sidebar-rail-${colorScheme}`);
    await context.close();
  });

// WEB-E2E-115 in the phone drawer: the same top block; rows at least 44 px; the current row's highlight across the
// row, inset as on the desktop; the account row at the foot opens the account menu.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-115 the drawer's top block at 390 ${colorScheme}`, async ({
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
    const open = await tasks(page);
    await page.goto(`/c/${open}`);
    await page.getByRole("button", { name: "Open navigation" }).click();
    const drawer = page.getByRole("dialog", { name: "Navigation" });
    await expect(drawer.locator(".conversation-row").first()).toBeVisible();
    await shot(drawer, `drawer-${colorScheme}`);
    const at = await parts(drawer);
    expect.soft(at.names).toEqual(["New task", "Workspaces", "Channels"]);
    expect.soft(at.titled).toEqual([]);
    for (const row of at.rows)
      expect
        .soft((row?.bottom ?? 0) - (row?.top ?? 0))
        .toBeGreaterThanOrEqual(44);
    expect.soft(near(at.title?.left, at.icons[0]?.left)).toBe(true);
    const current = await drawer
      .locator(".conversation-row:has([aria-current='page'])")
      .evaluate((row) => {
        // Inside the drawer's border, as the sidebar's rows are measured inside its own.
        const sheet = row.closest("dialog");
        const dialog = sheet?.getBoundingClientRect();
        const style = sheet ? getComputedStyle(sheet) : null;
        const rect = row.getBoundingClientRect();
        return {
          start:
            rect.left -
            (dialog?.left ?? 0) -
            parseFloat(style?.borderLeftWidth ?? "0"),
          end:
            (dialog?.right ?? 0) -
            parseFloat(style?.borderRightWidth ?? "0") -
            rect.right,
        };
      });
    expect.soft(current.start).toBe(12);
    expect.soft(current.end).toBe(12);
    await drawer.getByRole("button", { name: "Account", exact: true }).click();
    await expect(page.getByRole("menu", { name: "Account" })).toBeVisible();
    await shot(drawer, `drawer-account-${colorScheme}`);
    await context.close();
  });

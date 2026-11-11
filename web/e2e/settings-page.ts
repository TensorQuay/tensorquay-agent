import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, type Locator, type Page } from "@playwright/test";

import { settled } from "./motion";

/** The app's tables, each with what its rows hold (a person's tasks are added once their id is known). */
export const TABLE_PAGES = [
  "/settings/keys",
  "/settings/keys/shared",
  "/settings/agent-profiles",
  "/settings/people",
  "/settings/models",
  "/settings/environments",
  "/settings/connectors",
  "/settings/decision-engines",
  "/settings/internet-access",
  "/settings/limits",
  "/settings/usage",
  "/settings/tokens",
  "/workspaces",
] as const;

/** Evidence for an admin settings page, light or dark as the test set it; never a loading skeleton. */
export async function capture(
  page: Page,
  name: string,
  { fullPage = false }: { fullPage?: boolean } = {},
) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `${name}.png`), fullPage });
}

export async function axe(page: Page) {
  await settled(page);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
}

/**
 * Every relative time on the page drawn. One shows "…" until its first frame ("just now" after), and a table measured
 * before then is narrower than the person sees it: wait for this before reading a table's widths.
 */
export async function timesShown(page: Page) {
  await page.waitForFunction(() =>
    [...document.querySelectorAll("time")].every(
      (time) => time.textContent !== "…",
    ),
  );
}

/** UI-GUIDELINES §6: the table fits the settings column, so every row's delete action is on screen without scrolling. */
export async function fitsColumn(page: Page, deleteName: RegExp) {
  const scroll = page.locator(".data-table-scroll");
  expect(
    await scroll.evaluate(
      (element) => element.scrollWidth - element.clientWidth,
    ),
    "the table fits its column",
  ).toBe(0);
  // Horizontally: a long table may scroll the page down, but no row's action may sit beyond the column's edge.
  const column = await scroll.boundingBox();
  if (!column) throw new Error("The table must be visible");
  for (const button of await page
    .getByRole("button", { name: deleteName })
    .all()) {
    const box = await button.boundingBox();
    if (!box) throw new Error("Each row's action must be visible");
    expect(box.x).toBeGreaterThanOrEqual(column.x);
    expect(box.x + box.width).toBeLessThanOrEqual(column.x + column.width);
  }
}

/** Types into a dialog field by keyboard, replacing what it held. */
export async function type(
  page: Page,
  label: string,
  text: string,
  /** Where the field is: the open dialog unless a page's form is given (the UI standard's Page). */
  scope: Locator = page.getByRole("dialog"),
) {
  const field = scope.getByLabel(label, { exact: true });
  await field.focus();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type(text);
}

/** Chooses one of a row's actions from its "…" menu by keyboard (UI-GUIDELINES §7): Enter on the menu, then the item. */
export async function rowMenu(page: Page, menu: string, item: string) {
  await page.getByRole("button", { name: menu }).focus();
  await page.keyboard.press("Enter");
  const choice = page.getByRole("menuitem", { name: item });
  await choice.focus();
  await page.keyboard.press("Enter");
}

/** Adds a shared key from its dialog by keyboard: Add a shared key, Name, Value, then Add key. */
export async function addSharedKey(page: Page, name: string, value: string) {
  await page.getByRole("button", { name: "Add a shared key" }).focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Add a shared key" });
  await dialog.getByLabel("Name", { exact: true }).fill(name);
  await dialog.getByLabel("Value", { exact: true }).fill(value);
  await dialog.getByRole("button", { name: "Add key" }).press("Enter");
  await expect(dialog).toHaveCount(0);
}

/**
 * The moment every Settings table walk is drawn at (WEB-E2E-119, WEB-E2E-120), in the form of the fake front's own
 * dates: never today, so a date or a relative time in a row is as wide on any day as when the widths were measured.
 */
export const WALK_NOW = Date.parse("2026-11-10T09:00:00Z");
/** The zone the walks draw in, whatever the host's (CI draws in UTC): a time of day is the same hour everywhere. */
export const WALK_ZONE = "UTC";
const HOUR_MS = 3_600_000;
const STAMP =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})$/;

/**
 * An API answer with each time the fake front took from its own clock (a token made in the walk, its expiry, an
 * invitation's, a key's last use an hour ago) moved to the same distance from `WALK_NOW`, to the hour, however long
 * the walk has run; the fixture's own dates, weeks older, stay as written.
 */
export function pinStamps<T>(answer: T, fetched = Date.now()): T {
  return JSON.parse(JSON.stringify(answer), (_key, value: unknown) => {
    if (typeof value !== "string" || !STAMP.test(value)) return value;
    const offset = Date.parse(value) - fetched;
    if (offset < -48 * HOUR_MS) return value;
    return new Date(
      WALK_NOW + Math.round(offset / HOUR_MS) * HOUR_MS,
    ).toISOString();
  }) as T;
}

/** The page's clock at `WALK_NOW`, and every API answer it reads with its times pinned (`pinStamps`). */
export async function pinTime(page: Page) {
  await page.clock.setFixedTime(WALK_NOW);
  await page.route(
    (url) => url.pathname.startsWith("/api/v1/"),
    async (route) => {
      if (route.request().method() !== "GET") return route.fallback();
      // A connection of its own: a kept-alive one the fake front has just closed as idle hangs up (seen once).
      const response = await route.fetch({
        headers: { ...route.request().headers(), connection: "close" },
      });
      if (!(response.headers()["content-type"] ?? "").includes("json"))
        return route.fulfill({ response });
      await route.fulfill({
        response,
        json: pinStamps((await response.json()) as unknown),
      });
    },
  );
}

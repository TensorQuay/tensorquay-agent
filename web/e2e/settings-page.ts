import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, type Locator, type Page } from "@playwright/test";

import { settled } from "./motion";

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

/** Adds a shared key from its dialog by keyboard (K2): Add a shared key, Name, Value, then Add key. */
export async function addSharedKey(page: Page, name: string, value: string) {
  await page.getByRole("button", { name: "Add a shared key" }).focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Add a shared key" });
  await dialog.getByLabel("Name", { exact: true }).fill(name);
  await dialog.getByLabel("Value", { exact: true }).fill(value);
  await dialog.getByRole("button", { name: "Add key" }).press("Enter");
  await expect(dialog).toHaveCount(0);
}

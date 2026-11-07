import { expect, type Locator, type Page } from "@playwright/test";

/** Waits for the page's model and environment pickers in its header (there is no Change panel to open). */
export async function openChoices(page: Page) {
  // A task's and New task's model and environment are in the header (WEB-U-113): nothing to open.
  await expect(
    page.getByRole("group", { name: "Model and environment" }),
  ).toBeVisible();
}

/** The model a task (or New task) will use, read from its header's picker. */
export async function expectModel(page: Page, name: string) {
  await openChoices(page);
  await expect(page.getByRole("button", { name: /^Model:/ })).toHaveText(name);
  await closeChoices(page);
}

/** Nothing to close: the choices sit in the header (kept for the specs that call it). */
export async function closeChoices(page: Page) {
  await expect(
    page.getByRole("group", { name: "Model and environment" }),
  ).toBeVisible();
}

/**
 * Adds a workspace or an agent profile from New task's "+" (the UI standard's composer): its chip appears with its
 * picker open, ready for a choice.
 */
export async function addFromPlus(
  page: Page,
  item: "Workspace…" | "Agent profile…",
) {
  const composer = page.getByRole("form", { name: "Message composer" });
  await composer.getByRole("button", { name: "Add" }).click();
  await page
    .getByRole("menu", { name: "Add" })
    .getByRole("menuitem", { name: item })
    .click();
  await expect(
    page.getByRole("menu", {
      name: item === "Workspace…" ? "Workspace" : /^Agent/,
    }),
  ).toBeVisible();
}

const escaped = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * A form's standard picker (the shared Select, WEB-U-134) by its label, in `scope` or the page. Its name is menu.current
 * in the page's language: "Label: choice", "语言：English" (a full-width colon) or "Thème : Clair" (a space before it).
 */
export function picker(page: Page, label: string, scope?: Locator) {
  return (scope ?? page).getByRole("button", {
    name: new RegExp(`^${escaped(label)} ?[:：] ?`),
  });
}

/** Chooses in a form's standard picker by the words its choice starts with (or a pattern). */
export async function pick(
  page: Page,
  label: string,
  option: string | RegExp,
  scope?: Locator,
) {
  await picker(page, label, scope).click();
  await page
    .getByRole("menuitemradio", {
      name:
        typeof option === "string" ? new RegExp(`^${escaped(option)}`) : option,
    })
    .click();
}

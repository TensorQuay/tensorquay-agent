import { expect, type Page } from "@playwright/test";

import { closeChoices, openChoices } from "./choices";

/** Selects a catalogue display name through the keyboard-operable conversation menu. */
export async function chooseEnvironment(page: Page, label: string) {
  await openChoices(page);
  await page.getByRole("button", { name: /^Environment:/ }).click();
  await page
    .getByRole("menuitemradio")
    .filter({
      has: page.getByText(label, { exact: true }),
    })
    .click();
}

/** A switch is only requested after the inline consequence is acknowledged. */
export async function confirmEnvironment(page: Page, label: string) {
  const confirmation = page.getByRole("group", { name: "Switch environment" });
  await expect(confirmation).toContainText(
    "Files and history stay; anything still running stops",
  );
  await confirmation
    .getByRole("button", { name: `Switch to ${label}` })
    .click();
  await expect(confirmation).toHaveCount(0);
  // The new environment shows in its picker (the header's, WEB-U-113).
  await openChoices(page);
  await expect(
    page.getByRole("button", { name: `Environment: ${label}` }),
  ).toBeVisible();
  await closeChoices(page);
}

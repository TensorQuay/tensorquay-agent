import { expect, type Locator } from "@playwright/test";

/**
 * Opens a tool line's card unless it is open already: the latest tool's card opens with its group (finding T2), and
 * pressing it again would close it. Returns once the card is open.
 */
export async function openCard(line: Locator) {
  if ((await line.getAttribute("aria-expanded")) !== "true") await line.click();
  await expect(line).toHaveAttribute("aria-expanded", "true");
}

import { expect, type Page } from "@playwright/test";

import { modelName } from "../../src/conversations/model-name";
import { closeChoices, openChoices } from "../choices";

/** Inspect application-owned copy separately from arbitrary user messages and tool output. */
export async function humanChrome(page: Page) {
  await expect(
    page.getByRole("navigation").locator("[disabled], [aria-disabled=true]"),
  ).toHaveCount(0);
  const bar = page.getByRole("region", { name: "Task header" });
  // The header holds the title, the status and the model and environment pickers; the composer none (WEB-U-113).
  await expect(bar.getByRole("button", { name: /^Model:/ })).toHaveCount(1);
  await expect(
    page
      .getByRole("form", { name: "Message composer" })
      .getByRole("button", { name: /^Model:/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("form", { name: "Message composer" }).getByRole("combobox"),
  ).toHaveCount(0);
  const title = await bar.getByRole("heading").innerText();
  await expect(page).toHaveTitle(`${title} · TensorQuay Agent`);
  const visible = await page
    .locator(
      ".app-sidebar, .conversation-bar, .environment-status, [role=alert]",
    )
    .evaluateAll((elements) =>
      elements
        .filter((element) => element.checkVisibility())
        .map((element) => (element as HTMLElement).innerText)
        .join("\n"),
    );
  expect(
    visible,
    "copy: application chrome must not expose internal state words",
  ).not.toMatch(/\b(sandbox|idle)\b/i);
  await openChoices(page);
  const choice = page.getByRole("button", { name: /^Model:/ });
  const response = await page.request.get("/api/v1/models");
  expect(response.status()).toBe(200);
  const models = (await response.json()) as {
    items: { name: string; model: string; display_name?: string }[];
  };
  const label = await choice.innerText();
  const model = models.items.find((model) => modelName(model) === label);
  expect(
    model,
    "copy: selected model exists in the public catalogue",
  ).toBeDefined();
  await expect(choice).toHaveText(model ? modelName(model) : "");
  await closeChoices(page);
  await expect(
    page.getByRole("button", { name: /Jump to latest.*0 new/ }),
  ).toHaveCount(0);
}

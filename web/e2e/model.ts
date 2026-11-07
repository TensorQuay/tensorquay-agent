import { expect, type Page } from "@playwright/test";

import { modelName } from "../src/conversations/model-name";

import { openChoices } from "./choices";

/** Selects a public catalogue choice by its displayed name, preserving route ids only on the wire. */
export async function chooseModel(page: Page, route: string) {
  const response = await page.request.get("/api/v1/models");
  expect(response.status()).toBe(200);
  const catalogue = (await response.json()) as {
    items: { name: string; model: string; display_name?: string }[];
  };
  const model = catalogue.items.find((item) => item.name === route);
  if (!model) throw new Error("Requested model is absent from the catalogue");
  await openChoices(page);
  await page.getByRole("button", { name: /^Model:/ }).click();
  await page
    // The row's name starts with the model's; its muted second line follows (WEB-U-103).
    .getByRole("menuitemradio", {
      name: new RegExp(
        `^${modelName(model).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![a-z0-9])`,
      ),
    })
    .click();
  await expect(page.getByRole("button", { name: /^Model:/ })).toHaveText(
    modelName(model),
  );
}

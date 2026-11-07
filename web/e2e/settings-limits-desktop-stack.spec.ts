import { readFileSync } from "node:fs";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { signedIn } from "./ux/flow";

// WEB-E2E-067 (U12; backs up WEB-E2E-037's Limits case, WEB-U-280 and GW-U-146): Settings › Limits › Desktop against
// the real gateway, which needs no Desktop environment to show them. The section lists the gateway's own defaults
// (config/desktop.rs, tqa-types screen.rs): 2 Desktop environments at once, screenshots kept 30 days, and per run 200
// screenshots and 1 000 actions, the last two saying when a change applies (the hint's words from the catalogue). A
// Desktop count above Tasks running at once (this stack runs 4) is refused in words, and nothing changes.

test.skip(!process.env.TQA_STACK_URL, "Requires the owned settings stack");
test.describe.configure({ mode: "serial" });

const en = JSON.parse(readFileSync("src/i18n/locales/en.json", "utf8")) as {
  messages: Record<string, string>;
};

test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await signedIn(page);
  } finally {
    await page.close();
  }
});

async function serious(page: Page) {
  const { violations } = await new AxeBuilder({ page }).analyze();
  return violations
    .filter((violation) =>
      ["serious", "critical"].includes(violation.impact ?? ""),
    )
    .map((violation) => violation.id);
}

test("WEB-E2E-067 Settings › Limits › Desktop on the real gateway", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await signedIn(page);
  await page.goto("/settings/limits");
  const desktop = page.getByRole("table", { name: "Desktop limits" });
  const sessions = desktop.getByRole("row", {
    name: /Desktop environments at once/,
  });
  await expect(sessions).toContainText("2 tasks");
  await expect(
    desktop.getByRole("row", { name: /Keep action screenshots for/ }),
  ).toContainText("30 days");
  const applies = en.messages["limits.appliesNextDesktop"] ?? "";
  expect(applies).not.toBe("");
  const screenshots = desktop.getByRole("row", {
    name: /Screenshots a run may send the model/,
  });
  await expect(screenshots).toContainText("200 screenshots");
  await expect(screenshots).toContainText(applies);
  const actions = desktop.getByRole("row", {
    name: /Desktop actions a run may take/,
  });
  await expect(actions).toContainText(/1.000 actions/);
  await expect(actions).toContainText(applies);
  expect(await serious(page)).toEqual([]);

  // More Desktop environments than tasks running at once: refused in words with the slots' number (the gateway lists
  // this limit's max as the slots, 4 on the settings stack), nothing is sent, and it keeps 2.
  await page
    .getByRole("button", { name: "Edit Desktop environments at once" })
    .click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("spinbutton").fill("9");
  await dialog.getByRole("button", { name: "Save limit" }).click();
  await expect(
    dialog.getByText(
      "Desktop environments at once can't be more than Tasks running at once, which is 4 now.",
    ),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.reload();
  await expect(sessions).toContainText("2 tasks");
});

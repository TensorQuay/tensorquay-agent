import { randomUUID } from "node:crypto";

import { signIn } from "./session";
import { axe, capture, fitsColumn, type } from "./settings-page";
import { expect, test, type Page } from "./test";

// The Egress page on the allow-list admin API (slice 4, ADR-0028; WEB-U-040 browser part), keyboard only, at the
// widths UI-GUIDELINES §17.2 asks for in both themes. Each run adds and deletes its own rule in the shared fake store.
const EGRESS = "/api/v1/admin/egress";

async function addRule(page: Page, host: string) {
  await page.getByRole("button", { name: "Add a site" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog", { name: "Add a rule" })).toBeVisible();
  await type(page, "Host", host.toUpperCase());
  await type(page, "Ports", "8443, 0");
  await page.keyboard.press("Enter");
  await expect(
    page
      .getByRole("dialog")
      .getByText(
        "Enter 1 to 16 different ports from 1 to 65535, separated by commas.",
      ),
  ).toBeVisible();
  await type(page, "Ports", "8443, 443");
  await axe(page);
  await page.keyboard.press("Enter");
  await expect(page.getByText(`Saved rule ${host}.`)).toBeVisible();
  await expect(page.getByRole("row", { name: new RegExp(host) })).toContainText(
    "443, 8443",
  );
}

/** Another administrator changes the rule's ports through the API, so the page's version is stale. */
async function changeElsewhere(page: Page, host: string) {
  const changed = await page.request.put(`${EGRESS}/${host}`, {
    headers: { "Idempotency-Key": randomUUID() },
    data: { ports: [443], version: 1 },
  });
  expect(changed.status()).toBe(200);
}

async function editAfterConflict(page: Page, host: string) {
  await page.getByRole("button", { name: `Edit rule ${host}` }).click();
  await type(page, "Ports", "443, 9443");
  await changeElsewhere(page, host);
  const save = page.getByRole("button", { name: "Save rule" });
  await save.click();
  await expect(
    page.getByText(
      "Someone changed this rule since you opened it. Your edits are kept: save again to replace their change with yours.",
    ),
  ).toBeVisible();
  await save.click();
  await expect(page.getByText(`Saved rule ${host}.`)).toBeVisible();
  await expect(page.getByRole("row", { name: new RegExp(host) })).toContainText(
    "443, 9443",
  );
}

async function deleteRule(page: Page, host: string) {
  // WEB-U-162: Delete is under the row's ⋯, as a danger item.
  await page
    .getByRole("button", { name: `More actions for rule ${host}` })
    .click();
  await page.getByRole("menuitem", { name: `Delete rule ${host}` }).click();
  const dialog = page.getByRole("dialog", { name: `Delete rule ${host}?` });
  await expect(dialog).toContainText(
    "Environments' next connections to this site will be refused.",
  );
  await dialog.getByRole("button", { name: "Delete rule" }).click();
  await expect(page.getByText(`Deleted rule ${host}.`)).toBeVisible();
  await expect(page.getByRole("row", { name: new RegExp(host) })).toHaveCount(
    0,
  );
}

for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-040 egress rules for administrators ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const host = "status.example.com";
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/internet-access");
      await expect(page).toHaveTitle("Internet access · TensorQuay Agent");
      await expect(
        page.getByRole("row", { name: /api\.example\.com/ }),
      ).toContainText("Configuration file");
      await fitsColumn(page, /^More actions for rule /);
      await axe(page);
      await capture(page, `egress-list-${width}-${colorScheme}`);
      await addRule(page, host);
      await editAfterConflict(page, host);
      await fitsColumn(page, /^More actions for rule /);
      await capture(page, `egress-edited-${width}-${colorScheme}`);
      await deleteRule(page, host);
      await axe(page);
    });

test("WEB-U-040 members see no Egress section", async ({ page }) => {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
  await page.goto("/settings/internet-access");
  await expect(
    page.getByText("Only administrators can manage internet access."),
  ).toBeVisible();
  await expect(
    page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Egress" }),
  ).toHaveCount(0);
});

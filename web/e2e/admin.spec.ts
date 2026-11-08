import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { pick } from "./choices";
import { settled } from "./motion";
import { signIn } from "./session";
import { addSharedKey } from "./settings-page";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `admin-${name}.png`) });
}
async function serious(page: Page) {
  await settled(page);
  const { violations } = await new AxeBuilder({ page }).analyze();
  return violations
    .filter((violation) =>
      ["serious", "critical"].includes(violation.impact ?? ""),
    )
    .map((violation) => violation.id);
}

// FR-22's runtime backend status on the fake front: the Status section for administrators (WEB-U-033).
for (const [width, colorScheme] of [
  [1440, "light"],
  [1024, "dark"],
] as const)
  test(`WEB-U-033 status for administrators ${width} ${colorScheme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    await page.goto("/settings/status");
    await expect(page).toHaveTitle("Status · TensorQuay Agent");
    // One line first, the passing checks quietly under it (WEB-U-033).
    await expect(page.getByText("New tasks can start.")).toBeVisible();
    await expect(page.getByText(/^Passing: Database, Key store/)).toBeVisible();
    await expect(
      page.getByText("Environments run as Docker containers."),
    ).toBeVisible();
    expect(await serious(page)).toEqual([]);
    await capture(page, `status-ready-${width}-${colorScheme}`);

    await page.context().addCookies([
      {
        name: "fixture_readiness",
        value: "not-ready",
        url: new URL("/", page.url()).href,
      },
    ]);
    const again = page.getByRole("button", { name: "Check again" });
    await again.focus();
    await page.keyboard.press("Enter");
    await expect(
      page.getByText("New tasks can't start: 1 problem below."),
    ).toBeVisible();
    await expect(
      page.getByRole("region", { name: "Agent image" }),
    ).toContainText("Build or load it there, then check again.");
    // WEB-U-174: the default environment isn't "Available" while new tasks can't start.
    await expect(
      page.getByRole("region", { name: "Default environment" }),
    ).toContainText("Ready once the problem above is fixed");
    await expect(
      page
        .getByRole("region", { name: "Default environment" })
        .getByText("Available", { exact: true }),
    ).toHaveCount(0);
    expect(await serious(page)).toEqual([]);
    await capture(page, `status-not-ready-${width}-${colorScheme}`);
  });

// Token usage for administrators, from the admin usage API (WEB-U-034).
for (const [width, colorScheme] of [
  [1440, "light"],
  [1024, "dark"],
] as const)
  test(`WEB-U-034 usage for administrators ${width} ${colorScheme}`, async ({
    page,
  }) => {
    // The page compares with the period before today's: pin today to the fake usage's day (e2e/front/admin.mjs), so
    // "August" and "July" below hold on any date (guardrails §2.3: no wall clock). Timers still run.
    await page.clock.setFixedTime(new Date("2026-09-26T10:00:00Z"));
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    await page.goto("/settings/usage");
    await expect(page).toHaveTitle("Usage · TensorQuay Agent");
    // The period's total and its change first; people and models ranked; each day only on request (WEB-U-034).
    const total = page.getByRole("region", { name: "Total" });
    await expect(total).toContainText(/ tokens/);
    await expect(total).toContainText("vs the same days of August");
    await expect(
      page.getByRole("table", { name: "Usage by person" }),
    ).toContainText("member@example.invalid");
    await expect(
      page.getByRole("img", { name: /Tokens per day/ }),
    ).toBeVisible();
    // WEB-U-127 (found before 17.0): the chart has its dates and its busiest day in words; numbers are written one way.
    const chart = page.locator("figure.usage-chart");
    await expect(chart.locator(".usage-axis span").first()).toBeVisible();
    await expect(chart.locator("figcaption")).toHaveText(
      /^Tokens per day\. Busiest: /,
    );
    await expect(total).not.toContainText(/\d(\.\d)?[KMB]\b/);
    expect(await serious(page)).toEqual([]);
    await capture(page, `usage-${width}-${colorScheme}`);
    await page.getByRole("button", { name: "Show each day" }).focus();
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("table", { name: "Usage by day" }),
    ).toBeVisible();
    await pick(page, "Period", "Last month");
    await expect(total).toContainText("vs July");
  });

// Shared secrets for administrators, write-only, from the admin secrets API (WEB-U-035).
for (const [width, colorScheme] of [
  [1440, "light"],
  [1024, "dark"],
] as const)
  test(`WEB-U-035 secrets for administrators ${width} ${colorScheme}`, async ({
    page,
  }) => {
    const name = "search-api";
    const value = `fixture-value-${width}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    await page.goto("/settings/keys/shared");
    await expect(page).toHaveTitle("Keys · TensorQuay Agent");
    // In use: Delete stays in the menu, reachable, and says why it can't run (UI-GUIDELINES §5b).
    await page
      .getByRole("button", { name: "More actions for shared key local" })
      .click();
    await expect(
      page.getByRole("menuitem", { name: "Delete shared key local" }),
    ).toHaveAttribute("aria-disabled", "true");
    await page.keyboard.press("Escape");
    expect(await serious(page)).toEqual([]);
    await addSharedKey(page, name, value);
    await expect(page.getByText(`Added shared key ${name}.`)).toBeVisible();
    // WEB-U-181: the list names the key (no `secrets/`), with its full reference on hover, as Models does.
    await expect(
      page.locator(".secret-ref-line", { hasText: new RegExp(`^${name}$`) }),
    ).toHaveAttribute("title", `secrets/${name}`);
    await page
      .getByRole("button", { name: `Replace shared key ${name}` })
      .click();
    const dialog = page.getByRole("dialog", {
      name: `Replace shared key ${name}`,
    });
    expect(await serious(page)).toEqual([]);
    await dialog.getByLabel("New value").fill(`${value}-2`);
    await dialog.getByRole("button", { name: "Replace key" }).click();
    await expect(page.getByText(`Updated shared key ${name}.`)).toBeVisible();
    await page
      .getByRole("button", { name: `More actions for shared key ${name}` })
      .click();
    await page
      .getByRole("menuitem", { name: `Delete shared key ${name}` })
      .click();
    await page
      .getByRole("dialog", { name: `Delete shared key ${name}?` })
      .getByRole("button", { name: "Delete key" })
      .click();
    await expect(page.getByText(`Deleted shared key ${name}.`)).toBeVisible();
    expect(await page.content()).not.toContain(value);
    await capture(page, `secrets-${width}-${colorScheme}`);
  });

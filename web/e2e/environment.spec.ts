import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openChoices } from "./choices";
import { chooseEnvironment, confirmEnvironment } from "./environment";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `${name}.png`) });
}

for (const width of [1024, 1440])
  for (const theme of ["light", "dark"] as const) {
    test(`WEB-U-018 create, confirm and replay an environment switch ${width} ${theme}`, async ({
      page,
    }) => {
      test.setTimeout(60_000);
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme });
      await signIn(page);
      await page.goto("/");
      const suffix = `${width}-${theme}`;
      // The environment is New task's composer picker; its tools read in words on its line, not as icons.
      await openChoices(page);
      const picker = page.getByRole("button", { name: /^Environment:/ });
      await expect(picker).toHaveAccessibleName("Environment: Workstation");
      await expect(picker.getByRole("img")).toHaveCount(0);
      await capture(page, `environment-picker-${suffix}`);
      await picker.press("Enter");
      await expect(
        page.getByRole("menuitemradio", { checked: true }),
      ).toBeFocused();
      await settled(page);
      await capture(page, `environment-menu-${suffix}`);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await page.keyboard.press("Escape");
      await chooseEnvironment(page, "Minimal");
      await expect(
        page.getByRole("group", { name: "Switch environment" }),
      ).toHaveCount(0);
      const creation = page.waitForRequest(
        (request) =>
          request.method() === "POST" &&
          request.url().endsWith("/conversations"),
      );
      await page
        .getByLabel("Message", { exact: true })
        .fill("Wait for cancellation");
      await page.getByLabel("Message", { exact: true }).press("Enter");
      expect((await creation).postDataJSON()).toMatchObject({
        template: "minimal",
      });
      await expect(page).toHaveURL(/\/c\//);
      await expect(page.getByLabel("Run status")).toHaveText(/^Working/);
      await expect(page.getByText(/^Switched to /)).toHaveCount(0);
      await openChoices(page);
      await expect(picker).toHaveAttribute("aria-disabled", "true");
      await picker.focus();
      await expect(picker).toHaveAccessibleDescription(
        "Stop the current run before changing the environment.",
      );
      await capture(page, `environment-busy-${suffix}`);
      // Stop acts at once (c9): the work so far is kept, so there is nothing to confirm.
      await page.getByRole("button", { name: "Stop", exact: true }).click();
      await expect(page.getByLabel("Run status")).toHaveText("Stopped");
      const writes: { key: string | undefined; body: unknown }[] = [];
      page.on("request", (request) => {
        if (
          request.method() === "PUT" &&
          request.url().endsWith("/environment")
        )
          writes.push({
            key: request.headers()["idempotency-key"],
            body: request.postDataJSON() as unknown,
          });
      });
      await chooseEnvironment(page, "Workstation");
      await expect(
        page.getByRole("group", { name: "Switch environment" }),
      ).toHaveText(/Files and history stay; anything still running stops/);
      await capture(page, `environment-confirm-${suffix}`);
      await page.getByRole("button", { name: "Cancel switch" }).click();
      expect(writes).toEqual([]);
      await chooseEnvironment(page, "Workstation");
      await confirmEnvironment(page, "Workstation");
      expect(writes).toHaveLength(1);
      expect(writes[0]?.key).toMatch(/^[a-f0-9-]{36}$/);
      expect(writes[0]?.body).toEqual({ template: "workstation" });
      await expect(
        page.getByText("Switched to Workstation", { exact: true }),
      ).toHaveCount(0);
      await page.reload();
      await openChoices(page);
      await expect(picker).toHaveAccessibleName("Environment: Workstation");
      await page
        .getByLabel("Message", { exact: true })
        .fill("Use the new tools");
      await page.getByLabel("Message", { exact: true }).press("Enter");
      await expect(
        page.getByText("Switched to Workstation", { exact: true }),
      ).toHaveCount(1);
      await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
      await capture(page, `environment-switched-${suffix}`);
      await page.reload();
      await expect(
        page.getByText("Switched to Workstation", { exact: true }),
      ).toHaveCount(1);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
    });
  }

// The idle switch confirmation was inserted above the transcript and pushed it down (UI-GUIDELINES §5b).
for (const width of [1024, 1440])
  test(`WEB-U-031 the switch confirmation floats without moving the transcript ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await signIn(page);
    await page.goto(await createConversation(page));
    const input = page.getByLabel("Message", { exact: true });
    await input.fill("Summarize the workspace.");
    await input.press("Enter");
    await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
    const message = page.locator(".user-message").first();
    const before = (await message.boundingBox())?.y;
    await chooseEnvironment(page, "Minimal");
    const group = page.getByRole("group", { name: "Switch environment" });
    await expect(group).toBeVisible();
    expect((await message.boundingBox())?.y).toBe(before);
    const card = await group.boundingBox();
    const bar = await page.locator(".conversation-bar").boundingBox();
    if (!card || !bar)
      throw new Error("The bar and the confirmation must be visible");
    expect(card.y).toBeGreaterThanOrEqual(bar.y + bar.height - 1);
    expect(card.x + card.width).toBeLessThanOrEqual(width);
    await group.getByRole("button", { name: "Cancel switch" }).click();
    await expect(group).toHaveCount(0);
    expect((await message.boundingBox())?.y).toBe(before);
  });

import { readFileSync } from "node:fs";

import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

const en = JSON.parse(readFileSync("src/i18n/locales/en.json", "utf8")) as {
  messages: Record<string, string>;
};

// WEB-E2E-100: the task's ⋯ menu keeps the standard width (at most 220 px): Keep environment on says what it costs in
// one muted line of at most 36 characters, from the catalogue (UI-GUIDELINES §7 Action menu).
for (const [width, theme] of [
  [1440, "light"],
  [1440, "dark"],
  [390, "light"],
] as const)
  test(`WEB-E2E-100 the task's ⋯ menu ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    await page
      .getByRole("region", { name: "Task header" })
      .getByRole("button", { name: /^More actions for/ })
      .click();
    const menu = page.getByRole("menu");
    const toggle = menu.getByRole("menuitemcheckbox", {
      name: "Keep environment on",
    });
    await expect(toggle).toBeVisible();
    await settled(page);
    const cost = en.messages["task.keepOnNote"] ?? "";
    expect(cost.length).toBeLessThanOrEqual(36);
    const line = toggle.getByText(cost, { exact: true });
    const shape = await line.evaluate((element) => ({
      height: element.getBoundingClientRect().height,
      leading: Number.parseFloat(getComputedStyle(element).lineHeight),
    }));
    expect(shape.height).toBeLessThan(shape.leading * 1.5);
    const surface = await menu.locator("xpath=..").boundingBox();
    expect(surface?.width ?? Infinity).toBeLessThanOrEqual(220);
    await capture(page, `task-menu-${width}-${theme}`);
    expect(await axeViolations(page)).toEqual([]);
  });

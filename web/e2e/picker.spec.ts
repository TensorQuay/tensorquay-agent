import { mkdir } from "node:fs/promises";

import AxeBuilder from "@axe-core/playwright";

import { openChoices } from "./choices";
import { signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-085 on the fake front (the UI standard's Popover): the model picker past 7 routes, by keyboard only. It opens
// on its trigger, starts in its search field, finds by name or line, chooses with Enter and closes; Esc closes and
// gives the trigger its focus back.
const names = [
  "Alpha",
  "Beta",
  "Gamma",
  "Delta",
  "Epsilon",
  "Zeta",
  "Eta",
  "Theta",
  "Iota",
];
const route = (name: string) => ({
  name: name.toLowerCase(),
  model: `${name.toLowerCase()}-model`,
  protocol: "openai_chat",
  display_name: name,
  description: name === "Gamma" ? "The local one" : "Hosted",
  verified: null,
  context_window: 32768,
  vision: false,
  native_tools: true,
  reasoning: { allowed: ["off", "medium"], default: "medium" },
  // Required since 841f1cdd: where the route runs (the model's second line, WEB-U-103).
  runs_on: { where: "this_computer", provider: null },
});

for (const width of [1280, 390])
  test(`WEB-U-085 the model picker past 7 routes, by keyboard, at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 860 });
    await page.route("**/api/v1/models", (request) =>
      request.fulfill({ json: { items: names.map(route) } }),
    );
    await signIn(page);
    await page.goto("/");
    // New task's model picker is in its top bar (WEB-U-113).
    await openChoices(page);
    const trigger = page.getByRole("button", { name: /^Model:/ });
    await trigger.focus();
    await page.keyboard.press("ArrowDown");
    const search = page.getByRole("searchbox", { name: "Find" });
    await expect(search).toBeFocused();
    // Checked once it has faded in: mid-fade, every colour is lighter than it will be.
    await page
      .locator(".popover")
      .evaluate((element) =>
        Promise.all(element.getAnimations().map((motion) => motion.finished)),
      );
    expect(
      (await new AxeBuilder({ page }).include(".popover").analyze()).violations,
    ).toEqual([]);
    // Anchored to its trigger, never over it: under it, or above it when there's no room below (the UI standard).
    const button = await trigger.boundingBox();
    const surface = await page.locator(".popover").boundingBox();
    if (!button || !surface)
      throw new Error("Missing the trigger or the popover");
    expect(
      surface.y >= button.y + button.height ||
        surface.y + surface.height <= button.y,
      `popover ${JSON.stringify(surface)} overlaps its trigger ${JSON.stringify(button)}`,
    ).toBe(true);
    // Aligned to the trigger's end, the header's pickers sitting at its right (WEB-U-113), unless that would cross the
    // window's left edge, where it stops just inside it.
    expect(surface.x).toBeGreaterThanOrEqual(0);
    expect(
      surface.x <= 9 ||
        Math.round(surface.x + surface.width) ===
          Math.round(button.x + button.width),
      `popover ${JSON.stringify(surface)} is not aligned to its trigger's end ${JSON.stringify(button)}`,
    ).toBe(true);
    const shots = process.env.TQA_WEB_EVIDENCE_DIR;
    if (shots) {
      await mkdir(shots, { recursive: true });
      await page.screenshot({
        path: `${shots}/picker-open-${width}-light.png`,
      });
    }
    await page.keyboard.type("local");
    await expect(page.getByRole("menuitemradio")).toHaveCount(1);
    // It stays on the side it opened on while the list shrinks.
    const after = await page.locator(".popover").boundingBox();
    if (!after) throw new Error("Missing the popover");
    expect(after.y >= button.y + button.height).toBe(
      surface.y >= button.y + button.height,
    );
    const directory = process.env.TQA_WEB_EVIDENCE_DIR;
    if (directory) {
      await mkdir(directory, { recursive: true });
      for (const colorScheme of ["light", "dark"] as const) {
        await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
        await page.screenshot({
          path: `${directory}/picker-search-${width}-${colorScheme}.png`,
        });
      }
    }
    await page.keyboard.press("ArrowDown");
    await expect(
      page.getByRole("menuitemradio", { name: /Gamma/ }),
    ).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(trigger).toHaveText(/Gamma/);
    await expect(trigger).toBeFocused();
    await expect(page.getByRole("menu")).toHaveCount(0);
    await page.keyboard.press("ArrowDown");
    await expect(search).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("menu")).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });

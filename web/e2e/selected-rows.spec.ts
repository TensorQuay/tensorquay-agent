import { randomUUID } from "node:crypto";

import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, resetFront, test, type Locator, type Page } from "./test";

/** The selected fill, 8 % ink over paper, as the screen draws it in each theme. */
const SELECTED = { light: [236, 236, 236], dark: [36, 36, 35] } as const;

/** The colour an element shows: its background drawn over the page's paper, read back from a one-pixel canvas. */
function shown(element: Locator) {
  return element.evaluate((node) => {
    const canvas = document.createElement("canvas");
    canvas.width = 1;
    canvas.height = 1;
    const pen = canvas.getContext("2d");
    if (!pen) throw new Error("The page draws on a 2D canvas");
    pen.fillStyle = getComputedStyle(document.body).backgroundColor;
    pen.fillRect(0, 0, 1, 1);
    pen.fillStyle = getComputedStyle(node).backgroundColor;
    pen.fillRect(0, 0, 1, 1);
    return [...pen.getImageData(0, 0, 1, 1).data.slice(0, 3)];
  });
}

/** How far a grey fill stands from the paper: the selected one is the stronger step, a hover the lighter. */
async function step(page: Page, rgb: readonly number[]) {
  const [paper = 0] = await shown(page.locator("body"));
  return Math.abs((rgb[0] ?? 0) - paper);
}

/** Moves the pointer off every list, so nothing is hovered. */
const away = (page: Page) => page.mouse.move(900, 700);

// WEB-E2E-113 (17.3, UI-GUIDELINES §5c and §7): the open task, the open helper and the Settings menu's active item
// share one selected fill at the body weight; another row under the pointer is the lighter step; a selected row under
// the pointer keeps its fill, and the open task's title gains no underline.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-113 the open task, the open helper and the Settings menu's item share the selected fill ${colorScheme}`, async ({
    page,
  }) => {
    test.setTimeout(60_000);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await resetFront(page.request, ["agent-teams"]);
    await signIn(page);
    const selected = SELECTED[colorScheme];
    const strong = await step(page, selected);

    await test.step("the open task", async () => {
      let open = "";
      for (const title of ["Selected row one", "Selected row two"]) {
        const made = await page.request.post("/api/v1/conversations", {
          headers: { "Idempotency-Key": randomUUID() },
          data: { title, model_route: "local" },
        });
        expect(made.status()).toBe(201);
        const { id } = (await made.json()) as { id: string };
        open ||= id;
      }
      await page.goto(`/c/${open}`);
      const rows = page.locator(".app-sidebar .conversation-row");
      const current = rows.filter({
        has: page.locator(":scope > .conversation-link[aria-current='page']"),
      });
      await expect(current).toHaveCount(1);
      await away(page);
      expect(await shown(current)).toEqual(selected);
      await expect(current.locator(".conversation-link")).toHaveCSS(
        "font-weight",
        "400",
      );
      const other = rows.filter({ hasText: "Selected row two" });
      await other.hover();
      expect(await step(page, await shown(other))).toBeLessThan(strong);
      await current.hover();
      expect(await shown(current)).toEqual(selected);
      await expect(current.locator(".conversation-link")).toHaveCSS(
        "text-decoration-line",
        "none",
      );
      await capture(page, `selected-rows-task-${colorScheme}`);
    });

    await test.step("the open helper", async () => {
      await page.goto("/");
      const tasks = page.getByRole("region", { name: "Tasks", exact: true });
      await tasks
        .getByRole("button", { name: "Helpers of Report on Q3" })
        .click();
      await tasks
        .getByRole("list", { name: "Helpers" })
        .getByRole("link", { name: /^researcher/ })
        .click();
      const helper = page.locator(".task-helper:has([aria-current='page'])");
      await expect(helper).toBeVisible();
      await away(page);
      expect(await shown(helper)).toEqual(selected);
      await expect(helper.locator("[aria-current='page']")).toHaveCSS(
        "font-weight",
        "400",
      );
      await helper.hover();
      expect(await shown(helper)).toEqual(selected);
      await capture(page, `selected-rows-helper-${colorScheme}`);
    });

    await test.step("the Settings menu's active item", async () => {
      await page.goto("/settings/models");
      const menu = page.getByRole("navigation", { name: "Settings sections" });
      const active = menu.getByRole("link", { name: "Models" });
      await expect(active).toHaveAttribute("aria-current", "page");
      await away(page);
      expect(await shown(active)).toEqual(selected);
      await expect(active).toHaveCSS("font-weight", "400");
      const other = menu.getByRole("link", { name: "People" });
      await other.hover();
      expect(await step(page, await shown(other))).toBeLessThan(strong);
      await active.hover();
      expect(await shown(active)).toEqual(selected);
      await capture(page, `selected-rows-settings-${colorScheme}`);
    });
  });

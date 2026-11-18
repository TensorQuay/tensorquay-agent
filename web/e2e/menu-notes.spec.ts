import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { settled } from "./motion";
import { signIn } from "./session";
import { expect, resetFront, test, type Locator, type Page } from "./test";

// WEB-E2E-130 (18.0.1): every menu whose items carry a note (the task's ⋯, the Screen's ⋯ in its panel and in its
// Expand dialog) keeps its notes at 36 characters a line (WEB-U-490), so it opens inside its dialog, its panel or the
// window, nothing in it cut, beside its trigger; never 720 px across the window. At 1280 and 1440, light and dark,
// and 390. A desktop's classic scrollbars, as WEB-E2E-128.

test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `menu-notes-${name}.png`) });
}

async function desktopTask(page: Page): Promise<string> {
  const response = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: {
      title: "Fill in the form",
      model_route: "local",
      template: "desktop",
    },
  });
  expect(response.status()).toBe(201);
  const { id } = (await response.json()) as { id: string };
  return id;
}

/**
 * Opens `trigger`'s menu, named `label`, and checks it: inside `container`'s content box (the window when none), its
 * notes at most 36 characters wide, nothing cut, and beside its trigger: across from it, above or below it.
 */
async function check(
  page: Page,
  trigger: Locator,
  label: string,
  container: Locator | null,
) {
  await trigger.click();
  const surface = page.getByRole("region", { name: label });
  await expect(surface.getByRole("menu")).toBeVisible();
  await settled(page);
  const width = page.viewportSize()?.width ?? 0;
  const inside = container
    ? await container.evaluate((element) => {
        const box = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        return {
          left: box.left + parseFloat(style.paddingLeft),
          right: box.right - parseFloat(style.paddingRight),
        };
      })
    : { left: 0, right: width };
  const shape = await surface.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const notes = Array.from(
      element.querySelectorAll<HTMLElement>(".action-menu-word > .type-small"),
    );
    // 36 characters of the note's own face, measured as the browser counts ch.
    const probe = document.createElement("span");
    probe.textContent = "0".repeat(36);
    probe.style.whiteSpace = "nowrap";
    notes[0]?.append(probe);
    const line = probe.getBoundingClientRect().width;
    probe.remove();
    return {
      left: box.left,
      right: box.right,
      top: box.top,
      bottom: box.bottom,
      cut:
        element.scrollWidth > element.clientWidth ||
        Array.from(
          element.querySelectorAll<HTMLElement>("[role^='menuitem']"),
        ).some((item) => item.scrollWidth > item.clientWidth),
      notes: notes.length,
      widest: Math.max(
        0,
        ...notes.map((note) => note.getBoundingClientRect().width),
      ),
      line,
    };
  });
  const at = (await trigger.boundingBox())!;
  expect(shape.notes).toBeGreaterThan(0);
  expect(shape.widest).toBeLessThanOrEqual(shape.line + 1);
  expect(shape.left).toBeGreaterThanOrEqual(inside.left - 0.5);
  expect(shape.right).toBeLessThanOrEqual(inside.right + 0.5);
  expect(shape.cut).toBe(false);
  expect(shape.left).toBeLessThanOrEqual(at.x + at.width);
  expect(shape.right).toBeGreaterThanOrEqual(at.x);
  expect(shape.top >= at.y + at.height - 1 || shape.bottom <= at.y + 1).toBe(
    true,
  );
}

test.beforeEach(async ({ request }) => {
  await resetFront(request, ["desktop"]);
});

for (const [width, colorScheme] of [
  [1280, "light"],
  [1280, "dark"],
  [1440, "light"],
  [1440, "dark"],
  [390, "light"],
] as const)
  test(`WEB-E2E-130 menus with notes (${width} ${colorScheme})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await signIn(page);
    const id = await desktopTask(page);
    await page.goto(`/c/${encodeURIComponent(id)}`);

    // The task's ⋯: Keep environment on and its note; the window bounds it.
    const header = page.getByRole("region", { name: "Task header" });
    const more = header.getByRole("button", { name: /^More actions for/ });
    await check(
      page,
      more,
      (await more.getAttribute("aria-label")) ?? "",
      null,
    );
    await capture(page, `task-${width}-${colorScheme}`);
    await page.keyboard.press("Escape");

    // The Screen's ⋯ in its panel.
    await page.getByRole("button", { name: "Screen", exact: true }).click();
    const panel = page.getByRole("region", { name: "Screen" });
    await expect(panel.getByRole("status").first()).toHaveText("Watching.");
    const screenMore = "More actions for the screen";
    await check(
      page,
      panel.getByRole("button", { name: screenMore }),
      screenMore,
      page.locator("aside[data-menu-bounds]"),
    );
    await capture(page, `screen-${width}-${colorScheme}`);
    await page.keyboard.press("Escape");

    // The Screen's ⋯ in its Expand dialog: inside the dialog, under its ⋯.
    await panel.getByRole("button", { name: "Take over" }).click();
    await expect(panel.getByRole("status").first()).toHaveText(
      /^You have control\./,
    );
    await panel.getByRole("button", { name: "Expand" }).click();
    const dialog = page.getByRole("dialog", { name: "Screen" });
    await expect(
      dialog.getByRole("application", {
        name: "The agent's desktop, live, you have control",
      }),
    ).toBeVisible();
    await check(
      page,
      dialog.getByRole("button", { name: screenMore }),
      screenMore,
      dialog,
    );
    await capture(page, `expanded-${width}-${colorScheme}`);
  });

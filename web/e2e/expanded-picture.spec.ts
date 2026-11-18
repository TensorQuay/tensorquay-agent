import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { settled } from "./motion";
import { signIn } from "./session";
import { expect, resetFront, test, type Page } from "./test";

// WEB-E2E-131 (18.0.1): the Expand dialog's screen picture fits the dialog's content: its box and its outline (the
// frame, 2 px while this page has control) inside the area that clips it, on all four sides, at 390 and 1280, light
// and dark, with a desktop's classic scrollbars (as WEB-E2E-128).

test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({
    path: join(directory, `expanded-picture-${name}.png`),
  });
}

test.beforeEach(async ({ request }) => {
  await resetFront(request, ["desktop"]);
});

for (const [width, height] of [
  [390, 844],
  [1280, 800],
] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-131 the expanded screen picture fits (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
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
      await page.goto(`/c/${encodeURIComponent(id)}`);
      await page.getByRole("button", { name: "Screen", exact: true }).click();
      const panel = page.getByRole("region", { name: "Screen" });
      await panel.getByRole("button", { name: "Take over" }).click();
      await expect(panel.getByRole("status").first()).toHaveText(
        /^You have control\./,
      );
      await panel.getByRole("button", { name: "Expand" }).click();
      const dialog = page.getByRole("dialog", { name: "Screen" });
      const picture = dialog.getByRole("application", {
        name: "The agent's desktop, live, you have control",
      });
      await expect(picture.locator("canvas").last()).toBeVisible();
      await settled(page);
      const fit = await picture.evaluate((element: HTMLElement) => {
        const frame =
          element.closest<HTMLElement>(".screen-picture") ?? element;
        const style = getComputedStyle(frame);
        const outline =
          style.outlineStyle === "none"
            ? 0
            : parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
        // The nearest ancestor that clips: its padding box is what shows.
        let clip = frame.parentElement;
        while (clip && getComputedStyle(clip).overflowX === "visible")
          clip = clip.parentElement;
        if (!clip) throw new Error("No clipping ancestor");
        const box = frame.getBoundingClientRect();
        const area = clip.getBoundingClientRect();
        const left = area.left + clip.clientLeft;
        const top = area.top + clip.clientTop - clip.scrollTop;
        const shown = {
          left,
          right: left + clip.clientWidth,
          top,
          bottom: top + clip.scrollHeight,
        };
        return {
          geometry: {
            frame: [box.left, box.top, box.right, box.bottom],
            outline,
            clip: [shown.left, shown.top, shown.right, shown.bottom],
            clipClass: clip.className,
            clipScrollWidth: clip.scrollWidth,
          },
          past: {
            left: shown.left - (box.left - outline),
            right: box.right + outline - shown.right,
            top: shown.top - (box.top - outline),
            bottom: box.bottom + outline - shown.bottom,
          },
        };
      });
      for (const side of ["left", "right", "top", "bottom"] as const)
        expect(fit.past[side], side).toBeLessThanOrEqual(0.5);
      await capture(page, `${width}-${colorScheme}`);
    });

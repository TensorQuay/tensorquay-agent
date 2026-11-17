import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { axeViolations } from "./a11y";
import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-134 (18.1): the Files panel as a split view beside the chat, in place of the preview dialog. At 1280, 1440
// and 1920, light and dark: Expand gives the file 60 % of the task layout and the chat the rest (at least 360 px);
// the divider moves by mouse and keys and its share is remembered across a reload; Full width starts at the main
// area's edge with no line; Escape steps back from the file's side only; a page's frame is the same element across
// Expand and back. At 390 there is no split: the panel takes the column, with a back arrow.

test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `files-split-${name}.png`) });
}

async function openFile(page: Page, file: RegExp) {
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: "reports" }).click();
  await panel.getByRole("button", { name: file }).click();
  return panel;
}

/** The layout's, the panel's and the chat's boxes, and the panel's left border. */
async function boxes(page: Page) {
  return page.evaluate(() => {
    const box = (selector: string) => {
      const element = document.querySelector<HTMLElement>(selector);
      if (!element || getComputedStyle(element).display === "none") return null;
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right, width: rect.width };
    };
    const panel = document.querySelector<HTMLElement>("aside.files-panel");
    return {
      layout: box(".conversation-layout"),
      panel: box("aside.files-panel"),
      chat: box(".conversation-layout > .conversation-page"),
      border: panel ? parseFloat(getComputedStyle(panel).borderLeftWidth) : NaN,
    };
  });
}

for (const width of [1280, 1440, 1920] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-134 the split view (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const panel = await openFile(page, /^brief\.pdf/);
      await panel.getByRole("button", { name: "Expand" }).click();
      await expect(panel).toHaveAttribute("data-split", "split");
      await expect(
        panel.getByRole("heading", { name: "brief.pdf" }),
      ).toBeFocused();
      await expect(panel.getByRole("group", { name: "Page 1" })).toBeVisible();
      const split = await boxes(page);
      expect(
        Math.abs(split.panel!.width - 0.6 * split.layout!.width),
      ).toBeLessThanOrEqual(2);
      expect(split.chat!.width).toBeGreaterThanOrEqual(360);
      expect(await axeViolations(page)).toEqual([]);
      await capture(page, `pdf-${width}-${colorScheme}`);

      // The divider: a drag to the left gives the file more; the keys move it 2 %.
      const divider = panel.getByRole("separator", {
        name: "Resize the file beside the chat",
      });
      const grip = (await divider.boundingBox())!;
      await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
      await page.mouse.down();
      await page.mouse.move(
        grip.x + grip.width / 2 - 100,
        grip.y + grip.height / 2,
        { steps: 5 },
      );
      await page.mouse.up();
      const dragged = Number(await divider.getAttribute("aria-valuenow"));
      expect(dragged).toBeGreaterThan(60);
      await divider.focus();
      await page.keyboard.press("ArrowRight");
      await expect(divider).toHaveAttribute(
        "aria-valuenow",
        String(dragged - 2),
      );

      // Escape in the chat's composer leaves the split alone.
      await page.getByLabel("Message", { exact: true }).press("Escape");
      await expect(panel).toHaveAttribute("data-split", "split");

      // Full width: from the main area's edge, no line, the chat hidden.
      await panel.getByRole("button", { name: "Full width" }).click();
      await expect(panel).toHaveAttribute("data-split", "full");
      const full = await boxes(page);
      expect(full.chat).toBeNull();
      expect(
        Math.abs(full.panel!.left - full.layout!.left),
      ).toBeLessThanOrEqual(1);
      expect(full.border).toBe(0);
      expect(await axeViolations(page)).toEqual([]);
      await capture(page, `full-${width}-${colorScheme}`);
      await page.keyboard.press("Escape");
      await expect(panel).toHaveAttribute("data-split", "split");
      await page.keyboard.press("Escape");
      await expect(panel).not.toHaveAttribute("data-split");
      await expect(panel.getByRole("button", { name: "Expand" })).toBeFocused();

      // The share is remembered for this user across a reload.
      await page.reload();
      const again = await openFile(page, /^brief\.pdf/);
      await again.getByRole("button", { name: "Expand" }).click();
      await expect(
        again.getByRole("separator", {
          name: "Resize the file beside the chat",
        }),
      ).toHaveAttribute("aria-valuenow", String(dragged - 2));
    });

test("WEB-E2E-134 a page's frame is kept across Expand and back (1280)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  await page.goto(await createConversation(page));
  const panel = await openFile(page, /^report\.html/);
  const frame = panel.locator("iframe");
  await expect(frame).toBeVisible();
  await frame.evaluate((element) => {
    (element as HTMLElement & { kept?: boolean }).kept = true;
  });
  await panel.getByRole("button", { name: "Expand" }).click();
  await expect(panel).toHaveAttribute("data-split", "split");
  await panel.getByRole("button", { name: "Full width" }).click();
  await panel.getByRole("button", { name: "Show chat" }).click();
  await panel.getByRole("button", { name: "Close the file" }).click();
  await expect(panel).not.toHaveAttribute("data-split");
  expect(
    await frame.evaluate(
      (element) => (element as HTMLElement & { kept?: boolean }).kept === true,
    ),
  ).toBe(true);
  await capture(page, "page-1280");
});

for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-134 no split at 390, a back arrow (${colorScheme})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const panel = await openFile(page, /^brief\.pdf/);
    await expect(panel.getByRole("group", { name: "Page 1" })).toBeVisible();
    await expect(panel.getByRole("button", { name: "Expand" })).toHaveCount(0);
    await expect(
      panel.getByRole("button", { name: "Close panel" }),
    ).toHaveCount(0);
    await capture(page, `narrow-390-${colorScheme}`);
    await panel.getByRole("button", { name: "Back to the chat" }).click();
    await expect(panel).toBeHidden();
  });

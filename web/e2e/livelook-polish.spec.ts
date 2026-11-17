import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-133 (18.1 polish, from the 18.0.1 live look): at 390 and 1280, light and dark. The Files panel's heading
// keeps a file's size on one line beside or above its actions (WEB-U-492).

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `livelook-${name}.png`) });
}

for (const [width, height] of [
  [390, 844],
  [1280, 800],
] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-133 the file's size on one line (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const panel = await openFilesPanel(page);
      await panel.getByRole("button", { name: "reports" }).click();
      await panel.getByRole("button", { name: /^sizable\.pdf/ }).click();
      const preview = panel.getByRole("region", {
        name: "sizable.pdf",
        exact: true,
      });
      await expect(
        preview.getByText("661.6 kB", { exact: true }),
      ).toBeVisible();
      await settled(page);
      const shape = await preview.evaluate((section) => {
        const heading = section.querySelector<HTMLElement>(
          ".file-preview-heading",
        )!;
        const size = heading.firstElementChild as HTMLElement;
        const panel = section.closest<HTMLElement>("aside")!;
        const style = getComputedStyle(panel);
        const inside = panel.getBoundingClientRect();
        const left = inside.left + parseFloat(style.paddingLeft);
        const right = inside.right - parseFloat(style.paddingRight);
        const boxes = [
          size,
          ...heading.querySelectorAll<HTMLElement>(".file-preview-actions > *"),
        ].map((element) => element.getBoundingClientRect());
        const overlap = boxes.some((a, i) =>
          boxes.some(
            (b, j) =>
              i < j &&
              a.left < b.right &&
              b.left < a.right &&
              a.top < b.bottom &&
              b.top < a.bottom,
          ),
        );
        return {
          lines: Math.round(
            size.getBoundingClientRect().height /
              parseFloat(getComputedStyle(size).lineHeight),
          ),
          inside: boxes.every(
            (box) => box.left >= left - 0.5 && box.right <= right + 0.5,
          ),
          overlap,
          oneRow: boxes.every(
            (box) =>
              Math.abs(
                box.top +
                  box.height / 2 -
                  (boxes[0]!.top + boxes[0]!.height / 2),
              ) < 4,
          ),
        };
      });
      expect(shape.lines).toBe(1);
      expect(shape.inside).toBe(true);
      expect(shape.overlap).toBe(false);
      if (width === 1280) expect(shape.oneRow).toBe(true);
      await capture(page, `size-${width}-${colorScheme}`);
    });

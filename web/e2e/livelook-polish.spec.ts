import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { openCard } from "./cards";
import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-133 (18.1 polish, from the 18.0.1 live look): at 390 and 1280, light and dark. The Files panel's heading
// keeps a file's size on one line beside or above its actions (WEB-U-492); a PDF read's output block wraps its prose
// lines and a long address in them, while a code file's read keeps its lines (WEB-U-493).

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

/** The opened step's output block for `message`'s run: whether it scrolls sideways, and whether it wraps. */
async function readOutput(page: Page, message: string, step: RegExp) {
  await page.getByLabel("Message", { exact: true }).fill(message);
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 20_000,
  });
  await page.getByRole("button", { name: /Worked for/ }).click();
  await openCard(page.getByRole("button", { name: step }));
  const output = page.getByRole("group", { name: "Result" }).first();
  await expect(output).toBeVisible();
  return output.evaluate((block) => ({
    sideways: block.scrollWidth > block.clientWidth + 1,
    wraps: getComputedStyle(block).whiteSpace === "pre-wrap",
  }));
}

for (const [width, height] of [
  [390, 844],
  [1280, 800],
] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-133 a PDF read's output wraps; a code read's keeps its lines (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      expect(
        await readOutput(
          page,
          "Read the report PDF",
          /^Read uploads\/report\.pdf pages 1–2/,
        ),
      ).toEqual({ sideways: false, wraps: true });
      await capture(page, `pdf-read-${width}-${colorScheme}`);
      await page.goto(await createConversation(page));
      expect(
        await readOutput(page, "Read the code file", /^Read src\/app\.py/),
      ).toEqual({
        sideways: true,
        wraps: false,
      });
    });

import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// Task 084: a Markdown table's short columns squeezed until words and numbers broke ("Siz/e", "2,1/24 B"). Short
// columns keep their words whole, prose columns wrap, and a table too wide for the column scrolls in its own region.
async function measure(page: Page) {
  return page.evaluate(() => {
    const [files, wide] = Array.from(
      document.querySelectorAll<HTMLElement>(".markdown-table"),
    );
    if (!files || !wide) throw new Error("The answer needs both tables");
    const lines = (cell: Element) => {
      const range = document.createRange();
      range.selectNodeContents(cell);
      const tops = new Set(
        Array.from(range.getClientRects(), (rect) => Math.round(rect.top)),
      );
      return tops.size;
    };
    const column = (index: number) =>
      Array.from(files.querySelectorAll(`tr > :nth-child(${index})`), lines);
    const page = document.scrollingElement ?? document.documentElement;
    return {
      file: column(1),
      size: column(2),
      proseWraps: column(3)
        .slice(1)
        .every((count) => count > 1),
      filesFit: files.scrollWidth <= files.clientWidth,
      wideScrollsInside: wide.scrollWidth > wide.clientWidth,
      pageScrolls: page.scrollWidth > window.innerWidth,
    };
  });
}

for (const width of [1440, 1024])
  test(`WEB-U-027 a Markdown table keeps short columns whole at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(() => {
      localStorage.setItem("tensorquay-sidebar", "expanded");
      localStorage.setItem("tensorquay-files-panel", "closed");
    });
    await signIn(page);
    await page.goto(await createConversation(page));
    const input = page.getByRole("textbox", { name: "Message", exact: true });
    await input.fill("Show the files table");
    await input.press("Enter");
    await expect(page.getByLabel("Run status")).toHaveText(/Done/);
    await expect(page.getByRole("group", { name: "Table" })).toHaveCount(2);
    expect(await measure(page)).toEqual({
      file: [1, 1, 1, 1],
      size: [1, 1, 1, 1],
      proseWraps: true,
      filesFit: true,
      wideScrollsInside: true,
      pageScrolls: false,
    });
    const directory = process.env.TQA_WEB_EVIDENCE_DIR;
    if (directory) {
      await mkdir(directory, { recursive: true });
      await page
        .getByRole("group", { name: "Table" })
        .first()
        .screenshot({ path: join(directory, `markdown-table-${width}.png`) });
    }
  });

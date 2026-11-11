import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `${name}.png`) });
}

async function openFile(page: Page, folder: string, name: RegExp) {
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: folder }).click();
  await panel.getByRole("button", { name }).click();
  return panel;
}

// WEB-U-087 and WEB-U-088 in the browser: the agent's page runs in its sandbox, out of the app's reach, and a
// spreadsheet reads as a table; at 1280 px in both themes.
for (const theme of ["light", "dark"] as const) {
  test(`WEB-U-087 a web page in the Files panel 1280 ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const panel = await openFile(page, "reports", /^report\.html/);
    const preview = panel.getByRole("region", { name: "report.html" });
    await expect(
      preview.getByText("Scripts are running in a sandbox"),
    ).toBeVisible();
    const frame = preview.getByTitle("Preview of report.html");
    await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    const inside = preview.frameLocator("iframe");
    // Its script ran, drew the chart, and could not reach the app's page.
    await expect(inside.getByText("Scripts ran.")).toBeVisible();
    await expect(
      inside.getByText("The app's page is out of reach."),
    ).toBeVisible();
    await expect(inside.locator("rect.bar")).toHaveCount(4);
    await settled(page);
    await capture(page, `preview-page-1280-${theme}`);
    expect(
      (await new AxeBuilder({ page }).exclude("iframe").analyze()).violations,
    ).toEqual([]);
    // Reload starts the page again.
    await preview.getByRole("button", { name: "Reload" }).click();
    await expect(inside.getByText("Scripts ran.")).toBeVisible();
  });

  // WEB-U-467: the preview is offline, so a page that loads from the internet names its hosts, as text, on a row of
  // its own under the bar, which stays as before (the sandbox note and Reload, as high as a self-contained page's);
  // at 1280 and 390.
  test(`WEB-U-467 a page that needs the internet 1280 and 390 ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const panel = await openFile(page, "reports", /^weather\.html/);
    const preview = panel.getByRole("region", { name: "weather.html" });
    const line = preview.getByText(
      "Previews run offline: what this page loads from cdn.example.org and api.example.org won't show here. Download it to see it all.",
      { exact: true },
    );
    await expect(line).toBeVisible();
    await expect(line.locator("a")).toHaveCount(0);
    const note = preview.getByText("Scripts are running in a sandbox", {
      exact: true,
    });
    await expect(note).toBeVisible();
    await expect(preview.frameLocator("iframe").locator("#now")).toHaveText(
      "Loading…",
    );
    // The line is under the bar, across the preview's width; the bar is as high as a self-contained page's.
    const rows = async () =>
      note.evaluate((element) => {
        const bar = element.parentElement?.getBoundingClientRect();
        const row = element.parentElement?.nextElementSibling;
        const below = row?.getBoundingClientRect();
        return {
          bar: bar?.height ?? 0,
          barBottom: bar?.bottom ?? 0,
          barWidth: bar?.width ?? 0,
          rowTop: below?.top ?? 0,
          rowWidth: below?.width ?? 0,
          row: row?.tagName ?? "",
        };
      });
    const offline = await rows();
    expect(offline.row).toBe("P");
    expect(offline.rowTop).toBeGreaterThanOrEqual(offline.barBottom);
    expect(offline.rowWidth).toBe(offline.barWidth);
    await settled(page);
    await capture(page, `preview-offline-1280-${theme}`);
    expect(
      (await new AxeBuilder({ page }).exclude("iframe").analyze()).violations,
    ).toEqual([]);
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(line).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await settled(page);
    await capture(page, `preview-offline-390-${theme}`);
    await page.setViewportSize({ width: 1280, height: 900 });
    await panel
      .getByRole("navigation", { name: "Folder path" })
      .getByRole("button", { name: "reports" })
      .click();
    await panel.getByRole("button", { name: /^report\.html/ }).click();
    const report = panel.getByRole("region", { name: "report.html" });
    const reportNote = report.getByText("Scripts are running in a sandbox", {
      exact: true,
    });
    await expect(reportNote).toBeVisible();
    await expect(report.getByText(/^Previews run offline/)).toHaveCount(0);
    expect(
      await reportNote.evaluate(
        (element) => element.parentElement?.getBoundingClientRect().height,
      ),
    ).toBe(offline.bar);
  });

  test(`WEB-U-088 a spreadsheet in the Files panel 1280 ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const panel = await openFile(page, "reports", /^sales\.xlsx/);
    const table = panel.getByRole("table", {
      name: "Sheet Totals of sales.xlsx",
    });
    await expect(table.getByRole("cell", { name: "South" })).toBeVisible();
    await expect(panel.getByText("7 rows")).toBeVisible();
    // WEB-U-182: rows and columns lead; the file's size is no headline.
    await expect(panel.getByText(/^7 rows · \d+ columns$/)).toBeVisible();
    await expect(panel.getByText("6 B", { exact: true })).toHaveCount(0);
    // A URL in a cell is text, never a link; a long one is capped, whole on hover.
    const url =
      "https://example.invalid/reports/2026/q3/sales-by-region-final.xlsx";
    const long = table.getByRole("cell", { name: url });
    await expect(long).toHaveAttribute("title", url);
    expect(
      await long
        .locator(".file-cell")
        .evaluate((cell) => cell.getBoundingClientRect().width),
    ).toBeLessThanOrEqual(201);
    // Every column shows: E's totals are in view once scrolled, beside the sticky row numbers.
    await expect(table.getByRole("columnheader", { name: "E" })).toBeAttached();
    await expect(table.getByRole("link")).toHaveCount(0);
    await settled(page);
    await capture(page, `preview-sheet-1280-${theme}`);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    const sheets = panel.getByRole("group", { name: "Sheets" });
    await sheets.getByRole("button", { name: "Notes" }).click();
    await expect(
      panel
        .getByRole("table", { name: "Sheet Notes of sales.xlsx" })
        .getByRole("cell", { name: "Q3 is provisional." }),
    ).toBeVisible();
    await expect(sheets.getByRole("button", { name: "Notes" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    // Expand: the same sheet wide, over the conversation; Escape closes it.
    await sheets.getByRole("button", { name: "Totals" }).click();
    await panel.getByRole("button", { name: "Expand" }).click();
    const dialog = page.getByRole("dialog", { name: "sales.xlsx" });
    await expect(
      dialog.getByRole("table", { name: "Sheet Totals of sales.xlsx" }),
    ).toBeVisible();
    // The cap holds wide too (about 320 px): one long cell never decides a column.
    const wideCell = dialog
      .getByRole("cell", {
        name: "https://example.invalid/reports/2026/q3/sales-by-region-final.xlsx",
      })
      .locator(".file-cell");
    expect(
      await wideCell.evaluate((cell) => cell.getBoundingClientRect().width),
    ).toBeLessThanOrEqual(321);
    await settled(page);
    await capture(page, `preview-sheet-wide-1280-${theme}`);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(panel.getByRole("button", { name: "Expand" })).toBeFocused();
  });

  test(`WEB-U-087 a web page opens wide 1280 ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const panel = await openFile(page, "reports", /^report\.html/);
    await panel.getByRole("button", { name: "Expand" }).click();
    const dialog = page.getByRole("dialog", { name: "report.html" });
    await expect(
      dialog.frameLocator("iframe").getByText("Scripts ran."),
    ).toBeVisible();
    // One frame at a time: the panel's own is gone while the wide one shows.
    await expect(page.locator("iframe")).toHaveCount(1);
    await settled(page);
    await capture(page, `preview-page-wide-1280-${theme}`);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(
      panel.frameLocator("iframe").getByText("Scripts ran."),
    ).toBeVisible();
  });

  test(`WEB-U-087/088 previews at 390 ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    await page.getByRole("button", { name: "Files", exact: true }).click();
    const panel = await openFile(page, "reports", /^sales\.xlsx/);
    await expect(
      panel.getByRole("table", { name: "Sheet Totals of sales.xlsx" }),
    ).toBeVisible();
    // WEB-U-182: at a phone's width the sheet runs past its box, and its edge says there is more.
    await expect(
      panel.getByRole("region", { name: "Sheet Totals of sales.xlsx" }),
    ).toHaveAttribute("data-more", "true");
    // No horizontal page scroll at any width (§6): the table scrolls inside itself.
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await settled(page);
    // WEB-U-171: the panel starts where the header ends; its second line (the model and environment) stays whole.
    const header = await page
      .getByRole("region", { name: "Task header" })
      .boundingBox();
    const panelBox = await panel.boundingBox();
    expect(panelBox?.y ?? 0).toBeGreaterThanOrEqual(
      (header?.y ?? 0) + (header?.height ?? Infinity) - 1,
    );
    await capture(page, `preview-sheet-390-${theme}`);
    await panel.getByRole("button", { name: "Expand" }).click();
    const dialog = page.getByRole("dialog", { name: "sales.xlsx" });
    await expect(dialog.getByRole("table")).toBeVisible();
    await settled(page);
    await capture(page, `preview-sheet-wide-390-${theme}`);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    const path = panel.getByRole("navigation", { name: "Folder path" });
    await path.getByRole("button", { name: "reports" }).click();
    await panel.getByRole("button", { name: /^report\.html/ }).click();
    await expect(
      panel.frameLocator("iframe").getByText("Scripts ran."),
    ).toBeVisible();
    await settled(page);
    await capture(page, `preview-page-390-${theme}`);
    await panel.getByRole("button", { name: "Expand" }).click();
    await expect(
      page
        .getByRole("dialog", { name: "report.html" })
        .frameLocator("iframe")
        .getByText("Scripts ran."),
    ).toBeVisible();
    await settled(page);
    await capture(page, `preview-page-wide-390-${theme}`);
  });

  // WEB-U-091: pdf.js draws a real PDF in this page, under a policy without 'unsafe-eval' (as csp.spec), with its
  // worker from our own origin; the PDF's own script never runs.
  test(`WEB-U-091 a PDF in the Files panel 1280 ${theme}`, async ({ page }) => {
    const policy =
      "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; " +
      "img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'";
    await page.route("**/*", async (route) => {
      if (route.request().resourceType() !== "document")
        return route.continue();
      const response = await route.fetch();
      await route.fulfill({
        response,
        headers: { ...response.headers(), "content-security-policy": policy },
      });
    });
    const violations: string[] = [];
    page.on("console", (message) => {
      if (/Content Security Policy/i.test(message.text()))
        violations.push(message.text());
    });
    const origins = new Set<string>();
    page.on("request", (request) => {
      if (
        !request.url().startsWith("data:") &&
        !request.url().startsWith("blob:")
      )
        origins.add(new URL(request.url()).origin);
    });
    const dialogs: string[] = [];
    page.on("dialog", (dialog) => {
      dialogs.push(dialog.message());
      void dialog.dismiss();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const workers: string[] = [];
    page.on("worker", (worker) => workers.push(worker.url()));
    const panel = await openFile(page, "reports", /^brief\.pdf/);
    const page1 = panel.getByRole("group", { name: "Page 1" });
    const first = page1.locator("canvas");
    await expect(first).toBeVisible();
    // WEB-U-277: the page's text is there to read and select, laid over the picture, and nothing in it is a link.
    await expect(page1.locator(".textLayer")).not.toBeEmpty();
    await expect(page1.getByRole("link")).toHaveCount(0);
    // Drawn: the canvas holds dark pixels (the page's text), not only white.
    await expect
      .poll(() =>
        first.evaluate((canvas: HTMLCanvasElement) => {
          const context = canvas.getContext("2d");
          if (!context) return 0;
          const { data } = context.getImageData(
            0,
            0,
            canvas.width,
            canvas.height,
          );
          let dark = 0;
          for (let index = 0; index < data.length; index += 4)
            if ((data[index] ?? 255) < 128) dark++;
          return dark;
        }),
      )
      .toBeGreaterThan(50);
    await settled(page);
    await capture(page, `preview-pdf-1280-${theme}`);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    // The worker is a chunk of this build, and nothing left our origin.
    expect(workers.length).toBeGreaterThan(0);
    for (const url of workers)
      expect(new URL(url).origin).toBe(new URL(page.url()).origin);
    expect([...origins]).toEqual([new URL(page.url()).origin]);
    expect(dialogs).toEqual([]);
    expect(violations).toEqual([]);
    await panel.getByRole("button", { name: "Expand" }).click();
    const dialog = page.getByRole("dialog", { name: "brief.pdf" });
    await expect(dialog.getByRole("group", { name: "Page 1" })).toBeVisible();
    await settled(page);
    await capture(page, `preview-pdf-wide-1280-${theme}`);
    await page.keyboard.press("Escape");
    // A damaged PDF says so plainly.
    await panel
      .getByRole("navigation", { name: "Folder path" })
      .getByRole("button", { name: "reports" })
      .click();
    await panel.getByRole("button", { name: /^broken\.pdf/ }).click();
    await expect(
      panel.getByText("This PDF can't be shown: it's damaged."),
    ).toBeVisible();
  });
}

import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// The general-purpose walk-through (findings F1-F3): the files panel shows a brief, a table and a chart as what
// they are. The fake agent writes them for this message (e2e/front/files.mjs).
async function made(page: Page) {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Make a chart and a brief");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  const panel = await openFilesPanel(page);
  await expect(panel).toBeVisible();
  await panel.getByRole("button", { name: "Refresh" }).click();
  return panel;
}
async function open(panel: ReturnType<Page["getByRole"]>, names: string[]) {
  for (const name of names)
    await panel.getByRole("button", { name: new RegExp(`^${name}`) }).click();
}

test("WEB-U-022 a brief renders as a document, with its source a toggle away", async ({
  page,
}) => {
  const panel = await made(page);
  await open(panel, ["brief\\.md"]);
  await expect(panel.getByRole("heading", { name: "Findings" })).toBeVisible();
  await panel.getByRole("button", { name: "Show the source" }).click();
  await expect(
    panel.getByRole("region", { name: "Contents of brief.md" }),
  ).toContainText("# Findings");
});

test("WEB-U-022 a CSV shows as a table with its row count", async ({
  page,
}) => {
  const panel = await made(page);
  await open(panel, ["data", "clean\\.csv"]);
  const table = panel.getByRole("table", { name: "Table of clean.csv" });
  await expect(table.getByRole("columnheader")).toHaveText([
    "region",
    "total",
    "note",
  ]);
  await expect(table.getByRole("row")).toHaveCount(4);
  await expect(panel.getByText("3 rows", { exact: true })).toBeVisible();
});

test("WEB-U-022 a chart and an SVG show as images, and the SVG's script never runs", async ({
  page,
}) => {
  const panel = await made(page);
  await open(panel, ["out", "chart\\.png"]);
  const chart = panel.getByRole("img", { name: "chart.png" });
  await expect(chart).toBeVisible();
  // The browser decoded it: an image that failed to load has no natural size.
  expect(
    await chart.evaluate((image: HTMLImageElement) => image.naturalWidth),
  ).toBe(2);
  await panel
    .getByRole("navigation", { name: "Folder path" })
    .getByRole("button", { name: "out", exact: true })
    .click();
  await open(panel, ["logo\\.svg"]);
  await expect(panel.getByRole("img", { name: "logo.svg" })).toBeVisible();
  expect(
    await page.evaluate(
      () => (window as unknown as { svgRan?: boolean }).svgRan,
    ),
  ).toBeUndefined();
});

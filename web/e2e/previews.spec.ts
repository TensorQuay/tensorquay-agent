import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// The files panel shows a brief, a table and a chart as what they are. The fake agent writes them for this message
// (e2e/front/files.mjs).
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

// WEB-U-517 (WEB-E2E-056 on the real stack): an image over the preview's limit (10 MiB) shows its note and no Expand,
// where a small one, in the same layout, offers Expand.
test("WEB-U-517 an image too large to preview says so, with no Expand", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the front's origin");
  await page.setViewportSize({ width: 1280, height: 900 });
  const panel = await made(page);
  const id = new URL(page.url()).pathname.split("/").pop();
  const uploaded = await page.request.post(
    `/api/v1/conversations/${id}/uploads?name=big.png`,
    {
      headers: { Origin: baseURL, "content-type": "application/octet-stream" },
      data: Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        Buffer.alloc(11 * 1024 * 1024 - 8),
      ]),
    },
  );
  expect(uploaded.status()).toBe(201);
  await panel.getByRole("button", { name: "Refresh" }).click();
  await open(panel, ["uploads", "big\\.png"]);
  await expect(
    panel.getByText(/^Too large to preview \(.+\)\. Download it to open it\.$/),
  ).toBeVisible();
  await expect(panel.getByRole("button", { name: "Expand" })).toHaveCount(0);
  // The same layout offers Expand for a small image.
  await page.reload();
  const again = await openFilesPanel(page);
  await open(again, ["out", "chart\\.png"]);
  await expect(again.getByRole("button", { name: "Expand" })).toBeVisible();
});

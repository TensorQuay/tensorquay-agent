import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { openFilesPanel } from "./files-panel";
import { afterStart, budget } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// WEB-E2E-032: on the real stack, the agent writes an HTML report, PDFs and spreadsheets (deploy/tests/demo_previews.py
// builds them; the scripted model's one shell call writes them), and the Files panel shows each as what it is, with
// each failure in plain words. The report's script tries every way out it has against a witness server the browser can
// reach; the witness must see nothing but this spec's control request. The same suite runs on the Docker and the
// microVM backend (demo.py --backend firecracker).
test.skip(!process.env.TQA_STACK_URL, "Requires the owned previews demo stack");
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

const witness = process.env.TQA_WITNESS_URL ?? "";
const step = (title: string) => test.step(title, () => undefined);

/** Opens a file of the previews folder in the Files panel, from wherever the panel is. */
async function open(panel: Locator, name: string) {
  // From a file of the folder, its breadcrumb; from elsewhere, the root (a breadcrumb only when not already there),
  // then the folder in the list.
  const path = panel.getByRole("navigation", { name: "Folder path" });
  if (await path.getByRole("button", { name: "previews" }).isVisible())
    await path.getByRole("button", { name: "previews" }).click();
  else {
    if (await path.getByRole("button", { name: "Workspace" }).isVisible())
      await path.getByRole("button", { name: "Workspace" }).click();
    await panel.getByRole("button", { name: "previews" }).click();
  }
  await panel
    .getByRole("button", { name: new RegExp(`^${name.replace(".", "\\.")}`) })
    .click();
  return panel.getByRole("region", { name });
}

/** The dark pixels a drawn canvas holds (its text), so a blank one is told apart. */
function dark(canvas: Locator) {
  return canvas.evaluate((element: HTMLCanvasElement) => {
    const context = element.getContext("2d");
    if (!context) return 0;
    const { data } = context.getImageData(0, 0, element.width, element.height);
    let count = 0;
    for (let index = 0; index < data.length; index += 4)
      if ((data[index] ?? 255) < 128) count++;
    return count;
  });
}

async function witnessed(page: Page): Promise<string[]> {
  const answer = await page.request.get(`${witness}/log`);
  expect(answer.status()).toBe(200);
  return (await answer.json()) as string[];
}

test("WEB-E2E-032 the agent's page, PDFs and spreadsheets in the Files panel", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(budget(1, 330_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  if (!witness) throw new Error("The driver must supply the witness server");
  const dialogs: string[] = [];
  page.on("dialog", (dialog) => {
    dialogs.push(dialog.message());
    void dialog.dismiss();
  });
  const popups: string[] = [];
  context.on("page", (opened) => popups.push(opened.url()));

  await step("the browser reaches the witness (the control)");
  const control = await context.newPage();
  await control.goto(`${witness}/control`);
  await expect(control.getByText("witnessed")).toBeVisible();
  await control.close();
  popups.length = 0;

  await step("the agent writes the files");
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Previews", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string; workspace_id: string };
  await page.goto(`/c/${task.id}`);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("Make the preview files");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(90_000),
  });
  const panel = await openFilesPanel(page);
  await expect(panel).toBeVisible();
  await panel.getByRole("button", { name: "Refresh files" }).click();

  await step("the report runs sandboxed, and its every way out fails");
  const report = await open(panel, "report.html");
  const frame = report.getByTitle("Preview of report.html");
  await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
  const inside = report.frameLocator("iframe");
  await expect(inside.locator("#done")).toHaveText("Probes done.");
  await expect(inside.locator("#ran")).toHaveText("Scripts ran.");
  await expect(inside.locator("#parent")).toHaveText(
    "The app's page is out of reach.",
  );
  await expect(inside.locator("#cookie")).toHaveText("Cookies: none");
  await expect(inside.locator("#top")).toHaveText(
    "Moving the app was refused.",
  );
  await expect(inside.locator("#fetch")).toHaveText("The request was blocked.");
  await expect(inside.locator("#api")).toHaveText("The API was out of reach.");
  await expect(inside.locator("rect.bar")).toHaveCount(4);
  expect(new URL(page.url()).pathname).toBe(`/c/${task.id}`);

  await step("opened on its own, the page is still sandboxed");
  const direct = await context.newPage();
  await direct.goto(
    `/api/v1/conversations/${task.id}/files/view?path=previews%2Freport.html`,
  );
  await expect(direct.locator("#done")).toHaveText("Probes done.");
  await expect(direct.locator("#parent")).toHaveText("No app page around.");
  await expect(direct.locator("#cookie")).toHaveText("Cookies: none");
  await expect(direct.locator("#storage")).toHaveText("Storage refused.");
  await expect(direct.locator("#fetch")).toHaveText("The request was blocked.");
  await expect(direct.locator("#api")).toHaveText("The API was out of reach.");
  await direct.close();
  popups.length = 0;

  await step("a page over the limit is only downloaded");
  const big = await open(panel, "big.html");
  await expect(
    big.getByText(/^This page is too large to preview \(3 MB of 2 MB\)/),
  ).toBeVisible();
  await expect(big.getByTitle(/^Preview of/)).toHaveCount(0);

  await step("PDFs: drawn, counted, refused");
  const brief = await open(panel, "brief.pdf");
  const page1 = brief.getByRole("group", { name: "Page 1" }).locator("canvas");
  await expect(page1).toBeVisible();
  await expect.poll(() => dark(page1)).toBeGreaterThan(50);
  const long = await open(panel, "long.pdf");
  await expect(
    long.getByText("Showing 20 of 25 pages. Download it for the rest."),
  ).toBeVisible();
  await expect(long.getByRole("group", { name: /^Page \d+$/ })).toHaveCount(20);
  const broken = await open(panel, "broken.pdf");
  await expect(
    broken.getByText("This PDF can't be shown: it's damaged."),
  ).toBeVisible();
  const bigPdf = await open(panel, "big.pdf");
  await expect(
    bigPdf.getByText(/^This PDF is too large to preview \(21 MB of 20 MB\)/),
  ).toBeVisible();

  await step("spreadsheets: a table, a counted bomb, refusals");
  const sales = await open(panel, "sales.xlsx");
  const totals = sales.getByRole("table", {
    name: "Sheet Totals of sales.xlsx",
  });
  // A formula shows its saved result, never its text; a URL is text, never a link.
  await expect(totals.getByRole("cell", { name: "33" })).toBeVisible();
  await expect(totals.getByText("SUM(B2:D2)")).toHaveCount(0);
  await expect(totals.getByRole("link")).toHaveCount(0);
  await sales
    .getByRole("group", { name: "Sheets" })
    .getByRole("button", { name: "Notes" })
    .click();
  await expect(
    sales
      .getByRole("table", { name: "Sheet Notes of sales.xlsx" })
      .getByRole("cell", { name: "Q3 is provisional." }),
  ).toBeVisible();
  const plan = await open(panel, "plan.ods");
  await expect(
    plan.getByText(/^Showing 500 of 1\D000\D001 rows/),
  ).toBeVisible();
  await expect(plan.getByText(/the first 50 columns/)).toBeVisible();
  for (const [name, words] of [
    [
      "bomb.xlsx",
      "This spreadsheet is too large to preview. Download it to open it.",
    ],
    ["damaged.xlsx", "This spreadsheet can't be shown: it's damaged."],
  ] as const) {
    const view = await open(panel, name);
    await expect(view.getByText(words)).toBeVisible();
  }
  const bigSheet = await open(panel, "big.xlsx");
  await expect(
    bigSheet.getByText(
      /^This spreadsheet is too large to preview \(11 MB of 10 MB\)/,
    ),
  ).toBeVisible();

  await step("nothing left any preview: the witness saw only the control");
  // The probes are asynchronous; give any late request its chance to arrive.
  await page.waitForTimeout(2000);
  expect(await witnessed(page)).toEqual(["/control"]);
  expect(dialogs).toEqual([]);
  expect(popups).toEqual([]);
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({ workspaceId: task.workspace_id }),
    );
});

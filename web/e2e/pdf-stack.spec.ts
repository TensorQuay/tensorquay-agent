import { createHash, randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { openCard } from "./cards";
import { openFilesPanel } from "./files-panel";
import { afterStart, budget } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// WEB-E2E-126: read_file on PDFs on the real stack (PRD item 30; TOOL-U-145, CORE-U-092). The scripted model
// (deploy/tests/fixtures/pdf.json) reads the files this spec attaches: a three-page report on the stack's `local` route
// and again on its `vision` route, where the runtime draws the pages the agent read and sends them with the result;
// then a locked, a damaged and an over-nested file, each read as a result that says why, never a failed step.
// demo_pdf.py checks the uploads on disk afterwards, byte for byte, and that the agent wrote nothing.
test.skip(
  process.env.TQA_STACK_SUITE !== "pdf",
  "Requires the pdf stack suite",
);
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

/** A named step in the run's log (the list reporter prints steps): the log shows every part the test reached. */
const step = (title: string) => test.step(title, () => undefined);

/**
 * A small PDF, as crates/tqa-testkit/src/pdf.rs builds one: one uncompressed content stream a page, a plain
 * cross-reference table, Helvetica, so the reader's text comes back as written; an empty string is an empty page.
 * `extras` adds entries to the catalogue and the trailer, and further objects after the pages'.
 */
function pdf(
  pages: string[],
  extras: { catalog?: string; trailer?: string; objects?: string[] } = {},
) {
  const kids = pages.map((_, i) => `${4 + 2 * i} 0 R`).join(" ");
  const objects = [
    `<< /Type /Catalog /Pages 2 0 R ${extras.catalog ?? ""}>>`,
    `<< /Type /Pages /Kids [${kids}] /Count ${pages.length} >>`,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ];
  pages.forEach((content, i) => {
    const stream = content ? `BT /F1 12 Tf 72 720 Td (${content}) Tj ET` : "";
    objects.push(
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents ${5 + 2 * i} 0 R >>`,
      `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
    );
  });
  objects.push(...(extras.objects ?? []));
  let out = "%PDF-1.4\n";
  const offsets: number[] = [];
  objects.forEach((object, i) => {
    offsets.push(Buffer.byteLength(out));
    out += `${i + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = Buffer.byteLength(out);
  out += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets)
    out += `${String(offset).padStart(10, "0")} 00000 n \n`;
  out += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R ${extras.trailer ?? ""}>>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(out, "latin1");
}

const REPORT = pdf(["Page one words", "", "Page three words"]);
/** Encrypted by the standard handler with a user password the reader doesn't have (its /O and /U are made up). */
const LOCKED = pdf(["Hidden words"], {
  trailer:
    "/Encrypt 6 0 R /ID [<0123456789abcdef0123456789abcdef> <0123456789abcdef0123456789abcdef>] ",
  objects: [
    `<< /Filter /Standard /V 1 /R 2 /Length 40 /P -1 /O <${"ab".repeat(32)}> /U <${"cd".repeat(32)}> >>`,
  ],
});
/** A header and nothing else: a PDF nothing can be read from. */
const BROKEN = Buffer.from("%PDF-1.7\n");
/** Nested past the reader's bound of 256 levels (PDF-U-001), refused before the engine parses it. */
const NESTED = pdf(["Deep words"], {
  catalog: `/Deep ${"[".repeat(300)}${"]".repeat(300)} `,
});

const sha256 = (bytes: Buffer) =>
  createHash("sha256").update(bytes).digest("hex");

type Recorded = { workspaceId: string; uploads: Record<string, string> };
const recorded: Recorded[] = [];

/** A new task on `route`, by the API; returns its id and workspace. */
async function task(page: Page, baseURL: string, title: string, route: string) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title, model_route: route },
  });
  expect(created.status(), `create a task on ${route}`).toBe(201);
  return (await created.json()) as { id: string; workspace_id: string };
}

/** Attaches `files` to the composer and sends `text`; waits for the run to end. */
async function send(page: Page, files: Record<string, Buffer>, text: string) {
  for (const [name, buffer] of Object.entries(files)) {
    await page
      .getByLabel("Choose files to attach")
      .setInputFiles([{ name, mimeType: "application/pdf", buffer }]);
    await expect(
      page
        .getByRole("form", { name: "Message composer" })
        .getByRole("list", { name: "Attached files" })
        .getByText(name),
    ).toBeVisible();
  }
  await page.getByLabel("Message", { exact: true }).fill(text);
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(60_000),
  });
}

/** Opens every finished run's steps, which fold under "Worked for". */
async function openSteps(page: Page) {
  for (const toggle of await page
    .getByRole("button", { name: /^Worked for/ })
    .all())
    if ((await toggle.getAttribute("aria-expanded")) !== "true")
      await toggle.click();
}

/**
 * The card of the agent's read of `path`, opened, with its folded parts shown: its line names the file, never the tool
 * (WEB-U-327's rule for every tool line).
 */
async function readCard(page: Page, path: string) {
  await openSteps(page);
  const line = page.getByRole("button", {
    name: new RegExp(`^Read ${path.replace(/[./]/g, "\\$&")}`),
  });
  await expect(line).toBeVisible();
  await expect(line).not.toContainText("read_file");
  await openCard(line);
  // The card in the line's own step (a run with three reads has three cards; the last isn't this one's).
  const card = page
    .getByRole("listitem")
    .filter({ has: line })
    .getByRole("article", { name: "read_file tool call" });
  await expect(card).toBeVisible();
  for (const more of await card
    .getByRole("button", { name: "Show more" })
    .all())
    await more.click();
  for (const fold of await card.locator("details:not([open]) > summary").all())
    await fold.click();
  return card.getByRole("group", { name: "Result" });
}

/** The report's pages as the result shows them. */
async function expectReportPages(result: Locator) {
  await expect(result).toContainText("[PDF, 3 pages]");
  await expect(result).toContainText("--- page 1 ---");
  await expect(result).toContainText("Page one words");
  await expect(result).toContainText("--- page 2 ---");
  await expect(result).toContainText("[no text on this page]");
  await expect(result).toContainText("--- page 3 ---");
  await expect(result).toContainText("Page three words");
  await expect(result).toContainText("[end of document: 3 pages]");
}

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await mkdir(directory, { recursive: true });
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.screenshot({
      path: join(directory, `stack-pdf-${name}-${colorScheme}.png`),
      fullPage: true,
    });
  }
  await page.emulateMedia({ colorScheme: "light" });
}

/** The driver reads the result after the run: each task's workspace and the uploads' digests. */
async function record() {
  const result = process.env.TQA_STACK_RESULT;
  if (!result) throw new Error("The driver must supply TQA_STACK_RESULT");
  await writeFile(result, JSON.stringify({ tasks: recorded }));
}

/** The report read on `route`: the step, its card's pages, and the upload in the Files panel. */
async function reportOn(
  page: Page,
  baseURL: string,
  route: string,
  phrase: string,
) {
  const created = await task(page, baseURL, `Report on ${route}`, route);
  recorded.push({
    workspaceId: created.workspace_id,
    uploads: { "report.pdf": sha256(REPORT) },
  });
  await page.goto(`/c/${created.id}`);
  await send(page, { "report.pdf": REPORT }, phrase);
  await expectReportPages(await readCard(page, "uploads/report.pdf"));
  // The panel lists the workspace's folders; the upload is inside `uploads`.
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: "uploads", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: /report\.pdf/ }),
  ).toBeVisible();
  await capture(page, `report-${route}`);
}

test("WEB-E2E-126 read_file reads a PDF by pages, for a model without vision and one with", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 300_000));
  await signIn(page);
  await step("the report's three pages on the local route");
  await reportOn(page, baseURL, "local", "Read the attached report");
  await step(
    "the same read on the vision route, the pages drawn for the model",
  );
  await reportOn(
    page,
    baseURL,
    "vision",
    "Look at the attached report's pages",
  );
  await record();
});

test("WEB-E2E-126 a locked, a damaged and an over-nested PDF say so in words", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(1, 300_000));
  await signIn(page);
  const created = await task(
    page,
    baseURL,
    "Files that can't be read",
    "local",
  );
  const files = {
    "locked.pdf": LOCKED,
    "broken.pdf": BROKEN,
    "nested.pdf": NESTED,
  };
  recorded.push({
    workspaceId: created.workspace_id,
    uploads: Object.fromEntries(
      Object.entries(files).map(([name, bytes]) => [name, sha256(bytes)]),
    ),
  });
  await page.goto(`/c/${created.id}`);
  await send(page, files, "Read the attached files that can't be read");
  await step("each file's reason, as a result, never a failed step");
  await expect(await readCard(page, "uploads/locked.pdf")).toContainText(
    /^\[PDF, \d+ bytes: a password is needed to read it\]/,
  );
  await expect(await readCard(page, "uploads/broken.pdf")).toContainText(
    /^\[PDF, 9 bytes: damaged, nothing could be read: /,
  );
  await expect(await readCard(page, "uploads/nested.pdf")).toContainText(
    /nested more than 256 levels deep; not read\]/,
  );
  await expect(page.getByText("Didn't succeed")).toHaveCount(0);
  await capture(page, "unreadable");
  await record();
});

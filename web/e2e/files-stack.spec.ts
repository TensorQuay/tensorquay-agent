import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { request as post } from "node:http";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { afterStart, budget } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

/** Diff lines as the eye reads them: the screen reader's "Added"/"Removed" label left out (WEB-U-164). */
const shownLines = (lines: Locator) =>
  lines.evaluateAll((elements) =>
    elements.map((element) => {
      const copy = element.cloneNode(true) as HTMLElement;
      for (const label of copy.querySelectorAll(".sr-only")) label.remove();
      return (copy.textContent ?? "").trim();
    }),
  );

/** A named step in the run's log (the list reporter prints steps): the log shows every part the test reached. */
const step = (title: string) => test.step(title, () => undefined);

// WEB-E2E-021: on the real stack, an upload reaches the workspace, the agent reads it and writes files,
// the panel shows them, and the Changes tab shows each run's changes with a diff. demo_files.py then checks the disk.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned files demo stack");
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

const SALES = "region,total\nnorth,12\nsouth,30\n";

/**
 * Sends `size` bytes to an upload URL and reads the gateway's answer. It asks first (Expect: 100-continue), so a
 * gateway that refuses an oversize body (413) answers before any of it is sent, and closing early can't hide the
 * refusal behind a write error (seen once on a loaded stack as EPIPE). Without an interim answer within two seconds
 * the body goes anyway, as HTTP clients do. Plain HTTP only (node:http): the stack's origin is http://.
 */
function postBytes(url: string, headers: Record<string, string>, size: number) {
  return new Promise<{ status: number; body: string }>((resolve, reject) => {
    const chunk = Buffer.alloc(64 * 1024);
    let answered = false;
    let sending = false;
    let sent = 0;
    const call = post(
      url,
      {
        method: "POST",
        headers: {
          ...headers,
          "content-length": String(size),
          expect: "100-continue",
        },
      },
      (response) => {
        answered = true;
        let body = "";
        response.setEncoding("utf8");
        response.on("data", (part: string) => (body += part));
        response.on("end", () => {
          resolve({ status: response.statusCode ?? 0, body });
          call.destroy();
        });
      },
    );
    // A write that fails after the answer came is the gateway closing early, as it may; before it, a real error.
    call.on("error", (error) => {
      if (!answered) reject(error);
    });
    const next = () => {
      while (!answered && sent < size) {
        const part = chunk.subarray(0, Math.min(chunk.length, size - sent));
        sent += part.length;
        if (!call.write(part)) return void call.once("drain", next);
      }
      if (!answered) call.end();
    };
    const send = () => {
      if (sending || answered) return;
      sending = true;
      next();
    };
    call.on("continue", send);
    setTimeout(send, 2000);
    call.flushHeaders();
  });
}

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await settled(page);
  await mkdir(directory, { recursive: true });
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.screenshot({
      path: join(directory, `stack-files-${name}-${colorScheme}.png`),
    });
  }
  await page.emulateMedia({ colorScheme: "light" });
}

/**
 * The feature matrix: on a second task, the run writes a chart and a hidden note (files.json's last two replies). The
 * chart shows as an image and downloads, the note waits behind Show hidden files, and a created file opens from Changes
 * as the file itself, not a diff. A task of its own, so the first task's two snapshots stay
 * what demo_files.py checks.
 */
async function madeFiles(page: Page, baseURL: string) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Chart and note", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string };
  await page.goto(`/c/${task.id}`);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("Add a chart and a note");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: /^Refresh/ }).click();
  // The note is hidden until asked for.
  await step("G2 T22: a hidden note waits behind Show hidden files");
  await expect(panel.getByRole("button", { name: /^\.notes/ })).toHaveCount(0);
  await panel.getByRole("button", { name: /^Show hidden files/ }).click();
  await expect(panel.getByRole("button", { name: /^\.notes/ })).toBeVisible();
  // The chart as an image the browser decoded, and its download under its own name.
  await step("G2 T21, T23: the chart shows as an image and downloads");
  await panel.getByRole("button", { name: /^chart\.png/ }).click();
  const chart = panel.getByRole("img", { name: "chart.png" });
  await expect(chart).toBeVisible();
  expect(
    await chart.evaluate((image: HTMLImageElement) => image.naturalWidth),
  ).toBe(1);
  const download = page.waitForEvent("download");
  await panel.getByRole("link", { name: "Download" }).click();
  expect((await download).suggestedFilename()).toBe("chart.png");
  await capture(page, "chart");
  // From Changes, a created file opens as itself.
  await step("G2 T25: from Changes, the chart opens as itself");
  await panel.getByRole("tab", { name: "Changes" }).click();
  await panel
    .getByRole("list", { name: "Changed files" })
    .getByRole("button", { name: /chart\.png/ })
    .click();
  await expect(panel.getByRole("tab", { name: "Files" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(panel.getByRole("img", { name: "chart.png" })).toBeVisible();
}

/**
 * An upload the agent edits is one of its changes, with its diff; a file uploaded later, with the next message, is the
 * person's own and is listed in no run's changes (the uploads fix, WEB-E2E-021). files.json's "Correct the attached
 * figures" replies append a line to uploads/notes.csv, then answer twice.
 */
async function uploadEdited(page: Page, baseURL: string) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Corrected figures", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string };
  const runs: string[] = [];
  page.on("request", (request) => {
    const run = new RegExp(
      `/conversations/${task.id}/runs/([^/]+)/changes$`,
    ).exec(new URL(request.url()).pathname);
    if (run && !runs.includes(run[1] ?? "")) runs.push(run[1] ?? "");
  });
  await page.goto(`/c/${task.id}`);
  const message = page.getByLabel("Message", { exact: true });
  const status = page.getByLabel("Run status");
  const attach = async (name: string, text: string) => {
    await page
      .getByLabel("Choose files to attach")
      .setInputFiles([
        { name, mimeType: "text/csv", buffer: Buffer.from(text) },
      ]);
    await expect(
      page
        .getByRole("form", { name: "Message composer" })
        .getByRole("list", { name: "Attached files" })
        .getByText(name),
    ).toBeVisible();
  };

  await step(
    "uploads: the agent edits an uploaded file, and Changes lists it with its diff",
  );
  await attach("notes.csv", "region,total\nnorth,12\n");
  await message.fill("Correct the attached figures");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(status).toHaveText(/^Done/, { timeout: afterStart(30_000) });
  const panel = await openFilesPanel(page);
  await panel.getByRole("tab", { name: "Changes" }).click();
  const list = panel.getByRole("list", { name: "Changed files" });
  await expect(list.getByRole("listitem")).toHaveText([
    /^Modified.*notes\.csv\+1$/,
  ]);
  await list.getByRole("button", { name: /notes\.csv/ }).click();
  const notes = panel
    .getByRole("group", { name: /^Changes to (uploads\/)?notes\.csv$/ })
    .locator('[data-kind="added"]');
  await expect(notes).toHaveCount(1);
  await expect.poll(() => shownLines(notes)).toEqual(["+west,5"]);
  await expect(notes.locator(".sr-only")).toHaveText(["Added"]);
  await capture(page, "upload-edited");

  await step(
    "uploads: a file uploaded with the next message is in no run's changes",
  );
  await attach("extra.csv", "region,total\neast,7\n");
  await message.fill("Here is one more file");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(status).toHaveText(/^Done/, { timeout: 120_000 });
  expect(runs).toHaveLength(2);
  const changes = async (run: string) =>
    (
      (await (
        await page.request.get(
          `/api/v1/conversations/${task.id}/runs/${run}/changes`,
        )
      ).json()) as { files: { path: string; status: string }[] }
    ).files.map(({ path, status }) => ({ path, status }));
  expect(await changes(runs[0] ?? "")).toEqual([
    { path: "uploads/notes.csv", status: "modified" },
  ]);
  expect((await changes(runs[1] ?? "")).map(({ path }) => path)).not.toContain(
    "uploads/extra.csv",
  );
}

test("WEB-E2E-021 uploads, workspace files and run changes on the real stack", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(3, 60_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Sales files", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string; workspace_id: string };
  // Run ids in the order the page first asks for their changes (the turn summaries ask as each run finishes).
  const runs: string[] = [];
  page.on("request", (request) => {
    const run = /\/runs\/([^/]+)\/changes$/.exec(
      new URL(request.url()).pathname,
    )?.[1];
    if (run && !runs.includes(run)) runs.push(run);
  });
  await page.goto(`/c/${task.id}`);
  const status = page.getByLabel("Run status");
  const message = page.getByLabel("Message", { exact: true });
  const send = page.getByRole("button", { name: "Send" });

  await page
    .getByLabel("Choose files to attach")
    .setInputFiles([
      { name: "sales.csv", mimeType: "text/csv", buffer: Buffer.from(SALES) },
    ]);
  const chips = page
    .getByRole("form", { name: "Message composer" })
    .getByRole("list", { name: "Attached files" });
  await expect(chips.getByText("sales.csv")).toBeVisible();
  await message.fill("Summarise the attached sales file");
  await expect(send).toBeEnabled();
  await send.click();
  await expect(status).toHaveText(/^Done/, { timeout: afterStart(30_000) });
  // The shell wrote both files, and the turn summary says so.
  await step("the upload is read and the run writes two files");
  await expect(page.locator(".turn-summary")).toHaveText("2 files changed");
  await expect(
    page
      .getByRole("article", { name: "Your message" })
      .getByRole("list", { name: "Attached files" }),
  ).toHaveText("sales.csv");

  const panel = await openFilesPanel(page);
  await expect(
    panel.getByRole("button", { name: /^report\.md/ }),
  ).toBeVisible();
  await expect(panel.getByRole("button", { name: /^rows\.txt/ })).toBeVisible();
  await panel.getByRole("button", { name: /^report\.md/ }).click();
  await expect(
    panel
      .getByRole("region", { name: "report.md" })
      .getByText("Regions: north, south"),
  ).toBeVisible();
  await capture(page, "preview");
  // The feature matrix: a Markdown file shows as a document, the uploaded CSV as a table.
  await step(
    "G2 T21: a Markdown file shows as a document, the uploaded CSV as a table",
  );
  await expect(
    panel.getByRole("heading", { name: "Sales report" }),
  ).toBeVisible();
  await panel
    .getByRole("navigation", { name: "Folder path" })
    .getByRole("button", { name: "Workspace" })
    .click();
  await panel.getByRole("button", { name: /^uploads/ }).click();
  await panel.getByRole("button", { name: /^sales\.csv/ }).click();
  const table = panel.getByRole("table", { name: "Table of sales.csv" });
  await expect(table.getByRole("columnheader")).toHaveText(["region", "total"]);
  await expect(panel.getByText("2 rows", { exact: true })).toBeVisible();
  await panel
    .getByRole("navigation", { name: "Folder path" })
    .getByRole("button", { name: "Workspace" })
    .click();

  await panel.getByRole("tab", { name: "Changes" }).click();
  const list = panel.getByRole("list", { name: "Changed files" });
  await expect(list.getByRole("listitem")).toHaveText([
    "Createdreport.md+3",
    "Createdrows.txt+1",
  ]);
  // A file the run created opens as the file itself, not as a diff of nothing.
  await step("G2 T25: a created file opens from Changes as the file");
  await list.getByRole("button", { name: /report\.md/ }).click();
  await expect(panel.getByRole("tab", { name: "Files" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(
    panel.getByRole("heading", { name: "Sales report" }),
  ).toBeVisible();
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await capture(page, "added");
  await panel.getByRole("tab", { name: "Changes" }).click();
  const diff = panel.getByRole("group", { name: "Changes to report.md" });

  await message.fill("Update the report");
  await send.click();
  await expect(status).toHaveText(/^Done/, { timeout: 120_000 });
  await expect(list.getByRole("listitem")).toHaveText([
    "Modifiedreport.md+2−1",
  ]);
  await list.getByRole("button", { name: /report\.md/ }).click();
  // WEB-U-164: each line's sign is for the eye; a screen reader hears "Removed"/"Added". Checked apart.
  await expect
    .poll(() => shownLines(diff.locator('[data-kind="removed"]')))
    .toEqual(["-Regions: north, south"]);
  await expect(diff.locator('[data-kind="removed"] .sr-only')).toHaveText([
    "Removed",
  ]);
  await expect
    .poll(() => shownLines(diff.locator('[data-kind="added"]')))
    .toEqual(["+Regions: north, south, east", "+Checked twice."]);
  await expect(diff.locator('[data-kind="added"] .sr-only')).toHaveText([
    "Added",
    "Added",
  ]);
  await capture(page, "modified");

  // The first run's changes are now closed by the second message's snapshot.
  await step("the first run's changes are closed by the second message");
  const first = runs[0];
  expect(runs).toHaveLength(2);
  const closed = await page.request.get(
    `/api/v1/conversations/${task.id}/runs/${first}/changes`,
  );
  expect(await closed.json()).toMatchObject({
    live: false,
    files: [
      { path: "report.md", status: "added" },
      { path: "rows.txt", status: "added" },
    ],
  });

  // One byte over the per-file limit is refused before anything is kept; read even if the gateway closes early.
  const cookies = (await page.context().cookies(baseURL))
    .map(({ name, value }) => `${name}=${value}`)
    .join("; ");
  const oversize = await postBytes(
    new URL(`/api/v1/conversations/${task.id}/uploads?name=big.bin`, baseURL)
      .href,
    {
      Origin: baseURL,
      Cookie: cookies,
      "content-type": "application/octet-stream",
    },
    25 * 1024 * 1024 + 1,
  );
  const refusal = JSON.parse(oversize.body) as { error: { code: string } };
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({
        workspaceId: task.workspace_id,
        upload: SALES,
        refused: [oversize.status, refusal.error.code],
      }),
    );
  await madeFiles(page, baseURL);
  await uploadEdited(page, baseURL);
});

// WEB-E2E-027 (the owner's screenshot): the agent writes a file by the sandbox's absolute path; the task's Open shows
// that file in the files panel, not "This file is no longer there" under "Workspace / / workspace / …".
test("WEB-E2E-027 a file written by its absolute path opens on the real stack", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(1, 90_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Absolute path", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string };
  await page.goto(`/c/${task.id}`);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("Write a page by its full path");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  // The step itself first: the write succeeded, named as the workspace's own file (WEB-U-083).
  await page.locator(".activity-heading").last().click();
  await capture(page, "stack-files-absolute-path");
  await expect(
    page.getByRole("button", { name: /^Created page\.html/ }),
  ).toBeVisible();
  await expect(page.locator(".turn-summary").last()).toContainText(
    "Created page.html",
  );
  await page.getByRole("button", { name: "Open page.html" }).click();
  const panel = page.getByRole("complementary", { name: "Files" });
  // The preview, and the rendered page inside it, are both named for the file.
  await expect(
    panel.getByRole("region", { name: "page.html" }).first(),
  ).toBeVisible();
  await expect(panel.getByText("This file is no longer there.")).toHaveCount(0);
  // The folder path is the workspace itself: no empty segment, no second "workspace".
  const path = panel.getByRole("navigation", { name: "Folder path" });
  await expect(
    path.getByRole("button", { name: "workspace", exact: true }),
  ).toHaveCount(0);
});

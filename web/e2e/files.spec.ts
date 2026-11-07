import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";
import { auditFocus, auditHover, auditView } from "./ux/polish-audit";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `${name}.png`) });
}
async function audit(page: Page, view: string, deep: boolean) {
  await settled(page);
  const findings = [
    ...(await auditView(page, view)),
    ...(deep ? await auditFocus(page, view) : []),
    ...(deep ? await auditHover(page, view) : []),
  ];
  expect(findings).toEqual([]);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
}
const csv = (name: string) => ({
  name,
  mimeType: "text/csv",
  buffer: Buffer.from("region,total\nnorth,12\nsouth,30\n"),
});

// WEB-U-020 and WEB-U-022: attach, upload, send and see the files in the task's workspace, at both designed widths.
for (const width of [1024, 1440])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-022 files panel and attachments ${width} ${theme}`, async ({
      page,
    }) => {
      test.setTimeout(120_000);
      const deep = width === 1440 && theme === "light";
      const suffix = `${width}-${theme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const toggle = page.getByRole("button", { name: "Files", exact: true });
      const panel = page.getByRole("complementary", { name: "Files" });
      // Candidate12 F6 (WEB-U-124): closed until the person opens it, at every width.
      await expect(toggle).toHaveAttribute("aria-expanded", "false");
      await toggle.click();
      await expect(panel.getByRole("button", { name: "src" })).toBeVisible();
      await expect(panel.getByText(".tqa")).toHaveCount(0);
      // WEB-U-152 (the 13f screens, #1): beside files, the page never says the workspace starts empty.
      await expect(
        page.getByText("Send a message to begin.", { exact: true }),
      ).toBeVisible();
      await expect(page.getByText(/starts empty/)).toHaveCount(0);
      await capture(page, `files-panel-${suffix}`);
      await audit(page, `files-panel-${suffix}`, deep);

      await panel.getByRole("button", { name: "src" }).click();
      await panel.getByRole("button", { name: /^app\.py/ }).click();
      const preview = panel.getByRole("region", { name: "app.py" });
      await expect(preview.getByText('print("hello")')).toBeVisible();
      const download = page.waitForEvent("download");
      await preview.getByRole("link", { name: "Download" }).click();
      expect((await download).suggestedFilename()).toBe("app.py");
      await capture(page, `files-preview-${suffix}`);
      await audit(page, `files-preview-${suffix}`, deep);
      const path = panel.getByRole("navigation", { name: "Folder path" });
      await path.getByRole("button", { name: "Workspace" }).click();
      await panel.getByRole("button", { name: "data" }).click();
      await panel.getByRole("button", { name: /^sample\.bin/ }).click();
      await expect(
        panel.getByText(
          "This file can't be previewed. Download it to open it.",
        ),
      ).toBeVisible();
      if (width < 1280) {
        await page.keyboard.press("Escape");
        await expect(panel).toHaveCount(0);
        await expect(toggle).toBeFocused();
      }

      // Attach two files, one of them twice: the gateway keeps both under distinct names.
      await page
        .getByLabel("Choose files to attach")
        .setInputFiles([csv("Q3 sales.csv"), csv("Q3 sales.csv")]);
      const chips = page
        .getByRole("form", { name: "Message composer" })
        .getByRole("list", { name: "Attached files" });
      await expect(chips.getByText("Q3_sales.csv")).toBeVisible();
      await expect(chips.getByText("Q3_sales-2.csv")).toBeVisible();
      await page.getByLabel("Choose files to attach").setInputFiles([
        {
          name: "video.mov",
          mimeType: "video/quicktime",
          buffer: Buffer.alloc(25 * 1024 * 1024 + 1),
        },
      ]);
      await expect(
        chips.getByText("Too large: up to 25 MB per file"),
      ).toBeVisible();
      const message = page.getByLabel("Message", { exact: true });
      await message.fill("Summarise the sales files");
      await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
      await capture(page, `files-attachments-${suffix}`);
      await audit(page, `files-attachments-${suffix}`, deep);
      await chips.getByRole("button", { name: "Remove video.mov" }).click();
      const sent = page.waitForRequest(
        (request) =>
          request.method() === "POST" && request.url().endsWith("/messages"),
      );
      await page.getByRole("button", { name: "Send" }).click();
      expect((await sent).postDataJSON()).toEqual({
        text: "Summarise the sales files",
        attachments: ["uploads/Q3_sales.csv", "uploads/Q3_sales-2.csv"],
      });
      const yours = page.getByRole("article", { name: "Your message" });
      await expect(
        yours
          .getByRole("list", { name: "Attached files" })
          .getByRole("listitem"),
      ).toHaveText(["Q3_sales.csv", "Q3_sales-2.csv"]);
      await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
        timeout: 30_000,
      });
      // Reopened below 1280 px, the panel starts at the workspace root.
      if (width < 1280) await toggle.click();
      else await path.getByRole("button", { name: "Workspace" }).click();
      await expect(
        panel.getByRole("button", { name: "results" }),
      ).toBeVisible();
      await panel.getByRole("button", { name: "uploads" }).click();
      await expect(
        panel.getByRole("button", { name: /^Q3_sales-2\.csv/ }),
      ).toBeVisible();
      await capture(page, `files-after-run-${suffix}`);
    });

test("WEB-U-020 files dropped on the composer are attached", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto(await createConversation(page));
  const form = page.getByRole("form", { name: "Message composer" });
  const dataTransfer = await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(
      new File(["a,b\n"], "dropped.csv", { type: "text/csv" }),
    );
    return transfer;
  });
  await form.dispatchEvent("dragenter", { dataTransfer });
  await form.dispatchEvent("dragover", { dataTransfer });
  await expect(form.getByText("Drop files to attach")).toBeVisible();
  await form.dispatchEvent("drop", { dataTransfer });
  await expect(form.getByText("Drop files to attach")).toHaveCount(0);
  await expect(
    form.getByRole("list", { name: "Attached files" }).getByText("dropped.csv"),
  ).toBeVisible();
});

test("WEB-U-020 a message naming a missing upload is refused", async ({
  page,
}) => {
  await signIn(page);
  const path = await createConversation(page);
  const response = await page.request.post(
    `/api/v1${path.replace(/^\/c/, "/conversations")}/messages`,
    {
      headers: { "Idempotency-Key": "missing-attachment" },
      data: { text: "Read it", attachments: ["uploads/none.csv"] },
    },
  );
  expect(response.status()).toBe(400);
  expect(await response.json()).toMatchObject({
    error: { code: "attachment_missing" },
  });
});

// WEB-U-024 (task 068c): the Changes tab shows what each run added, modified or deleted, with a tinted diff.
for (const [width, theme] of [
  [1440, "light"],
  [1024, "dark"],
] as const)
  test(`WEB-U-024 run changes ${width} ${theme}`, async ({ page }) => {
    test.setTimeout(120_000);
    const suffix = `${width}-${theme}`;
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const status = page.getByLabel("Run status");
    const message = page.getByLabel("Message", { exact: true });
    const panel = page.getByRole("complementary", { name: "Files" });
    // Review of 068c: below 1280 px the floating panel ends above the composer, so sending works with it open.
    async function send(text: string) {
      await message.fill(text);
      await page.getByRole("button", { name: "Send" }).click({ trial: true });
      await page.getByRole("button", { name: "Send" }).click();
      await expect(status).toHaveText(/^Done/, { timeout: 30_000 });
      await expect(panel.getByRole("tab", { name: "Changes" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    }
    await page
      .getByLabel("Choose files to attach")
      .setInputFiles([csv("sales.csv")]);
    await expect(page.getByText("sales.csv").first()).toBeVisible();
    await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
    await message.fill("Summarise the sales files");
    await expect(page.getByRole("button", { name: "Send" })).toBeEnabled();
    await page.getByRole("button", { name: "Send" }).click();
    await expect(status).toHaveText(/^Done/, { timeout: 30_000 });

    // Review of 068c: the turn summary counts the run's recorded changes, not only file-edit tools; finding T-b: the
    // count alone, and Open changes (WEB-U-016) shows them in the files panel at any width.
    await expect(page.locator(".turn-summary")).toHaveText("1 file changed");
    await page.getByRole("button", { name: "Open changes" }).click();
    await expect(panel.getByRole("tab", { name: "Changes" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const picker = panel.getByRole("button", { name: /^Run: / });
    await expect(picker).toHaveAccessibleName("Run: Summarise the sales files");
    await expect(picker.getByText("Latest")).toBeVisible();
    const list = panel.getByRole("list", { name: "Changed files" });
    await expect(list.getByRole("listitem")).toHaveText([
      /^Createdresults\/summary\.md\+4$/,
    ]);
    await expect(
      panel.getByText("Your uploads are not listed as changes."),
    ).toBeVisible();
    await capture(page, `changes-list-${suffix}`);
    await audit(page, `changes-list-${suffix}`, width === 1440);
    // Finding F5: a file the run created opens as what it is, in the Files tab, not as a diff.
    await list.getByRole("button", { name: /summary\.md/ }).click();
    await expect(panel.getByRole("tab", { name: "Files" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await expect(
      panel.getByRole("region", { name: "summary.md" }),
    ).toBeVisible();
    await panel.getByRole("tab", { name: "Changes" }).click();

    await send("Update the summary");
    await expect(list.getByRole("listitem")).toHaveText([
      /^Modifiedresults\/summary\.md\+\d+−\d+$/,
    ]);
    await list.getByRole("button", { name: /summary\.md/ }).click();
    const diff = panel.getByRole("group", {
      name: "Changes to results/summary.md",
    });
    await expect(diff.locator('[data-kind="added"]').first()).toBeVisible();
    await expect(diff.locator('[data-kind="removed"]').first()).toBeVisible();
    await expect(diff.getByText("+++ b/results/summary.md")).toHaveCount(0);
    // WEB-U-164 (the 13f screens, #9): where the change is, in words; never the diff's "@@" notation.
    await expect(diff).not.toContainText("@@");
    await expect(diff.locator('[data-kind="hunk"]').first()).toHaveText(
      /^Lines? 1\b/,
    );
    // Review of 068c (walk-through): a long line wraps inside the panel, its marker kept, nothing clipped.
    const long = diff.locator("li", { hasText: /deliberately long line/ });
    await expect(long).toBeVisible();
    expect(
      await diff.evaluate(
        (element) => element.scrollWidth <= element.clientWidth + 1,
      ),
    ).toBe(true);
    await expect(long.locator(".diff-marker")).toHaveText("+");
    await expect(panel.getByRole("heading", { level: 2 })).toHaveText(
      /^Modifiedresults\/summary\.md\+\d+−\d+$/,
    );
    // WEB-U-170: the panel never covers the transcript; the person's own messages and the replies sit beside it.
    const panelLeft = (await panel.boundingBox())?.x ?? 0;
    const transcriptBox = await page
      .getByRole("region", { name: "Transcript" })
      .boundingBox();
    expect(
      (transcriptBox?.x ?? Infinity) + (transcriptBox?.width ?? 0),
    ).toBeLessThanOrEqual(panelLeft + 1);
    for (const mine of await page
      .getByRole("article", { name: "Your message" })
      .all()) {
      const box = await mine.boundingBox();
      expect((box?.x ?? Infinity) + (box?.width ?? 0)).toBeLessThanOrEqual(
        panelLeft + 1,
      );
    }
    await capture(page, `changes-diff-${suffix}`);
    await audit(page, `changes-diff-${suffix}`, width === 1440);
    await panel.getByRole("button", { name: "All changes" }).click();

    await picker.click();
    await page
      .getByRole("menuitemradio", { name: /^Summarise the sales files/ })
      .click();
    await expect(list.getByRole("listitem")).toHaveText([
      /^Createdresults\/summary\.md/,
    ]);
    await send("Remove the summary");
    await expect(picker).toHaveAccessibleName("Run: Remove the summary");
    await expect(list.getByRole("listitem")).toHaveText([
      /^Deletedresults\/summary\.md−\d+$/,
    ]);
  });

// The review of WEB-U-170: the layout's own width decides. At 1100, and at 1024 with the sidebar expanded, there is
// no room beside the conversation: the open panel takes the whole column, and the composer stays whole and reachable.
for (const [width, theme] of [
  [1100, "light"],
  [1024, "dark"],
] as const)
  test(`WEB-U-170 the files panel with the sidebar expanded ${width} ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    const expand = page.getByRole("button", { name: "Expand sidebar" });
    if (await expand.isVisible()) await expand.click();
    await expect(
      page.getByRole("button", { name: "Collapse sidebar" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Files", exact: true }).click();
    const panel = page.getByRole("complementary", { name: "Files" });
    await expect(panel).toBeVisible();
    const layout = await page.locator(".conversation-layout").boundingBox();
    const box = await panel.boundingBox();
    if (!layout || !box) throw new Error("The layout and the panel must show");
    // The whole column, never part of it.
    expect(Math.abs(box.x - layout.x)).toBeLessThanOrEqual(1);
    expect(
      Math.abs(box.x + box.width - (layout.x + layout.width)),
    ).toBeLessThanOrEqual(1);
    // The composer's + and Send are whole, inside the column, and what a click at their centre reaches.
    for (const control of [
      page.getByRole("button", { name: "Attach files" }),
      page.getByRole("button", { name: "Send" }),
    ]) {
      await expect(control).toBeInViewport({ ratio: 1 });
      const at = await control.boundingBox();
      if (!at) throw new Error("The composer's controls must show");
      expect(at.x).toBeGreaterThanOrEqual(layout.x);
      expect(at.x + at.width).toBeLessThanOrEqual(layout.x + layout.width);
      expect(
        await control.evaluate(
          (element, [x, y]) => {
            const hit = document.elementFromPoint(x as number, y as number);
            return hit === element || element.contains(hit);
          },
          [at.x + at.width / 2, at.y + at.height / 2],
        ),
      ).toBe(true);
    }
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await settled(page);
    await capture(page, `panel-narrow-${width}-${theme}`);
  });

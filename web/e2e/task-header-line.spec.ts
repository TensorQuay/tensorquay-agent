import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { openFilesPanel } from "./files-panel";
import { settled } from "./motion";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-135 (18.1, decided on 17 Nov 2026): the task header is one quiet line: the title at body size in regular weight,
// "· In <workspace>" muted after it, both cut with "…" when short of room, the workspace first; no "Done" (still read,
// off the screen), every other status on the line. At 1280, 1440 and 390, light and dark, a long title in a workspace
// with a long name; and at 1280 in the split (WEB-E2E-134), where the chat's header is narrow.

test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

const TITLE =
  "Quarterly sales by region with the returns and the regional notes";
const WORKSPACE = "Northern and western regions, quarterly planning and review";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `task-header-${name}.png`) });
}

/** A task with a long title in a workspace with a long name. */
async function task(page: Page): Promise<string> {
  const made = await page.request.post("/api/v1/workspaces", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { name: WORKSPACE },
  });
  expect(made.status()).toBe(201);
  const { id: workspace } = (await made.json()) as { id: string };
  const response = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title: TITLE, model_route: "local", workspace_id: workspace },
  });
  expect(response.status()).toBe(201);
  const { id } = (await response.json()) as { id: string };
  return `/c/${encodeURIComponent(id)}`;
}

/** The header's title and origin: their type, their line, whether each is cut, and whether Done shows. */
async function header(page: Page) {
  return page.evaluate(() => {
    const title = document.querySelector<HTMLElement>(".conversation-bar h1")!;
    const origin = document.querySelector<HTMLElement>(".conversation-origin");
    const word = document.querySelector<HTMLElement>(
      ".conversation-status [role='status']",
    );
    const bar = document.querySelector<HTMLElement>(".conversation-bar-main")!;
    const style = getComputedStyle(title);
    const box = (element: HTMLElement) => element.getBoundingClientRect();
    return {
      size: style.fontSize,
      weight: style.fontWeight,
      lines: Math.round(box(title).height / parseFloat(style.lineHeight)),
      // On the title's line: the origin's box inside the title's (they share a baseline, not a size).
      sameLine: origin
        ? box(origin).top >= box(title).top - 1 &&
          box(origin).bottom <= box(title).bottom + 1
        : null,
      // Cut: its text wider than its box, measured exactly (scrollWidth rounds a fraction of a pixel away).
      titleCut: (() => {
        const range = document.createRange();
        range.selectNodeContents(title);
        return range.getBoundingClientRect().width > box(title).width + 0.5;
      })(),
      originCut: origin ? origin.scrollWidth > origin.clientWidth + 1 : null,
      originText: origin?.textContent ?? "",
      // What a sighted person could see of the word: its box (sr-only leaves 1 × 1 px), and a short word drawn for it.
      wordBox: word
        ? [Math.round(box(word).width), Math.round(box(word).height)]
        : null,
      shortWord:
        word?.closest("[data-short]")?.getAttribute("data-short") ?? null,
      word: word?.textContent ?? "",
      // Shown: the word itself, or the short word the group draws in its place (WEB-U-155).
      wordShown:
        (word ? box(word).width > 1 : false) ||
        Boolean(
          word?.closest("[data-short]")?.getAttribute("data-short") &&
          box(word.closest<HTMLElement>("[data-short]")!).width > 1,
        ),
      overflow: bar.scrollWidth > bar.clientWidth + 1,
    };
  });
}

async function finish(page: Page) {
  await page.getByLabel("Message", { exact: true }).fill("List the files");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 20_000,
  });
  await settled(page);
}

for (const [width, height] of [
  [1280, 800],
  [1440, 900],
  [390, 844],
] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-135 the task header's one line (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await task(page));
      await finish(page);
      const done = await header(page);
      expect(done.size).toBe("16px");
      expect(done.weight).toBe("400");
      expect(done.lines).toBe(1);
      expect(done.overflow).toBe(false);
      expect(done.originText).toContain(WORKSPACE.slice(0, 10));
      if (width !== 390) {
        expect(done.sameLine).toBe(true);
        // The workspace gives way first: cut while the title still reads whole.
        expect(done.titleCut).toBe(false);
      }
      // Done stays in the accessibility tree on purpose (the status announces the run's end), and a sighted person
      // sees none of it: no box at the title's line, no short word in its place.
      expect(done.word).toBe("Done");
      expect(done.wordShown).toBe(false);
      expect(done.wordBox![0]).toBeLessThanOrEqual(1);
      expect(done.wordBox![1]).toBeLessThanOrEqual(1);
      expect(done.shortWord).toBeNull();
      await expect(
        page.getByRole("status").filter({ hasText: /^Done$/ }),
      ).toHaveCount(1);
      await capture(page, `done-${width}-${colorScheme}`);
    });

test("WEB-E2E-135 Working… shows on the line while a run goes (1280)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await signIn(page);
  await page.goto(await task(page));
  await page
    .getByLabel("Message", { exact: true })
    .fill(`Stream Markdown ${"unbroken".repeat(400)}`);
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  const working = await header(page);
  expect(working.wordShown).toBe(true);
  expect(working.lines).toBe(1);
  await capture(page, "working-1280");
});

for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-135 the header beside the split (1280 ${colorScheme})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await task(page));
    await finish(page);
    const panel = await openFilesPanel(page);
    await panel.getByRole("button", { name: "reports" }).click();
    await panel.getByRole("button", { name: /^brief\.pdf/ }).click();
    await panel.getByRole("button", { name: "Expand" }).click();
    await expect(panel).toHaveAttribute("data-split", "split");
    await settled(page);
    const narrow = await header(page);
    expect(narrow.lines).toBe(1);
    expect(narrow.overflow).toBe(false);
    expect(narrow.wordShown).toBe(false);
    expect(narrow.wordBox![0]).toBeLessThanOrEqual(1);
    expect(narrow.shortWord).toBeNull();
    await capture(page, `split-1280-${colorScheme}`);
  });

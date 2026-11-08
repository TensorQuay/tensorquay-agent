import { mkdir, readFile } from "node:fs/promises";
import { networkInterfaces } from "node:os";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openCard } from "./cards";
import { openFilesPanel } from "./files-panel";
import { markdownAnswer } from "./front/markdown.mjs";
import { ADD_TEST, EDIT_TEST } from "./front/stream.mjs";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";
import { followingBottom } from "./ux/follow-up";

const address = Object.values(networkInterfaces())
  .flat()
  .find((entry) => entry?.family === "IPv4" && !entry.internal)?.address;
if (!address)
  throw new Error(
    "A non-loopback interface is required for the copy regression.",
  );
test.use({ baseURL: `http://${address}:3100` });

async function open(page: Page) {
  await signIn(page);
  await page.goto(await createConversation(page));
  await expect(page.getByLabel("Message", { exact: true })).toBeEnabled();
}
async function send(page: Page) {
  await page
    .getByLabel("Message", { exact: true })
    .fill(`Stream Markdown ${"unbroken".repeat(400)}`);
  await page.getByRole("button", { name: "Send" }).click();
}
async function generated<T>(path: string): Promise<T> {
  return JSON.parse(
    await readFile(new URL(`../src/${path}`, import.meta.url), "utf8"),
  ) as T;
}

/**
 * WEB-U-014: the conversation page loads no administration schema and no settings copy; only the settings pages'
 * chunks carry them.
 */
async function noAdminSchema(page: Page) {
  const admin = await generated<{ definitions: Record<string, unknown> }>(
    "protocol/generated/admin.schema.json",
  );
  const names = Object.keys(admin.definitions);
  expect(names.length).toBeGreaterThan(0);
  const settings = Object.keys(
    (
      await generated<{ messages: Record<string, string> }>(
        "i18n/generated/en.settings.json",
      )
    ).messages,
  );
  expect(settings.length).toBeGreaterThan(0);
  const scripts = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .map((entry) => entry.name)
      .filter((name) => new URL(name).pathname.endsWith(".js")),
  );
  expect(scripts.length).toBeGreaterThan(0);
  // Whole names, quoted or not: minified JSON keeps a definition's name as a bare object key.
  const patterns = names.map(
    (name) => [name, new RegExp(`(?<![\\w$])${name}(?![\\w$])`)] as const,
  );
  const found: string[] = [];
  const copy: string[] = [];
  const paths: string[] = [];
  for (const script of scripts) {
    const source = await (await page.request.get(script)).text();
    for (const [name, pattern] of patterns)
      if (pattern.test(source))
        found.push(`${new URL(script).pathname}: ${name}`);
    // A message key has dots, so it is always quoted, in the catalogue and where a component names it.
    for (const key of settings)
      if (source.includes(`"${key}"`))
        copy.push(`${new URL(script).pathname}: ${key}`);
    // The settings pages' operations table stays with them (src/settings/operations.ts).
    for (const path of source.match(
      /\/api\/v1\/(?:admin\/|connectors)[\w/{}-]*/g,
    ) ?? [])
      paths.push(`${new URL(script).pathname}: ${path}`);
  }
  expect(paths, "settings API paths in the conversation scripts").toEqual([]);
  expect(found, "admin definitions in the conversation scripts").toEqual([]);
  expect(
    copy.slice(0, 20),
    "settings copy in the conversation scripts",
  ).toEqual([]);
}
/**
 * Waits for the outcome, not for a time: the answer is rendered and no "Formatting answer" placeholder is left. How long
 * the delayed chunk takes to arrive depends on the machine's load (WEB-U-014 flake); what the test asserts after this
 * (in view, still following) does not.
 */
async function formatted(page: Page) {
  await page.waitForFunction(
    () =>
      !document.querySelector('[aria-label="Formatting answer"]') &&
      document.body.innerText.includes("End of streamed answer."),
    undefined,
    { timeout: 0 },
  );
}
async function evidence(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

test("WEB-U-014/WEB-U-015 follows a long Markdown stream and copies code on HTTP", async ({
  page,
}) => {
  await open(page);
  expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
  const scriptBytes = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .filter((entry) => new URL(entry.name).pathname.endsWith(".js"))
      .reduce(
        (total, entry) =>
          total + (entry as PerformanceResourceTiming).encodedBodySize,
        0,
      ),
  );
  await noAdminSchema(page);
  expect(Buffer.byteLength(markdownAnswer)).toBeGreaterThanOrEqual(20 * 1024);
  const measurement = page.evaluate(async () => {
    let frames = 0;
    let started = 0;
    const deadline = performance.now() + 25_000;
    while (performance.now() < deadline) {
      const now = await new Promise<number>(requestAnimationFrame);
      const status = document.querySelector(
        '[aria-label="Run status"]',
      )?.textContent;
      if (status?.startsWith("Working…")) {
        started ||= now;
        frames++;
      }
      if (status?.startsWith("Done") && started)
        return (frames * 1000) / (now - started);
    }
    throw new Error("The stream did not finish within the measurement window.");
  });
  await send(page);
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: 20_000,
  });
  console.log(
    `Markdown stream: ${Buffer.byteLength(markdownAnswer)} bytes, ${(await measurement).toFixed(1)} frames/s; conversation scripts: ${scriptBytes} encoded bytes`,
  );
  expect(
    scriptBytes,
    "Conversation JavaScript must fit the 250 KiB compressed budget",
  ).toBeLessThanOrEqual(250 * 1024);
  expect(
    scriptBytes,
    "Conversation JavaScript must retain at least 10 KiB of headroom",
  ).toBeLessThanOrEqual(240 * 1024);
  await expect(
    page.getByText("End of streamed answer.", { exact: true }),
  ).toBeInViewport();
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  await region.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByRole("button", { name: /Jump to latest/ }),
  ).toBeVisible();
  const code = page.locator(".markdown-code").first();
  await code.getByRole("button", { name: "Copy code" }).click();
  await expect(code.getByRole("button", { name: "Copied" })).toBeVisible();
  const input = page.getByLabel("Message", { exact: true });
  await input.focus();
  await page.keyboard.press(
    process.platform === "darwin" ? "Meta+V" : "Control+V",
  );
  await expect(input).toHaveValue(
    (await code.locator("pre").textContent()) ?? "",
  );
  await page.getByRole("button", { name: /Jump to latest/ }).press("Enter");
  await expect(
    page.getByText("End of streamed answer.", { exact: true }),
  ).toBeInViewport();
  // WEB-U-274: the button goes once the view is at the end, so the transcript takes the focus, never the page.
  await expect(
    page.getByRole("region", { name: "Transcript", exact: true }),
  ).toBeFocused();
});

test("WEB-U-015 pauses during streaming and keeps the shell fixed at every desktop width", async ({
  page,
}) => {
  await open(page);
  await send(page);
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  await expect(
    page.getByRole("heading", { name: "Section 2", exact: true }),
  ).toBeAttached();
  await region.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByRole("button", { name: /Jump to latest · [1-9][\d\u202f]* new/ }),
  ).toBeVisible();
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: 20_000,
  });
  expect(await region.evaluate((element) => element.scrollTop)).toBe(0);
  await page.getByRole("button", { name: /Worked for/ }).click();
  await openCard(page.getByRole("button", { name: /^Ran / }));
  for (const width of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 1000 });
    // No global header (UI-GUIDELINES §6): the sidebar and the conversation bar must stay put.
    const bar = await page.locator(".conversation-bar").boundingBox();
    const sidebar = await page.locator(".app-sidebar").boundingBox();
    for (const position of [0, 1]) {
      await region.evaluate((element, position) => {
        element.scrollTop = position * element.scrollHeight;
      }, position);
      expect(await page.locator(".conversation-bar").boundingBox()).toEqual(
        bar,
      );
      expect(await page.locator(".app-sidebar").boundingBox()).toEqual(sidebar);
      await expect(page.getByRole("button", { name: "Send" })).toBeInViewport();
      expect(
        await page.evaluate(() => ({
          width: document.documentElement.scrollWidth,
          height: document.documentElement.scrollHeight,
        })),
        `The document fits the ${width} px viewport at transcript position ${position}`,
      ).toEqual({ width, height: 1000 });
    }
    if (width === 1024 || width === 1280)
      for (const theme of ["light", "dark"] as const) {
        await page.emulateMedia({ colorScheme: theme });
        await region.evaluate((element) => {
          element.scrollTop = element.scrollHeight / 2;
        });
        await expect(
          page.getByRole("button", { name: /Jump to latest/ }),
        ).toBeVisible();
        await evidence(page, `transcript-${width}-${theme}-middle`);
      }
  }
});

test("WEB-U-014/WEB-U-015 preserves completed output while the Markdown chunk is delayed", async ({
  page,
}) => {
  await open(page);
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requested = 0;
  await page.route("**/_next/static/**/*.js", async (route) => {
    requested++;
    await held;
    await route.continue();
  });
  try {
    await send(page);
    await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
      timeout: 20_000,
    });
    expect(requested, "The parser is requested on demand").toBeGreaterThan(0);
    await expect(page.getByLabel("Formatting answer").first()).toBeAttached();
    await expect(
      page.getByText("End of streamed answer.", { exact: true }),
    ).toHaveCount(0);
  } finally {
    release();
  }
  await formatted(page);
  await expect(
    page.getByText("End of streamed answer.", { exact: true }),
  ).toBeInViewport();
  await followingBottom(page);
  await page.reload();
  await formatted(page);
  await expect(
    page.getByText("End of streamed answer.", { exact: true }),
  ).toBeInViewport();
  await followingBottom(page);
});

// WEB-U-016: a run that retried the model twice and recovered shows no notice for it; its step line counts
// the retries, with the codes in the tooltip only.
test("WEB-U-016 recovered retries fold into the step", async ({ page }) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Retry, then answer");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  // No notice, and no live retry line ("{reason}; trying again."). The codes are said only after "Details:", in the
  // step's tooltip and its last line (WEB-U-304), so a bare code check would match that line too.
  await expect(page.getByTestId("transcript")).not.toContainText(
    /Model request failed|trying again/,
  );
  // The summary says the last reason; the step's line, the other one and the codes (Z27).
  await expect(page.locator(".activity-retries")).toHaveText(
    "The model took too long to answer. Details: timeout, fallback_parse: schema",
  );
  // Counted on the step's summary line by the last reason; its tooltip says each reason in words, then
  // the codes after "Details:" (WEB-U-304).
  const heading = page.getByRole("button", { name: /Worked for/ });
  await expect(heading).toHaveText(
    /· the model's reply couldn't be read, retried twice$/,
  );
  await expect(heading.locator("[title]")).toHaveAttribute(
    "title",
    "The model took too long to answer. The model's reply couldn't be read. Details: timeout, fallback_parse: schema",
  );
});

async function run(page: Page, text: string) {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill(text);
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  return input;
}

// Two open tool cards on one page (an axe check found it, 12 Oct 2026): each card's Result, Output and arguments are a
// named, focusable block that can scroll, never a page landmark, so two of them never break landmark-unique.
test("WEB-U-007 two open tool cards keep the page's landmarks unique", async ({
  page,
}) => {
  const input = await run(page, ADD_TEST);
  await input.fill(EDIT_TEST);
  await input.press("Enter");
  await expect(page.locator(".turn-summary").last()).toHaveText(
    "Updated test_parser.py",
  );
  for (const group of await page
    .getByRole("button", { name: /Worked for/ })
    .all())
    if ((await group.getAttribute("aria-expanded")) !== "true")
      await group.click();
  await openCard(page.getByRole("button", { name: /^Created tests/ }));
  await openCard(page.getByRole("button", { name: /^Edited tests/ }));
  for (const result of await page
    .locator(".tool-card details")
    .filter({ has: page.getByText("Result", { exact: true }) })
    .all())
    await result.evaluate((element) => {
      (element as HTMLDetailsElement).open = true;
    });
  await expect(page.locator(".result-text")).toHaveCount(2);
  // And the file's diff open beside them in the Changes tab: the panel shows one diff at a time.
  const panel = await openFilesPanel(page);
  await panel.getByRole("tab", { name: "Changes" }).click();
  await panel
    .getByRole("button", { name: /test_parser\.py/ })
    .first()
    .click();
  await expect(
    panel.getByRole("group", { name: /^Changes to tests\/test_parser\.py$/ }),
  ).toHaveAttribute("tabindex", "0");
  const { violations } = await new AxeBuilder({ page }).analyze();
  expect(
    violations.map((violation) => violation.id),
    JSON.stringify(violations.map((v) => [v.id, v.nodes.length])),
  ).toEqual([]);
  // Each block is still reached by the keyboard and named.
  for (const block of await page.locator(".result-text").all()) {
    await expect(block).toHaveAttribute("tabindex", "0");
    await expect(block).toHaveAccessibleName("Result");
  }
});

// An answer with many code blocks of one language and many tables (the streamed report: 40 of each): each is a named,
// focusable group, never a landmark, so the page's landmarks stay unique.
test("WEB-U-007 an answer's code blocks and tables keep the page's landmarks unique", async ({
  page,
}) => {
  await open(page);
  await send(page);
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: 20_000,
  });
  const code = page.getByRole("group", { name: "ts code" });
  const tables = page.getByRole("group", { name: "Table" });
  expect(await code.count()).toBeGreaterThan(1);
  expect(await tables.count()).toBeGreaterThan(1);
  await expect(code.first()).toHaveAttribute("tabindex", "0");
  await expect(tables.first()).toHaveAttribute("tabindex", "0");
  const { violations } = await new AxeBuilder({ page })
    .include(".transcript-scroll")
    .analyze();
  expect(
    violations.map((violation) => violation.id),
    JSON.stringify(violations.map((v) => [v.id, v.nodes.length])),
  ).toEqual([]);
});

// The demo's second turn (12 Oct 2026): an edit's step says "updated 1 file", and its footer says the same.
test("WEB-U-016 an edited file's footer says Updated", async ({ page }) => {
  await run(page, EDIT_TEST);
  await expect(page.locator(".turn-summary")).toHaveText(
    "Updated test_parser.py",
  );
  await expect(page.getByRole("button", { name: /Worked for/ })).toHaveText(
    /updated 1 file/,
  );
  await expect(
    page.getByRole("button", { name: "Open test_parser.py", exact: true }),
  ).toBeVisible();
});

// The demo's evidence (12 Oct 2026): read up, then the window grew until the whole task fit, and "Jump to latest"
// stayed with nothing below it. The browser moves the view without a scroll event; following resumes all the same.
test("WEB-U-015 a paused view that grows to fit everything follows again", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 640 });
  await run(page, ADD_TEST);
  await page.getByRole("button", { name: /Worked for/ }).click();
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  const overflow = () =>
    region.evaluate((element) => element.scrollHeight - element.clientHeight);
  expect(
    await overflow(),
    "the opened turn overflows the short window",
  ).toBeGreaterThan(24);
  await region.evaluate((element) => {
    element.scrollTop = 0;
  });
  const jump = page.getByRole("button", { name: /Jump to latest/ });
  await expect(jump).toBeVisible();
  await page.setViewportSize({ width: 1440, height: 2000 });
  await expect
    .poll(overflow, { message: "everything fits the tall window" })
    .toBeLessThanOrEqual(0);
  await expect(jump).toBeHidden();
});

// Plan §3, Done: a run that added a test offers Run all the tests, which puts the request in the box to send.
test("WEB-U-016 a new test offers Run all the tests", async ({ page }) => {
  const input = await run(page, ADD_TEST);
  // The one file it wrote, by name, with Open, named for its file.
  await expect(page.locator(".turn-summary")).toHaveText(
    "Created test_parser.py",
  );
  await expect(
    page.getByRole("button", { name: "Open test_parser.py", exact: true }),
  ).toHaveText("Open");
  await page
    .getByRole("group", { name: "What next" })
    .getByRole("button", { name: "Run all the tests" })
    .click();
  await expect(input).toHaveValue("Run all the tests");
  await expect(input).toBeFocused();
});
